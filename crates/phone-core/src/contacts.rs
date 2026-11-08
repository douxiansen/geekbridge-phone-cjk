// SPDX-License-Identifier: GPL-3.0-or-later
//! The phonebook: lookup by number, search by name, number or keypad digits.

use crate::vcard::Card;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contact {
    pub name: String,
    pub numbers: Vec<String>,
}

/// Same person however the number is written: the last nine digits, or the
/// whole text for a sender such as a bank's alphanumeric id.
pub fn number_key(number: &str) -> String {
    let digits: String = number.chars().filter(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return number.trim().to_lowercase();
    }
    let start = digits.len().saturating_sub(9);
    digits[start..].to_string()
}

/// Lower-cases and strips the accents of Latin letters, so "ali" finds "Alí".
fn fold(s: &str) -> String {
    s.chars()
        .flat_map(|c| c.to_lowercase())
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'ç' => 'c',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ñ' => 'n',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            'ý' | 'ÿ' => 'y',
            other => other,
        })
        .collect()
}

/// The digit a letter is on, on a phone keypad.
fn keypad_digit(c: char) -> Option<char> {
    Some(match c {
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

#[derive(Debug, Clone, Default)]
pub struct ContactBook {
    contacts: Vec<Contact>,
}

impl ContactBook {
    pub fn from_cards(cards: Vec<Card>) -> Self {
        let mut contacts: Vec<Contact> = cards
            .into_iter()
            .filter(|c| !c.numbers.is_empty())
            .map(|c| Contact {
                name: if c.name.trim().is_empty() {
                    c.numbers[0].clone()
                } else {
                    c.name
                },
                numbers: c.numbers,
            })
            .collect();
        contacts.sort_by_key(|c| fold(&c.name));
        Self { contacts }
    }

    pub fn from_contacts(contacts: Vec<Contact>) -> Self {
        Self { contacts }
    }

    pub fn contacts(&self) -> &[Contact] {
        &self.contacts
    }

    pub fn len(&self) -> usize {
        self.contacts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.contacts.is_empty()
    }

    /// The contact's name for a number, when we have one.
    pub fn name_for(&self, number: &str) -> Option<&str> {
        let key = number_key(number);
        if key.is_empty() {
            return None;
        }
        self.contacts
            .iter()
            .find(|c| c.numbers.iter().any(|n| number_key(n) == key))
            .map(|c| c.name.as_str())
    }

    /// A name to show: the contact's, else the number itself.
    pub fn display(&self, number: &str) -> String {
        self.name_for(number)
            .map_or_else(|| number.to_string(), str::to_string)
    }

    /// Contacts matching `query` by name, by number, or by keypad digits
    /// (`254` finds "Ali"). An empty query matches everything.
    pub fn search(&self, query: &str) -> Vec<&Contact> {
        let q = fold(query.trim());
        if q.is_empty() {
            return self.contacts.iter().collect();
        }
        let q_digits: String = q.chars().filter(char::is_ascii_digit).collect();
        let all_digits = !q_digits.is_empty()
            && q_digits.len()
                == q.chars()
                    .filter(|c| !matches!(c, ' ' | '-' | '+' | '(' | ')'))
                    .count();
        self.contacts
            .iter()
            .filter(|c| {
                let name = fold(&c.name);
                if name.contains(&q) {
                    return true;
                }
                if all_digits {
                    if c.numbers.iter().any(|n| {
                        n.chars()
                            .filter(char::is_ascii_digit)
                            .collect::<String>()
                            .contains(&q_digits)
                    }) {
                        return true;
                    }
                    let keypad: String = name.chars().filter_map(keypad_digit).collect();
                    return keypad.contains(&q_digits);
                }
                false
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book() -> ContactBook {
        ContactBook::from_cards(vec![
            Card {
                name: "Zoé Martin".into(),
                numbers: vec!["+33 6 12 34 56 78".into()],
                call_time: None,
            },
            Card {
                name: "Alí".into(),
                numbers: vec!["0550123456".into(), "+213550123457".into()],
                call_time: None,
            },
            Card {
                name: "".into(),
                numbers: vec!["999".into()],
                call_time: None,
            },
            Card {
                name: "No numbers".into(),
                numbers: vec![],
                call_time: None,
            },
        ])
    }

    #[test]
    fn numbers_match_across_formats() {
        let b = book();
        assert_eq!(b.name_for("+213 550 12 34 56"), Some("Alí"));
        assert_eq!(b.name_for("0033612345678"), Some("Zoé Martin"));
        assert_eq!(b.name_for("12345"), None);
        assert_eq!(b.display("12345"), "12345");
    }

    #[test]
    fn contacts_without_numbers_are_dropped_and_unnamed_get_the_number() {
        let b = book();
        assert_eq!(b.len(), 3);
        assert!(b.contacts().iter().any(|c| c.name == "999"));
    }

    #[test]
    fn search_by_name_accents_and_keypad() {
        let b = book();
        assert_eq!(b.search("ali")[0].name, "Alí");
        assert_eq!(b.search("zoe").len(), 1);
        assert_eq!(b.search("254")[0].name, "Alí"); // A-L-I on the keypad
        assert_eq!(b.search("55012")[0].name, "Alí"); // by number
        assert_eq!(b.search("").len(), 3);
        assert!(b.search("qqq").is_empty());
    }

    #[test]
    fn alphanumeric_senders_key_by_text() {
        assert_eq!(number_key("MyBank"), "mybank");
        assert_eq!(number_key("+4915112345678"), "112345678");
    }
}
