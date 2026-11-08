// SPDX-License-Identifier: GPL-3.0-or-later
//! Long lists draw only the rows on screen. This is the arithmetic for that, and the grouping
//! of contacts under their section letters.

use std::ops::Range;

/// Where each row of a list starts, given every row's height.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Offsets {
    /// `starts[i]` is the top of row `i`; the last entry is the total height.
    starts: Vec<f32>,
}

impl Offsets {
    pub fn new(heights: impl IntoIterator<Item = f32>) -> Self {
        let mut starts = vec![0.0];
        let mut y = 0.0;
        for h in heights {
            y += h.max(0.0);
            starts.push(y);
        }
        Self { starts }
    }

    pub fn len(&self) -> usize {
        self.starts.len() - 1
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn total(&self) -> f32 {
        self.starts.last().copied().unwrap_or(0.0)
    }

    /// Top of row `i`.
    pub fn top(&self, i: usize) -> f32 {
        self.starts.get(i).copied().unwrap_or_else(|| self.total())
    }

    /// Height of row `i`.
    pub fn height(&self, i: usize) -> f32 {
        self.top(i + 1) - self.top(i)
    }

    /// The rows that touch the vertical span `top..bottom`.
    pub fn visible(&self, top: f32, bottom: f32) -> Range<usize> {
        let n = self.len();
        // First row whose bottom edge is below `top`.
        let first = self.starts[1..].partition_point(|end| *end <= top).min(n);
        // One past the last row whose top edge is above `bottom`.
        let last = self.starts[..n].partition_point(|start| *start < bottom).max(first);
        first..last
    }
}

/// The section a name is filed under: its first letter without accents, or `#` for anything that
/// does not start with a letter.
pub fn section_letter(name: &str) -> char {
    let Some(c) = name.chars().find(|c| c.is_alphanumeric()) else {
        return '#';
    };
    if !c.is_alphabetic() {
        return '#';
    }
    let lower = c.to_lowercase().next().unwrap_or(c);
    let base = match lower {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'æ' | 'ā' | 'ą' => 'a',
        'ç' | 'ć' | 'č' => 'c',
        'ď' | 'đ' => 'd',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' | 'ě' => 'e',
        'ì' | 'í' | 'î' | 'ï' | 'ī' => 'i',
        'ł' => 'l',
        'ñ' | 'ń' | 'ň' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => 'o',
        'ř' => 'r',
        'ś' | 'š' | 'ş' | 'ß' => 's',
        'ť' => 't',
        'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' => 'u',
        'ý' | 'ÿ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        other => other,
    };
    base.to_uppercase().next().unwrap_or('#')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_add_up() {
        let o = Offsets::new([10.0, 20.0, 30.0]);
        assert_eq!(o.len(), 3);
        assert_eq!(o.total(), 60.0);
        assert_eq!(o.top(1), 10.0);
        assert_eq!(o.top(3), 60.0);
        assert_eq!(o.height(2), 30.0);
        assert!(Offsets::new([]).is_empty());
    }

    #[test]
    fn visible_rows_are_the_ones_touching_the_span() {
        let o = Offsets::new([10.0; 10]);
        assert_eq!(o.visible(0.0, 25.0), 0..3);
        assert_eq!(o.visible(10.0, 20.0), 1..2);
        // A span that begins mid-row includes that row.
        assert_eq!(o.visible(15.0, 35.0), 1..4);
        assert_eq!(o.visible(95.0, 500.0), 9..10);
        // Scrolled past the end: nothing.
        assert_eq!(o.visible(500.0, 600.0), 10..10);
        assert_eq!(o.visible(0.0, 0.0), 0..0);
    }

    #[test]
    fn visible_handles_rows_of_different_heights_and_empty_lists() {
        let o = Offsets::new([30.0, 56.0, 56.0, 150.0, 56.0]);
        assert_eq!(o.visible(80.0, 200.0), 1..4);
        let empty = Offsets::new([]);
        assert_eq!(empty.visible(0.0, 100.0), 0..0);
    }

    #[test]
    fn sections_ignore_accents_and_case() {
        assert_eq!(section_letter("Alí Boudjemaa"), 'A');
        assert_eq!(section_letter("álvaro"), 'A');
        assert_eq!(section_letter("Ørjan"), 'O');
        assert_eq!(section_letter("Zoé Martin"), 'Z');
        assert_eq!(section_letter("Жанна"), 'Ж');
    }

    #[test]
    fn numbers_and_symbols_go_under_hash() {
        assert_eq!(section_letter("+213 550"), '#');
        assert_eq!(section_letter("123 Pizza"), '#');
        assert_eq!(section_letter(""), '#');
        assert_eq!(section_letter("(Work) Sam"), 'W');
    }
}
