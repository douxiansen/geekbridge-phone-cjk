// SPDX-License-Identifier: GPL-3.0-or-later
//! Call recording on Linux: capture the phone's voice stream and your
//! microphone with PipeWire's `pw-record`, as two files the core mixes.
//!
//! The phone's voice stream only exists once the call audio link is up, so
//! the recorder looks for it for a few seconds on a worker thread and never
//! blocks the call tracking.

use phone_core::platform::{PlatformError, Result};
use phone_core::recording::{RecordedAudio, Recorder, RecordingError};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// How long to wait for the phone's voice stream to appear.
const FIND_TIMEOUT: Duration = Duration::from_secs(12);
const SAMPLE_RATE: &str = "16000";

/// Picks the node that carries the other person's voice out of `pw-dump`
/// output: an audio *source* belonging to the phone at `address`.
pub fn find_remote_source(pw_dump_json: &str, address: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(pw_dump_json).ok()?;
    let mac_us = address.replace(':', "_").to_ascii_uppercase();
    let mac_colon = address.to_ascii_uppercase();
    let mut best: Option<(u8, String)> = None;
    for obj in value.as_array()? {
        let props = &obj["info"]["props"];
        let class = props["media.class"].as_str().unwrap_or("");
        if class != "Audio/Source" {
            continue;
        }
        let name = props["node.name"].as_str().unwrap_or("");
        let belongs = name.to_ascii_uppercase().contains(&mac_us)
            || props["api.bluez5.address"]
                .as_str()
                .is_some_and(|a| a.eq_ignore_ascii_case(&mac_colon))
            || props["device.name"]
                .as_str()
                .is_some_and(|d| d.to_ascii_uppercase().contains(&mac_us));
        if !belongs || name.is_empty() {
            continue;
        }
        // The call voice stream is `bluez_input...`; the music one is not a
        // source in this direction. Prefer the explicit name.
        let rank = if name.starts_with("bluez_input") {
            0
        } else {
            1
        };
        if best.as_ref().is_none_or(|(r, _)| rank < *r) {
            best = Some((rank, name.to_string()));
        }
    }
    best.map(|(_, n)| n)
}

fn pw_dump() -> Option<String> {
    let out = Command::new("pw-dump")
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn spawn_record(target: Option<&str>, file: &Path) -> std::io::Result<Child> {
    let mut cmd = Command::new("pw-record");
    cmd.args(["--rate", SAMPLE_RATE, "--channels", "1"]);
    if let Some(t) = target {
        cmd.args(["--target", t]);
    }
    cmd.arg(file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

/// Asks `pw-record` to finish (SIGINT writes the WAV header), then waits.
fn stop_child(mut child: Child) {
    // SAFETY: signalling a child process we own.
    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGINT);
    }
    let end = Instant::now() + Duration::from_secs(4);
    while Instant::now() < end {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn non_empty(p: PathBuf) -> Option<PathBuf> {
    std::fs::metadata(&p)
        .ok()
        .filter(|m| m.len() > 44)
        .map(|_| p)
}

pub struct LinuxRecorder {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<RecordedAudio>>,
}

impl LinuxRecorder {
    /// Starts looking for the call audio and recording it into `dir`.
    pub fn start(address: &str, dir: &Path) -> Result<Self> {
        if Command::new("pw-record")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_err()
        {
            return Err(PlatformError::Other(
                "pw-record is not installed (it comes with PipeWire's utilities)".into(),
            ));
        }
        let stop = Arc::new(AtomicBool::new(false));
        let (flag, address, dir) = (stop.clone(), address.to_string(), dir.to_path_buf());
        let worker = thread::Builder::new()
            .name("gbp-record".into())
            .spawn(move || {
                let remote_file = dir.join("remote.wav");
                let local_file = dir.join("local.wav");
                // The phone's voice needs the audio link, which exists only while the call
                // audio is on this computer. Your microphone is recorded only together with
                // it: a recording of just your room, with no call in it, is never wanted.
                let mut remote = None;
                let mut local = None;
                let end = Instant::now() + FIND_TIMEOUT;
                while remote.is_none() && !flag.load(Ordering::Relaxed) && Instant::now() < end {
                    if let Some(name) = pw_dump().and_then(|j| find_remote_source(&j, &address)) {
                        remote = spawn_record(Some(&name), &remote_file).ok();
                        if remote.is_some() {
                            local = spawn_record(None, &local_file).ok();
                        }
                    } else {
                        thread::sleep(Duration::from_millis(400));
                    }
                }
                if remote.is_none() {
                    log::warn!("no voice stream for {address} appeared; nothing is recorded");
                }
                while !flag.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(100));
                }
                if let Some(c) = remote {
                    stop_child(c);
                }
                if let Some(c) = local {
                    stop_child(c);
                }
                // Without the other side there is no call recording.
                let remote = non_empty(remote_file);
                RecordedAudio {
                    local: remote.as_ref().and_then(|_| non_empty(local_file)),
                    remote,
                }
            })
            .map_err(|e| PlatformError::Other(e.to_string()))?;
        Ok(Self {
            stop,
            worker: Some(worker),
        })
    }
}

impl Recorder for LinuxRecorder {
    fn finish(mut self: Box<Self>) -> std::result::Result<RecordedAudio, RecordingError> {
        self.stop.store(true, Ordering::Relaxed);
        match self.worker.take() {
            Some(w) => w
                .join()
                .map_err(|_| RecordingError::Other("the recorder stopped unexpectedly".into())),
            None => Err(RecordingError::Empty),
        }
    }
}

impl Drop for LinuxRecorder {
    fn drop(&mut self) {
        // Abandoned without finish: stop the capture processes.
        self.stop.store(true, Ordering::Relaxed);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUMP: &str = r#"[
      {"id": 40, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Audio/Sink", "node.name": "alsa_output.pci"}}},
      {"id": 71, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Audio/Sink", "node.name": "bluez_output.AA_BB_CC_DD_EE_FF.1"}}},
      {"id": 72, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Audio/Source", "node.name": "bluez_input.AA_BB_CC_DD_EE_FF.0", "api.bluez5.address": "AA:BB:CC:DD:EE:FF"}}},
      {"id": 90, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Audio/Source", "node.name": "alsa_input.pci"}}},
      {"id": 91, "type": "PipeWire:Interface:Device", "info": {"props": {"device.name": "bluez_card.AA_BB_CC_DD_EE_FF"}}}
    ]"#;

    #[test]
    fn finds_the_phones_voice_source() {
        assert_eq!(
            find_remote_source(DUMP, "AA:BB:CC:DD:EE:FF").as_deref(),
            Some("bluez_input.AA_BB_CC_DD_EE_FF.0")
        );
        // Case and separators do not matter.
        assert!(find_remote_source(DUMP, "aa:bb:cc:dd:ee:ff").is_some());
    }

    #[test]
    fn another_phone_or_no_stream_finds_nothing() {
        assert_eq!(find_remote_source(DUMP, "11:22:33:44:55:66"), None);
        assert_eq!(find_remote_source("[]", "AA:BB:CC:DD:EE:FF"), None);
        assert_eq!(find_remote_source("not json", "AA:BB:CC:DD:EE:FF"), None);
    }

    #[test]
    fn prefers_the_bluez_input_name() {
        let dump = r#"[
          {"info": {"props": {"media.class": "Audio/Source", "node.name": "other.AA_BB_CC_DD_EE_FF.monitor"}}},
          {"info": {"props": {"media.class": "Audio/Source", "node.name": "bluez_input.AA_BB_CC_DD_EE_FF.0"}}}
        ]"#;
        assert_eq!(
            find_remote_source(dump, "AA:BB:CC:DD:EE:FF").as_deref(),
            Some("bluez_input.AA_BB_CC_DD_EE_FF.0")
        );
    }
}
