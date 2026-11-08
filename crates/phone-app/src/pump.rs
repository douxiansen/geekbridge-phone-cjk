// SPDX-License-Identifier: GPL-3.0-or-later
//! The event loop. It owns the receiving end of the service's events and the [`Notifier`], and
//! runs on its own thread so alerts work while the window is hidden and the UI is not drawing.
//!
//! For every event it (1) records what the UI needs to know in [`Shared`], (2) decides, from the
//! user's settings, whether an alert is due, and (3) wakes the UI.

use crate::notify::{CallEndedNotice, CallNotice, Notifier, NotifyId, TextNotice};
use crate::shared::Shared;
use phone_core::calls::{CallEvent, Direction};
use phone_core::contacts::number_key;
use phone_core::service::{Event, Handle};
use phone_core::settings::Settings;
use std::collections::HashSet;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// An alert the loop decided on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Alert {
    IncomingCall(CallNotice),
    CallEnded(CallEndedNotice),
    Text(TextNotice),
    Clear(NotifyId),
}

/// Turns events into alerts. Remembers which calls it announced so it can withdraw them.
#[derive(Debug, Default)]
pub struct Decider {
    announced: HashSet<String>,
}

impl Decider {
    /// `name_of` finds a contact's name for a number; `watching_messages` is true while the user
    /// is looking at the Messages tab.
    pub fn decide(&mut self, event: &Event, settings: &Settings, name_of: &dyn Fn(&str) -> Option<String>, watching_messages: bool) -> Vec<Alert> {
        match event {
            Event::Call(CallEvent::Incoming { id, number }) if settings.notify_calls => {
                self.announced.insert(id.clone());
                vec![Alert::IncomingCall(CallNotice { call_id: id.clone(), number: number.clone(), name: name_of(number) })]
            }
            Event::Call(CallEvent::Changed { id, state, .. }) if !state.is_ringing() && self.announced.remove(id) => {
                vec![Alert::Clear(NotifyId::Call(id.clone()))]
            }
            Event::Call(CallEvent::Ended { id, number, direction, seconds }) => {
                let mut out = Vec::new();
                if self.announced.remove(id) {
                    out.push(Alert::Clear(NotifyId::Call(id.clone())));
                }
                let missed = *direction == Direction::Missed;
                if (missed && settings.notify_missed) || (!missed && settings.notify_calls) {
                    out.push(Alert::CallEnded(CallEndedNotice { call_id: id.clone(), number: number.clone(), name: name_of(number), missed, seconds: *seconds }));
                }
                out
            }
            Event::TextReceived(m) if m.incoming => {
                if watching_messages {
                    // The user is reading it right now: no alert, and drop any earlier one.
                    return vec![Alert::Clear(NotifyId::Text(number_key(&m.number)))];
                }
                if !settings.notify_texts {
                    return Vec::new();
                }
                vec![Alert::Text(TextNotice {
                    number: m.number.clone(),
                    name: name_of(&m.number),
                    body: m.text.clone(),
                    at: m.at,
                    quick_reply: settings.quick_reply,
                    timeout_secs: settings.text_banner_secs,
                })]
            }
            _ => Vec::new(),
        }
    }
}

/// Hands an alert to the notifier.
pub fn deliver(notifier: &dyn Notifier, alert: &Alert) {
    match alert {
        Alert::IncomingCall(c) => notifier.incoming_call(c),
        Alert::CallEnded(c) => notifier.call_ended(c),
        Alert::Text(t) => notifier.text(t),
        Alert::Clear(id) => notifier.clear(id),
    }
}

/// The number an event is about, for the name lookup.
fn event_number(e: &Event) -> Option<&str> {
    match e {
        Event::Call(CallEvent::Incoming { number, .. }) | Event::Call(CallEvent::Ended { number, .. }) => Some(number),
        Event::TextReceived(m) => Some(&m.number),
        _ => None,
    }
}

/// Starts the event thread. It stops when `shared.stop()` is called or the service is gone.
pub fn spawn(rx: Receiver<Event>, handle: Handle, shared: Arc<Shared>, notifier: Box<dyn Notifier>, wake: Arc<dyn Fn() + Send + Sync>) -> std::io::Result<JoinHandle<()>> {
    thread::Builder::new().name("gbp-events".into()).spawn(move || {
        let mut decider = Decider::default();
        loop {
            let event = match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(e) => e,
                Err(RecvTimeoutError::Timeout) => {
                    if shared.is_stopped() {
                        return;
                    }
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => return,
            };
            shared.apply(&event);
            // One guard, copied from and dropped: the state lock is not reentrant.
            let (settings, name) = {
                let st = handle.state();
                let name = event_number(&event).and_then(|n| st.contacts.name_for(n).map(str::to_string));
                (st.settings.clone(), name)
            };
            let name_of = |_: &str| name.clone();
            for alert in decider.decide(&event, &settings, &name_of, shared.watching_messages()) {
                deliver(notifier.as_ref(), &alert);
            }
            wake();
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use phone_core::calls::CallState;
    use phone_core::conversations::Message;
    use std::sync::Mutex;

    fn name(n: &str) -> Option<String> {
        (n == "+213550123456").then(|| "Alí".to_string())
    }

    fn incoming(id: &str, number: &str) -> Event {
        Event::Call(CallEvent::Incoming { id: id.into(), number: number.into() })
    }

    fn ended(id: &str, direction: Direction) -> Event {
        Event::Call(CallEvent::Ended { id: id.into(), number: "+213550123456".into(), direction, seconds: 12 })
    }

    fn text(number: &str) -> Event {
        Event::TextReceived(Message { number: number.into(), incoming: true, text: "hello".into(), at: 5, read: false, pending: false })
    }

    #[test]
    fn a_ringing_call_is_announced_with_the_contact_name() {
        let mut d = Decider::default();
        let out = d.decide(&incoming("c1", "+213550123456"), &Settings::default(), &name, false);
        assert_eq!(out, vec![Alert::IncomingCall(CallNotice { call_id: "c1".into(), number: "+213550123456".into(), name: Some("Alí".into()) })]);
    }

    #[test]
    fn answering_withdraws_the_alert_once() {
        let mut d = Decider::default();
        d.decide(&incoming("c1", "+1"), &Settings::default(), &name, false);
        let changed = Event::Call(CallEvent::Changed { id: "c1".into(), state: CallState::Active, number: "+1".into(), since_ms: Some(1) });
        assert_eq!(d.decide(&changed, &Settings::default(), &name, false), vec![Alert::Clear(NotifyId::Call("c1".into()))]);
        assert!(d.decide(&changed, &Settings::default(), &name, false).is_empty());
    }

    #[test]
    fn staying_in_the_ringing_state_changes_nothing() {
        let mut d = Decider::default();
        d.decide(&incoming("c1", "+1"), &Settings::default(), &name, false);
        let still = Event::Call(CallEvent::Changed { id: "c1".into(), state: CallState::Waiting, number: "+1".into(), since_ms: None });
        assert!(d.decide(&still, &Settings::default(), &name, false).is_empty());
    }

    #[test]
    fn call_alerts_obey_the_settings() {
        let mut d = Decider::default();
        let off = Settings { notify_calls: false, ..Settings::default() };
        assert!(d.decide(&incoming("c1", "+1"), &off, &name, false).is_empty());
        // Ended: a normal call is silent when call alerts are off, a missed one follows its own switch.
        assert!(d.decide(&ended("c1", Direction::Incoming), &off, &name, false).is_empty());
        assert_eq!(d.decide(&ended("c2", Direction::Missed), &off, &name, false).len(), 1);
        let no_missed = Settings { notify_missed: false, ..Settings::default() };
        assert!(d.decide(&ended("c3", Direction::Missed), &no_missed, &name, false).is_empty());
    }

    #[test]
    fn a_missed_call_clears_the_ringing_alert_and_posts_a_missed_one() {
        let mut d = Decider::default();
        d.decide(&incoming("c1", "+213550123456"), &Settings::default(), &name, false);
        let out = d.decide(&ended("c1", Direction::Missed), &Settings::default(), &name, false);
        assert_eq!(out[0], Alert::Clear(NotifyId::Call("c1".into())));
        assert!(matches!(&out[1], Alert::CallEnded(c) if c.missed && c.name.as_deref() == Some("Alí")));
    }

    #[test]
    fn texts_carry_the_quick_reply_and_timeout_settings() {
        let mut d = Decider::default();
        let s = Settings { quick_reply: false, text_banner_secs: 20, ..Settings::default() };
        let out = d.decide(&text("+213550123456"), &s, &name, false);
        match &out[..] {
            [Alert::Text(t)] => {
                assert!(!t.quick_reply);
                assert_eq!(t.timeout_secs, 20);
                assert_eq!(t.name.as_deref(), Some("Alí"));
                assert_eq!(t.id(), NotifyId::Text(number_key("+213550123456")));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn texts_are_silent_when_off_or_being_read() {
        let mut d = Decider::default();
        let off = Settings { notify_texts: false, ..Settings::default() };
        assert!(d.decide(&text("+1"), &off, &name, false).is_empty());
        let out = d.decide(&text("+1"), &Settings::default(), &name, true);
        assert_eq!(out, vec![Alert::Clear(NotifyId::Text(number_key("+1")))]);
    }

    #[test]
    fn our_own_sent_texts_never_alert() {
        let mut d = Decider::default();
        let e = Event::TextReceived(Message { number: "+1".into(), incoming: false, text: "x".into(), at: 1, read: true, pending: false });
        assert!(d.decide(&e, &Settings::default(), &name, false).is_empty());
    }

    #[test]
    fn unrelated_events_are_ignored() {
        let mut d = Decider::default();
        for e in [Event::CallsChanged, Event::ContactsChanged, Event::Notice("x".into()), Event::RecentsChanged] {
            assert!(d.decide(&e, &Settings::default(), &name, false).is_empty());
        }
    }

    #[derive(Default)]
    struct Recording(Mutex<Vec<String>>);
    impl Notifier for Recording {
        fn incoming_call(&self, c: &CallNotice) {
            self.0.lock().unwrap().push(format!("call {}", c.call_id));
        }
        fn call_ended(&self, c: &CallEndedNotice) {
            self.0.lock().unwrap().push(format!("ended {}", c.call_id));
        }
        fn text(&self, t: &TextNotice) {
            self.0.lock().unwrap().push(format!("text {}", t.number));
        }
        fn clear(&self, id: &NotifyId) {
            self.0.lock().unwrap().push(format!("clear {id:?}"));
        }
    }

    #[test]
    fn deliver_routes_each_alert_to_its_method() {
        let n = Recording::default();
        deliver(&n, &Alert::IncomingCall(CallNotice { call_id: "a".into(), number: "1".into(), name: None }));
        deliver(&n, &Alert::Clear(NotifyId::Call("a".into())));
        deliver(&n, &Alert::Text(TextNotice { number: "2".into(), name: None, body: String::new(), at: 0, quick_reply: true, timeout_secs: 8 }));
        let log = n.0.lock().unwrap().clone();
        assert_eq!(log, vec!["call a".to_string(), "clear Call(\"a\")".into(), "text 2".into()]);
    }
}
