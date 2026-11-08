// SPDX-License-Identifier: GPL-3.0-or-later
//! Turns repeated snapshots of "the calls the phone has" into events.
//!
//! The gateway reports no calls for a moment during some transitions, so a
//! call that disappears is only declared ended after a grace window, and one
//! that reappears inside it is treated as never having left.

use std::collections::HashMap;

/// How long a missing call is waited for before it counts as ended.
pub const GRACE_MS: u64 = 1500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallState {
    Incoming,
    Waiting,
    Dialing,
    Alerting,
    Active,
    Held,
    Disconnected,
}

impl CallState {
    pub fn parse(s: &str) -> Self {
        match s {
            "incoming" => Self::Incoming,
            "waiting" => Self::Waiting,
            "dialing" => Self::Dialing,
            "alerting" => Self::Alerting,
            "active" => Self::Active,
            "held" => Self::Held,
            _ => Self::Disconnected,
        }
    }

    pub fn is_ringing(self) -> bool {
        matches!(self, Self::Incoming | Self::Waiting)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallSnapshot {
    pub id: String,
    pub state: CallState,
    pub number: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Direction {
    Incoming,
    Outgoing,
    Missed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallEvent {
    /// A call started ringing.
    Incoming { id: String, number: String },
    /// A call is new or changed state. `since_ms` is when it became active.
    Changed {
        id: String,
        state: CallState,
        number: String,
        since_ms: Option<u64>,
    },
    /// A call is over.
    Ended {
        id: String,
        number: String,
        direction: Direction,
        seconds: u64,
    },
}

#[derive(Debug, Clone)]
struct Tracked {
    state: CallState,
    number: String,
    was_incoming: bool,
    active_since: Option<u64>,
    missing_since: Option<u64>,
}

#[derive(Debug, Default)]
pub struct CallTracker {
    calls: HashMap<String, Tracked>,
}

impl CallTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one snapshot taken at `now_ms` and returns what changed.
    pub fn ingest(&mut self, snapshots: &[CallSnapshot], now_ms: u64) -> Vec<CallEvent> {
        let mut events = Vec::new();

        for snap in snapshots {
            let entry = self.calls.entry(snap.id.clone());
            let is_new = matches!(entry, std::collections::hash_map::Entry::Vacant(_));
            let t = entry.or_insert_with(|| Tracked {
                state: CallState::Disconnected,
                number: snap.number.clone(),
                was_incoming: false,
                active_since: None,
                missing_since: None,
            });
            t.missing_since = None;
            let prev = t.state;
            t.state = snap.state;
            if !snap.number.is_empty() {
                t.number = snap.number.clone();
            }
            // Sticky: an answered call reports "active", which would turn an
            // incoming call into an outgoing one in the history.
            if snap.state.is_ringing() {
                t.was_incoming = true;
            }
            if snap.state == CallState::Active && t.active_since.is_none() {
                t.active_since = Some(now_ms);
            }
            if snap.state.is_ringing() && !prev.is_ringing() {
                events.push(CallEvent::Incoming {
                    id: snap.id.clone(),
                    number: t.number.clone(),
                });
            }
            if is_new || prev != snap.state {
                events.push(CallEvent::Changed {
                    id: snap.id.clone(),
                    state: snap.state,
                    number: t.number.clone(),
                    since_ms: t.active_since,
                });
            }
        }

        // Calls that are not in this snapshot start (or continue) their grace.
        let live: Vec<&str> = snapshots.iter().map(|s| s.id.as_str()).collect();
        let mut gone = Vec::new();
        for (id, t) in self.calls.iter_mut() {
            if live.contains(&id.as_str()) {
                continue;
            }
            let since = *t.missing_since.get_or_insert(now_ms);
            if now_ms.saturating_sub(since) >= GRACE_MS {
                gone.push(id.clone());
            }
        }
        gone.sort();
        for id in gone {
            if let Some(t) = self.calls.remove(&id) {
                // Talk time ends when the call was last seen, not after the grace wait.
                let last_seen = t.missing_since.unwrap_or(now_ms);
                let seconds = t
                    .active_since
                    .map_or(0, |s| last_seen.saturating_sub(s) / 1000);
                // Answered means it ever became active, however short the call.
                let direction = if !t.was_incoming {
                    Direction::Outgoing
                } else if t.active_since.is_some() {
                    Direction::Incoming
                } else {
                    Direction::Missed
                };
                events.push(CallEvent::Ended {
                    id,
                    number: t.number,
                    direction,
                    seconds,
                });
            }
        }
        events
    }

    pub fn is_idle(&self) -> bool {
        self.calls.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(id: &str, state: CallState, number: &str) -> CallSnapshot {
        CallSnapshot {
            id: id.into(),
            state,
            number: number.into(),
            name: None,
        }
    }

    #[test]
    fn incoming_then_answered_then_ended() {
        let mut t = CallTracker::new();
        let ev = t.ingest(&[snap("c1", CallState::Incoming, "+1")], 0);
        assert!(ev.contains(&CallEvent::Incoming {
            id: "c1".into(),
            number: "+1".into()
        }));
        let ev = t.ingest(&[snap("c1", CallState::Active, "+1")], 5_000);
        assert!(matches!(
            ev[0],
            CallEvent::Changed {
                state: CallState::Active,
                since_ms: Some(5_000),
                ..
            }
        ));
        assert!(t.ingest(&[], 65_000).is_empty()); // grace starts
        let ev = t.ingest(&[], 66_600);
        assert_eq!(
            ev,
            vec![CallEvent::Ended {
                id: "c1".into(),
                number: "+1".into(),
                direction: Direction::Incoming,
                seconds: 60
            }]
        );
        assert!(t.is_idle());
    }

    #[test]
    fn never_answered_is_missed() {
        let mut t = CallTracker::new();
        t.ingest(&[snap("c1", CallState::Incoming, "+1")], 0);
        t.ingest(&[], 1_000);
        let ev = t.ingest(&[], 3_000);
        assert!(matches!(
            &ev[0],
            CallEvent::Ended {
                direction: Direction::Missed,
                seconds: 0,
                ..
            }
        ));
    }

    #[test]
    fn dialed_call_is_outgoing() {
        let mut t = CallTracker::new();
        t.ingest(&[snap("c1", CallState::Dialing, "+1")], 0);
        t.ingest(&[snap("c1", CallState::Active, "+1")], 2_000);
        t.ingest(&[], 12_000);
        let ev = t.ingest(&[], 14_000);
        assert!(matches!(
            &ev[0],
            CallEvent::Ended {
                direction: Direction::Outgoing,
                seconds: 10,
                ..
            }
        ));
    }

    #[test]
    fn a_very_short_answered_call_is_not_missed() {
        let mut t = CallTracker::new();
        t.ingest(&[snap("c1", CallState::Incoming, "+1")], 0);
        t.ingest(&[snap("c1", CallState::Active, "+1")], 100);
        t.ingest(&[], 300);
        let ev = t.ingest(&[], 2_000);
        assert!(matches!(
            &ev[0],
            CallEvent::Ended {
                direction: Direction::Incoming,
                seconds: 0,
                ..
            }
        ));
    }

    #[test]
    fn a_blip_inside_the_grace_window_is_not_an_end() {
        let mut t = CallTracker::new();
        t.ingest(&[snap("c1", CallState::Active, "+1")], 0);
        assert!(t.ingest(&[], 1_000).is_empty());
        let ev = t.ingest(&[snap("c1", CallState::Active, "+1")], 1_200);
        assert!(ev.is_empty(), "no spurious event: {ev:?}");
        assert!(t.ingest(&[], 1_300).is_empty());
        assert!(!t.is_idle());
    }

    #[test]
    fn answering_does_not_re_announce_the_ring() {
        let mut t = CallTracker::new();
        t.ingest(&[snap("c1", CallState::Incoming, "+1")], 0);
        let ev = t.ingest(&[snap("c1", CallState::Active, "+1")], 1_000);
        assert!(!ev.iter().any(|e| matches!(e, CallEvent::Incoming { .. })));
    }
}
