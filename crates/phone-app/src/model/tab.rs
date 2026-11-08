// SPDX-License-Identifier: GPL-3.0-or-later
//! The five places of the app.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tab {
    #[default]
    Keypad,
    Contacts,
    Recents,
    Messages,
    Settings,
}

impl Tab {
    /// In sidebar order; the position is also the Ctrl/Cmd+number shortcut (1 to 5).
    pub const ALL: [Tab; 5] = [
        Tab::Keypad,
        Tab::Contacts,
        Tab::Recents,
        Tab::Messages,
        Tab::Settings,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Keypad => "Keypad",
            Tab::Contacts => "Contacts",
            Tab::Recents => "Recents",
            Tab::Messages => "Messages",
            Tab::Settings => "Settings",
        }
    }

    /// 1-based shortcut number.
    pub fn number(self) -> usize {
        Self::ALL
            .iter()
            .position(|t| *t == self)
            .map_or(1, |i| i + 1)
    }

    /// The tab with shortcut number `n` (1 to 5).
    pub fn from_number(n: usize) -> Option<Tab> {
        n.checked_sub(1).and_then(|i| Self::ALL.get(i)).copied()
    }

    /// Parses a tab name from the command line.
    pub fn parse(s: &str) -> Option<Tab> {
        Self::ALL
            .iter()
            .copied()
            .find(|t| t.label().eq_ignore_ascii_case(s.trim()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_round_trip() {
        for t in Tab::ALL {
            assert_eq!(Tab::from_number(t.number()), Some(t));
        }
        assert_eq!(Tab::from_number(0), None);
        assert_eq!(Tab::from_number(6), None);
        assert_eq!(Tab::Messages.number(), 4);
    }

    #[test]
    fn names_parse_in_any_case() {
        assert_eq!(Tab::parse("messages"), Some(Tab::Messages));
        assert_eq!(Tab::parse(" RECENTS "), Some(Tab::Recents));
        assert_eq!(Tab::parse("nope"), None);
    }

    #[test]
    fn saved_names_are_lowercase() {
        assert_eq!(
            serde_json::to_string(&Tab::Contacts).unwrap(),
            "\"contacts\""
        );
    }
}
