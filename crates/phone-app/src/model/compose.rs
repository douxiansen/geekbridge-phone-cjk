// SPDX-License-Identifier: GPL-3.0-or-later
//! Rules for writing a text: the length limit, the counter and who it can go to.

/// Longest text the composer accepts, in characters.
pub const LIMIT: usize = 1000;
/// The counter is emphasised once this many characters are left or fewer.
pub const COUNTER_FROM_LEFT: usize = 300;

/// Where the composer stands for a draft.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counter {
    pub used: usize,
    pub left: usize,
}

impl Counter {
    pub fn of(text: &str) -> Self {
        let used = text.chars().count();
        Self {
            used,
            left: LIMIT.saturating_sub(used),
        }
    }

    /// Whether to show the "used/limit" label: whenever there is a draft.
    pub fn visible(&self) -> bool {
        self.used > 0
    }

    /// Whether the label should stand out because the limit is close.
    pub fn emphasised(&self) -> bool {
        self.left <= COUNTER_FROM_LEFT
    }

    pub fn at_limit(&self) -> bool {
        self.left == 0
    }

    pub fn label(&self) -> String {
        format!("{}/{}", self.used, LIMIT)
    }
}

/// Whether a draft can be sent: it has something other than whitespace and fits the limit.
pub fn can_send(text: &str) -> bool {
    !text.trim().is_empty() && text.chars().count() <= LIMIT
}

/// What actually gets sent: the draft without leading or trailing whitespace.
pub fn outgoing_text(text: &str) -> String {
    text.trim().to_string()
}

/// Cuts `text` to the limit (on a character boundary), for pasted text.
pub fn clamp(text: &mut String) {
    if text.chars().count() > LIMIT {
        *text = text.chars().take(LIMIT).collect();
    }
}

/// A recipient typed by hand, cleaned to what the phone needs: digits with an optional leading `+`.
/// `None` when it cannot be a phone number (fewer than three digits, or letters in it).
pub fn recipient_number(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut out = String::new();
    for (i, ch) in trimmed.chars().enumerate() {
        match ch {
            '0'..='9' => out.push(ch),
            '+' if i == 0 => out.push(ch),
            ' ' | '-' | '(' | ')' | '.' => {}
            _ => return None,
        }
    }
    (out.chars().filter(char::is_ascii_digit).count() >= 3).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_shows_with_a_draft_and_stands_out_near_the_limit() {
        assert!(!Counter::of("").visible());
        assert!(Counter::of("hello").visible());
        assert!(!Counter::of("hello").emphasised());
        let near = "x".repeat(LIMIT - COUNTER_FROM_LEFT);
        assert!(Counter::of(&near).emphasised());
        assert_eq!(
            Counter::of(&near).label(),
            format!("{}/1000", LIMIT - COUNTER_FROM_LEFT)
        );
    }

    #[test]
    fn counter_counts_characters_not_bytes() {
        let c = Counter::of("héllo 🍽️");
        assert_eq!(c.used, "héllo 🍽️".chars().count());
        assert!(c.used < "héllo 🍽️".len());
    }

    #[test]
    fn limit_is_inclusive() {
        let full = "a".repeat(LIMIT);
        assert!(Counter::of(&full).at_limit());
        assert!(can_send(&full));
        assert!(!can_send(&format!("{full}a")));
    }

    #[test]
    fn blank_drafts_cannot_be_sent() {
        assert!(!can_send(""));
        assert!(!can_send("  \n\t "));
        assert!(can_send(" hi "));
        assert_eq!(outgoing_text("  hi there \n"), "hi there");
    }

    #[test]
    fn clamp_cuts_on_a_character_boundary() {
        let mut s = "é".repeat(LIMIT + 50);
        clamp(&mut s);
        assert_eq!(s.chars().count(), LIMIT);
        let mut short = "ok".to_string();
        clamp(&mut short);
        assert_eq!(short, "ok");
    }

    #[test]
    fn recipients_are_cleaned_or_refused() {
        assert_eq!(
            recipient_number("+1 (555) 010-0100").as_deref(),
            Some("+15550100100")
        );
        assert_eq!(
            recipient_number(" 0612 345 678 ").as_deref(),
            Some("0612345678")
        );
        assert_eq!(recipient_number("12"), None);
        assert_eq!(recipient_number("Alice"), None);
        assert_eq!(recipient_number("555+0100"), None);
        assert_eq!(recipient_number(""), None);
    }
}
