// SPDX-License-Identifier: GPL-3.0-or-later
use crate::rfcomm::{RfcommStream, MAP_MAS, PBAP_PSE};
use phone_core::calls::CallSnapshot;
use phone_core::platform::{NearbyDevice, PhoneInfo, Platform, PlatformError, Result, Stream};
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

/// Bluetooth SIG's company id for Apple, as Windows writes it in a device's id: `VID&0001` (the
/// Bluetooth SIG numbering) followed by the company id `004C`.
const APPLE_VID: &str = "VID&0001004C";

/// Whether a Windows device id belongs to the phone at `hex_address` and carries Apple's vendor id.
fn id_is_apple(id: &str, hex_address: &str) -> bool {
    let id = id.to_uppercase();
    id.contains(hex_address) && id.contains(APPLE_VID)
}

/// Ids of the phones' call-link transports. They name the phone's address and vendor, which is how
/// an iPhone is told apart (the Linux version reads the same fact from BlueZ's modalias).
/// Looked up at most every 20 seconds: the phone list is asked for often.
fn transport_ids() -> Vec<String> {
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};
    use windows::ApplicationModel::Calls::{PhoneLineTransport, PhoneLineTransportDevice};
    type Cached = Mutex<Option<(Instant, Vec<String>)>>;
    static CACHE: OnceLock<Cached> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some((at, ids)) = cache.lock().unwrap().as_ref() {
        if at.elapsed() < Duration::from_secs(20) {
            return ids.clone();
        }
    }
    let ids: Vec<String> = PhoneLineTransportDevice::GetDeviceSelectorForPhoneLineTransport(
        PhoneLineTransport::Bluetooth,
    )
    .and_then(|sel| DeviceInformation::FindAllAsyncAqsFilter(&sel))
    .and_then(|op| op.join())
    .map(|found| {
        found
            .into_iter()
            .filter_map(|d| d.Id().ok().map(|i| i.to_string()))
            .collect()
    })
    .unwrap_or_default();
    *cache.lock().unwrap() = Some((Instant::now(), ids.clone()));
    ids
}

/// Whether the paired phone is an iPhone: Apple's vendor id in its device ids, or "iPhone" in the
/// name it gives itself.
fn is_apple(address: &str, name: &str) -> bool {
    if name.to_lowercase().contains("iphone") {
        return true;
    }
    let hex = address.replace(':', "").to_uppercase();
    transport_ids().iter().any(|id| id_is_apple(id, &hex))
}

/// The phone's address in a Windows device id such as `Bluetooth#Bluetooth90:e8:...-6c:ac:...`
/// (this computer's adapter, a dash, the phone).
fn address_from_device_id(id: &str) -> Option<String> {
    let tail = id.rsplit('-').next()?;
    let parts: Vec<&str> = tail.split(':').collect();
    let ok = parts.len() == 6
        && parts
            .iter()
            .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()));
    ok.then(|| tail.to_uppercase())
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
            let name = dev.Name().map_err(werr)?.to_string();
            let apple = is_apple(&address, &name);
            out.push(PhoneInfo {
                address,
                name,
                connected,
                battery: None,
                apple,
            });
        }
        Ok(out)
    }
    fn computer_name(&self) -> Option<String> {
        // Windows names its Bluetooth radio after the computer; that is what a phone lists.
        std::env::var("COMPUTERNAME")
            .ok()
            .filter(|n| !n.trim().is_empty())
    }
    fn can_pair(&self) -> bool {
        true
    }
    fn scan_nearby(&self, seconds: u32) -> Result<Vec<NearbyDevice>> {
        let found = crate::pairing::scan(std::time::Duration::from_secs(u64::from(seconds)))?;
        Ok(found
            .into_iter()
            .filter(|d| !d.paired)
            .map(|d| NearbyDevice {
                id: d.id,
                name: d.name,
            })
            .collect())
    }
    fn pair(
        &self,
        id: &str,
        show_code: std::sync::Arc<dyn Fn(Option<String>) + Send + Sync>,
    ) -> Result<()> {
        // The phone asks its owner to confirm the same number; this side shows it and accepts,
        // because the user started the pairing here by picking the phone.
        crate::pairing::pair(id, move |pin| {
            show_code(pin.map(str::to_string));
            true
        })?;
        // The phone only keeps "Calls" switched on once this computer's call link is up, so set
        // it up right away instead of waiting for the next check. Windows needs a few seconds
        // after pairing before it offers the link.
        if let Some(address) = address_from_device_id(id) {
            if crate::winline::is_packaged() {
                for _ in 0..15 {
                    if crate::winline::ensure_registered(&address).is_ok() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
            }
        }
        Ok(())
    }
    fn unpair(&self, address: &str) -> Result<()> {
        match crate::pairing::paired_id(address)? {
            Some(id) => crate::pairing::unpair(&id),
            None => Ok(()),
        }
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

#[cfg(test)]
mod tests {
    use super::{address_from_device_id, id_is_apple};

    #[test]
    fn the_phone_address_is_the_last_part_of_the_device_id() {
        assert_eq!(
            address_from_device_id("Bluetooth#Bluetooth90:e8:68:0f:30:72-6c:ac:c2:5c:46:da")
                .as_deref(),
            Some("6C:AC:C2:5C:46:DA")
        );
        assert_eq!(address_from_device_id("nonsense"), None);
    }

    #[test]
    fn apple_is_told_by_the_vendor_id_in_the_phone_s_own_device_id() {
        let iphone = r"\\?\BTHENUM#{0000111f-0000-1000-8000-00805f9b34fb}_VID&0001004c_PID&12a8#9&2e5a28ba&1&AABBCCDDEEFF_C00000000#{bd41df2d}\service";
        let samsung = r"\\?\BTHENUM#{0000111f-0000-1000-8000-00805f9b34fb}_VID&00010075_PID&0100#9&2e5a28ba&1&6CACC25C46DA_C00000000#{bd41df2d}\service";
        assert!(id_is_apple(iphone, "AABBCCDDEEFF"));
        assert!(!id_is_apple(samsung, "6CACC25C46DA"));
        // Another phone's address does not make this phone Apple.
        assert!(!id_is_apple(iphone, "6CACC25C46DA"));
    }
}
