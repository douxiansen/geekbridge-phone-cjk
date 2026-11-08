// SPDX-License-Identifier: GPL-3.0-or-later
//! The D-Bus side of the Linux backend: BlueZ for phones, PipeWire's
//! telephony service for calls.

use phone_core::calls::{CallSnapshot, CallState};
use phone_core::platform::{PhoneInfo, PlatformError, Result};
use std::collections::HashMap;
use zbus::blocking::Connection;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

type Props = HashMap<String, OwnedValue>;
type Managed = HashMap<OwnedObjectPath, HashMap<String, Props>>;

const BLUEZ: &str = "org.bluez";
const TELEPHONY: &str = "org.pipewire.Telephony";
const TELEPHONY_ROOT: &str = "/org/pipewire/Telephony";
const AG_IFACE: &str = "org.pipewire.Telephony.AudioGateway1";
const CALL_MGR_IFACE: &str = "org.ofono.VoiceCallManager";
const CALL_IFACE: &str = "org.ofono.VoiceCall";
const HFP_AG_UUID: &str = "0000111f-0000-1000-8000-00805f9b34fb";

fn other(e: impl std::fmt::Display) -> PlatformError {
    PlatformError::Other(e.to_string())
}

fn get_str(p: &Props, k: &str) -> Option<String> {
    p.get(k).and_then(|v| String::try_from(v.clone()).ok())
}

fn get_bool(p: &Props, k: &str) -> bool {
    p.get(k)
        .and_then(|v| bool::try_from(v.clone()).ok())
        .unwrap_or(false)
}

fn managed(conn: &Connection, service: &str, root: &str) -> Result<Managed> {
    let reply = conn
        .call_method(
            Some(service),
            root,
            Some("org.freedesktop.DBus.ObjectManager"),
            "GetManagedObjects",
            &(),
        )
        .map_err(other)?;
    reply.body().deserialize::<Managed>().map_err(other)
}

/// Paired phones known to BlueZ.
pub fn phones(system: &Connection) -> Result<Vec<PhoneInfo>> {
    let objects = managed(system, BLUEZ, "/")?;
    let mut out = Vec::new();
    for ifaces in objects.values() {
        let Some(dev) = ifaces.get("org.bluez.Device1") else {
            continue;
        };
        if !get_bool(dev, "Paired") {
            continue;
        }
        let uuids: Vec<String> = dev
            .get("UUIDs")
            .and_then(|v| Vec::<String>::try_from(v.clone()).ok())
            .unwrap_or_default();
        let is_phone = get_str(dev, "Icon").as_deref() == Some("phone")
            || uuids.iter().any(|u| u.eq_ignore_ascii_case(HFP_AG_UUID));
        if !is_phone {
            continue;
        }
        let Some(address) = get_str(dev, "Address") else {
            continue;
        };
        let battery = ifaces
            .get("org.bluez.Battery1")
            .and_then(|b| b.get("Percentage"))
            .and_then(|v| u8::try_from(v.clone()).ok());
        out.push(PhoneInfo {
            name: get_str(dev, "Alias")
                .or_else(|| get_str(dev, "Name"))
                .unwrap_or_else(|| address.clone()),
            address,
            connected: get_bool(dev, "Connected"),
            battery,
        });
    }
    out.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(out)
}

/// The audio-gateway object path for the phone at `address`.
pub fn gateway_path(session: &Connection, address: &str) -> Result<String> {
    let objects = managed(session, TELEPHONY, TELEPHONY_ROOT)?;
    for (path, ifaces) in objects {
        if let Some(ag) = ifaces.get(AG_IFACE) {
            if get_str(ag, "Address").is_some_and(|a| a.eq_ignore_ascii_case(address)) {
                return Ok(path.to_string());
            }
        }
    }
    Err(other(
        "the phone is not connected for calls (no hands-free link)",
    ))
}

/// The calls the phone has now.
pub fn calls(session: &Connection, address: &str) -> Result<Vec<CallSnapshot>> {
    let path = gateway_path(session, address)?;
    let reply = session
        .call_method(
            Some(TELEPHONY),
            path.as_str(),
            Some(CALL_MGR_IFACE),
            "GetCalls",
            &(),
        )
        .map_err(other)?;
    let calls: HashMap<OwnedObjectPath, Props> = reply.body().deserialize().map_err(other)?;
    let mut out: Vec<CallSnapshot> = calls
        .into_iter()
        .map(|(id, p)| CallSnapshot {
            id: id.to_string(),
            state: CallState::parse(&get_str(&p, "State").unwrap_or_default()),
            number: get_str(&p, "LineIdentification")
                .filter(|n| !n.is_empty())
                .or_else(|| get_str(&p, "IncomingLine"))
                .unwrap_or_default(),
            name: get_str(&p, "Name").filter(|n| !n.is_empty()),
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

pub fn dial(session: &Connection, address: &str, number: &str) -> Result<()> {
    let path = gateway_path(session, address)?;
    session
        .call_method(
            Some(TELEPHONY),
            path.as_str(),
            Some(AG_IFACE),
            "Dial",
            &(number,),
        )
        .map(|_| ())
        .map_err(other)
}

pub fn call_method(session: &Connection, call_path: &str, method: &str) -> Result<()> {
    // Hangup/Answer on the ofono-compatible interface is the one that works.
    session
        .call_method(Some(TELEPHONY), call_path, Some(CALL_IFACE), method, &())
        .map(|_| ())
        .map_err(other)
}

pub fn hang_up_all(session: &Connection, address: &str) -> Result<()> {
    let path = gateway_path(session, address)?;
    session
        .call_method(
            Some(TELEPHONY),
            path.as_str(),
            Some(AG_IFACE),
            "HangupAll",
            &(),
        )
        .map(|_| ())
        .map_err(other)
}

/// The A2DP *source* service a phone offers: the profile through which its music and videos
/// reach this computer.
const A2DP_SOURCE_UUID: &str = "0000110a-0000-1000-8000-00805f9b34fb";

/// Disconnects the phone's media-audio profile, leaving hands-free calls alone. BlueZ answers
/// with an error when the profile is not connected, which is the normal case and not a failure.
pub fn block_media_audio(system: &Connection, address: &str) -> Result<()> {
    let objects = managed(system, BLUEZ, "/")?;
    let path = objects.iter().find_map(|(path, ifaces)| {
        let dev = ifaces.get("org.bluez.Device1")?;
        get_str(dev, "Address")
            .is_some_and(|a| a.eq_ignore_ascii_case(address))
            .then(|| path.to_string())
    });
    let Some(path) = path else { return Ok(()) };
    match system.call_method(
        Some(BLUEZ),
        path.as_str(),
        Some("org.bluez.Device1"),
        "DisconnectProfile",
        &(A2DP_SOURCE_UUID,),
    ) {
        Ok(_) => Ok(()),
        Err(zbus::Error::MethodError(name, _, _))
            if name.as_str().ends_with("NotConnected")
                || name.as_str().ends_with("DoesNotExist")
                || name.as_str().ends_with("Failed") =>
        {
            Ok(())
        }
        Err(e) => Err(other(e)),
    }
}

/// Asks the gateway to open the audio link to this computer.
pub fn activate_audio(session: &Connection, address: &str) -> Result<()> {
    let path = gateway_path(session, address)?;
    session
        .call_method(
            Some(TELEPHONY),
            path.as_str(),
            Some("org.pipewire.Telephony.AudioGatewayTransport1"),
            "Activate",
            &(),
        )
        .map(|_| ())
        .map_err(other)
}

pub fn send_tones(session: &Connection, address: &str, tones: &str) -> Result<()> {
    let path = gateway_path(session, address)?;
    session
        .call_method(
            Some(TELEPHONY),
            path.as_str(),
            Some(AG_IFACE),
            "SendTones",
            &(tones,),
        )
        .map(|_| ())
        .map_err(other)
}
