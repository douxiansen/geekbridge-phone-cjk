// SPDX-License-Identifier: GPL-3.0-or-later
//! The event loop. It owns the receiving end of the service's events and the [`Notifier`], and
//! runs on its own thread so alerts work while the window is hidden and the UI is not drawing.
//!
//! For every event it (1) records what the UI needs to know in [`Shared`], (2) decides, from the
//! user's settings, whether an alert is due, and (3) wakes the UI.

use crate::notify::{CallEndedNotice, CallNotice, CallUpdate, Notifier, NotifyId, TextNotice};
use crate::shared::Shared;
use phone_core::calls::{CallEvent, CallState, Direction};
use phone_core::contacts::number_key;
use phone_core::service::{ActiveCall, Event, Handle};
use phone_core::settings::Settings;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// An alert the loop decided on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Alert {
    IncomingCall(CallNotice),
    /// A call that was ringing is now answered, being placed, or on hold (or a call being placed
    /// has been answered).
    CallUpdate(CallUpdate),
    CallEnded(CallEndedNotice),
    Text(TextNotice),
    Clear(NotifyId),
}

/// What the decision depends on besides the event itself.
pub struct Context<'a> {
    pub settings: &'a Settings,
    /// A contact's name for a number.
    pub name_of: &'a dyn Fn(&str) -> Option<String>,
    /// The user is looking at the Messages tab, so a new text needs no alert.
    pub watching_messages: bool,
    /// The calls the phone has right now (read when the event is `CallsChanged`).
    pub calls: &'a [ActiveCall],
}

/// Turns events into alerts. Remembers which calls the notifier was told about, so it can
/// withdraw their alerts when they end, and the last state it announced for each.
#[derive(Debug, Default)]
pub struct Decider {
    told: HashSet<String>,
    states: HashMap<String, CallState>,
}

impl Decider {
    pub fn decide(&mut self, event: &Event, cx: &Context) -> Vec<Alert> {
        let settings = cx.settings;
        match event {
            Event::Call(CallEvent::Incoming { id, number }) if settings.notify_calls => {
                self.told.insert(id.clone());
                // The next CallsChanged must not announce this call's ringing state again.
                self.states.insert(id.clone(), CallState::Incoming);
                vec![Alert::IncomingCall(CallNotice {
                    call_id: id.clone(),
                    number: number.clone(),
                    name: (cx.name_of)(number),
                })]
            }
            Event::CallsChanged if settings.notify_calls => self.call_updates(cx),
            Event::Call(CallEvent::Ended {
                id,
                number,
                direction,
                seconds,
            }) => {
                self.states.remove(id);
                let mut out = Vec::new();
                if self.told.remove(id) {
                    out.push(Alert::Clear(NotifyId::Call(id.clone())));
                }
                let missed = *direction == Direction::Missed;
                if (missed && settings.notify_missed) || (!missed && settings.notify_calls) {
                    out.push(Alert::CallEnded(CallEndedNotice {
                        call_id: id.clone(),
                        number: number.clone(),
                        name: (cx.name_of)(number),
                        missed,
                        seconds: *seconds,
                    }));
                }
                out
            }
            Event::TextReceived(m) if m.incoming => {
                if cx.watching_messages {
                    // The user is reading it right now: no alert, and drop any earlier one.
                    return vec![Alert::Clear(NotifyId::Text(number_key(&m.number)))];
                }
                if !settings.notify_texts {
                    return Vec::new();
                }
                vec![Alert::Text(TextNotice {
                    number: m.number.clone(),
                    name: (cx.name_of)(&m.number),
                    body: m.text.clone(),
                    at: m.at,
                    quick_reply: settings.quick_reply,
                    timeout_secs: settings.text_banner_secs,
                })]
            }
            _ => Vec::new(),
        }
    }

    /// Announces calls whose state changed since last time, except ringing ones (which have their
    /// own alert) and forgets calls that are gone.
    fn call_updates(&mut self, cx: &Context) -> Vec<Alert> {
        let mut out = Vec::new();
        for call in cx.calls {
            let previous = self.states.insert(call.id.clone(), call.state);
            if previous == Some(call.state) || call.state.is_ringing() {
                continue;
            }
            self.told.insert(call.id.clone());
            out.push(Alert::CallUpdate(CallUpdate {
                call_id: call.id.clone(),
                number: call.number.clone(),
                name: call.name.clone(),
                state: call.state,
                since: call.since,
            }));
        }
        self.states
            .retain(|id, _| cx.calls.iter().any(|c| &c.id == id));
        out
    }
}

/// Hands an alert to the notifier.
pub fn deliver(notifier: &dyn Notifier, alert: &Alert) {
    match alert {
        Alert::IncomingCall(c) => notifier.incoming_call(c),
        Alert::CallUpdate(c) => notifier.call_updated(c),
        Alert::CallEnded(c) => notifier.call_ended(c),
        Alert::Text(t) => notifier.text(t),
        Alert::Clear(id) => notifier.clear(id),
    }
}

/// The number an event is about, for the name lookup.
fn event_number(e: &Event) -> Option<&str> {
    match e {
        Event::Call(CallEvent::Incoming { number, .. })
        | Event::Call(CallEvent::Ended { number, .. }) => Some(number),
        Event::TextReceived(m) => Some(&m.number),
        _ => None,
    }
}

/// Starts the event thread. It stops when `shared.stop()` is called or the service is gone.
pub fn spawn(
    rx: Receiver<Event>,
    handle: Handle,
    shared: Arc<Shared>,
    notifier: Box<dyn Notifier>,
    wake: Arc<dyn Fn() + Send + Sync>,
) -> std::io::Result<JoinHandle<()>> {
    thread::Builder::new()
        .name("gbp-events".into())
        .spawn(move || {
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
                let (settings, name, calls) = {
                    let st = handle.state();
                    let name = event_number(&event)
                        .and_then(|n| st.contacts.name_for(n).map(str::to_string));
                    let calls = if matches!(event, Event::CallsChanged) {
                        st.calls.clone()
                    } else {
                        Vec::new()
                    };
                    (st.settings.clone(), name, calls)
                };
                let name_of = |_: &str| name.clone();
                let cx = Context {
                    settings: &settings,
                    name_of: &name_of,
                    watching_messages: shared.watching_messages(),
                    calls: &calls,
                };
                for alert in decider.decide(&event, &cx) {
                    deliver(notifier.as_ref(), &alert);
                }
                if let Event::TextResult {
                    token,
                    number,
                    result,
                } = &event
                {
                    notifier.reply_result(*token, number, result);
                }
                wake();
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use phone_core::conversations::Message;
    use std::sync::Mutex;
    use std::time::Instant;

    fn name(n: &str) -> Option<String> {
        (n == "+213550123456").then(|| "Alí".to_string())
    }

    fn incoming(id: &str, number: &str) -> Event {
        Event::Call(CallEvent::Incoming {
            id: id.into(),
            number: number.into(),
        })
    }

    fn ended(id: &str, direction: Direction) -> Event {
        Event::Call(CallEvent::Ended {
            id: id.into(),
            number: "+213550123456".into(),
            direction,
            seconds: 12,
        })
    }

    fn text(number: &str) -> Event {
        Event::TextReceived(Message {
            number: number.into(),
            incoming: true,
            text: "hello".into(),
            at: 5,
            read: false,
            pending: false,
        })
    }

    fn call(id: &str, state: CallState) -> ActiveCall {
        ActiveCall {
            id: id.into(),
            state,
            number: "+213550123456".into(),
            name: Some("Alí".into()),
            since: (state == CallState::Active).then(Instant::now),
            recording: false,
        }
    }

    /// Decides with default settings, no text being read and no calls.
    fn decide(d: &mut Decider, e: &Event, s: &Settings) -> Vec<Alert> {
        decide_with(d, e, s, false, &[])
    }

    fn decide_with(
        d: &mut Decider,
        e: &Event,
        s: &Settings,
        watching: bool,
        calls: &[ActiveCall],
    ) -> Vec<Alert> {
        d.decide(
            e,
            &Context {
                settings: s,
                name_of: &name,
                watching_messages: watching,
                calls,
            },
        )
    }

    #[test]
    fn a_ringing_call_is_announced_with_the_contact_name() {
        let mut d = Decider::default();
        let out = decide(
            &mut d,
            &incoming("c1", "+213550123456"),
            &Settings::default(),
        );
        assert_eq!(
            out,
            vec![Alert::IncomingCall(CallNotice {
                call_id: "c1".into(),
                number: "+213550123456".into(),
                name: Some("Alí".into())
            })]
        );
    }

    #[test]
    fn answering_is_an_update_not_a_second_ringing_alert() {
        let mut d = Decider::default();
        let s = Settings::default();
        decide(&mut d, &incoming("c1", "+1"), &s);
        // Still ringing: nothing new to say.
        let ringing = [call("c1", CallState::Incoming)];
        assert!(decide_with(&mut d, &Event::CallsChanged, &s, false, &ringing).is_empty());
        // Answered: one update, carrying the start time for a timer.
        let active = [call("c1", CallState::Active)];
        let out = decide_with(&mut d, &Event::CallsChanged, &s, false, &active);
        assert!(
            matches!(&out[..], [Alert::CallUpdate(u)] if u.call_id == "c1" && u.state == CallState::Active && u.since.is_some())
        );
        // And only once.
        assert!(decide_with(&mut d, &Event::CallsChanged, &s, false, &active).is_empty());
    }

    #[test]
    fn a_call_placed_from_here_is_announced_as_it_changes() {
        let mut d = Decider::default();
        let s = Settings::default();
        let dialing = [call("c2", CallState::Dialing)];
        let out = decide_with(&mut d, &Event::CallsChanged, &s, false, &dialing);
        assert!(matches!(&out[..], [Alert::CallUpdate(u)] if u.state == CallState::Dialing));
        let active = [call("c2", CallState::Active)];
        let out = decide_with(&mut d, &Event::CallsChanged, &s, false, &active);
        assert!(matches!(&out[..], [Alert::CallUpdate(u)] if u.state == CallState::Active));
        // Ending withdraws it.
        let out = decide(&mut d, &ended("c2", Direction::Outgoing), &s);
        assert_eq!(out[0], Alert::Clear(NotifyId::Call("c2".into())));
    }

    #[test]
    fn call_alerts_obey_the_settings() {
        let mut d = Decider::default();
        let off = Settings {
            notify_calls: false,
            ..Settings::default()
        };
        assert!(decide(&mut d, &incoming("c1", "+1"), &off).is_empty());
        let active = [call("c1", CallState::Active)];
        assert!(decide_with(&mut d, &Event::CallsChanged, &off, false, &active).is_empty());
        // Ended: a normal call is silent when call alerts are off, a missed one follows its own switch.
        assert!(decide(&mut d, &ended("c1", Direction::Incoming), &off).is_empty());
        assert_eq!(
            decide(&mut d, &ended("c2", Direction::Missed), &off).len(),
            1
        );
        let no_missed = Settings {
            notify_missed: false,
            ..Settings::default()
        };
        assert!(decide(&mut d, &ended("c3", Direction::Missed), &no_missed).is_empty());
    }

    #[test]
    fn a_missed_call_clears_the_ringing_alert_and_posts_a_missed_one() {
        let mut d = Decider::default();
        let s = Settings::default();
        decide(&mut d, &incoming("c1", "+213550123456"), &s);
        let out = decide(&mut d, &ended("c1", Direction::Missed), &s);
        assert_eq!(out[0], Alert::Clear(NotifyId::Call("c1".into())));
        assert!(
            matches!(&out[1], Alert::CallEnded(c) if c.missed && c.name.as_deref() == Some("Alí"))
        );
    }

    #[test]
    fn a_vanished_call_is_forgotten_so_its_id_can_come_back() {
        let mut d = Decider::default();
        let s = Settings::default();
        let active = [call("c1", CallState::Active)];
        decide_with(&mut d, &Event::CallsChanged, &s, false, &active);
        decide_with(&mut d, &Event::CallsChanged, &s, false, &[]);
        let out = decide_with(&mut d, &Event::CallsChanged, &s, false, &active);
        assert_eq!(out.len(), 1, "same id, new call");
    }

    #[test]
    fn texts_carry_the_quick_reply_and_timeout_settings() {
        let mut d = Decider::default();
        let s = Settings {
            quick_reply: false,
            text_banner_secs: 20,
            ..Settings::default()
        };
        let out = decide(&mut d, &text("+213550123456"), &s);
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
        let off = Settings {
            notify_texts: false,
            ..Settings::default()
        };
        assert!(decide(&mut d, &text("+1"), &off).is_empty());
        let out = decide_with(&mut d, &text("+1"), &Settings::default(), true, &[]);
        assert_eq!(out, vec![Alert::Clear(NotifyId::Text(number_key("+1")))]);
    }

    #[test]
    fn our_own_sent_texts_never_alert() {
        let mut d = Decider::default();
        let e = Event::TextReceived(Message {
            number: "+1".into(),
            incoming: false,
            text: "x".into(),
            at: 1,
            read: true,
            pending: false,
        });
        assert!(decide(&mut d, &e, &Settings::default()).is_empty());
    }

    #[test]
    fn unrelated_events_are_ignored() {
        let mut d = Decider::default();
        for e in [
            Event::ContactsChanged,
            Event::Notice("x".into()),
            Event::RecentsChanged,
        ] {
            assert!(decide(&mut d, &e, &Settings::default()).is_empty());
        }
    }

    #[derive(Default)]
    struct Recording(Mutex<Vec<String>>);
    impl Notifier for Recording {
        fn incoming_call(&self, c: &CallNotice) {
            self.0.lock().unwrap().push(format!("call {}", c.call_id));
        }
        fn call_updated(&self, c: &CallUpdate) {
            self.0.lock().unwrap().push(format!("update {}", c.call_id));
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
        deliver(
            &n,
            &Alert::IncomingCall(CallNotice {
                call_id: "a".into(),
                number: "1".into(),
                name: None,
            }),
        );
        deliver(&n, &Alert::Clear(NotifyId::Call("a".into())));
        deliver(
            &n,
            &Alert::CallUpdate(CallUpdate {
                call_id: "a".into(),
                number: "1".into(),
                name: None,
                state: CallState::Active,
                since: None,
            }),
        );
        deliver(
            &n,
            &Alert::Text(TextNotice {
                number: "2".into(),
                name: None,
                body: String::new(),
                at: 0,
                quick_reply: true,
                timeout_secs: 8,
            }),
        );
        let log = n.0.lock().unwrap().clone();
        assert_eq!(
            log,
            vec![
                "call a".to_string(),
                "clear Call(\"a\")".into(),
                "update a".into(),
                "text 2".into()
            ]
        );
    }
}
