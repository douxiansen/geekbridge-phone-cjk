// SPDX-License-Identifier: GPL-3.0-or-later
//! Call recording: what a platform captures, and how it becomes one file.
//!
//! A platform records the two sides of a call separately (what the other
//! person says, and what you say) and the core mixes them into one stereo WAV:
//! the other person on the left, you on the right.

use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum RecordingError {
    #[error("nothing was recorded")]
    Empty,
    #[error("audio file error: {0}")]
    Audio(#[from] hound::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

/// The raw captures of one call. Either side may be missing.
#[derive(Debug, Default, Clone)]
pub struct RecordedAudio {
    /// The other person.
    pub remote: Option<PathBuf>,
    /// You, from the microphone.
    pub local: Option<PathBuf>,
}

/// A recording in progress. Dropping it without `finish` abandons it.
pub trait Recorder: Send {
    /// Stops capturing and returns the raw files.
    fn finish(self: Box<Self>) -> Result<RecordedAudio, RecordingError>;
}

/// Where recordings go when the setting is empty.
pub fn default_dir() -> PathBuf {
    dirs::audio_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("GeekBridge Calls")
}

/// A safe file name stem such as `2026-11-08 20-33 Alí Boudjemaa (incoming)`.
pub fn file_stem(who: &str, kind: &str, at: i64) -> String {
    let when = chrono::DateTime::from_timestamp(at, 0)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H-%M")
                .to_string()
        })
        .unwrap_or_else(|| "call".into());
    let clean: String = who
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '+' | '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let clean = clean.trim().trim_matches('.');
    let clean = if clean.is_empty() { "Unknown" } else { clean };
    let clean: String = clean.chars().take(60).collect();
    format!("{when} {clean} ({kind})")
}

/// Reads a WAV as mono f32 samples and its sample rate.
fn read_mono(path: &Path) -> Result<(Vec<f32>, u32), RecordingError> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let channels = usize::from(spec.channels.max(1));
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = (1u64 << (spec.bits_per_sample.saturating_sub(1))) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / scale))
                .collect::<Result<_, _>>()?
        }
    };
    let mono = samples
        .chunks(channels)
        .map(|f| f.iter().sum::<f32>() / f.len() as f32)
        .collect();
    Ok((mono, spec.sample_rate))
}

/// Linear resample; good enough for speech at 8 to 48 kHz.
fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    let n = (input.len() as u64 * u64::from(to) / u64::from(from)) as usize;
    (0..n)
        .map(|i| {
            let pos = i as f64 * f64::from(from) / f64::from(to);
            let a = pos.floor() as usize;
            let frac = (pos - a as f64) as f32;
            let x0 = input[a.min(input.len() - 1)];
            let x1 = input[(a + 1).min(input.len() - 1)];
            x0 + (x1 - x0) * frac
        })
        .collect()
}

fn to_i16(x: f32) -> i16 {
    (x.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
}

/// Mixes the two sides into one stereo WAV at `out` (remote left, local right).
/// A missing or empty side becomes silence, or a copy of the other when only
/// one exists.
pub fn mix_stereo(audio: &RecordedAudio, out: &Path) -> Result<(), RecordingError> {
    let load = |p: &Option<PathBuf>| -> Option<(Vec<f32>, u32)> {
        let path = p.as_ref()?;
        match read_mono(path) {
            Ok((s, r)) if !s.is_empty() => Some((s, r)),
            _ => None,
        }
    };
    let remote = load(&audio.remote);
    let local = load(&audio.local);
    let rate = match (&remote, &local) {
        (None, None) => return Err(RecordingError::Empty),
        (Some((_, a)), Some((_, b))) => (*a).max(*b),
        (Some((_, r)), None) | (None, Some((_, r))) => *r,
    };
    let prep = |side: Option<(Vec<f32>, u32)>| side.map(|(s, r)| resample(&s, r, rate));
    let (left, right) = match (prep(remote), prep(local)) {
        (Some(l), Some(r)) => (l, r),
        (Some(l), None) => (l.clone(), l),
        (None, Some(r)) => (r.clone(), r),
        (None, None) => return Err(RecordingError::Empty),
    };
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(out, spec)?;
    for i in 0..left.len().max(right.len()) {
        writer.write_sample(to_i16(left.get(i).copied().unwrap_or(0.0)))?;
        writer.write_sample(to_i16(right.get(i).copied().unwrap_or(0.0)))?;
    }
    writer.finalize()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(path: &Path, rate: u32, freq: f32, secs: f32) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..(rate as f32 * secs) as usize {
            let t = i as f32 / rate as f32;
            w.write_sample((f32::sin(2.0 * std::f32::consts::PI * freq * t) * 12000.0) as i16)
                .unwrap();
        }
        w.finalize().unwrap();
    }

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("gbp-rec-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn mixes_two_sides_into_stereo_at_the_higher_rate() {
        let d = dir("mix");
        tone(&d.join("r.wav"), 16000, 440.0, 1.0);
        tone(&d.join("l.wav"), 8000, 880.0, 1.5);
        let out = d.join("out.wav");
        mix_stereo(
            &RecordedAudio {
                remote: Some(d.join("r.wav")),
                local: Some(d.join("l.wav")),
            },
            &out,
        )
        .unwrap();
        let r = hound::WavReader::open(&out).unwrap();
        assert_eq!(r.spec().channels, 2);
        assert_eq!(r.spec().sample_rate, 16000);
        // The longer side (1.5 s) sets the length; the shorter is padded.
        assert_eq!(r.duration(), 24000);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn one_side_only_is_duplicated_and_nothing_is_an_error() {
        let d = dir("one");
        tone(&d.join("r.wav"), 16000, 440.0, 0.5);
        let out = d.join("o.wav");
        mix_stereo(
            &RecordedAudio {
                remote: Some(d.join("r.wav")),
                local: None,
            },
            &out,
        )
        .unwrap();
        let mut r = hound::WavReader::open(&out).unwrap();
        let s: Vec<i16> = r.samples::<i16>().take(200).map(Result::unwrap).collect();
        assert!(s.chunks(2).all(|p| p[0] == p[1]));
        assert!(matches!(
            mix_stereo(&RecordedAudio::default(), &d.join("none.wav")),
            Err(RecordingError::Empty)
        ));
        assert!(matches!(
            mix_stereo(
                &RecordedAudio {
                    remote: Some(d.join("missing.wav")),
                    local: None
                },
                &d.join("x.wav")
            ),
            Err(RecordingError::Empty)
        ));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn file_names_are_safe() {
        let s = file_stem("Alí/Boud:jemaa", "incoming", 1_780_000_000);
        assert!(!s.contains('/') && !s.contains(':'));
        assert!(s.ends_with("(incoming)"));
        assert!(file_stem("", "missed", 0).contains("Unknown"));
        assert!(file_stem(&"x".repeat(300), "a", 0).len() < 120);
    }
}
