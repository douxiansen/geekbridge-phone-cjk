// SPDX-License-Identifier: GPL-3.0-or-later
//! Cleaning text from the phone before it is drawn.

use std::borrow::Cow;

/// Characters that only steer how the previous character is drawn (emoji presentation selectors,
/// joiners, directional marks). The text renderer has no glyph for them and shows an empty box.
fn is_invisible_format(c: char) -> bool {
    matches!(c, '\u{FE00}'..='\u{FE0F}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
}

/// `text` without the characters the renderer cannot show, and without control characters other
/// than newline and tab. Borrows when there is nothing to remove.
pub fn display_text(text: &str) -> Cow<'_, str> {
    let drop = |c: char| is_invisible_format(c) || (c.is_control() && c != '\n' && c != '\t');
    if text.chars().any(drop) {
        Cow::Owned(text.chars().filter(|c| !drop(*c)).collect())
    } else {
        Cow::Borrowed(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_borrowed_unchanged() {
        assert!(matches!(display_text("Merci pour hier, c'était génial !"), Cow::Borrowed(_)));
        assert_eq!(display_text("line one\nline two\tend"), "line one\nline two\tend");
    }

    #[test]
    fn emoji_selectors_and_joiners_are_removed() {
        assert_eq!(display_text("table 🍽\u{FE0F}"), "table 🍽");
        assert_eq!(display_text("👩\u{200D}💻"), "👩💻");
    }

    #[test]
    fn control_characters_and_direction_marks_are_removed() {
        assert_eq!(display_text("a\u{0007}b\r\nc"), "ab\nc");
        assert_eq!(display_text("\u{202E}abc\u{202C}"), "abc");
        assert_eq!(display_text("\u{FEFF}hello"), "hello");
    }
}
