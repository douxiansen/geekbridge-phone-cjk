// SPDX-License-Identifier: GPL-3.0-or-later
//! The keypad's data: keys, their letters, and the rules for what a number field accepts.

/// Longest number the keypad accepts. E.164 allows 15 digits; the rest is room for `*`, `#` and
/// a pause or extension typed by hand.
pub const MAX_LEN: usize = 32;

/// A key on the pad: the character it types and the letters under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub ch: char,
    pub letters: &'static str,
}

/// The twelve keys in reading order.
pub const KEYS: [Key; 12] = [
    Key { ch: '1', letters: "" },
    Key { ch: '2', letters: "ABC" },
    Key { ch: '3', letters: "DEF" },
    Key { ch: '4', letters: "GHI" },
    Key { ch: '5', letters: "JKL" },
    Key { ch: '6', letters: "MNO" },
    Key { ch: '7', letters: "PQRS" },
    Key { ch: '8', letters: "TUV" },
    Key { ch: '9', letters: "WXYZ" },
    Key { ch: '*', letters: "" },
    Key { ch: '0', letters: "+" },
    Key { ch: '#', letters: "" },
];

/// The letters printed under a digit key.
pub fn letters_for(ch: char) -> &'static str {
    KEYS.iter().find(|k| k.ch == ch).map_or("", |k| k.letters)
}

/// The key a letter is on, for typing "FLOWERS" on a keyboard (`a` to `z`, any case).
pub fn digit_for_letter(c: char) -> Option<char> {
    Some(match c.to_ascii_lowercase() {
        'a'..='c' => '2',
        'd'..='f' => '3',
        'g'..='i' => '4',
        'j'..='l' => '5',
        'm'..='o' => '6',
        'p'..='s' => '7',
        't'..='v' => '8',
        'w'..='z' => '9',
        _ => return None,
    })
}

/// Whether `ch` can be part of a dialled number.
pub fn is_dial_char(ch: char) -> bool {
    ch.is_ascii_digit() || matches!(ch, '*' | '#' | '+')
}

/// Appends typed characters to `number`, keeping only dialable ones, a `+` only at the start,
/// and never growing past [`MAX_LEN`]. Returns true when anything was added.
pub fn push_str(number: &mut String, typed: &str) -> bool {
    let mut added = false;
    for ch in typed.chars() {
        if number.chars().count() >= MAX_LEN {
            break;
        }
        if !is_dial_char(ch) || (ch == '+' && !number.is_empty()) {
            continue;
        }
        number.push(ch);
        added = true;
    }
    added
}

/// Removes the last character. Returns true when there was one.
pub fn backspace(number: &mut String) -> bool {
    number.pop().is_some()
}

/// Cleans text pasted into the keypad: "+1 (555) 010-0100" becomes "+15550100100"; letters
/// are mapped to their keys only when the text looks like a vanity number would ("1-800-FLOWERS").
pub fn sanitise_paste(text: &str) -> String {
    let trimmed = text.trim();
    let has_digit = trimmed.chars().any(|c| c.is_ascii_digit());
    let mut out = String::new();
    for ch in trimmed.chars() {
        let mapped = if ch.is_ascii_alphabetic() && has_digit { digit_for_letter(ch) } else { Some(ch) };
        if let Some(c) = mapped {
            push_str(&mut out, &c.to_string());
        }
    }
    out
}

/// The number in a `tel:` (or `callto:`) link: `tel:+1-555-010-0100;ext=5` is `+15550100100`.
/// Percent escapes are decoded (`%2B` is `+`). `None` when the text is not such a link or has no
/// dialable characters.
pub fn parse_tel_uri(uri: &str) -> Option<String> {
    let lower = uri.trim();
    let rest = ["tel://", "tel:", "callto://", "callto:"].iter().find_map(|p| lower.get(..p.len()).filter(|h| h.eq_ignore_ascii_case(p)).map(|_| &lower[p.len()..]))?;
    // Parameters after `;` (extension, context) and a query after `?` are not part of the number.
    let number = rest.split([';', '?', '#']).next().unwrap_or("");
    let mut decoded = String::new();
    let mut chars = number.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            match u8::from_str_radix(&hex, 16) {
                Ok(b) => decoded.push(char::from(b)),
                Err(_) => return None,
            }
        } else {
            decoded.push(c);
        }
    }
    let mut out = String::new();
    push_str(&mut out, &decoded);
    can_dial(&out).then_some(out)
}

/// What the readout shows: the whole number, or its tail with an ellipsis when it is longer than
/// `max_chars`, so the digits being typed are always visible.
pub fn readout(number: &str, max_chars: usize) -> String {
    let count = number.chars().count();
    if count <= max_chars || max_chars < 2 {
        return number.to_string();
    }
    let tail: String = number.chars().skip(count - (max_chars - 1)).collect();
    format!("…{tail}")
}

/// Whether the number can be dialled at all.
pub fn can_dial(number: &str) -> bool {
    number.chars().filter(char::is_ascii_digit).count() >= 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_match_a_phone() {
        assert_eq!(letters_for('2'), "ABC");
        assert_eq!(letters_for('7'), "PQRS");
        assert_eq!(letters_for('9'), "WXYZ");
        assert_eq!(letters_for('1'), "");
        assert_eq!(letters_for('0'), "+");
        assert_eq!(letters_for('x'), "");
    }

    #[test]
    fn every_letter_is_on_exactly_the_key_that_prints_it() {
        for c in 'a'..='z' {
            let d = digit_for_letter(c).unwrap();
            assert!(letters_for(d).contains(c.to_ascii_uppercase()), "{c} is not under {d}");
        }
        assert_eq!(digit_for_letter('Q'), Some('7'));
        assert_eq!(digit_for_letter('1'), None);
    }

    #[test]
    fn typing_keeps_only_dialable_characters() {
        let mut n = String::new();
        assert!(push_str(&mut n, "+1 (555) 010-0100x"));
        assert_eq!(n, "+15550100100");
        assert!(!push_str(&mut n, "abc"));
        assert_eq!(n, "+15550100100");
    }

    #[test]
    fn a_plus_is_only_allowed_first() {
        let mut n = String::new();
        push_str(&mut n, "12+3");
        assert_eq!(n, "123");
        let mut m = String::new();
        push_str(&mut m, "+4+4");
        assert_eq!(m, "+44");
    }

    #[test]
    fn the_length_is_capped() {
        let mut n = String::new();
        push_str(&mut n, &"5".repeat(100));
        assert_eq!(n.len(), MAX_LEN);
        assert!(!push_str(&mut n, "1"));
    }

    #[test]
    fn backspace_removes_one_char_and_reports_empty() {
        let mut n = "12".to_string();
        assert!(backspace(&mut n));
        assert!(backspace(&mut n));
        assert!(!backspace(&mut n));
        assert!(n.is_empty());
    }

    #[test]
    fn paste_is_cleaned() {
        assert_eq!(sanitise_paste("  +44 20 7946 0958\n"), "+442079460958");
        assert_eq!(sanitise_paste("1-800-FLOWERS"), "18003569377");
        assert_eq!(sanitise_paste("call me"), "");
        assert_eq!(sanitise_paste(&"1".repeat(80)).len(), MAX_LEN);
    }

    #[test]
    fn readout_shows_the_tail_of_a_long_number() {
        assert_eq!(readout("12345", 8), "12345");
        assert_eq!(readout("1234567890", 6), "…67890");
        assert_eq!(readout("1234567890", 6).chars().count(), 6);
        assert_eq!(readout("123", 1), "123");
    }

    #[test]
    fn tel_links_give_the_number() {
        assert_eq!(parse_tel_uri("tel:+1-555-010-0100").as_deref(), Some("+15550100100"));
        assert_eq!(parse_tel_uri("TEL:%2B44%2020%207946%200958").as_deref(), Some("+442079460958"));
        assert_eq!(parse_tel_uri("tel://0612345678;ext=12").as_deref(), Some("0612345678"));
        assert_eq!(parse_tel_uri("callto:112?x=1").as_deref(), Some("112"));
        assert_eq!(parse_tel_uri("  tel:*21#  ").as_deref(), Some("*21#"));
    }

    #[test]
    fn other_text_is_not_a_tel_link() {
        assert_eq!(parse_tel_uri("https://example.com"), None);
        assert_eq!(parse_tel_uri("tel:"), None);
        assert_eq!(parse_tel_uri("tel:abc"), None);
        assert_eq!(parse_tel_uri("tel:%zz1"), None);
        assert_eq!(parse_tel_uri("telephone:123"), None);
        assert_eq!(parse_tel_uri(""), None);
    }

    #[test]
    fn dialling_needs_a_digit() {
        assert!(can_dial("112"));
        assert!(can_dial("+4"));
        assert!(!can_dial(""));
        assert!(!can_dial("+"));
        assert!(!can_dial("*#"));
    }
}
