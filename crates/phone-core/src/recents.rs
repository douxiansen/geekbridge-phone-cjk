// SPDX-License-Identifier: GPL-3.0-or-later
//! The call history shown in Recents.

use crate::calls::Direction;
use crate::contacts::number_key;
use serde::{Deserialize, Serialize};

const MAX_ENTRIES: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Incoming,
    Outgoing,
    Missed,
}

impl From<&Direction> for Kind {
    fn from(d: &Direction) -> Self {
        match d {
            Direction::Incoming => Kind::Incoming,
            Direction::Outgoing => Kind::Outgoing,
            Direction::Missed => Kind::Missed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recent {
    pub number: String,
    pub kind: Kind,
    /// Unix seconds, local clock.
    pub at: i64,
    pub seconds: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Recents {
    entries: Vec<Recent>,
}

impl Recents {
    /// Newest first.
    pub fn entries(&self) -> &[Recent] {
        &self.entries
    }

    /// Rebuilds from stored entries.
    pub fn from_entries(mut entries: Vec<Recent>) -> Self {
        entries.sort_by_key(|e| std::cmp::Reverse(e.at));
        entries.truncate(MAX_ENTRIES);
        Self { entries }
    }

    /// Adds a call; false when it was already there (same number, kind and minute).
    pub fn add(&mut self, entry: Recent) -> bool {
        // The same call can arrive from the live tracker and from the phone's
        // history: treat the same number, kind and minute as one call.
        let dup = self.entries.iter().any(|e| {
            number_key(&e.number) == number_key(&entry.number)
                && e.kind == entry.kind
                && (e.at - entry.at).abs() < 90
        });
        if dup {
            return false;
        }
        self.entries.push(entry);
        self.entries.sort_by_key(|e| std::cmp::Reverse(e.at));
        self.entries.truncate(MAX_ENTRIES);
        true
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(n: &str, kind: Kind, at: i64) -> Recent {
        Recent {
            number: n.into(),
            kind,
            at,
            seconds: 0,
        }
    }

    #[test]
    fn newest_first_and_deduplicated() {
        let mut h = Recents::default();
        h.add(r("+1 555", Kind::Missed, 100));
        h.add(r("0155 5", Kind::Missed, 140)); // same call seen twice, formatted differently? different digits, kept
        h.add(r("+1 555", Kind::Missed, 130)); // within 90 s of the first
        h.add(r("+1 555", Kind::Incoming, 1000));
        assert_eq!(h.entries()[0].at, 1000);
        assert_eq!(h.len(), 3);
    }

    #[test]
    fn is_capped() {
        let mut h = Recents::default();
        for i in 0..400 {
            h.add(r("1", Kind::Incoming, i * 1000));
        }
        assert_eq!(h.len(), 300);
    }
}
