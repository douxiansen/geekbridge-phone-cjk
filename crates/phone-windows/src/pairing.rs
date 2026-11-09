// SPDX-License-Identifier: GPL-3.0-or-later
//! Find nearby Bluetooth devices and pair with one from inside the app, so the user never has
//! to open Windows Settings.
use phone_core::platform::{PlatformError, Result};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::core::{IInspectable, Ref};
use windows::Devices::Enumeration::{
    DeviceInformation, DeviceInformationCustomPairing, DeviceInformationKind,
    DeviceInformationUpdate, DevicePairingKinds, DevicePairingRequestedEventArgs,
    DevicePairingResultStatus, DeviceWatcher,
};
use windows::Foundation::TypedEventHandler;

fn werr(e: windows::core::Error) -> PlatformError {
    PlatformError::Other(e.to_string())
}

fn require_bluetooth() -> Result<()> {
    use crate::radio::{bluetooth_state, BluetoothState::*};
    match bluetooth_state()? {
        On => Ok(()),
        Off => Err(PlatformError::Other(
            "Bluetooth is turned off on this computer".into(),
        )),
        NoAdapter => Err(PlatformError::Other(
            "this computer has no Bluetooth adapter".into(),
        )),
        Denied => Err(PlatformError::Other(
            "Windows blocks this app from using Bluetooth".into(),
        )),
    }
}

/// A Bluetooth device in range.
#[derive(Debug, Clone)]
pub struct NearbyDevice {
    /// Windows' device id; pass it to [`pair`].
    pub id: String,
    pub name: String,
    pub paired: bool,
}

/// Scans for `duration` and returns the named, not yet paired devices Windows heard.
pub fn scan(duration: Duration) -> Result<Vec<NearbyDevice>> {
    require_bluetooth()?;
    // Association endpoints are what Windows' own "Add a device" screen lists, and starting
    // such a watcher makes the radio run a Bluetooth inquiry. Classic Bluetooth only: phones
    // pair for calls and texts over BR/EDR.
    let filter = "System.Devices.Aep.ProtocolId:=\"{e0cbf06c-cd8b-4647-bb8a-263b43f0f974}\"";
    let watcher = DeviceInformation::CreateWatcherWithKindAqsFilterAndAdditionalProperties(
        &filter.into(),
        None,
        DeviceInformationKind::AssociationEndpoint,
    )
    .map_err(werr)?;
    let seen: Arc<Mutex<BTreeMap<String, NearbyDevice>>> = Default::default();
    let sink = seen.clone();
    watcher
        .Added(&TypedEventHandler::new(
            move |_: Ref<DeviceWatcher>, info: Ref<DeviceInformation>| {
                if let Some(info) = info.as_ref() {
                    let name = info.Name()?.to_string();
                    let id = info.Id()?.to_string();
                    // Unnamed entries are usually beacons and other noise.
                    if !name.trim().is_empty() {
                        let paired = info.Pairing()?.IsPaired()?;
                        sink.lock()
                            .unwrap()
                            .insert(id.clone(), NearbyDevice { id, name, paired });
                    }
                }
                Ok(())
            },
        ))
        .map_err(werr)?;
    // Windows only delivers `Added` results once the watcher's other events have handlers too
    // (found by experiment: without these the scan stays silent).
    watcher
        .Updated(&TypedEventHandler::new(
            |_: Ref<DeviceWatcher>, _: Ref<DeviceInformationUpdate>| Ok(()),
        ))
        .map_err(werr)?;
    watcher
        .EnumerationCompleted(&TypedEventHandler::new(
            |_: Ref<DeviceWatcher>, _: Ref<IInspectable>| Ok(()),
        ))
        .map_err(werr)?;
    watcher
        .Stopped(&TypedEventHandler::new(
            |_: Ref<DeviceWatcher>, _: Ref<IInspectable>| Ok(()),
        ))
        .map_err(werr)?;
    watcher.Start().map_err(werr)?;
    let end = Instant::now() + duration;
    while Instant::now() < end {
        std::thread::sleep(Duration::from_millis(200));
    }
    let _ = watcher.Stop();
    let found = seen.lock().unwrap().values().cloned().collect();
    Ok(found)
}

/// Pairs with the device `id`. `confirm` is called with the number Windows shows (when there is
/// one) and returns whether the user saw the same number on the phone and agrees.
pub fn pair(
    id: &str,
    confirm: impl Fn(Option<&str>) -> bool + Send + Sync + 'static,
) -> Result<()> {
    require_bluetooth()?;
    let info = DeviceInformation::CreateFromIdAsync(&id.into())
        .map_err(werr)?
        .join()
        .map_err(werr)?;
    let pairing = info.Pairing().map_err(werr)?;
    if pairing.IsPaired().map_err(werr)? {
        return Ok(());
    }
    let custom: DeviceInformationCustomPairing = pairing.Custom().map_err(werr)?;
    custom
        .PairingRequested(&TypedEventHandler::new(
            move |_: Ref<DeviceInformationCustomPairing>,
                  args: Ref<DevicePairingRequestedEventArgs>| {
                if let Some(args) = args.as_ref() {
                    let pin = match args.PairingKind()? {
                        DevicePairingKinds::ConfirmPinMatch | DevicePairingKinds::DisplayPin => {
                            Some(args.Pin()?.to_string())
                        }
                        _ => None,
                    };
                    if confirm(pin.as_deref()) {
                        args.Accept()?;
                    }
                }
                Ok(())
            },
        ))
        .map_err(werr)?;
    let kinds = DevicePairingKinds::ConfirmOnly
        | DevicePairingKinds::ConfirmPinMatch
        | DevicePairingKinds::DisplayPin;
    let result = custom
        .PairAsync(kinds)
        .map_err(werr)?
        .join()
        .map_err(werr)?;
    match result.Status().map_err(werr)? {
        DevicePairingResultStatus::Paired | DevicePairingResultStatus::AlreadyPaired => Ok(()),
        other => Err(PlatformError::Other(format!("pairing failed: {other:?}"))),
    }
}

/// Removes the pairing with the device `id`, so it can be paired again from scratch.
pub fn unpair(id: &str) -> Result<()> {
    let info = DeviceInformation::CreateFromIdAsync(&id.into())
        .map_err(werr)?
        .join()
        .map_err(werr)?;
    let result = info
        .Pairing()
        .map_err(werr)?
        .UnpairAsync()
        .map_err(werr)?
        .join()
        .map_err(werr)?;
    match result.Status().map_err(werr)? {
        windows::Devices::Enumeration::DeviceUnpairingResultStatus::Unpaired
        | windows::Devices::Enumeration::DeviceUnpairingResultStatus::AlreadyUnpaired => Ok(()),
        other => Err(PlatformError::Other(format!("could not unpair: {other:?}"))),
    }
}

/// Windows' device id of the paired phone with this Bluetooth address, if any.
pub fn paired_id(address: &str) -> Result<Option<String>> {
    let hex = address.to_lowercase();
    let sel = windows::Devices::Bluetooth::BluetoothDevice::GetDeviceSelectorFromPairingState(true)
        .map_err(werr)?;
    let found = DeviceInformation::FindAllAsyncAqsFilter(&sel)
        .map_err(werr)?
        .join()
        .map_err(werr)?;
    for d in found {
        let id = d.Id().map_err(werr)?.to_string();
        if id.to_lowercase().contains(&hex) {
            return Ok(Some(id));
        }
    }
    Ok(None)
}
