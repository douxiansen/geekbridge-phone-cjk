// SPDX-License-Identifier: GPL-3.0-or-later
//! Choosing the phone platform and starting the pieces: the service, the event thread and the
//! single-instance listener.

use phone_core::calls::CallSnapshot;
use phone_core::platform::{PhoneInfo, Platform, PlatformError, Result, Stream};
use std::sync::Arc;

/// The platform used when the real one cannot start (no Bluetooth service, no D-Bus). Every
/// call reports why, so the user sees one clear banner instead of a window that does nothing.
pub struct UnavailablePlatform {
    reason: String,
}

impl UnavailablePlatform {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }

    fn fail<T>(&self) -> Result<T> {
        Err(PlatformError::Other(self.reason.clone()))
    }
}

impl Platform for UnavailablePlatform {
    fn phones(&self) -> Result<Vec<PhoneInfo>> {
        self.fail()
    }
    fn open_map(&self, _: &str) -> Result<Box<dyn Stream>> {
        self.fail()
    }
    fn open_pbap(&self, _: &str) -> Result<Box<dyn Stream>> {
        self.fail()
    }
    fn calls(&self, _: &str) -> Result<Vec<CallSnapshot>> {
        self.fail()
    }
    fn dial(&self, _: &str, _: &str) -> Result<()> {
        self.fail()
    }
    fn answer(&self, _: &str, _: &str) -> Result<()> {
        self.fail()
    }
    fn hang_up(&self, _: &str, _: &str) -> Result<()> {
        self.fail()
    }
    fn hang_up_all(&self, _: &str) -> Result<()> {
        self.fail()
    }
    fn send_tones(&self, _: &str, _: &str) -> Result<()> {
        self.fail()
    }
}

/// The real phone platform for this operating system.
///
/// TODO(windows, macos): construct the WinRT / IOBluetooth platforms here (see docs/PORTING.md).
pub fn live_platform() -> Arc<dyn Platform> {
    #[cfg(target_os = "linux")]
    {
        match phone_linux::LinuxPlatform::new() {
            Ok(p) => Arc::new(p),
            Err(e) => {
                log::error!("Linux Bluetooth platform failed to start: {e}");
                Arc::new(UnavailablePlatform::new(format!(
                    "Bluetooth is not available: {e}"
                )))
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        Arc::new(UnavailablePlatform::new(
            "This system is not supported yet.",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_unavailable_platform_always_says_why() {
        let p = UnavailablePlatform::new("no bus");
        assert_eq!(p.phones().unwrap_err().to_string(), "no bus");
        assert_eq!(p.dial("a", "1").unwrap_err().to_string(), "no bus");
        assert!(p.open_map("a").is_err());
        assert!(p.calls("a").is_err());
    }
}
