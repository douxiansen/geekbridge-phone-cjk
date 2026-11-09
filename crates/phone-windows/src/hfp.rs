// SPDX-License-Identifier: GPL-3.0-or-later
//! Our own Hands-Free Profile client, the same role the Linux version plays: the computer opens
//! the hands-free link to the phone, so the phone treats it like a car kit and lets it see and
//! control calls. Windows' own telephony API is not used (it needs a packaged app and never
//! brought the link up for us).
//!
//! Call control only. The voice (SCO audio) is not carried by this module.
use crate::rfcomm::{RfcommReader, RfcommStream, RfcommWriter};
use phone_core::calls::{CallSnapshot, CallState};
use phone_core::platform::{PlatformError, Result};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// Service class of the phone's hands-free gateway.
const HFP_AG: u32 = 0x111F;
const REPLY_TIMEOUT: Duration = Duration::from_secs(6);

pub struct HfpSession {
    writer: Arc<Mutex<RfcommWriter>>,
    audio_requested: AtomicBool,
    codec_negotiation: AtomicBool,
    lines: Mutex<Receiver<String>>,
    alive: Arc<AtomicBool>,
}

fn other(s: impl Into<String>) -> PlatformError {
    PlatformError::Other(s.into())
}

/// Splits incoming bytes into the AT lines the phone sends (separated by CR and/or LF).
fn spawn_reader(
    mut r: RfcommReader,
    alive: Arc<AtomicBool>,
    writer: Arc<Mutex<RfcommWriter>>,
) -> Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 512];
        let mut line = Vec::new();
        'outer: loop {
            let n = match r.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            for &b in &buf[..n] {
                if b == b'\r' || b == b'\n' {
                    if !line.is_empty() {
                        let text = String::from_utf8_lossy(&line).trim().to_string();
                        line.clear();
                        if !text.is_empty() {
                            log::info!("hfp <- {text}");
                            // The phone picked a codec for the audio link: confirm it, or it
                            // will not open the voice channel.
                            if let Some(c) = text.strip_prefix("+BCS:") {
                                let cmd = format!("AT+BCS={}\r", c.trim());
                                log::info!("hfp -> {}", cmd.trim());
                                let _ = writer.lock().unwrap().write_all(cmd.as_bytes());
                            }
                            if tx.send(text).is_err() {
                                break 'outer;
                            }
                        }
                    }
                } else {
                    line.push(b);
                }
            }
        }
        alive.store(false, Ordering::SeqCst);
    });
    rx
}

impl HfpSession {
    /// Opens the link and runs the service level connection setup.
    pub fn connect(address: &str) -> Result<Self> {
        Self::from_stream(RfcommStream::connect(address, HFP_AG)?)
    }

    /// Runs the service level connection setup over a stream, whichever side opened it.
    pub fn from_stream(stream: RfcommStream) -> Result<Self> {
        let (r, w) = stream.split();
        let alive = Arc::new(AtomicBool::new(true));
        let writer = Arc::new(Mutex::new(w));
        let lines = spawn_reader(r, alive.clone(), writer.clone());
        let s = Self {
            writer,
            audio_requested: AtomicBool::new(false),
            codec_negotiation: AtomicBool::new(false),
            lines: Mutex::new(lines),
            alive,
        };
        // Features we claim: caller id presentation (bit 2) and codec negotiation (bit 7), which
        // is what lets this side ask the phone to open the voice channel (AT+BCC).
        let agent = s.command("AT+BRSF=132")?;
        let ag_features: u32 = agent
            .iter()
            .find_map(|l| l.strip_prefix("+BRSF:"))
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0);
        // Bit 9 of the phone's features: it supports codec negotiation too.
        if ag_features & (1 << 9) != 0 {
            s.codec_negotiation.store(true, Ordering::SeqCst);
        }
        s.command("AT+CIND=?")?;
        s.command("AT+CIND?")?;
        if s.codec_negotiation.load(Ordering::SeqCst) {
            // Plain CVSD (1) is the one codec every phone and PC supports.
            let _ = s.command("AT+BAC=1");
        }
        s.command("AT+CMER=3,0,0,1")?;
        // Nice to have: caller number on ring, call waiting. Phones that lack them say ERROR.
        let _ = s.command("AT+CLIP=1");
        let _ = s.command("AT+CCWA=1");
        Ok(s)
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    /// Sends one command and returns the information lines before its final `OK`.
    pub fn command(&self, cmd: &str) -> Result<Vec<String>> {
        if !self.is_alive() {
            return Err(other("the hands-free link is down"));
        }
        let rx = self.lines.lock().unwrap();
        while rx.try_recv().is_ok() {} // stale ring and indicator lines
        {
            let mut w = self.writer.lock().unwrap();
            w.write_all(format!("{cmd}\r").as_bytes())
                .map_err(|e| other(format!("write failed: {e}")))?;
        }
        let mut info = Vec::new();
        loop {
            let line = rx
                .recv_timeout(REPLY_TIMEOUT)
                .map_err(|_| other(format!("the phone did not answer {cmd}")))?;
            match line.as_str() {
                "OK" => return Ok(info),
                "ERROR" => return Err(other(format!("the phone refused {cmd}"))),
                l if l.starts_with("+CME ERROR") => {
                    return Err(other(format!("the phone refused {cmd}: {l}")))
                }
                l if l.starts_with("AT") => {} // echo
                _ => info.push(line),
            }
        }
    }

    pub fn calls(&self) -> Result<Vec<CallSnapshot>> {
        let calls: Vec<CallSnapshot> = self
            .command("AT+CLCC")?
            .iter()
            .filter_map(|l| parse_clcc(l))
            .collect();
        // Once a call is up, ask the phone to put its voice on this computer.
        let live = calls.iter().any(|c| c.state == CallState::Active);
        if live && !self.audio_requested.swap(true, Ordering::SeqCst) {
            self.request_audio();
        } else if calls.is_empty() {
            self.audio_requested.store(false, Ordering::SeqCst);
        }
        Ok(calls)
    }

    /// Asks the phone to open the voice channel to this computer.
    pub fn request_audio(&self) {
        if self.codec_negotiation.load(Ordering::SeqCst) {
            log::info!("hfp: asking the phone for call audio");
            if let Err(e) = self.command("AT+BCC") {
                log::warn!("hfp: AT+BCC failed: {e}");
            }
        } else {
            log::info!("hfp: the phone has no codec negotiation, cannot request call audio");
        }
    }
}

/// `+CLCC: 1,1,4,0,0,"+40123456789",145` to a snapshot (index, direction, status, mode,
/// multiparty, number, type).
fn parse_clcc(line: &str) -> Option<CallSnapshot> {
    let rest = line.strip_prefix("+CLCC:")?.trim();
    let mut parts = rest.splitn(6, ',');
    let idx = parts.next()?.trim().to_string();
    let _dir = parts.next()?;
    let stat: u8 = parts.next()?.trim().parse().ok()?;
    let _mode = parts.next();
    let _mpty = parts.next();
    let number = parts
        .next()
        .map(|t| {
            let t = t.split(',').next().unwrap_or("");
            t.trim().trim_matches('"').to_string()
        })
        .unwrap_or_default();
    let state = match stat {
        0 => CallState::Active,
        1 => CallState::Held,
        2 => CallState::Dialing,
        3 => CallState::Alerting,
        4 => CallState::Incoming,
        5 => CallState::Waiting,
        _ => return None,
    };
    Some(CallSnapshot {
        id: idx,
        state,
        number,
        name: None,
    })
}

fn sessions() -> &'static Mutex<HashMap<String, Arc<HfpSession>>> {
    static S: OnceLock<Mutex<HashMap<String, Arc<HfpSession>>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

/// Whether a hands-free link to this phone is up right now.
pub fn is_up(address: &str) -> bool {
    sessions()
        .lock()
        .unwrap()
        .get(address)
        .is_some_and(|s| s.is_alive())
}

/// Tries to bring the link to a paired but disconnected phone up, in the background and at most
/// every 10 seconds per phone. Windows only connects to a phone when something asks, whereas
/// the phone expects the computer to behave like a car kit that reconnects by itself.
pub fn reconnect_in_background(address: &str) {
    use std::time::Instant;
    static LAST: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    {
        let mut last = LAST.get_or_init(Default::default).lock().unwrap();
        if last
            .get(address)
            .is_some_and(|t| t.elapsed() < Duration::from_secs(10))
        {
            return;
        }
        last.insert(address.to_string(), Instant::now());
    }
    let address = address.to_string();
    std::thread::spawn(move || {
        if let Err(e) = session(&address) {
            log::debug!("reconnect to {address}: {e}");
        }
    });
}

/// The live session for this phone, opening one if there is none.
pub fn session(address: &str) -> Result<Arc<HfpSession>> {
    let mut map = sessions().lock().unwrap();
    if let Some(s) = map.get(address) {
        if s.is_alive() {
            return Ok(s.clone());
        }
    }
    let s = Arc::new(HfpSession::connect(address)?);
    map.insert(address.to_string(), s.clone());
    Ok(s)
}

pub fn calls(address: &str) -> Result<Vec<CallSnapshot>> {
    session(address)?.calls()
}

pub fn dial(address: &str, number: &str) -> Result<()> {
    if !number
        .chars()
        .all(|c| c.is_ascii_digit() || "+*#".contains(c))
        || number.is_empty()
    {
        return Err(other("that is not a phone number"));
    }
    session(address)?
        .command(&format!("ATD{number};"))
        .map(|_| ())
}

pub fn answer(address: &str, _call_id: &str) -> Result<()> {
    session(address)?.command("ATA").map(|_| ())
}

pub fn hang_up(address: &str, _call_id: &str) -> Result<()> {
    // CHUP ends the active call, or rejects the ringing one.
    session(address)?.command("AT+CHUP").map(|_| ())
}

pub fn hang_up_all(address: &str) -> Result<()> {
    let s = session(address)?;
    // One CHUP per call, bounded so a stubborn phone cannot loop us.
    for _ in 0..4 {
        if s.calls()?.is_empty() {
            break;
        }
        s.command("AT+CHUP")?;
    }
    Ok(())
}

pub fn send_tones(address: &str, tones: &str) -> Result<()> {
    let s = session(address)?;
    for c in tones
        .chars()
        .filter(|c| c.is_ascii_digit() || "*#".contains(*c))
    {
        s.command(&format!("AT+VTS={c}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_an_incoming_call() {
        let c = parse_clcc("+CLCC: 1,1,4,0,0,\"+40722123456\",145").unwrap();
        assert_eq!(c.id, "1");
        assert_eq!(c.state, CallState::Incoming);
        assert_eq!(c.number, "+40722123456");
    }

    #[test]
    fn parses_an_active_call_without_number() {
        let c = parse_clcc("+CLCC: 2,0,0,0,0").unwrap();
        assert_eq!(c.state, CallState::Active);
        assert_eq!(c.number, "");
    }

    #[test]
    fn ignores_other_lines() {
        assert!(parse_clcc("+CIEV: 2,1").is_none());
    }
}
