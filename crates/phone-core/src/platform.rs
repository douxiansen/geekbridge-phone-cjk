// SPDX-License-Identifier: GPL-3.0-or-later
//! What an operating system must provide.

use crate::calls::CallSnapshot;
use crate::recording::Recorder;
use std::io::{Read, Write};

/// A byte stream to a service on the phone (an RFCOMM socket).
pub trait Stream: Read + Write + Send {}
impl<T: Read + Write + Send> Stream for T {}

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("the phone does not offer that service")]
    NoService,
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, PlatformError>;

/// A paired phone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhoneInfo {
    pub address: String,
    pub name: String,
    pub connected: bool,
    pub battery: Option<u8>,
}

/// The per-OS half of the app: finding phones, opening the message and
/// phonebook services, and controlling calls.
pub trait Platform: Send + Sync {
    /// Paired phones with their current state.
    fn phones(&self) -> Result<Vec<PhoneInfo>>;
    /// A stream to the phone's message access service.
    fn open_map(&self, address: &str) -> Result<Box<dyn Stream>>;
    /// A stream to the phone's phonebook service.
    fn open_pbap(&self, address: &str) -> Result<Box<dyn Stream>>;
    /// Calls the phone has right now.
    fn calls(&self, address: &str) -> Result<Vec<CallSnapshot>>;
    fn dial(&self, address: &str, number: &str) -> Result<()>;
    fn answer(&self, address: &str, call_id: &str) -> Result<()>;
    fn hang_up(&self, address: &str, call_id: &str) -> Result<()>;
    fn hang_up_all(&self, address: &str) -> Result<()>;
    fn send_tones(&self, address: &str, tones: &str) -> Result<()>;
    /// Starts capturing the current call's audio into `dir`. Platforms that
    /// cannot record keep the default.
    fn start_recording(&self, _address: &str, _dir: &std::path::Path) -> Result<Box<dyn Recorder>> {
        Err(PlatformError::Other(
            "call recording is not supported on this system".into(),
        ))
    }
    /// Moves the call's voice onto this computer's speakers and microphone.
    /// Platforms whose system does this by itself keep the default.
    fn activate_audio(&self, _address: &str) -> Result<()> {
        Err(PlatformError::Other(
            "this system moves call audio by itself".into(),
        ))
    }
}
