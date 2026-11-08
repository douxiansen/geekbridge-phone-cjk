// SPDX-License-Identifier: GPL-3.0-or-later
//! What an avatar circle shows: initials and a colour that is stable per person.

/// How many avatar colours the UI has.
pub const COLOURS: usize = 8;

/// Up to two capital letters from a name: first and last word ("Mara Ionescu" is "MI", "Dr. Keller"
/// is "DK", "Zoé" is "Z"). `None` when the name has no letters (a bare number), so the UI can draw
/// a silhouette instead.
pub fn initials(name: &str) -> Option<String> {
    let first_letter = |word: &str| word.chars().find(|c| c.is_alphabetic()).map(|c| c.to_uppercase().to_string());
    let mut words = name.split_whitespace().filter(|w| w.chars().any(char::is_alphabetic));
    let first = words.next()?;
    let a = first_letter(first)?;
    match words.last().and_then(first_letter) {
        Some(b) => Some(format!("{a}{b}")),
        None => Some(a),
    }
}

/// A colour slot in `0..COLOURS` that never changes for the same key (FNV-1a, so it is the same
/// on every run and every platform).
pub fn colour_index(key: &str) -> usize {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in key.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    (hash % COLOURS as u64) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_are_first_and_last_word() {
        assert_eq!(initials("Mara Ionescu").as_deref(), Some("MI"));
        assert_eq!(initials("Dr. Keller").as_deref(), Some("DK"));
        assert_eq!(initials("Zoé").as_deref(), Some("Z"));
        assert_eq!(initials("anna maria de souza").as_deref(), Some("AS"));
        assert_eq!(initials("  Tomás   Ribeiro ").as_deref(), Some("TR"));
    }

    #[test]
    fn initials_handle_non_latin_and_decorated_names() {
        assert_eq!(initials("Жанна Петрова").as_deref(), Some("ЖП"));
        assert_eq!(initials("(Work) Sam").as_deref(), Some("WS"));
        // A lone ß upper-cases to two letters; the UI only ever draws what this returns.
        assert_eq!(initials("ß").as_deref(), Some("SS"));
    }

    #[test]
    fn numbers_and_blanks_have_no_initials() {
        assert_eq!(initials("+213 550 123 456"), None);
        assert_eq!(initials(""), None);
        assert_eq!(initials("   "), None);
        assert_eq!(initials("12345"), None);
    }

    #[test]
    fn colours_are_stable_and_in_range() {
        for key in ["Alí Boudjemaa", "+213550123456", "", "a"] {
            assert!(colour_index(key) < COLOURS);
            assert_eq!(colour_index(key), colour_index(key));
        }
        // Known value so an accidental change of hash is caught: it would recolour everyone.
        assert_eq!(colour_index("a"), (0xaf63_dc4c_8601_ec8c_u64 % COLOURS as u64) as usize);
    }

    #[test]
    fn the_palette_is_used_widely() {
        let mut seen = [false; COLOURS];
        for i in 0..200 {
            seen[colour_index(&format!("contact {i}"))] = true;
        }
        assert!(seen.iter().all(|s| *s));
    }
}
