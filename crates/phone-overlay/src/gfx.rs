// SPDX-License-Identifier: GPL-3.0-or-later

//! Small 2D drawing layer over `tiny-skia`: everything takes logical
//! coordinates and a scale, so the renderer never multiplies by hand.

use tiny_skia::{
    BlendMode, FillRule, FilterQuality, GradientStop, LinearGradient, Mask, Paint, Path,
    PathBuilder, Pixmap, PixmapPaint, Point, SpreadMode, Stroke, Transform,
};

use crate::layout::Rect;
use crate::theme::Rgba;

/// A pixmap plus the logical-to-physical scale.
pub struct Canvas {
    /// Target pixels.
    pub pm: Pixmap,
    /// Physical pixels per logical pixel.
    pub s: f32,
    /// Logical offset added to everything drawn (and blitted).
    pub origin: (f32, f32),
}

/// Rounded-rectangle outline as a path. `r` is clamped to half the short side.
pub fn rrect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let r = r.clamp(0.0, w.min(h) / 2.0);
    let k = r * 0.552_284_8;
    let (x1, y1) = (x + w, y + h);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x1 - r, y);
    pb.cubic_to(x1 - r + k, y, x1, y + r - k, x1, y + r);
    pb.line_to(x1, y1 - r);
    pb.cubic_to(x1, y1 - r + k, x1 - r + k, y1, x1 - r, y1);
    pb.line_to(x + r, y1);
    pb.cubic_to(x + r - k, y1, x, y1 - r + k, x, y1 - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

fn paint(c: Rgba) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(c.to_skia());
    p.anti_alias = true;
    p
}

impl Canvas {
    /// A transparent canvas of `w` x `h` logical pixels. `None` when the
    /// physical size is zero or absurd.
    pub fn new(w: f32, h: f32, s: f32) -> Option<Self> {
        let pw = (w * s).ceil();
        let ph = (h * s).ceil();
        if !(1.0..=16384.0).contains(&pw) || !(1.0..=16384.0).contains(&ph) {
            return None;
        }
        Some(Self {
            pm: Pixmap::new(pw as u32, ph as u32)?,
            s,
            origin: (0.0, 0.0),
        })
    }

    fn ts(&self) -> Transform {
        Transform::from_row(
            self.s,
            0.0,
            0.0,
            self.s,
            self.origin.0 * self.s,
            self.origin.1 * self.s,
        )
    }

    /// Fill a path given in logical coordinates.
    pub fn fill_path(&mut self, path: &Path, c: Rgba) {
        let t = self.ts();
        self.pm
            .fill_path(path, &paint(c), FillRule::Winding, t, None);
    }

    /// Fill a rounded rectangle.
    pub fn fill_rrect(&mut self, r: Rect, radius: f32, c: Rgba) {
        if let Some(p) = rrect_path(r.x, r.y, r.w, r.h, radius) {
            self.fill_path(&p, c);
        }
    }

    /// Fill a rounded rectangle with a vertical gradient from `top` to `bottom`.
    pub fn fill_rrect_gradient(&mut self, r: Rect, radius: f32, top: Rgba, bottom: Rgba) {
        let Some(p) = rrect_path(r.x, r.y, r.w, r.h, radius) else {
            return;
        };
        let stops = vec![
            GradientStop::new(0.0, top.to_skia()),
            GradientStop::new(1.0, bottom.to_skia()),
        ];
        let Some(shader) = LinearGradient::new(
            Point::from_xy(r.x, r.y),
            Point::from_xy(r.x, r.bottom()),
            stops,
            SpreadMode::Pad,
            Transform::identity(),
        ) else {
            return;
        };
        let pt = Paint {
            shader,
            anti_alias: true,
            ..Paint::default()
        };
        let t = self.ts();
        self.pm.fill_path(&p, &pt, FillRule::Winding, t, None);
    }

    /// Stroke a rounded rectangle centred on its edge.
    pub fn stroke_rrect(&mut self, r: Rect, radius: f32, width: f32, c: Rgba) {
        if let Some(p) = rrect_path(r.x, r.y, r.w, r.h, radius) {
            self.stroke_path(&p, width, c);
        }
    }

    /// Stroke a path with round caps and joins.
    pub fn stroke_path(&mut self, path: &Path, width: f32, c: Rgba) {
        let stroke = Stroke {
            width,
            line_cap: tiny_skia::LineCap::Round,
            line_join: tiny_skia::LineJoin::Round,
            ..Stroke::default()
        };
        let t = self.ts();
        self.pm.stroke_path(path, &paint(c), &stroke, t, None);
    }

    /// Fill a circle.
    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, c: Rgba) {
        if r <= 0.0 {
            return;
        }
        if let Some(p) = PathBuilder::from_circle(cx, cy, r) {
            self.fill_path(&p, c);
        }
    }

    /// Stroke a circle.
    pub fn ring(&mut self, cx: f32, cy: f32, r: f32, width: f32, c: Rgba) {
        if let Some(p) = PathBuilder::from_circle(cx, cy, r) {
            self.stroke_path(&p, width, c);
        }
    }

    /// Stroke a straight segment.
    pub fn line(&mut self, a: (f32, f32), b: (f32, f32), width: f32, c: Rgba) {
        let mut pb = PathBuilder::new();
        pb.move_to(a.0, a.1);
        pb.line_to(b.0, b.1);
        if let Some(p) = pb.finish() {
            self.stroke_path(&p, width, c);
        }
    }

    /// Composite `src` at logical offset (`x`, `y`) with opacity, optionally
    /// clipped by `mask`. The offset snaps to whole physical pixels.
    pub fn blit(&mut self, src: &Pixmap, x: f32, y: f32, opacity: f32, mask: Option<&Mask>) {
        let px = ((x + self.origin.0) * self.s).round() as i32;
        let py = ((y + self.origin.1) * self.s).round() as i32;
        let pp = PixmapPaint {
            opacity: opacity.clamp(0.0, 1.0),
            blend_mode: BlendMode::SourceOver,
            quality: FilterQuality::Nearest,
        };
        self.pm
            .draw_pixmap(px, py, src.as_ref(), &pp, Transform::identity(), mask);
    }

    /// A mask of the rounded rectangle, for clipping.
    pub fn rrect_mask(&self, r: Rect, radius: f32) -> Option<Mask> {
        let mut m = Mask::new(self.pm.width(), self.pm.height())?;
        let p = rrect_path(r.x, r.y, r.w, r.h, radius)?;
        m.fill_path(&p, FillRule::Winding, true, self.ts());
        Some(m)
    }

    /// Erase the rounded rectangle (used to cut the panel out of its shadow).
    pub fn clear_rrect(&mut self, r: Rect, radius: f32) {
        let Some(p) = rrect_path(r.x, r.y, r.w, r.h, radius) else {
            return;
        };
        let mut pt = Paint {
            blend_mode: BlendMode::Clear,
            anti_alias: true,
            ..Paint::default()
        };
        pt.set_color(tiny_skia::Color::BLACK);
        let t = self.ts();
        self.pm.fill_path(&p, &pt, FillRule::Winding, t, None);
    }
}

/// Separable box blur of an 8-bit alpha plane with the given radius, treating
/// everything outside as transparent.
pub fn box_blur(data: &mut [u8], w: usize, h: usize, radius: usize) {
    if radius == 0 || w == 0 || h == 0 || data.len() < w * h {
        return;
    }
    let mut tmp = vec![0u8; w * h];
    let div = (2 * radius + 1) as u32;
    // Horizontal.
    for y in 0..h {
        let row = &data[y * w..(y + 1) * w];
        let out = &mut tmp[y * w..(y + 1) * w];
        let mut sum: u32 = row.iter().take(radius + 1).map(|&v| u32::from(v)).sum();
        for x in 0..w {
            out[x] = (sum / div) as u8;
            let add = x + radius + 1;
            if add < w {
                sum += u32::from(row[add]);
            }
            if x >= radius {
                sum -= u32::from(row[x - radius]);
            }
        }
    }
    // Vertical.
    for x in 0..w {
        let mut sum: u32 = (0..=radius.min(h - 1))
            .map(|y| u32::from(tmp[y * w + x]))
            .sum();
        for y in 0..h {
            data[y * w + x] = (sum / div) as u8;
            let add = y + radius + 1;
            if add < h {
                sum += u32::from(tmp[add * w + x]);
            }
            if y >= radius {
                sum -= u32::from(tmp[(y - radius) * w + x]);
            }
        }
    }
}

/// A soft rounded-rect shadow layer: a pixmap of `w` x `h` logical pixels at
/// scale `s` holding the blurred shape of `rect` shifted by (`dx`, `dy`),
/// tinted `color`. It is computed at `1/down` resolution and scaled up, which
/// is indistinguishable for a blur and far cheaper.
#[allow(clippy::too_many_arguments)]
pub fn shadow_layer(
    w: f32,
    h: f32,
    s: f32,
    rect: Rect,
    radius: f32,
    offset: (f32, f32),
    sigma: f32,
    down: f32,
    color: Rgba,
) -> Option<Pixmap> {
    let ds = s / down;
    let mut small = Canvas::new(w, h, ds)?;
    small.fill_rrect(
        rect.translate(offset.0, offset.1),
        radius,
        Rgba(255, 255, 255, 255),
    );
    let (pw, ph) = (small.pm.width() as usize, small.pm.height() as usize);
    // Extract alpha, blur three times (a good Gaussian approximation).
    let mut a: Vec<u8> = small
        .pm
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| p[3])
        .collect();
    let sigma_px = (sigma * ds).max(0.3);
    let box_w = ((12.0 * sigma_px * sigma_px / 3.0 + 1.0).sqrt())
        .floor()
        .max(1.0);
    let radius_px = (((box_w - 1.0) / 2.0).round() as usize).max(1);
    for _ in 0..3 {
        box_blur(&mut a, pw, ph, radius_px);
    }
    let ca = f32::from(color.3) / 255.0;
    let (cr, cg, cb) = (f32::from(color.0), f32::from(color.1), f32::from(color.2));
    for (px, &cov) in small
        .pm
        .data_mut()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(a.iter())
    {
        let al = f32::from(cov) / 255.0 * ca;
        px[0] = (cr * al).round() as u8;
        px[1] = (cg * al).round() as u8;
        px[2] = (cb * al).round() as u8;
        px[3] = (al * 255.0).round() as u8;
    }
    let mut big = Canvas::new(w, h, s)?;
    let pp = PixmapPaint {
        opacity: 1.0,
        blend_mode: BlendMode::Source,
        quality: FilterQuality::Bilinear,
    };
    let k = big.pm.width() as f32 / small.pm.width() as f32;
    let ky = big.pm.height() as f32 / small.pm.height() as f32;
    big.pm.draw_pixmap(
        0,
        0,
        small.pm.as_ref(),
        &pp,
        Transform::from_scale(k, ky),
        None,
    );
    Some(big.pm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rrect_path_handles_degenerates() {
        assert!(rrect_path(0.0, 0.0, 0.0, 10.0, 4.0).is_none());
        assert!(rrect_path(0.0, 0.0, 10.0, 10.0, 99.0).is_some());
        assert!(rrect_path(0.0, 0.0, 10.0, 10.0, -3.0).is_some());
    }

    #[test]
    fn canvas_refuses_silly_sizes() {
        assert!(Canvas::new(0.0, 10.0, 1.0).is_none());
        assert!(Canvas::new(1e9, 10.0, 1.0).is_none());
        assert!(Canvas::new(10.0, 10.0, 2.0).is_some());
    }

    #[test]
    fn blur_spreads_and_conserves_roughly() {
        let (w, h) = (21, 21);
        let mut a = vec![0u8; w * h];
        a[10 * w + 10] = 255;
        box_blur(&mut a, w, h, 2);
        let nonzero = a.iter().filter(|&&v| v > 0).count();
        assert!(nonzero > 9, "blur should spread the dot, got {nonzero}");
        assert!(a[10 * w + 10] < 255);
        // Symmetric.
        assert_eq!(a[10 * w + 8], a[10 * w + 12]);
        assert_eq!(a[8 * w + 10], a[12 * w + 10]);
        // Degenerate input is a no-op.
        box_blur(&mut [], 0, 0, 3);
    }

    #[test]
    fn shadow_is_soft_and_clear_cuts_the_panel() {
        let rect = Rect::new(20.0, 20.0, 100.0, 40.0);
        let sh = shadow_layer(
            140.0,
            100.0,
            1.0,
            rect,
            12.0,
            (0.0, 6.0),
            8.0,
            2.0,
            Rgba(0, 0, 0, 200),
        )
        .expect("shadow");
        let at = |x: usize, y: usize| sh.data()[(y * sh.width() as usize + x) * 4 + 3];
        assert!(at(70, 40) > 100, "under the panel the shadow is dense");
        assert!(
            at(70, 75) > 0 && at(70, 75) < at(70, 40),
            "below it fades out"
        );
        assert_eq!(at(2, 2), 0, "far corner is clear");
    }
}
