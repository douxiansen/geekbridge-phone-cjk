// SPDX-License-Identifier: GPL-3.0-or-later
//! Is Bluetooth on? The app checks this before scanning or pairing and tells the user plainly.
use phone_core::platform::{PlatformError, Result};
use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioKind, RadioState};

/// What the computer's Bluetooth radio is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BluetoothState {
    On,
    /// Switched off in Windows (quick settings, airplane mode).
    Off,
    /// The computer has no Bluetooth adapter, or its driver is missing.
    NoAdapter,
    /// Windows refuses this app access to the radio (Settings > Privacy).
    Denied,
}

fn werr(e: windows::core::Error) -> PlatformError {
    PlatformError::Other(e.to_string())
}

fn bluetooth_radio() -> Result<Option<Radio>> {
    let radios = Radio::GetRadiosAsync()
        .map_err(werr)?
        .join()
        .map_err(werr)?;
    for r in radios {
        if r.Kind().map_err(werr)? == RadioKind::Bluetooth {
            return Ok(Some(r));
        }
    }
    Ok(None)
}

pub fn bluetooth_state() -> Result<BluetoothState> {
    if Radio::RequestAccessAsync()
        .map_err(werr)?
        .join()
        .map_err(werr)?
        == RadioAccessStatus::DeniedByUser
    {
        return Ok(BluetoothState::Denied);
    }
    Ok(match bluetooth_radio()? {
        None => BluetoothState::NoAdapter,
        Some(r) if r.State().map_err(werr)? == RadioState::On => BluetoothState::On,
        Some(_) => BluetoothState::Off,
    })
}

/// Asks Windows to switch Bluetooth on. The app should offer this as a button, never do it
/// silently.
pub fn turn_on_bluetooth() -> Result<()> {
    let Some(r) = bluetooth_radio()? else {
        return Err(PlatformError::Other(
            "this computer has no Bluetooth adapter".into(),
        ));
    };
    match r
        .SetStateAsync(RadioState::On)
        .map_err(werr)?
        .join()
        .map_err(werr)?
    {
        RadioAccessStatus::Allowed => Ok(()),
        other => Err(PlatformError::Other(format!(
            "Windows would not turn Bluetooth on: {other:?}"
        ))),
    }
}
