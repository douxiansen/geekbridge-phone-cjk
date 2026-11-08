// SPDX-License-Identifier: GPL-3.0-or-later
//! How a conversation is laid out: which bubbles share a run, and where timestamps go.

use super::format;
use chrono::{DateTime, Datelike, FixedOffset};

/// A gap this long between two messages gets a timestamp between them.
pub const STAMP_GAP_SECS: i64 = 15 * 60;
/// Messages from the same side this close together are drawn as one tight run.
pub const RUN_GAP_SECS: i64 = 5 * 60;

/// What the layout needs to know about one message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    pub at: i64,
    pub outgoing: bool,
}

/// One line of the thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// A centred timestamp.
    Stamp(String),
    /// A bubble: `index` into the slots, and where it sits in its run (for corner shapes and spacing).
    Bubble { index: usize, first: bool, last: bool },
}

fn day_of(ts: i64, off: FixedOffset) -> Option<(i32, u32)> {
    DateTime::from_timestamp(ts, 0).map(|d| d.with_timezone(&off)).map(|d| (d.year(), d.ordinal()))
}

/// Lays out `slots` (oldest first): a stamp before the first message and wherever the conversation
/// paused or changed day, and `first`/`last` flags for each bubble's run.
pub fn rows(slots: &[Slot], now: i64, off: FixedOffset) -> Vec<Row> {
    let mut out = Vec::with_capacity(slots.len() * 2);
    for (i, slot) in slots.iter().enumerate() {
        let prev = i.checked_sub(1).map(|p| slots[p]);
        let stamp = match prev {
            None => true,
            Some(p) => slot.at - p.at > STAMP_GAP_SECS || day_of(slot.at, off) != day_of(p.at, off),
        };
        if stamp {
            out.push(Row::Stamp(format::thread_stamp(slot.at, now, off)));
        }
        let joins_prev = !stamp && prev.is_some_and(|p| p.outgoing == slot.outgoing && slot.at - p.at <= RUN_GAP_SECS);
        let next = slots.get(i + 1);
        let next_stamps = next.is_some_and(|n| n.at - slot.at > STAMP_GAP_SECS || day_of(n.at, off) != day_of(slot.at, off));
        let joins_next = !next_stamps && next.is_some_and(|n| n.outgoing == slot.outgoing && n.at - slot.at <= RUN_GAP_SECS);
        out.push(Row::Bubble { index: i, first: !joins_prev, last: !joins_next });
    }
    out
}

/// One line for a conversation list: newlines and runs of spaces collapsed, "You: " in front of
/// our own messages.
pub fn preview(text: &str, outgoing: bool) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if outgoing {
        format!("You: {flat}")
    } else {
        flat
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn off() -> FixedOffset {
        FixedOffset::east_opt(0).unwrap()
    }
    fn t(d: u32, h: u32, m: u32) -> i64 {
        off().with_ymd_and_hms(2026, 11, d, h, m, 0).unwrap().timestamp()
    }
    fn s(at: i64, outgoing: bool) -> Slot {
        Slot { at, outgoing }
    }

    #[test]
    fn the_first_message_always_has_a_stamp() {
        let r = rows(&[s(t(8, 12, 0), false)], t(8, 12, 5), off());
        assert_eq!(r.len(), 2);
        assert!(matches!(&r[0], Row::Stamp(x) if x == "Today 12:00"));
        assert_eq!(r[1], Row::Bubble { index: 0, first: true, last: true });
    }

    #[test]
    fn a_quiet_spell_or_a_new_day_gets_a_stamp() {
        let slots = [s(t(7, 23, 50), false), s(t(8, 0, 5), false), s(t(8, 0, 40), true)];
        let r = rows(&slots, t(8, 1, 0), off());
        let stamps: Vec<_> = r.iter().filter(|x| matches!(x, Row::Stamp(_))).collect();
        // First message; new calendar day (15 minutes later); 35 minutes later.
        assert_eq!(stamps.len(), 3);
    }

    #[test]
    fn same_side_messages_close_together_form_a_run() {
        let slots = [s(t(8, 12, 0), false), s(t(8, 12, 1), false), s(t(8, 12, 2), false), s(t(8, 12, 3), true)];
        let r = rows(&slots, t(8, 12, 10), off());
        let bubbles: Vec<_> = r.iter().filter_map(|x| if let Row::Bubble { first, last, .. } = x { Some((*first, *last)) } else { None }).collect();
        assert_eq!(bubbles, vec![(true, false), (false, false), (false, true), (true, true)]);
    }

    #[test]
    fn a_pause_inside_a_side_ends_the_run_without_a_stamp() {
        // 8 minutes: more than a run, less than a stamp.
        let slots = [s(t(8, 12, 0), true), s(t(8, 12, 8), true)];
        let r = rows(&slots, t(8, 12, 20), off());
        assert_eq!(r.iter().filter(|x| matches!(x, Row::Stamp(_))).count(), 1);
        assert_eq!(r[1], Row::Bubble { index: 0, first: true, last: true });
        assert_eq!(r[2], Row::Bubble { index: 1, first: true, last: true });
    }

    #[test]
    fn an_empty_thread_has_no_rows() {
        assert!(rows(&[], 0, off()).is_empty());
    }

    #[test]
    fn previews_are_one_line() {
        assert_eq!(preview("Hello\n  there\tfriend", false), "Hello there friend");
        assert_eq!(preview("On my way", true), "You: On my way");
        assert_eq!(preview("", false), "");
    }
}
