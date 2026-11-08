// SPDX-License-Identifier: GPL-3.0-or-later
//! A scrolling list that draws only the rows on screen and gives every frame exactly one hovered
//! row.
//!
//! Hover is not remembered anywhere: each frame it is worked out again from the pointer position
//! with [`Offsets::row_at`], so two rows can never both look hovered. It is also withheld while
//! the list scrolls and when the pointer is outside the list or over something in front of it.

use crate::model::list::Offsets;
use eframe::egui::{self, Rect, ScrollArea, Sense, Ui, Vec2, vec2};
use std::hash::Hash;
use std::time::Duration;

/// Remembered between frames: only where the list was, to notice that it moved.
#[derive(Debug, Default)]
pub struct VListState {
    last_offset: f32,
}

/// One row to draw.
#[derive(Debug, Clone, Copy)]
pub struct RowCtx {
    pub index: usize,
    /// Where the row is on screen.
    pub rect: Rect,
    /// The pointer is on this row, and on no other.
    pub hovered: bool,
}

/// Which row the pointer is on, if any. Separate from the drawing so it can be reasoned about:
/// `None` while scrolling or when the pointer is not inside `visible`.
pub fn hovered_row(pointer: Option<egui::Pos2>, visible: Rect, origin_y: f32, offsets: &Offsets, blocked: bool) -> Option<usize> {
    if blocked {
        return None;
    }
    let p = pointer.filter(|p| visible.contains(*p))?;
    offsets.row_at(p.y - origin_y)
}

/// Shows `offsets.len()` rows, calling `row` for those in view.
pub fn show(ui: &mut Ui, salt: impl Hash, state: &mut VListState, offsets: &Offsets, mut row: impl FnMut(&mut Ui, RowCtx)) {
    ScrollArea::vertical().id_salt(salt).auto_shrink([false, false]).show_viewport(ui, |ui, viewport| {
        let width = ui.available_width();
        let (area, _) = ui.allocate_exact_size(vec2(width, offsets.total()), Sense::hover());
        let origin = area.min;
        let scrolling = ui.input(|i| i.raw_scroll_delta != Vec2::ZERO || i.smooth_scroll_delta != Vec2::ZERO);
        let moved = (viewport.min.y - state.last_offset).abs() > 0.01;
        let blocked = scrolling || moved;
        // Only count the pointer when this list's layer is the one under it (no menu in front).
        let pointer = ui.input(|i| i.pointer.hover_pos()).filter(|p| ui.ctx().layer_id_at(*p) == Some(ui.layer_id()));
        let hovered = hovered_row(pointer, ui.clip_rect(), origin.y, offsets, blocked);
        for index in offsets.visible(viewport.min.y, viewport.max.y) {
            let rect = Rect::from_min_size(origin + vec2(0.0, offsets.top(index)), vec2(width, offsets.height(index)));
            row(ui, RowCtx { index, rect, hovered: hovered == Some(index) });
        }
        state.last_offset = viewport.min.y;
        if blocked {
            // One more look after the list settles, so the row under the pointer lights up again.
            ui.ctx().request_repaint_after(Duration::from_millis(120));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::pos2;

    fn rect() -> Rect {
        Rect::from_min_max(pos2(0.0, 100.0), pos2(300.0, 400.0))
    }

    #[test]
    fn the_row_under_the_pointer_is_hovered() {
        let o = Offsets::new([50.0; 10]);
        // The list content starts at y=100 and is scrolled by 25: origin is 75.
        assert_eq!(hovered_row(Some(pos2(10.0, 110.0)), rect(), 75.0, &o, false), Some(0));
        assert_eq!(hovered_row(Some(pos2(10.0, 130.0)), rect(), 75.0, &o, false), Some(1));
    }

    #[test]
    fn nothing_is_hovered_while_scrolling_or_outside() {
        let o = Offsets::new([50.0; 10]);
        assert_eq!(hovered_row(Some(pos2(10.0, 130.0)), rect(), 75.0, &o, true), None);
        assert_eq!(hovered_row(None, rect(), 75.0, &o, false), None);
        // Above and below the visible part of the list (the toolbar, the composer, ...).
        assert_eq!(hovered_row(Some(pos2(10.0, 90.0)), rect(), 75.0, &o, false), None);
        assert_eq!(hovered_row(Some(pos2(10.0, 410.0)), rect(), 75.0, &o, false), None);
        assert_eq!(hovered_row(Some(pos2(400.0, 150.0)), rect(), 75.0, &o, false), None);
    }

    #[test]
    fn at_most_one_row_is_hovered_for_any_pointer() {
        let o = Offsets::new([30.0, 56.0, 56.0, 150.0, 56.0]);
        for y in 90..420 {
            let hits = (0..o.len()).filter(|i| hovered_row(Some(pos2(5.0, y as f32)), rect(), 100.0, &o, false) == Some(*i)).count();
            assert!(hits <= 1, "y={y} hovered {hits} rows");
        }
    }
}
