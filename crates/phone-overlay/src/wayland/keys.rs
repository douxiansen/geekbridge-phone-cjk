// SPDX-License-Identifier: GPL-3.0-or-later

//! Translate xkb key events into the core's platform-neutral [`Key`].

use smithay_client_toolkit::seat::keyboard::Keysym;

use crate::input::Key;

/// Map a key press. `ctrl` and `alt` are the held modifiers; `utf8` is what
/// xkb composed for the press.
pub fn translate(keysym: Keysym, utf8: Option<&str>, ctrl: bool, alt: bool) -> Option<Key> {
    let key = match keysym {
        Keysym::BackSpace if ctrl => Key::BackspaceWord,
        Keysym::BackSpace => Key::Backspace,
        Keysym::Delete | Keysym::KP_Delete => Key::Delete,
        Keysym::Left | Keysym::KP_Left if ctrl => Key::WordLeft,
        Keysym::Left | Keysym::KP_Left => Key::Left,
        Keysym::Right | Keysym::KP_Right if ctrl => Key::WordRight,
        Keysym::Right | Keysym::KP_Right => Key::Right,
        Keysym::Home | Keysym::KP_Home => Key::Home,
        Keysym::End | Keysym::KP_End => Key::End,
        Keysym::Return | Keysym::KP_Enter => Key::Enter,
        Keysym::Escape => Key::Escape,
        _ => {
            // Ctrl chords are shortcuts, not text.
            if ctrl {
                return None;
            }
            let _ = alt;
            let s = utf8?;
            if s.is_empty() || s.chars().all(char::is_control) {
                return None;
            }
            Key::Text(s.to_owned())
        }
    };
    Some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing_keys() {
        assert_eq!(
            translate(Keysym::BackSpace, None, false, false),
            Some(Key::Backspace)
        );
        assert_eq!(
            translate(Keysym::BackSpace, None, true, false),
            Some(Key::BackspaceWord)
        );
        assert_eq!(
            translate(Keysym::Return, Some("\r"), false, false),
            Some(Key::Enter)
        );
        assert_eq!(
            translate(Keysym::Escape, Some("\u{1b}"), false, false),
            Some(Key::Escape)
        );
        assert_eq!(
            translate(Keysym::Left, None, true, false),
            Some(Key::WordLeft)
        );
    }

    #[test]
    fn text_and_chords() {
        assert_eq!(
            translate(Keysym::a, Some("a"), false, false),
            Some(Key::Text("a".into()))
        );
        assert_eq!(
            translate(Keysym::eacute, Some("\u{e9}"), false, false),
            Some(Key::Text("\u{e9}".into()))
        );
        assert_eq!(translate(Keysym::a, Some("\u{1}"), true, false), None);
        assert_eq!(translate(Keysym::Shift_L, None, false, false), None);
        assert_eq!(translate(Keysym::Tab, Some("\t"), false, false), None);
    }
}
