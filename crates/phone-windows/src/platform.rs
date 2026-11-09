// SPDX-License-Identifier: GPL-3.0-or-later
use crate::rfcomm::{RfcommStream, MAP_MAS, PBAP_PSE};
use phone_core::calls::CallSnapshot;
use phone_core::platform::{PhoneInfo, Platform, PlatformError, Result, Stream};
use windows::Devices::Bluetooth::{BluetoothConnectionStatus, BluetoothDevice};
use windows::Devices::Enumeration::DeviceInformation;

pub struct WindowsPlatform;

fn werr(e: windows::core::Error) -> PlatformError {
    PlatformError::Other(e.to_string())
}

/// 48-bit Bluetooth address as the usual `AA:BB:CC:DD:EE:FF`.
fn format_address(a: u64) -> String {
    (0..6)
        .rev()
        .map(|i| format!("{:02X}", (a >> (i * 8)) & 0xff))
        .collect::<Vec<_>>()
        .join(":")
}

impl Platform for WindowsPlatform {
    fn phones(&self) -> Result<Vec<PhoneInfo>> {
        let selector = BluetoothDevice::GetDeviceSelectorFromPairingState(true).map_err(werr)?;
        let found = DeviceInformation::FindAllAsyncAqsFilter(&selector)
            .map_err(werr)?
            .join()
            .map_err(werr)?;
        let mut out = Vec::new();
        for info in found {
            let id = info.Id().map_err(werr)?;
            let dev = match BluetoothDevice::FromIdAsync(&id).and_then(|op| op.join()) {
                Ok(d) => d,
                Err(e) => {
                    log::warn!("skipping {id}: {e}");
                    continue;
                }
            };
            let address = format_address(dev.BluetoothAddress().map_err(werr)?);
            // With a Windows phone line, Windows holds the hands-free link itself and our own raw
            // link must stay out of its way (a phone takes one hands-free connection).
            let windows_line = crate::winline::available(&address);
            let connected = dev.ConnectionStatus().map_err(werr)?
                == BluetoothConnectionStatus::Connected
                || crate::hfp::is_up(&address);
            if !connected && !windows_line && !crate::winline::is_packaged() {
                crate::hfp::reconnect_in_background(&address);
            }
            out.push(PhoneInfo {
                address,
                name: dev.Name().map_err(werr)?.to_string(),
                connected,
                battery: None,
                apple: false,
            });
        }
        Ok(out)
    }
    fn open_map(&self, a: &str) -> Result<Box<dyn Stream>> {
        Ok(Box::new(RfcommStream::connect(a, MAP_MAS)?))
    }
    fn open_pbap(&self, a: &str) -> Result<Box<dyn Stream>> {
        Ok(Box::new(RfcommStream::connect(a, PBAP_PSE)?))
    }
    // Windows' own phone line when it has one (the voice can move to this computer); our raw
    // hands-free client otherwise (call control only).
    fn calls(&self, a: &str) -> Result<Vec<CallSnapshot>> {
        if crate::winline::available(a) {
            crate::winline::calls(a)
        } else {
            crate::hfp::calls(a)
        }
    }
    fn dial(&self, a: &str, n: &str) -> Result<()> {
        if crate::winline::available(a) {
            crate::winline::dial(a, n)
        } else {
            crate::hfp::dial(a, n)
        }
    }
    fn answer(&self, a: &str, c: &str) -> Result<()> {
        if crate::winline::available(a) {
            crate::winline::answer(c)
        } else {
            crate::hfp::answer(a, c)
        }
    }
    fn hang_up(&self, a: &str, c: &str) -> Result<()> {
        if crate::winline::available(a) {
            crate::winline::hang_up(c)
        } else {
            crate::hfp::hang_up(a, c)
        }
    }
    fn hang_up_all(&self, a: &str) -> Result<()> {
        if crate::winline::available(a) {
            crate::winline::hang_up_all(a)
        } else {
            crate::hfp::hang_up_all(a)
        }
    }
    fn send_tones(&self, a: &str, t: &str) -> Result<()> {
        if crate::winline::available(a) {
            crate::winline::send_tones(a, t)
        } else {
            crate::hfp::send_tones(a, t)
        }
    }
}
