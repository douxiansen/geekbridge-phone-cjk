// SPDX-License-Identifier: GPL-3.0-or-later
//! The Linux [`Platform`]: BlueZ, PipeWire telephony and RFCOMM sockets.

use crate::{dbus, rfcomm_connect, sdp};
use phone_core::calls::CallSnapshot;
use phone_core::platform::{PhoneInfo, Platform, PlatformError, Result, Stream};
use zbus::blocking::Connection;

pub struct LinuxPlatform {
    system: Connection,
    session: Connection,
}

impl LinuxPlatform {
    pub fn new() -> Result<Self> {
        let system =
            Connection::system().map_err(|e| PlatformError::Other(format!("system bus: {e}")))?;
        let session =
            Connection::session().map_err(|e| PlatformError::Other(format!("session bus: {e}")))?;
        Ok(Self { system, session })
    }

    fn open(&self, address: &str, service: u16) -> Result<Box<dyn Stream>> {
        let channel = sdp::find_channel(address, service)?.ok_or(PlatformError::NoService)?;
        Ok(Box::new(rfcomm_connect(address, channel)?))
    }
}

impl Platform for LinuxPlatform {
    fn phones(&self) -> Result<Vec<PhoneInfo>> {
        dbus::phones(&self.system)
    }
    fn open_map(&self, address: &str) -> Result<Box<dyn Stream>> {
        self.open(address, sdp::service::MAP_MAS)
    }
    fn open_pbap(&self, address: &str) -> Result<Box<dyn Stream>> {
        self.open(address, sdp::service::PBAP_PSE)
    }
    fn calls(&self, address: &str) -> Result<Vec<CallSnapshot>> {
        dbus::calls(&self.session, address)
    }
    fn dial(&self, address: &str, number: &str) -> Result<()> {
        dbus::dial(&self.session, address, number)
    }
    fn answer(&self, _address: &str, call_id: &str) -> Result<()> {
        dbus::call_method(&self.session, call_id, "Answer")
    }
    fn hang_up(&self, _address: &str, call_id: &str) -> Result<()> {
        dbus::call_method(&self.session, call_id, "Hangup")
    }
    fn hang_up_all(&self, address: &str) -> Result<()> {
        dbus::hang_up_all(&self.session, address)
    }
    fn send_tones(&self, address: &str, tones: &str) -> Result<()> {
        dbus::send_tones(&self.session, address, tones)
    }
}
