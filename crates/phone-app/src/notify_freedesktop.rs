// SPDX-License-Identifier: GPL-3.0-or-later
//! The built-in notifier for Linux: posts alerts through the `org.freedesktop.Notifications`
//! D-Bus API, so any notification daemon shows them. With no daemon running, every call quietly
//! does nothing. Buttons on the alerts (Answer, Decline, Open) are routed back through the
//! [`NotifierHost`].

use crate::notify::{
    CallEndedNotice, CallNotice, CallUpdate, Notifier, NotifierHost, NotifyId, TextNotice,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use zbus::blocking::{Connection, MessageIterator};
use zbus::zvariant::Value;

const DEST: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
const APP_NAME: &str = "GeekBridge Phone";
const ICON: &str = "call-start";
/// An incoming-call alert that nobody touched goes away after this long, in case we never hear
/// that the call ended.
const CALL_TIMEOUT_MS: i32 = 45_000;
/// Longest text body shown in an alert.
const BODY_CHARS: usize = 300;

/// What to do when the user presses a button on an alert.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    Call { call_id: String },
    Conversation { number: String },
    Window,
}

type Pending = Arc<Mutex<HashMap<u32, Target>>>;
type Slots = Arc<Mutex<HashMap<NotifyId, u32>>>;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Posts freedesktop notifications.
pub struct FreedesktopNotifier {
    conn: Option<Connection>,
    /// Open alerts, by what they are about, so a newer one can replace the older.
    slots: Slots,
    /// What each open alert's buttons do, by the daemon's id.
    targets: Pending,
}

impl FreedesktopNotifier {
    pub fn new(host: NotifierHost) -> Self {
        let conn = Connection::session()
            .map_err(|e| log::info!("no session bus, no desktop notifications: {e}"))
            .ok();
        let slots: Slots = Arc::default();
        let targets: Pending = Arc::default();
        if let Some(c) = &conn {
            listen(c.clone(), host, targets.clone(), slots.clone());
        }
        Self {
            conn,
            slots,
            targets,
        }
    }

    #[allow(clippy::too_many_arguments)] // one field of the D-Bus call each
    fn post(
        &self,
        slot: NotifyId,
        target: Target,
        summary: &str,
        body: &str,
        actions: &[(&str, &str)],
        timeout_ms: i32,
        category: &str,
    ) {
        let Some(conn) = &self.conn else { return };
        let replaces = lock(&self.slots).get(&slot).copied().unwrap_or(0);
        let flat_actions: Vec<&str> = actions.iter().flat_map(|(k, l)| [*k, *l]).collect();
        let mut hints: HashMap<&str, Value<'_>> = HashMap::new();
        hints.insert("desktop-entry", Value::from("geekbridge-phone"));
        hints.insert("category", Value::from(category));
        let reply = conn.call_method(
            Some(DEST),
            PATH,
            Some(DEST),
            "Notify",
            &(
                APP_NAME,
                replaces,
                ICON,
                summary,
                body,
                flat_actions,
                hints,
                timeout_ms,
            ),
        );
        match reply.and_then(|m| m.body().deserialize::<u32>()) {
            Ok(id) => {
                lock(&self.slots).insert(slot, id);
                lock(&self.targets).insert(id, target);
            }
            Err(e) => log::debug!("notification not shown: {e}"),
        }
    }
}

fn shorten(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= BODY_CHARS {
        flat
    } else {
        let cut: String = flat.chars().take(BODY_CHARS - 1).collect();
        format!("{cut}…")
    }
}

fn who(name: &Option<String>, number: &str) -> String {
    match name {
        Some(n) if !n.trim().is_empty() => n.clone(),
        _ if !number.trim().is_empty() => number.to_string(),
        _ => "Unknown".to_string(),
    }
}

impl Notifier for FreedesktopNotifier {
    fn incoming_call(&self, call: &CallNotice) {
        let title = who(&call.name, &call.number);
        let body = if call.name.is_some() {
            call.number.clone()
        } else {
            "Incoming call".to_string()
        };
        self.post(
            call.id(),
            Target::Call {
                call_id: call.call_id.clone(),
            },
            &format!("Incoming call: {title}"),
            &body,
            &[("answer", "Answer"), ("decline", "Decline")],
            CALL_TIMEOUT_MS,
            "call.incoming",
        );
    }

    fn call_updated(&self, call: &CallUpdate) {
        // The call was answered (here or on the phone): the ringing alert has done its job.
        self.clear(&call.id());
    }

    fn call_ended(&self, call: &CallEndedNotice) {
        if !call.missed {
            return;
        }
        let title = who(&call.name, &call.number);
        self.post(
            NotifyId::Call(format!("missed:{}", call.call_id)),
            Target::Window,
            &format!("Missed call: {title}"),
            if call.name.is_some() {
                &call.number
            } else {
                ""
            },
            &[("default", "Open")],
            -1,
            "call.missed",
        );
    }

    fn text(&self, text: &TextNotice) {
        let title = who(&text.name, &text.number);
        let timeout = i32::try_from(text.timeout_secs.saturating_mul(1000)).unwrap_or(8000);
        self.post(
            text.id(),
            Target::Conversation {
                number: text.number.clone(),
            },
            &title,
            &shorten(&text.body),
            &[("default", "Open")],
            timeout,
            "im.received",
        );
    }

    fn clear(&self, id: &NotifyId) {
        let Some(conn) = &self.conn else { return };
        let Some(open) = lock(&self.slots).remove(id) else {
            return;
        };
        lock(&self.targets).remove(&open);
        if let Err(e) =
            conn.call_method(Some(DEST), PATH, Some(DEST), "CloseNotification", &(open,))
        {
            log::debug!("could not close notification: {e}");
        }
    }
}

/// Watches the daemon's signals on a background thread and routes button presses.
fn listen(conn: Connection, host: NotifierHost, targets: Pending, slots: Slots) {
    let spawned = std::thread::Builder::new()
        .name("gbp-notify-signals".into())
        .spawn(move || {
            let rule = match zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .interface(DEST)
                .map(|b| b.build())
            {
                Ok(r) => r,
                Err(e) => {
                    log::debug!("notification signals unavailable: {e}");
                    return;
                }
            };
            let iter = match MessageIterator::for_match_rule(rule, &conn, None) {
                Ok(i) => i,
                Err(e) => {
                    log::debug!("notification signals unavailable: {e}");
                    return;
                }
            };
            for msg in iter {
                let Ok(msg) = msg else { continue };
                let header = msg.header();
                let member = header
                    .member()
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default();
                match member.as_str() {
                    "ActionInvoked" => {
                        if let Ok((id, key)) = msg.body().deserialize::<(u32, String)>() {
                            let target = lock(&targets).get(&id).cloned();
                            if let Some(target) = target {
                                route(&host, &target, &key);
                            }
                        }
                    }
                    "NotificationClosed" => {
                        if let Ok((id, _reason)) = msg.body().deserialize::<(u32, u32)>() {
                            lock(&targets).remove(&id);
                            lock(&slots).retain(|_, v| *v != id);
                        }
                    }
                    _ => {}
                }
            }
        });
    if let Err(e) = spawned {
        log::debug!("cannot watch notification buttons: {e}");
    }
}

/// What a button press does.
fn route(host: &NotifierHost, target: &Target, key: &str) {
    match (target, key) {
        (Target::Call { call_id }, "answer") => host.answer(call_id),
        (Target::Call { call_id }, "decline") => host.decline(call_id),
        (Target::Call { .. }, _) | (Target::Window, _) => host.show_window(),
        (Target::Conversation { number }, _) => host.open_conversation(number),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::Type;

    /// The arguments of `Notify`, as the Desktop Notifications spec declares them.
    #[test]
    fn the_notify_call_has_the_signature_the_spec_declares() {
        type Body<'a> = (
            &'a str,
            u32,
            &'a str,
            &'a str,
            &'a str,
            Vec<&'a str>,
            HashMap<&'a str, Value<'a>>,
            i32,
        );
        assert_eq!(Body::SIGNATURE.to_string(), "(susssasa{sv}i)");
    }

    #[test]
    fn long_bodies_are_cut_and_flattened() {
        assert_eq!(shorten("a\n b"), "a b");
        let long = "x".repeat(1000);
        let s = shorten(&long);
        assert_eq!(s.chars().count(), BODY_CHARS);
        assert!(s.ends_with('…'));
    }

    #[test]
    fn titles_prefer_the_name_then_the_number() {
        assert_eq!(who(&Some("Alí".into()), "+1"), "Alí");
        assert_eq!(who(&None, "+1"), "+1");
        assert_eq!(who(&Some(" ".into()), "+1"), "+1");
        assert_eq!(who(&None, ""), "Unknown");
    }
}
