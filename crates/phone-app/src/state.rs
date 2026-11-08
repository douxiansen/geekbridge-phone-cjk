// SPDX-License-Identifier: GPL-3.0-or-later
//! The app's own small memory, kept next to the settings: the tab it was on and which texts the
//! user has already read on this computer. (The phone keeps its own read state; reading here
//! does not change it.)

use crate::model::tab::Tab;
use phone_core::conversations::Conversation;
use phone_core::store;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiState {
    pub tab: Tab,
    /// For each conversation key, the time of the newest message that has been seen.
    pub read_until: HashMap<String, i64>,
}

impl UiState {
    /// Marks everything in `key` up to `at` as read. Returns true when that changed anything.
    pub fn mark_read(&mut self, key: &str, at: i64) -> bool {
        let entry = self.read_until.entry(key.to_string()).or_insert(i64::MIN);
        if at > *entry {
            *entry = at;
            true
        } else {
            false
        }
    }

    /// Texts in `conv` that came in, are unread on the phone, and have not been seen here.
    pub fn unread_in(&self, conv: &Conversation) -> usize {
        let seen = self.read_until.get(&conv.key).copied().unwrap_or(i64::MIN);
        conv.messages.iter().filter(|m| m.incoming && !m.read && m.at > seen).count()
    }

    pub fn unread_total(&self, conversations: &[Conversation]) -> usize {
        conversations.iter().map(|c| self.unread_in(c)).sum()
    }
}

/// Loads and saves a [`UiState`].
pub struct UiStore {
    path: PathBuf,
}

impl UiStore {
    pub fn new(config_dir: &std::path::Path) -> Self {
        Self { path: config_dir.join("ui.json") }
    }

    pub fn load(&self) -> UiState {
        store::load(&self.path)
    }

    /// Saves; a failure is logged and otherwise ignored (losing a tab choice is harmless).
    pub fn save(&self, state: &UiState) {
        if let Err(e) = store::save(&self.path, state) {
            log::warn!("cannot save {}: {e}", self.path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phone_core::conversations::Message;

    fn conv(messages: Vec<Message>) -> Conversation {
        Conversation { key: "k".into(), number: "+1".into(), messages }
    }
    fn m(at: i64, incoming: bool, read: bool) -> Message {
        Message { number: "+1".into(), incoming, text: "x".into(), at, read, pending: false }
    }

    #[test]
    fn unread_counts_only_incoming_unread_and_unseen() {
        let c = conv(vec![m(10, true, false), m(20, true, false), m(30, false, true), m(40, true, true)]);
        let mut st = UiState::default();
        assert_eq!(st.unread_in(&c), 2);
        st.mark_read("k", 10);
        assert_eq!(st.unread_in(&c), 1);
        st.mark_read("k", 40);
        assert_eq!(st.unread_in(&c), 0);
    }

    #[test]
    fn reading_never_goes_backwards() {
        let mut st = UiState::default();
        assert!(st.mark_read("k", 50));
        assert!(!st.mark_read("k", 40));
        assert!(!st.mark_read("k", 50));
        assert_eq!(st.read_until["k"], 50);
    }

    #[test]
    fn a_new_text_after_reading_is_unread_again() {
        let mut st = UiState::default();
        st.mark_read("k", 100);
        let c = conv(vec![m(90, true, false), m(110, true, false)]);
        assert_eq!(st.unread_in(&c), 1);
        assert_eq!(st.unread_total(&[c.clone(), c]), 2);
    }

    #[test]
    fn state_survives_a_round_trip_and_a_bad_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = UiStore::new(dir.path());
        let mut st = UiState { tab: Tab::Messages, ..UiState::default() };
        st.mark_read("k", 5);
        store.save(&st);
        assert_eq!(store.load(), st);
        std::fs::write(dir.path().join("ui.json"), b"{ nope").unwrap();
        assert_eq!(store.load(), UiState::default());
    }
}
