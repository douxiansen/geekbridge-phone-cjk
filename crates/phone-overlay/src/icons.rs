// SPDX-License-Identifier: GPL-3.0-or-later

//! Vector glyphs drawn with strokes: handset, send arrow, cross, check,
//! person silhouette, and the GeekBridge app tile.

use tiny_skia::{PathBuilder, Transform};

use crate::dotfont;
use crate::gfx::Canvas;
use crate::layout::Rect;
use crate::theme::Rgba;

/// A telephone handset in a `size` box centred on (`cx`, `cy`). With
/// `hangup` it is turned to lie flat, receiver down, like the end-call icon.
pub fn handset(cv: &mut Canvas, cx: f32, cy: f32, size: f32, hangup: bool, color: Rgba) {
    let u = size / 16.0;
    let mut pb = PathBuilder::new();
    // The handle: a curve bulging to the bottom-left between two ends.
    pb.move_to(3.6, 3.4);
    pb.cubic_to(3.2, 9.4, 6.8, 12.9, 12.6, 12.5);
    let Some(handle) = pb.finish() else { return };
    let mut ends = PathBuilder::new();
    // Earpiece and mouthpiece, perpendicular to the handle's ends.
    ends.move_to(1.9, 3.1);
    ends.line_to(6.8, 3.1);
    ends.move_to(12.9, 9.0);
    ends.line_to(12.9, 14.3);
    let Some(ends) = ends.finish() else { return };
    let mut t = Transform::from_translate(cx - 8.0 * u, cy - 8.0 * u).pre_scale(u, u);
    if hangup {
        // Rotate 135 degrees clockwise about the box centre.
        t = Transform::from_translate(cx, cy)
            .pre_rotate(135.0)
            .pre_translate(-8.0 * u, -8.0 * u)
            .pre_scale(u, u);
    }
    for (p, w) in [(handle, 2.8 * u), (ends, 3.5 * u)] {
        if let Some(p) = p.transform(t) {
            cv.stroke_path(&p, w, color);
        }
    }
}

/// An upward arrow (the send button).
pub fn arrow_up(cv: &mut Canvas, cx: f32, cy: f32, size: f32, color: Rgba) {
    let u = size / 16.0;
    let (x, y) = (cx - 8.0 * u, cy - 8.0 * u);
    let w = 2.0 * u;
    cv.line(
        (x + 8.0 * u, y + 13.0 * u),
        (x + 8.0 * u, y + 3.2 * u),
        w,
        color,
    );
    let mut pb = PathBuilder::new();
    pb.move_to(x + 3.6 * u, y + 7.4 * u);
    pb.line_to(x + 8.0 * u, y + 3.0 * u);
    pb.line_to(x + 12.4 * u, y + 7.4 * u);
    if let Some(p) = pb.finish() {
        cv.stroke_path(&p, w, color);
    }
}

/// A cross (the dismiss button).
pub fn cross(cv: &mut Canvas, cx: f32, cy: f32, size: f32, width: f32, color: Rgba) {
    let h = size / 2.0;
    cv.line((cx - h, cy - h), (cx + h, cy + h), width, color);
    cv.line((cx - h, cy + h), (cx + h, cy - h), width, color);
}

/// A check mark, `size` wide.
pub fn check(cv: &mut Canvas, x: f32, cy: f32, size: f32, width: f32, color: Rgba) {
    let u = size / 10.0;
    let mut pb = PathBuilder::new();
    pb.move_to(x + 0.8 * u, cy + 0.2 * u);
    pb.line_to(x + 3.8 * u, cy + 3.2 * u);
    pb.line_to(x + 9.4 * u, cy - 3.4 * u);
    if let Some(p) = pb.finish() {
        cv.stroke_path(&p, width, color);
    }
}

/// A small exclamation mark in a circle (error marker).
pub fn alert(cv: &mut Canvas, cx: f32, cy: f32, r: f32, color: Rgba) {
    cv.ring(cx, cy, r, 1.2, color);
    cv.line((cx, cy - r * 0.5), (cx, cy + r * 0.1), 1.3, color);
    cv.circle(cx, cy + r * 0.52, 0.8, color);
}

/// A neutral person silhouette filling a circle at (`cx`, `cy`) of radius `r`.
/// The caller clips to the circle; this only draws head and shoulders.
pub fn person(cv: &mut Canvas, cx: f32, cy: f32, r: f32, color: Rgba) {
    cv.circle(cx, cy - r * 0.18, r * 0.30, color);
    let mut pb = PathBuilder::new();
    let (l, rt, top, bot) = (cx - r * 0.62, cx + r * 0.62, cy + r * 0.28, cy + r * 1.1);
    pb.move_to(l, bot);
    pb.cubic_to(l, top - r * 0.05, cx - r * 0.35, top, cx, top);
    pb.cubic_to(cx + r * 0.35, top, rt, top - r * 0.05, rt, bot);
    pb.close();
    if let Some(p) = pb.finish() {
        cv.fill_path(&p, color);
    }
}

/// The GeekBridge app tile: a rounded black square with a dot-matrix "G" in
/// off-white and one red dot in the glyph's empty top-right corner.
pub fn app_tile(cv: &mut Canvas, r: Rect, ring: Rgba) {
    let radius = r.w * 0.3;
    cv.fill_rrect(r, radius, Rgba::hex(0x000000));
    cv.stroke_rrect(r.inset(0.5), radius - 0.5, 1.0, ring);
    let pitch = r.w * 0.118;
    let (cols, rows) = (5.0, 7.0);
    let x0 = r.x + (r.w - cols * pitch) / 2.0;
    let y0 = r.y + (r.h - rows * pitch) / 2.0;
    let dot = pitch * 0.4;
    for (c, row) in dotfont::lit_dots("G") {
        cv.circle(
            x0 + (c as f32 + 0.5) * pitch,
            y0 + (row as f32 + 0.5) * pitch,
            dot,
            Rgba::hex(0xF4F4F1),
        );
    }
    cv.circle(
        x0 + 4.5 * pitch,
        y0 + 0.5 * pitch,
        dot * 1.15,
        Rgba::hex(0xD71921),
    );
}
