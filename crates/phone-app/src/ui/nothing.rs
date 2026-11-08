// SPDX-License-Identifier: GPL-3.0-or-later
//! Dot-matrix text widgets for the Nothing-inspired look. Everything is painted in code with the
//! shared dot-matrix renderer; no fonts, images or logos are involved.
//!
//! Adapted from the GeekBridge design system (`bridge-apps/src/nothing.rs`, same licence): the
//! tile, progress and i18n parts are dropped; the dot-text widgets are kept.

use super::dotmatrix;
use super::theme::Palette;
use eframe::egui::{self, Color32, Sense, Ui, WidgetInfo, WidgetType, pos2, vec2};

/// Dot pitch (distance between dot centres) for the dot-matrix sizes at 100% text.
pub const PITCH_NUMERAL: f32 = 5.0;
pub const PITCH_TITLE: f32 = 3.6;
pub const PITCH_LABEL: f32 = 2.2;

const T_BODY: f32 = 14.0;

/// Dot-matrix pitch follows the user's text scale (body text size relative to the default).
pub fn scaled(ui: &Ui, pitch: f32) -> f32 {
    let body = ui.style().text_styles.get(&egui::TextStyle::Body).map_or(T_BODY, |f| f.size);
    pitch * (body / T_BODY).clamp(0.8, 2.0)
}

/// A run of dot-matrix text. Reserves its size, paints lit dots in `color` and (optionally) the
/// unlit grid faintly, and is announced to screen readers as the plain text.
pub fn dot_text(ui: &mut Ui, pal: &Palette, text: &str, pitch: f32, color: Color32, grid: bool) {
    let pitch = scaled(ui, pitch);
    let size = dotmatrix::measure(text, pitch) + vec2(pitch * 0.8, pitch * 0.8);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let origin = pos2(rect.left() + pitch * 0.4, rect.top() + pitch * 0.4);
    let off = grid.then_some(pal.dot_off);
    dotmatrix::paint(ui.painter(), origin, text, pitch, color, off);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, spoken(text)));
}

/// What a screen reader says for a dot run: the words themselves, but placeholder runs such as
/// "---" (no value) and "..." (still loading) are said as what they mean, not as punctuation.
pub fn spoken(text: &str) -> &str {
    if text.len() > 1 && text.chars().all(|c| c == '-') {
        "not available"
    } else if text.len() > 1 && text.chars().all(|c| c == '.') {
        "loading"
    } else {
        text
    }
}

/// Paints dot text at an exact position without allocating layout space (for overlays).
pub fn dot_text_at(ui: &Ui, pal: &Palette, origin: egui::Pos2, text: &str, pitch: f32, color: Color32, grid: bool) {
    let off = grid.then_some(pal.dot_off);
    dotmatrix::paint(ui.painter(), origin, text, pitch, color, off);
}

/// Width and height a dot run takes at `pitch`, including the half-dot margin [`dot_text`] adds.
pub fn dot_size(text: &str, pitch: f32) -> egui::Vec2 {
    dotmatrix::measure(text, pitch) + vec2(pitch * 0.8, pitch * 0.8)
}

/// Largest pitch (at most `max_pitch`) at which `text` fits `max_width`, never below `min_pitch`.
pub fn fit_pitch(text: &str, max_pitch: f32, min_pitch: f32, max_width: f32) -> f32 {
    let n = text.chars().count() as f32;
    if n == 0.0 {
        return max_pitch;
    }
    // measure().x = (n * 6 - 1) * pitch, plus the 0.8-pitch margin of dot_size.
    let per_pitch = n * (dotmatrix::COLS as f32 + 1.0) - 1.0 + 0.8;
    (max_width / per_pitch).clamp(min_pitch, max_pitch)
}

/// Like [`dot_text`], but picks the longest candidate that fits `max_width`.
pub fn dot_text_fit(ui: &mut Ui, pal: &Palette, candidates: &[&str], pitch: f32, max_width: f32, color: Color32, grid: bool) {
    let p = scaled(ui, pitch);
    let text = dotmatrix::fit(candidates, p, max_width);
    dot_text(ui, pal, text, pitch, color, grid);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_dots_are_spoken_as_meaning() {
        assert_eq!(spoken("---"), "not available");
        assert_eq!(spoken("..."), "loading");
        assert_eq!(spoken("-"), "-");
        assert_eq!(spoken("Missed call"), "Missed call");
    }

    #[test]
    fn fit_pitch_shrinks_to_the_width_and_respects_bounds() {
        let big = fit_pitch("123", 5.0, 2.0, 1000.0);
        assert_eq!(big, 5.0);
        let w = 200.0;
        let p = fit_pitch("+213550123456789", 5.0, 2.0, w);
        assert!(p < 5.0 && p >= 2.0);
        assert!(dot_size("+213550123456789", p).x <= w + 0.01);
        assert_eq!(fit_pitch("1234567890123456789012345678901234567890", 5.0, 2.0, 50.0), 2.0);
    }
}
