// SPDX-License-Identifier: GPL-3.0-or-later

//! A minimal single-line text-edit state machine: cursor, backspace, delete,
//! left/right/home/end, word jumps. Paste-less. All positions are byte
//! offsets on grapheme-cluster boundaries, so multibyte text and emoji
//! sequences are never split.

use unicode_segmentation::UnicodeSegmentation;

use crate::input::Key;

/// Upper bound on the reply length in bytes; further input is ignored.
pub const MAX_BYTES: usize = 2000;

/// What a key press did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditOutcome {
    /// Nothing changed.
    None,
    /// Text or cursor changed; redraw.
    Changed,
    /// Enter was pressed.
    Submit,
    /// Escape was pressed.
    Cancel,
}

/// A single-line text field.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextField {
    text: String,
    /// Byte offset of the cursor; always on a grapheme boundary.
    cursor: usize,
    /// Horizontal scroll in logical pixels, maintained by the renderer's caller.
    pub scroll: f32,
}

impl TextField {
    /// An empty field.
    pub fn new() -> Self {
        Self::default()
    }

    /// The current text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether the field is empty.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Cursor position as a byte offset.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The text left of the cursor.
    pub fn before_cursor(&self) -> &str {
        &self.text[..self.cursor]
    }

    /// Empty the field.
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.scroll = 0.0;
    }

    /// Insert at the cursor. Control characters are dropped, tabs and
    /// newlines become spaces. Returns whether anything was inserted.
    pub fn insert(&mut self, s: &str) -> bool {
        let mut clean = String::with_capacity(s.len());
        for c in s.chars() {
            match c {
                '\t' | '\n' | '\r' => clean.push(' '),
                c if c.is_control() => {}
                c => clean.push(c),
            }
        }
        if clean.is_empty() || self.text.len() + clean.len() > MAX_BYTES {
            return false;
        }
        self.text.insert_str(self.cursor, &clean);
        self.cursor += clean.len();
        true
    }

    fn prev_boundary(&self, from: usize) -> usize {
        self.text[..from]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    fn next_boundary(&self, from: usize) -> usize {
        self.text[from..]
            .graphemes(true)
            .next()
            .map_or(self.text.len(), |g| from + g.len())
    }

    fn word_left(&self) -> usize {
        let mut i = self.cursor;
        let b = &self.text;
        while i > 0 {
            let p = self.prev_boundary(i);
            if b[p..i].chars().all(char::is_whitespace) {
                i = p;
            } else {
                break;
            }
        }
        while i > 0 {
            let p = self.prev_boundary(i);
            if b[p..i].chars().all(char::is_whitespace) {
                break;
            }
            i = p;
        }
        i
    }

    fn word_right(&self) -> usize {
        let mut i = self.cursor;
        let len = self.text.len();
        while i < len {
            let n = self.next_boundary(i);
            if self.text[i..n].chars().all(char::is_whitespace) {
                i = n;
            } else {
                break;
            }
        }
        while i < len {
            let n = self.next_boundary(i);
            if self.text[i..n].chars().all(char::is_whitespace) {
                break;
            }
            i = n;
        }
        i
    }

    /// Apply a key press.
    pub fn apply(&mut self, key: &Key) -> EditOutcome {
        match key {
            Key::Text(s) => {
                if self.insert(s) {
                    EditOutcome::Changed
                } else {
                    EditOutcome::None
                }
            }
            Key::Backspace => {
                if self.cursor == 0 {
                    return EditOutcome::None;
                }
                let p = self.prev_boundary(self.cursor);
                self.text.replace_range(p..self.cursor, "");
                self.cursor = p;
                EditOutcome::Changed
            }
            Key::Delete => {
                if self.cursor >= self.text.len() {
                    return EditOutcome::None;
                }
                let n = self.next_boundary(self.cursor);
                self.text.replace_range(self.cursor..n, "");
                EditOutcome::Changed
            }
            Key::BackspaceWord => {
                let p = self.word_left();
                if p == self.cursor {
                    return EditOutcome::None;
                }
                self.text.replace_range(p..self.cursor, "");
                self.cursor = p;
                EditOutcome::Changed
            }
            Key::Left => self.move_to(self.prev_boundary(self.cursor)),
            Key::Right => self.move_to(self.next_boundary(self.cursor)),
            Key::WordLeft => self.move_to(self.word_left()),
            Key::WordRight => self.move_to(self.word_right()),
            Key::Home => self.move_to(0),
            Key::End => self.move_to(self.text.len()),
            Key::Enter => EditOutcome::Submit,
            Key::Escape => EditOutcome::Cancel,
        }
    }

    fn move_to(&mut self, to: usize) -> EditOutcome {
        if to == self.cursor {
            EditOutcome::None
        } else {
            self.cursor = to;
            EditOutcome::Changed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(s: &str) -> TextField {
        let mut f = TextField::new();
        f.apply(&Key::Text(s.to_owned()));
        f
    }

    #[test]
    fn insert_and_backspace_ascii() {
        let mut f = typed("abc");
        assert_eq!(f.text(), "abc");
        assert_eq!(f.apply(&Key::Backspace), EditOutcome::Changed);
        assert_eq!(f.text(), "ab");
        f.apply(&Key::Left);
        f.apply(&Key::Text("X".into()));
        assert_eq!(f.text(), "aXb");
        assert_eq!(f.before_cursor(), "aX");
    }

    #[test]
    fn backspace_on_empty_and_delete_at_end_do_nothing() {
        let mut f = TextField::new();
        assert_eq!(f.apply(&Key::Backspace), EditOutcome::None);
        assert_eq!(f.apply(&Key::Delete), EditOutcome::None);
        assert_eq!(f.apply(&Key::Left), EditOutcome::None);
        assert_eq!(f.apply(&Key::Right), EditOutcome::None);
    }

    #[test]
    fn multibyte_is_never_split() {
        let mut f = typed("héllo wörld");
        for _ in 0..4 {
            f.apply(&Key::Left);
        }
        assert_eq!(f.before_cursor(), "héllo w");
        f.apply(&Key::Backspace);
        assert_eq!(f.text(), "héllo örld");
        f.apply(&Key::Home);
        f.apply(&Key::Right);
        f.apply(&Key::Right);
        assert_eq!(f.before_cursor(), "hé");
        f.apply(&Key::Backspace);
        assert_eq!(f.before_cursor(), "h");
        f.apply(&Key::Delete);
        assert_eq!(f.text(), "hlo örld");
    }

    #[test]
    fn emoji_clusters_move_as_one() {
        // Family emoji (ZWJ sequence) + flag + combining accent.
        let mut f = typed("a\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{1F1E9}\u{1F1EA}e\u{301}");
        f.apply(&Key::Backspace);
        assert_eq!(
            f.text(),
            "a\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{1F1E9}\u{1F1EA}"
        );
        f.apply(&Key::Backspace);
        assert_eq!(f.text(), "a\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}");
        f.apply(&Key::Left);
        assert_eq!(f.cursor(), 1);
        f.apply(&Key::Delete);
        assert_eq!(f.text(), "a");
    }

    #[test]
    fn home_end_and_word_moves() {
        let mut f = typed("one two  three");
        f.apply(&Key::Home);
        assert_eq!(f.cursor(), 0);
        f.apply(&Key::WordRight);
        assert_eq!(f.before_cursor(), "one");
        f.apply(&Key::WordRight);
        assert_eq!(f.before_cursor(), "one two");
        f.apply(&Key::End);
        f.apply(&Key::WordLeft);
        assert_eq!(f.before_cursor(), "one two  ");
        f.apply(&Key::BackspaceWord);
        assert_eq!(f.text(), "one three");
    }

    #[test]
    fn enter_submits_escape_cancels() {
        let mut f = typed("hi");
        assert_eq!(f.apply(&Key::Enter), EditOutcome::Submit);
        assert_eq!(f.apply(&Key::Escape), EditOutcome::Cancel);
        assert_eq!(f.text(), "hi");
    }

    #[test]
    fn control_characters_are_filtered() {
        let mut f = TextField::new();
        assert!(!f.insert("\u{7}\u{1b}"));
        assert!(f.insert("a\tb\nc\u{0}"));
        assert_eq!(f.text(), "a b c");
    }

    #[test]
    fn length_is_capped() {
        let mut f = TextField::new();
        assert!(f.insert(&"x".repeat(MAX_BYTES)));
        assert!(!f.insert("y"));
        assert_eq!(f.text().len(), MAX_BYTES);
        f.clear();
        assert!(f.is_empty());
        assert_eq!(f.cursor(), 0);
    }
}
