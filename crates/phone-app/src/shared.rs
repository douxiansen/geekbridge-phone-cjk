// SPDX-License-Identifier: GPL-3.0-or-later
//! What the event thread hands to the UI. The UI never waits on it: it reads a few atomics every
//! frame and drains small queues when they are not empty.

use phone_core::service::Event;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

/// Queue lengths are capped so a window that stays hidden for days cannot grow without bound.
const MAX_QUEUED: usize = 64;

/// The outcome of one `send_text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextOutcome {
    pub token: u64,
    pub number: String,
    pub result: Result<(), String>,
}

/// Counters that change whenever a part of the shared state changed. The UI re-reads a part only
/// when its counter moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Generations {
    pub contacts: u64,
    pub conversations: u64,
    pub recents: u64,
}

#[derive(Default)]
struct Queues {
    notices: VecDeque<String>,
    outcomes: VecDeque<TextOutcome>,
}

/// State shared between the event thread and the UI.
#[derive(Default)]
pub struct Shared {
    contacts: AtomicU64,
    conversations: AtomicU64,
    recents: AtomicU64,
    /// The user is looking at the Messages tab in a focused window: no need to alert for texts.
    watching_messages: AtomicBool,
    stop: AtomicBool,
    queues: Mutex<Queues>,
}

fn push_capped<T>(q: &mut VecDeque<T>, item: T) {
    if q.len() >= MAX_QUEUED {
        q.pop_front();
    }
    q.push_back(item);
}

impl Shared {
    fn queues(&self) -> MutexGuard<'_, Queues> {
        self.queues.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Records what an event means for the UI.
    pub fn apply(&self, event: &Event) {
        match event {
            Event::Phone(_) => {
                // A different phone means different data everywhere.
                self.contacts.fetch_add(1, Ordering::Release);
                self.conversations.fetch_add(1, Ordering::Release);
                self.recents.fetch_add(1, Ordering::Release);
            }
            Event::ContactsChanged => {
                self.contacts.fetch_add(1, Ordering::Release);
            }
            Event::ConversationsChanged | Event::TextReceived(_) => {
                self.conversations.fetch_add(1, Ordering::Release);
            }
            Event::RecentsChanged => {
                self.recents.fetch_add(1, Ordering::Release);
            }
            Event::TextResult { token, number, result } => {
                push_capped(&mut self.queues().outcomes, TextOutcome { token: *token, number: number.clone(), result: result.clone() });
            }
            Event::Notice(text) => {
                let mut q = self.queues();
                if !q.notices.iter().any(|n| n == text) {
                    push_capped(&mut q.notices, text.clone());
                }
            }
            Event::Call(_) | Event::CallsChanged => {}
        }
    }

    pub fn generations(&self) -> Generations {
        Generations {
            contacts: self.contacts.load(Ordering::Acquire),
            conversations: self.conversations.load(Ordering::Acquire),
            recents: self.recents.load(Ordering::Acquire),
        }
    }

    pub fn take_notices(&self) -> Vec<String> {
        self.queues().notices.drain(..).collect()
    }

    pub fn take_outcomes(&self) -> Vec<TextOutcome> {
        self.queues().outcomes.drain(..).collect()
    }

    pub fn set_watching_messages(&self, watching: bool) {
        self.watching_messages.store(watching, Ordering::Relaxed);
    }

    pub fn watching_messages(&self) -> bool {
        self.watching_messages.load(Ordering::Relaxed)
    }

    /// Asks the event thread to finish.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }

    pub fn is_stopped(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phone_core::conversations::Message;

    fn text() -> Message {
        Message { number: "+1".into(), incoming: true, text: "hi".into(), at: 1, read: false, pending: false }
    }

    #[test]
    fn events_move_only_the_counters_they_concern() {
        let s = Shared::default();
        let before = s.generations();
        s.apply(&Event::RecentsChanged);
        let after = s.generations();
        assert_eq!(after.recents, before.recents + 1);
        assert_eq!(after.contacts, before.contacts);
        s.apply(&Event::TextReceived(text()));
        assert_eq!(s.generations().conversations, before.conversations + 1);
        s.apply(&Event::Phone(None));
        let g = s.generations();
        assert_eq!((g.contacts, g.conversations, g.recents), (before.contacts + 1, before.conversations + 2, before.recents + 2));
    }

    #[test]
    fn repeated_notices_collapse_and_queues_drain() {
        let s = Shared::default();
        for _ in 0..3 {
            s.apply(&Event::Notice("No phone is connected.".into()));
        }
        s.apply(&Event::Notice("Other".into()));
        assert_eq!(s.take_notices(), vec!["No phone is connected.".to_string(), "Other".to_string()]);
        assert!(s.take_notices().is_empty());
    }

    #[test]
    fn queues_are_bounded() {
        let s = Shared::default();
        for i in 0..(MAX_QUEUED as u64 + 20) {
            s.apply(&Event::TextResult { token: i, number: "1".into(), result: Ok(()) });
        }
        let out = s.take_outcomes();
        assert_eq!(out.len(), MAX_QUEUED);
        assert_eq!(out[0].token, 20, "the oldest were dropped");
    }

    #[test]
    fn outcomes_keep_their_failure_text() {
        let s = Shared::default();
        s.apply(&Event::TextResult { token: 7, number: "+1".into(), result: Err("no link".into()) });
        let out = s.take_outcomes();
        assert_eq!(out[0].result, Err("no link".to_string()));
        assert_eq!(out[0].token, 7);
    }
}
