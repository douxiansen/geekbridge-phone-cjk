// SPDX-License-Identifier: GPL-3.0-or-later

//! Light/dark detection from the freedesktop settings portal
//! (`org.freedesktop.appearance` / `color-scheme`).
//!
//! The portal may be missing, slow to start or absent altogether, so nothing
//! here ever runs on the presenter thread: a small helper thread reads the
//! value, publishes it in an atomic, and pings the presenter when it changes.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Weak};

use smithay_client_toolkit::reexports::calloop::ping::Ping;
use zbus::blocking::{Connection, MessageIterator};
use zbus::zvariant::{OwnedValue, Value};
use zbus::MatchRule;

const DEST: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const IFACE: &str = "org.freedesktop.portal.Settings";
const NAMESPACE: &str = "org.freedesktop.appearance";
const KEY: &str = "color-scheme";

/// What the desktop last reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    /// Nothing reported (yet) or "no preference".
    Unknown,
    /// Dark preferred.
    Dark,
    /// Light preferred.
    Light,
}

/// A shared, lock-free view of the desktop's colour scheme.
#[derive(Clone, Default)]
pub struct SchemeCell(Arc<AtomicU8>);

impl SchemeCell {
    /// The latest scheme.
    pub fn get(&self) -> Scheme {
        decode(self.0.load(Ordering::Relaxed))
    }

    /// `Some(true)` for dark, `Some(false)` for light, `None` if unknown.
    pub fn is_dark(&self) -> Option<bool> {
        match self.get() {
            Scheme::Unknown => None,
            Scheme::Dark => Some(true),
            Scheme::Light => Some(false),
        }
    }

    fn set(&self, s: Scheme) -> bool {
        let v = match s {
            Scheme::Unknown => 0,
            Scheme::Dark => 1,
            Scheme::Light => 2,
        };
        self.0.swap(v, Ordering::Relaxed) != v
    }

    fn weak(&self) -> Weak<AtomicU8> {
        Arc::downgrade(&self.0)
    }
}

fn decode(v: u8) -> Scheme {
    match v {
        1 => Scheme::Dark,
        2 => Scheme::Light,
        _ => Scheme::Unknown,
    }
}

/// Map the portal's `color-scheme` number: 1 prefers dark, 2 prefers light.
pub fn scheme_from_portal(v: u32) -> Scheme {
    match v {
        1 => Scheme::Dark,
        2 => Scheme::Light,
        _ => Scheme::Unknown,
    }
}

fn as_u32(v: &Value<'_>) -> Option<u32> {
    match v {
        Value::U32(n) => Some(*n),
        Value::Value(inner) => as_u32(inner),
        _ => None,
    }
}

fn read_current(conn: &Connection) -> Option<Scheme> {
    let reply = conn
        .call_method(Some(DEST), PATH, Some(IFACE), "ReadOne", &(NAMESPACE, KEY))
        .or_else(|_| conn.call_method(Some(DEST), PATH, Some(IFACE), "Read", &(NAMESPACE, KEY)))
        .ok()?;
    let v: OwnedValue = reply.body().deserialize().ok()?;
    as_u32(&v).map(scheme_from_portal)
}

/// Start watching the portal. `ping` is signalled whenever the scheme changes.
/// The helper thread exits on its own once the returned cell is dropped (at
/// the next portal signal) and does nothing at all when there is no portal.
pub fn watch(ping: Ping) -> SchemeCell {
    let cell = SchemeCell::default();
    let weak = cell.weak();
    let shared = cell.clone();
    let spawned = std::thread::Builder::new()
        .name("phone-overlay-portal".into())
        .spawn(move || {
            let Ok(conn) = Connection::session() else {
                return;
            };
            if let Some(s) = read_current(&conn) {
                if shared.set(s) {
                    ping.ping();
                }
            }
            // The cell is only kept alive by the presenter from here on.
            drop(shared);
            let Ok(rule) = MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .interface(IFACE)
                .and_then(|b| b.member("SettingChanged"))
                .map(|b| b.build())
            else {
                return;
            };
            let Ok(iter) = MessageIterator::for_match_rule(rule, &conn, Some(8)) else {
                return;
            };
            for msg in iter.flatten() {
                let Some(cell) = weak.upgrade() else { return };
                let Ok((ns, key, value)) = msg.body().deserialize::<(String, String, OwnedValue)>()
                else {
                    continue;
                };
                if ns == NAMESPACE && key == KEY {
                    if let Some(n) = as_u32(&value) {
                        let new = SchemeCell(cell);
                        if new.set(scheme_from_portal(n)) {
                            ping.ping();
                        }
                    }
                }
            }
        });
    if spawned.is_err() {
        // No helper thread: stay on the dark default.
    }
    cell
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portal_values_map() {
        assert_eq!(scheme_from_portal(0), Scheme::Unknown);
        assert_eq!(scheme_from_portal(1), Scheme::Dark);
        assert_eq!(scheme_from_portal(2), Scheme::Light);
        assert_eq!(scheme_from_portal(9), Scheme::Unknown);
    }

    #[test]
    fn cell_reports_changes_once() {
        let c = SchemeCell::default();
        assert_eq!(c.is_dark(), None);
        assert!(c.set(Scheme::Light));
        assert!(!c.set(Scheme::Light));
        assert_eq!(c.is_dark(), Some(false));
        assert!(c.set(Scheme::Dark));
        assert_eq!(c.is_dark(), Some(true));
    }

    #[test]
    fn nested_variants_unwrap() {
        let inner = Value::U32(1);
        let nested = Value::Value(Box::new(Value::Value(Box::new(inner))));
        assert_eq!(as_u32(&nested), Some(1));
        assert_eq!(as_u32(&Value::Bool(true)), None);
    }
}
