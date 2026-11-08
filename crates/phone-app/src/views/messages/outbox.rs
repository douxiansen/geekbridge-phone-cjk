// SPDX-License-Identifier: GPL-3.0-or-later
//! Texts sent from this window that the phone has not confirmed yet, or that failed.
//!
//! The service adds a faded "pending" copy to the conversation only after the phone accepted the
//! text. Until then, and for good if it failed, the text lives here so the user sees it.

/// How one text is doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    Sending,
    Failed(String),
}

/// One text sent from here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    pub token: u64,
    pub key: String,
    pub number: String,
    pub text: String,
    pub at: i64,
    pub delivery: Delivery,
}

impl Outgoing {
    pub fn failure(&self) -> Option<&str> {
        match &self.delivery {
            Delivery::Failed(why) => Some(why),
            Delivery::Sending => None,
        }
    }
}

#[derive(Debug, Default)]
pub struct Outbox {
    items: Vec<Outgoing>,
}

impl Outbox {
    /// Records a text just handed to the service under `token`.
    pub fn add_with_token(&mut self, token: u64, key: &str, number: &str, text: &str, at: i64) -> u64 {
        self.items.push(Outgoing { token, key: key.into(), number: number.into(), text: text.into(), at, delivery: Delivery::Sending });
        token
    }

    /// Applies a result. Returns false when no text here has that token.
    pub fn settle(&mut self, token: u64, result: &Result<(), String>) -> bool {
        let Some(pos) = self.items.iter().position(|o| o.token == token) else {
            return false;
        };
        match result {
            Ok(()) => {
                self.items.remove(pos);
            }
            Err(why) => self.items[pos].delivery = Delivery::Failed(why.clone()),
        }
        true
    }

    /// Drops a text (the user dismissed a failure, or is retrying it).
    pub fn remove(&mut self, token: u64) -> Option<Outgoing> {
        let pos = self.items.iter().position(|o| o.token == token)?;
        Some(self.items.remove(pos))
    }

    /// What is outstanding in one conversation, oldest first.
    pub fn entries<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a Outgoing> {
        self.items.iter().filter(move |o| o.key == key)
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_removes_failure_stays() {
        let mut b = Outbox::default();
        let a = b.add_with_token(1, "k", "+1", "hi", 10);
        let c = b.add_with_token(2, "k", "+1", "again", 11);
        assert!(b.settle(a, &Ok(())));
        assert!(b.settle(c, &Err("link down".into())));
        let left: Vec<_> = b.entries("k").collect();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].failure(), Some("link down"));
    }

    #[test]
    fn unknown_tokens_are_reported_as_not_ours() {
        let mut b = Outbox::default();
        assert!(!b.settle(5, &Ok(())));
    }

    #[test]
    fn entries_are_per_conversation_and_removable() {
        let mut b = Outbox::default();
        let t = b.add_with_token(1, "a", "+1", "x", 1);
        b.add_with_token(2, "b", "+2", "y", 1);
        assert_eq!(b.entries("a").count(), 1);
        assert_eq!(b.remove(t).map(|o| o.text), Some("x".to_string()));
        assert_eq!(b.entries("a").count(), 0);
        assert!(!b.is_empty());
        assert!(b.remove(t).is_none());
    }
}
