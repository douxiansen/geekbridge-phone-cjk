// SPDX-License-Identifier: GPL-3.0-or-later
//! Windows platform layer: paired phones, pairing from inside the app, the message and phonebook
//! services over RFCOMM, and calls with their voice.
//!
//! Calls use the phone line Windows itself creates when an installed (packaged) app registers
//! as a call handler for the phone's hands-free link; see [`winline`] and docs/WINDOWS.md. An
//! unpackaged program falls back to [`hfp`], a small hands-free client that controls calls but
//! cannot carry their voice.

#[cfg(windows)]
mod hfp;
#[cfg(windows)]
mod pairing;
#[cfg(windows)]
mod platform;
#[cfg(windows)]
mod radio;
#[cfg(windows)]
mod rfcomm;
#[cfg(windows)]
mod winline;

#[cfg(windows)]
pub use pairing::{pair, paired_id, scan, unpair, NearbyDevice};
#[cfg(windows)]
pub use platform::WindowsPlatform;
#[cfg(windows)]
pub use radio::{bluetooth_state, turn_on_bluetooth, BluetoothState};
#[cfg(windows)]
pub use rfcomm::{RfcommStream, MAP_MAS, PBAP_PSE};
#[cfg(windows)]
pub use winline::{ensure_registered, is_packaged};
