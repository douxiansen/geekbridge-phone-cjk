// SPDX-License-Identifier: GPL-3.0-or-later
//! Calls through the phone line Windows itself holds for a paired phone (the line Phone Link's
//! Calls setup creates). Windows keeps the hands-free link and carries the voice, so the call
//! audio can be moved to this computer, which our own raw hands-free link could never do.
//!
//! A plain (unpackaged) program may open that line and control its calls. Without the line
//! (Phone Link's Calls never set up, or the phone not paired for calls) [`available`] says so
//! and the caller falls back to the raw hands-free client in [`crate::hfp`].
use phone_core::calls::{CallSnapshot, CallState};
use phone_core::platform::{PlatformError, Result};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use windows::core::{s, w, GUID};
use windows::ApplicationModel::Calls::{
    DtmfKey, DtmfToneAudioPlayback, PhoneCall, PhoneCallAudioDevice, PhoneCallManager,
    PhoneCallOperationStatus, PhoneCallStatus, PhoneLine, PhoneLineTransport,
    PhoneLineTransportDevice,
};
use windows::Devices::Enumeration::{DeviceAccessStatus, DeviceInformation};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

fn werr(e: windows::core::Error) -> PlatformError {
    PlatformError::Other(e.to_string())
}

fn other(s: impl Into<String>) -> PlatformError {
    PlatformError::Other(s.into())
}

fn hex_address(address: &str) -> String {
    address.replace(':', "").to_uppercase()
}

fn lines_cache() -> &'static Mutex<HashMap<String, PhoneLine>> {
    static C: OnceLock<Mutex<HashMap<String, PhoneLine>>> = OnceLock::new();
    C.get_or_init(Default::default)
}

/// Calls whose voice we already asked Windows to move to this computer (once per call).
fn moved() -> &'static Mutex<HashSet<String>> {
    static M: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    M.get_or_init(Default::default)
}

/// Line ids Windows knows: the one the phone object model lists (an undocumented export, so
/// every step is checked), plus the default line from the public call store.
fn candidate_line_ids() -> Vec<GUID> {
    let mut ids = Vec::new();
    unsafe {
        if let Ok(lib) = LoadLibraryW(w!("PhoneOm.dll")) {
            if let (Some(init), Some(get)) = (
                GetProcAddress(lib, s!("PhoneAPIInitialize")),
                GetProcAddress(lib, s!("PhoneGetLines")),
            ) {
                let init: unsafe extern "system" fn() -> i32 = std::mem::transmute(init);
                let get: unsafe extern "system" fn(*mut u64, *mut u64) -> i32 =
                    std::mem::transmute(get);
                if init() >= 0 {
                    // (array pointer, count) come back through two 8-byte slots.
                    let (mut array, mut count) = ([0u64; 4], [0u64; 4]);
                    if get(array.as_mut_ptr(), count.as_mut_ptr()) >= 0
                        && array[0] > 0x10000
                        && (count[0] & 0xffff_ffff) >= 1
                    {
                        let raw = std::slice::from_raw_parts(array[0] as *const u8, 16);
                        ids.push(GUID::from_values(
                            u32::from_le_bytes(raw[0..4].try_into().unwrap()),
                            u16::from_le_bytes(raw[4..6].try_into().unwrap()),
                            u16::from_le_bytes(raw[6..8].try_into().unwrap()),
                            raw[8..16].try_into().unwrap(),
                        ));
                    }
                }
            }
        }
    }
    if let Ok(store) = PhoneCallManager::RequestStoreAsync().and_then(|o| o.join()) {
        if let Ok(id) = store.GetDefaultLineAsync().and_then(|o| o.join()) {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids
}

/// The phone's call-link transport, if Windows lets this program use it.
fn open_transport(hex: &str) -> Result<PhoneLineTransportDevice> {
    let selector = PhoneLineTransportDevice::GetDeviceSelectorForPhoneLineTransport(
        PhoneLineTransport::Bluetooth,
    )
    .map_err(werr)?;
    let found = DeviceInformation::FindAllAsyncAqsFilter(&selector)
        .map_err(werr)?
        .join()
        .map_err(werr)?;
    for d in found {
        let id = d.Id().map_err(werr)?;
        if !id.to_string().to_uppercase().contains(hex) {
            continue;
        }
        let transport = PhoneLineTransportDevice::FromId(&id).map_err(werr)?;
        return match transport
            .RequestAccessAsync()
            .map_err(werr)?
            .join()
            .map_err(werr)?
        {
            DeviceAccessStatus::Allowed => Ok(transport),
            DeviceAccessStatus::DeniedBySystem => Err(other(
                "Windows blocks call control: the app must run as an installed (packaged) app",
            )),
            denied => Err(other(format!("call access refused: {denied:?}"))),
        };
    }
    Err(PlatformError::NoService)
}

/// Whether this program runs as an installed (packaged) app. Only such an app can register for
/// the phone's call link; an unpackaged program keeps the raw hands-free fallback.
pub fn is_packaged() -> bool {
    use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
    let mut len = 0u32;
    // APPMODEL_ERROR_NO_PACKAGE (15700) means "no package identity"; anything else has one.
    let r = unsafe { GetCurrentPackageFullName(&mut len, None) };
    r.0 != 15700
}

/// Makes Windows create the phone line for this phone: register this app as a call handler for
/// the phone's hands-free link and connect it, the same steps Phone Link takes. Only works for
/// an installed (packaged) app that declares `windows.phoneCallActivation`; elsewhere Windows
/// says no and the caller keeps its fallback.
pub fn ensure_registered(address: &str) -> Result<()> {
    let transport = open_transport(&hex_address(address))?;
    let was_registered = transport.IsRegistered().map_err(werr)?;
    if !was_registered {
        transport.RegisterApp().map_err(werr)?;
    }
    let connected = transport
        .ConnectAsync()
        .map_err(werr)?
        .join()
        .map_err(werr)?;
    log::info!(
        "call link for {address}: registered before {was_registered}, now {:?}, connected {connected}, audio routing {:?}",
        transport.IsRegistered().ok(),
        transport.AudioRoutingStatus().ok()
    );
    if connected {
        Ok(())
    } else {
        Err(other("Windows could not bring up the phone's call link"))
    }
}

fn find_line(address: &str) -> Option<PhoneLine> {
    // Rate limited: asking Windows to register and connect is not free.
    static LAST: OnceLock<Mutex<HashMap<String, std::time::Instant>>> = OnceLock::new();
    {
        let mut last = LAST.get_or_init(Default::default).lock().unwrap();
        let due = last
            .get(address)
            .is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(15));
        if due {
            last.insert(address.to_string(), std::time::Instant::now());
            drop(last);
            if let Err(e) = ensure_registered(address) {
                log::debug!("no Windows phone line for {address} yet: {e}");
            }
        }
    }
    let hex = hex_address(address);
    for id in candidate_line_ids() {
        let Ok(line) = PhoneLine::FromIdAsync(id).and_then(|o| o.join()) else {
            continue;
        };
        let bluetooth = line.Transport().ok() == Some(PhoneLineTransport::Bluetooth);
        let ours = line
            .TransportDeviceId()
            .map(|t| t.to_string().to_uppercase().contains(&hex))
            .unwrap_or(false);
        if bluetooth && ours {
            return Some(line);
        }
    }
    None
}

fn line(address: &str) -> Option<PhoneLine> {
    if let Some(l) = lines_cache().lock().unwrap().get(address) {
        return Some(l.clone());
    }
    let l = find_line(address)?;
    lines_cache()
        .lock()
        .unwrap()
        .insert(address.to_string(), l.clone());
    Some(l)
}

/// Whether Windows holds a phone line for this phone.
pub fn available(address: &str) -> bool {
    line(address).is_some()
}

fn state_of(s: PhoneCallStatus) -> Option<CallState> {
    match s {
        PhoneCallStatus::Incoming => Some(CallState::Incoming),
        PhoneCallStatus::Dialing => Some(CallState::Dialing),
        PhoneCallStatus::Talking => Some(CallState::Active),
        PhoneCallStatus::Held => Some(CallState::Held),
        _ => None,
    }
}

fn active_calls(address: &str) -> Result<Vec<PhoneCall>> {
    let line = line(address).ok_or_else(|| other("Windows has no phone line for this phone"))?;
    let res = line.GetAllActivePhoneCalls().map_err(werr)?;
    Ok(res
        .AllActivePhoneCalls()
        .map_err(werr)?
        .into_iter()
        .collect())
}

fn check(s: PhoneCallOperationStatus) -> Result<()> {
    if s == PhoneCallOperationStatus::Succeeded {
        Ok(())
    } else {
        Err(other(format!("Windows could not do that: {s:?}")))
    }
}

pub fn calls(address: &str) -> Result<Vec<CallSnapshot>> {
    let mut out = Vec::new();
    let mut live = HashSet::new();
    for c in active_calls(address)? {
        let status = c.Status().map_err(werr)?;
        let Some(state) = state_of(status) else {
            continue;
        };
        let id = c.CallId().map_err(werr)?.to_string();
        live.insert(id.clone());
        // Put the voice on this computer once the call is up (Windows leaves it on the phone
        // unless asked).
        if status == PhoneCallStatus::Talking
            && c.AudioDevice().ok() == Some(PhoneCallAudioDevice::RemoteDevice)
            && moved().lock().unwrap().insert(id.clone())
        {
            log::info!("call {id}: moving the voice to this computer");
            let _ = c.ChangeAudioDevice(PhoneCallAudioDevice::LocalDevice);
        }
        let info = c.GetPhoneCallInfo().map_err(werr)?;
        let name = info
            .DisplayName()
            .map(|n| n.to_string())
            .unwrap_or_default();
        out.push(CallSnapshot {
            id,
            state,
            number: info
                .PhoneNumber()
                .map(|n| n.to_string())
                .unwrap_or_default(),
            name: (!name.trim().is_empty()).then_some(name),
        });
    }
    // Forget calls that are over, so a recycled id is handled afresh.
    moved().lock().unwrap().retain(|id| live.contains(id));
    Ok(out)
}

pub fn dial(address: &str, number: &str) -> Result<()> {
    if number.is_empty()
        || !number
            .chars()
            .all(|c| c.is_ascii_digit() || "+*#".contains(c))
    {
        return Err(other("that is not a phone number"));
    }
    let line = line(address).ok_or_else(|| other("Windows has no phone line for this phone"))?;
    let r = line
        .DialWithResult(&number.into(), &"".into())
        .map_err(werr)?;
    check(r.DialCallStatus().map_err(werr)?)
}

pub fn answer(call_id: &str) -> Result<()> {
    let c = PhoneCall::GetFromId(&call_id.into()).map_err(werr)?;
    check(c.AcceptIncoming().map_err(werr)?)
}

pub fn hang_up(call_id: &str) -> Result<()> {
    let c = PhoneCall::GetFromId(&call_id.into()).map_err(werr)?;
    check(if c.Status().map_err(werr)? == PhoneCallStatus::Incoming {
        c.RejectIncoming().map_err(werr)?
    } else {
        c.End().map_err(werr)?
    })
}

pub fn hang_up_all(address: &str) -> Result<()> {
    for c in active_calls(address)? {
        if state_of(c.Status().map_err(werr)?).is_some() {
            check(c.End().map_err(werr)?)?;
        }
    }
    Ok(())
}

pub fn send_tones(address: &str, tones: &str) -> Result<()> {
    let call = active_calls(address)?
        .into_iter()
        .find(|c| c.Status().ok() == Some(PhoneCallStatus::Talking))
        .ok_or_else(|| other("no call in progress"))?;
    for ch in tones.chars() {
        let key = match ch {
            '0'..='9' => DtmfKey(ch as i32 - '0' as i32),
            '*' => DtmfKey::Star,
            '#' => DtmfKey::Pound,
            _ => continue,
        };
        check(
            call.SendDtmfKey(key, DtmfToneAudioPlayback::DoNotPlay)
                .map_err(werr)?,
        )?;
    }
    Ok(())
}
