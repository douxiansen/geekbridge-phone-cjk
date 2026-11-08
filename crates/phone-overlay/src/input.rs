// SPDX-License-Identifier: GPL-3.0-or-later

//! Platform-neutral input events a presenter feeds into the core.

/// A keyboard event for the reply field, already translated by the presenter
/// (keysyms, virtual keys, ... are the platform's business).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    /// Typed text (a key press that produces characters, or an IME commit).
    Text(String),
    /// Delete the grapheme before the cursor.
    Backspace,
    /// Delete the word before the cursor (Ctrl+Backspace).
    BackspaceWord,
    /// Delete the grapheme after the cursor.
    Delete,
    /// Cursor left.
    Left,
    /// Cursor right.
    Right,
    /// Cursor one word left (Ctrl+Left).
    WordLeft,
    /// Cursor one word right (Ctrl+Right).
    WordRight,
    /// Cursor to the start.
    Home,
    /// Cursor to the end.
    End,
    /// Send the reply.
    Enter,
    /// Collapse the reply field.
    Escape,
}

/// Pointer button transitions the core cares about (the primary button).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonState {
    /// Pressed.
    Down,
    /// Released.
    Up,
}
