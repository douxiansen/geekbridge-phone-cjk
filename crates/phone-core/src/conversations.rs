// SPDX-License-Identifier: GPL-3.0-or-later
//! Texts grouped into conversations, kept between runs so the history grows
//! past what the phone lists at once.

use crate::contacts::number_key;
use phone_obex::MessageEntry;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

const MAX_STORED: usize = 5000;
/// A pending text counts as delivered when the phone lists the same text
/// within this many seconds of it.
const PENDING_MATCH_SECS: i64 = 180;
const PENDING_MAX_AGE_SECS: i64 = 300;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub number: String,
    pub incoming: bool,
    pub text: String,
    /// Unix seconds.
    pub at: i64,
    pub read: bool,
    #[serde(skip)]
    pub pending: bool,
}

impl Message {
    fn identity(&self) -> String {
        format!("{}|{}|{}|{}", self.incoming, number_key(&self.number), self.at, self.text)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    pub key: String,
    /// The number as the phone wrote it.
    pub number: String,
    /// Oldest first.
    pub messages: Vec<Message>,
}

impl Conversation {
    pub fn last(&self) -> &Message {
        // Conversations are never built empty.
        &self.messages[self.messages.len() - 1]
    }

    pub fn unread(&self) -> usize {
        self.messages.iter().filter(|m| m.incoming && !m.read).count()
    }
}

/// Parses MAP's `20260417T123456` (optionally followed by `Z` or `+hhmm`) into
/// unix seconds. A stamp without a zone is the phone's local time, which we
/// take to be ours.
pub fn parse_map_time(stamp: &str) -> Option<i64> {
    use chrono::{FixedOffset, Local, NaiveDateTime, TimeZone};
    let base = stamp.get(..15)?;
    let naive = NaiveDateTime::parse_from_str(base, "%Y%m%dT%H%M%S").ok()?;
    let zone = stamp.get(15..).unwrap_or("");
    let secs = match zone {
        "" => Local.from_local_datetime(&naive).earliest()?.timestamp(),
        "Z" => naive.and_utc().timestamp(),
        z if z.len() == 5 && (z.starts_with('+') || z.starts_with('-')) => {
            let h: i32 = z[1..3].parse().ok()?;
            let m: i32 = z[3..5].parse().ok()?;
            let off = (h * 3600 + m * 60) * if z.starts_with('-') { -1 } else { 1 };
            FixedOffset::east_opt(off)?.from_local_datetime(&naive).earliest()?.timestamp()
        }
        _ => return None,
    };
    Some(secs)
}

fn to_message(e: &MessageEntry, incoming: bool) -> Option<Message> {
    let number = if incoming { &e.sender_address } else { &e.recipient_address };
    if number.is_empty() {
        return None;
    }
    Some(Message {
        number: number.clone(),
        incoming,
        text: e.subject.clone(),
        at: parse_map_time(&e.datetime)?,
        read: if incoming { e.read } else { true },
        pending: false,
    })
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Conversations {
    stored: Vec<Message>,
    #[serde(skip)]
    pending: Vec<Message>,
}

impl Conversations {
    pub fn is_empty(&self) -> bool {
        self.stored.is_empty()
    }

    /// Merges what the phone lists. Returns true when anything changed.
    pub fn ingest(&mut self, inbox: &[MessageEntry], sent: &[MessageEntry]) -> bool {
        let mut known: HashSet<String> = self.stored.iter().map(Message::identity).collect();
        let mut changed = false;
        let incoming = inbox.iter().filter_map(|e| to_message(e, true));
        let outgoing = sent.iter().filter_map(|e| to_message(e, false));
        for m in incoming.chain(outgoing) {
            let id = m.identity();
            if known.insert(id.clone()) {
                self.stored.push(m);
                changed = true;
            } else if m.incoming && m.read {
                // The phone's read flag only ever turns on.
                if let Some(s) = self.stored.iter_mut().find(|s| s.identity() == id) {
                    if !s.read {
                        s.read = true;
                        changed = true;
                    }
                }
            }
        }
        // A pending text the phone now lists is replaced by the real one.
        let before = self.pending.len();
        let stored = &self.stored;
        self.pending.retain(|p| {
            !stored.iter().any(|s| {
                !s.incoming
                    && s.text == p.text
                    && number_key(&s.number) == number_key(&p.number)
                    && (s.at - p.at).abs() <= PENDING_MATCH_SECS
            })
        });
        changed |= self.pending.len() != before;
        if self.stored.len() > MAX_STORED {
            self.stored.sort_by_key(|m| m.at);
            let excess = self.stored.len() - MAX_STORED;
            self.stored.drain(..excess);
        }
        changed
    }

    /// A text sent from here, shown at once until the phone lists it.
    pub fn add_pending(&mut self, number: &str, text: &str, now: i64) {
        self.pending.push(Message { number: number.into(), incoming: false, text: text.into(), at: now, read: true, pending: true });
    }

    /// Drops pending texts that never showed up.
    pub fn expire_pending(&mut self, now: i64) -> bool {
        let before = self.pending.len();
        self.pending.retain(|p| now - p.at <= PENDING_MAX_AGE_SECS);
        before != self.pending.len()
    }

    /// Conversations, the one with the newest message first.
    pub fn conversations(&self) -> Vec<Conversation> {
        let mut by_key: Vec<Conversation> = Vec::new();
        for m in self.stored.iter().chain(self.pending.iter()) {
            let key = number_key(&m.number);
            match by_key.iter_mut().find(|c| c.key == key) {
                Some(c) => c.messages.push(m.clone()),
                None => by_key.push(Conversation { key, number: m.number.clone(), messages: vec![m.clone()] }),
            }
        }
        for c in &mut by_key {
            c.messages.sort_by_key(|m| m.at);
        }
        by_key.sort_by_key(|c| std::cmp::Reverse(c.last().at));
        by_key
    }

    pub fn thread(&self, number: &str) -> Vec<Message> {
        let key = number_key(number);
        self.conversations().into_iter().find(|c| c.key == key).map(|c| c.messages).unwrap_or_default()
    }

    pub fn unread_total(&self) -> usize {
        self.stored.iter().filter(|m| m.incoming && !m.read).count()
    }
}

/// Spots texts that arrive. Identity is the content, not the phone's handle,
/// so reconnecting does not announce old texts again.
#[derive(Debug, Default)]
pub struct InboxWatcher {
    seen: HashSet<String>,
    baselined: bool,
}

impl InboxWatcher {
    /// `known` is what is already stored: when it is empty this is a first
    /// run, and the first listing is recorded without announcing anything.
    pub fn new(known: &Conversations) -> Self {
        let seen = known.stored.iter().filter(|m| m.incoming).map(Message::identity).collect();
        Self { seen, baselined: !known.is_empty() }
    }

    /// Unread texts not seen before, oldest first.
    pub fn take_new(&mut self, inbox: &[MessageEntry]) -> Vec<Message> {
        let mut fresh = Vec::new();
        for e in inbox {
            let Some(m) = to_message(e, true) else { continue };
            if self.seen.insert(m.identity()) && self.baselined && !m.read {
                fresh.push(m);
            }
        }
        self.baselined = true;
        fresh.sort_by_key(|m| m.at);
        fresh
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(from: &str, to: &str, text: &str, dt: &str, read: bool) -> MessageEntry {
        MessageEntry {
            handle: String::new(),
            subject: text.into(),
            datetime: dt.into(),
            sender_name: String::new(),
            sender_address: from.into(),
            recipient_name: String::new(),
            recipient_address: to.into(),
            kind: "SMS_GSM".into(),
            read,
            sent: false,
        }
    }

    #[test]
    fn parses_map_timestamps() {
        assert!(parse_map_time("20260417T123456").is_some());
        assert_eq!(parse_map_time("20260417T123456Z"), Some(1776429296));
        assert_eq!(parse_map_time("20260417T123456+0200"), Some(1776429296 - 7200));
        assert_eq!(parse_map_time("garbage"), None);
        assert_eq!(parse_map_time("20260417"), None);
    }

    #[test]
    fn groups_by_person_across_number_formats() {
        let mut c = Conversations::default();
        c.ingest(
            &[entry("+49 111 222 333", "", "hi", "20260101T100000Z", true), entry("+49 999 888 777", "", "later", "20260102T090000Z", false)],
            &[entry("", "0111222333", "hello", "20260101T100100Z", true)],
        );
        let convs = c.conversations();
        assert_eq!(convs.len(), 2);
        assert_eq!(convs[0].last().text, "later");
        let t = c.thread("0111 222 333");
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].text, "hi");
        assert!(!t[1].incoming);
        assert_eq!(c.unread_total(), 1);
    }

    #[test]
    fn ingest_is_idempotent_and_history_survives_the_phone_dropping_it() {
        let mut c = Conversations::default();
        let inbox = [entry("+1", "", "a", "20260101T100000Z", false)];
        assert!(c.ingest(&inbox, &[]));
        assert!(!c.ingest(&inbox, &[]));
        // The phone no longer lists it: we still have it.
        c.ingest(&[], &[]);
        assert_eq!(c.thread("+1").len(), 1);
    }

    #[test]
    fn read_flag_turns_on_never_off() {
        let mut c = Conversations::default();
        c.ingest(&[entry("+1", "", "a", "20260101T100000Z", false)], &[]);
        assert_eq!(c.unread_total(), 1);
        assert!(c.ingest(&[entry("+1", "", "a", "20260101T100000Z", true)], &[]));
        assert_eq!(c.unread_total(), 0);
        c.ingest(&[entry("+1", "", "a", "20260101T100000Z", false)], &[]);
        assert_eq!(c.unread_total(), 0);
    }

    #[test]
    fn pending_shows_then_is_replaced_by_the_phones_copy() {
        let mut c = Conversations::default();
        c.add_pending("+49123", "on my way", 1_000);
        let t = c.thread("+49123");
        assert!(t[0].pending);
        // The phone lists it a little later.
        let dt = chrono::DateTime::from_timestamp(1_050, 0).unwrap().format("%Y%m%dT%H%M%SZ").to_string();
        c.ingest(&[], &[entry("", "+49123", "on my way", &dt, true)]);
        let t = c.thread("+49123");
        assert_eq!(t.len(), 1);
        assert!(!t[0].pending);
    }

    #[test]
    fn pending_expires() {
        let mut c = Conversations::default();
        c.add_pending("+1", "x", 0);
        assert!(!c.expire_pending(100));
        assert!(c.expire_pending(1000));
        assert!(c.conversations().is_empty());
    }

    #[test]
    fn watcher_baselines_then_announces_only_new_unread() {
        let mut w = InboxWatcher::new(&Conversations::default());
        let a = entry("+1", "", "old", "20260101T100000Z", false);
        assert!(w.take_new(std::slice::from_ref(&a)).is_empty());
        let b = entry("+1", "", "new", "20260101T110000Z", false);
        let read = entry("+1", "", "seen", "20260101T120000Z", true);
        let fresh = w.take_new(&[a.clone(), b.clone(), read]);
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].text, "new");
        assert!(w.take_new(&[a, b]).is_empty());
    }

    #[test]
    fn watcher_with_a_cache_catches_up_on_texts_that_arrived_while_closed() {
        let mut stored = Conversations::default();
        stored.ingest(&[entry("+1", "", "old", "20260101T100000Z", true)], &[]);
        let mut w = InboxWatcher::new(&stored);
        let fresh = w.take_new(&[entry("+1", "", "old", "20260101T100000Z", true), entry("+1", "", "while away", "20260102T100000Z", false)]);
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].text, "while away");
    }
}
