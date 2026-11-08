// SPDX-License-Identifier: GPL-3.0-or-later
//! A small vCard reader for what PBAP returns: contacts and call history.

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Card {
    pub name: String,
    pub numbers: Vec<String>,
    /// `X-IRMC-CALL-DATETIME` value (`yyyymmddThhmmss`), for call history.
    pub call_time: Option<String>,
}

/// Joins folded lines (a line starting with a space or tab continues the last).
fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.trim_end_matches('\r');
        if (line.starts_with(' ') || line.starts_with('\t')) && !lines.is_empty() {
            lines.last_mut().unwrap().push_str(&line[1..]);
        } else {
            lines.push(line.to_string());
        }
    }
    lines
}

fn unescape(value: &str) -> String {
    value
        .replace("\\n", " ")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
}

/// Builds a display name from an `N:` value (family;given;additional;prefix;suffix).
fn name_from_n(value: &str) -> String {
    let p: Vec<&str> = value.split(';').collect();
    let given = p.get(1).copied().unwrap_or("").trim();
    let family = p.first().copied().unwrap_or("").trim();
    unescape(format!("{given} {family}").trim())
}

pub fn parse(text: &str) -> Vec<Card> {
    let mut cards = Vec::new();
    let mut current: Option<Card> = None;
    let mut n_name = String::new();
    for line in unfold(text) {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let property = key.split(';').next().unwrap_or("").to_ascii_uppercase();
        match property.as_str() {
            "BEGIN" if value.eq_ignore_ascii_case("VCARD") => {
                current = Some(Card::default());
                n_name.clear();
            }
            "END" if value.eq_ignore_ascii_case("VCARD") => {
                if let Some(mut c) = current.take() {
                    if c.name.is_empty() {
                        c.name = std::mem::take(&mut n_name);
                    }
                    cards.push(c);
                }
            }
            "FN" => {
                if let Some(c) = current.as_mut() {
                    c.name = unescape(value.trim());
                }
            }
            "N" => n_name = name_from_n(value),
            "TEL" => {
                if let Some(c) = current.as_mut() {
                    let n = value.trim().to_string();
                    if !n.is_empty() && !c.numbers.contains(&n) {
                        c.numbers.push(n);
                    }
                }
            }
            "X-IRMC-CALL-DATETIME" => {
                if let Some(c) = current.as_mut() {
                    c.call_time = Some(value.trim().to_string());
                }
            }
            _ => {}
        }
    }
    cards
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_contacts_with_folding_and_escapes() {
        let text = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Alí\r\n  Boudjemaa\r\nN:Boudjemaa;Alí;;;\r\nTEL;TYPE=CELL:+213 550 00 00 01\r\nTEL;TYPE=HOME:0550000002\r\nEND:VCARD\r\nBEGIN:VCARD\r\nN:Smith;Jo\\, Jr;;;\r\nTEL:123\r\nEND:VCARD\r\n";
        let cards = parse(text);
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].name, "Alí Boudjemaa");
        assert_eq!(cards[0].numbers, vec!["+213 550 00 00 01", "0550000002"]);
        assert_eq!(cards[1].name, "Jo, Jr Smith");
    }

    #[test]
    fn reads_call_history_time() {
        let text = "BEGIN:VCARD\r\nN:;;;;\r\nTEL:+49123\r\nX-IRMC-CALL-DATETIME;RECEIVED:20260101T120000\r\nEND:VCARD\r\n";
        let c = parse(text);
        assert_eq!(c[0].call_time.as_deref(), Some("20260101T120000"));
        assert_eq!(c[0].numbers, vec!["+49123"]);
    }

    #[test]
    fn garbage_is_ignored() {
        assert!(parse("hello\nworld").is_empty());
        assert!(parse("").is_empty());
    }
}
