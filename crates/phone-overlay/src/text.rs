// SPDX-License-Identifier: GPL-3.0-or-later

//! Text shaping, measuring and drawing with `cosmic-text` and the system
//! fonts. Missing fonts never panic: text just draws as nothing and measures
//! as zero.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use cosmic_text::{
    Attrs, Buffer, Color as CtColor, Family, FontSystem, Metrics, Shaping, SwashCache, Weight,
};
use tiny_skia::Pixmap;
use unicode_segmentation::UnicodeSegmentation;

use crate::theme::Rgba;

/// Preferred UI families, first installed one wins; glyphs it lacks (accents,
/// emoji, symbols) fall through to other installed fonts.
pub const FAMILY_FALLBACKS: &[&str] = &[
    "SF Pro Text",
    "Inter",
    "Cantarell",
    "Noto Sans",
    "DejaVu Sans",
];

/// Line height as a multiple of the font size.
pub const LINE_HEIGHT: f32 = 1.25;

/// Font weights used by the banner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Wt {
    /// 400
    Regular,
    /// 500
    Medium,
    /// 600
    Semibold,
}

impl Wt {
    fn weight(self) -> Weight {
        match self {
            Wt::Regular => Weight::NORMAL,
            Wt::Medium => Weight::MEDIUM,
            Wt::Semibold => Weight::SEMIBOLD,
        }
    }
}

/// A font size and weight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// Font size in pixels.
    pub size: f32,
    /// Weight.
    pub weight: Wt,
    /// Extra tracking in em.
    pub tracking: f32,
}

impl Style {
    /// Regular weight, no tracking.
    pub fn new(size: f32, weight: Wt) -> Self {
        Self {
            size,
            weight,
            tracking: 0.0,
        }
    }

    /// With tracking in em.
    pub fn tracked(mut self, em: f32) -> Self {
        self.tracking = em;
        self
    }

    /// Same style at another size.
    pub fn scaled(self, k: f32) -> Self {
        Self {
            size: self.size * k,
            ..self
        }
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct BufKey {
    text: String,
    size: u32,
    weight: Wt,
    tracking: u32,
    width: Option<u32>,
}

struct Engine {
    fs: FontSystem,
    swash: SwashCache,
    family: Option<String>,
    buffers: HashMap<BufKey, Buffer>,
}

impl Engine {
    fn new() -> Self {
        let mut fs = FontSystem::new();
        prune_unrenderable_emoji(&mut fs);
        let family = FAMILY_FALLBACKS.iter().find_map(|want| {
            fs.db()
                .faces()
                .flat_map(|f| f.families.iter())
                .find(|(name, _)| name.eq_ignore_ascii_case(want))
                .map(|(name, _)| name.clone())
        });
        Self {
            fs,
            swash: SwashCache::new(),
            family,
            buffers: HashMap::new(),
        }
    }

    fn buffer(&mut self, text: &str, style: Style, width: Option<f32>) -> Option<&Buffer> {
        if !(style.size.is_finite() && style.size > 0.5) {
            return None;
        }
        let key = BufKey {
            text: text.to_owned(),
            size: style.size.to_bits(),
            weight: style.weight,
            tracking: style.tracking.to_bits(),
            width: width.map(f32::to_bits),
        };
        if !self.buffers.contains_key(&key) {
            if self.buffers.len() > 256 {
                self.buffers.clear();
            }
            let mut buf = Buffer::new(
                &mut self.fs,
                Metrics::new(style.size, style.size * LINE_HEIGHT),
            );
            buf.set_size(&mut self.fs, width, None);
            let mut attrs = Attrs::new().weight(style.weight.weight());
            if let Some(f) = &self.family {
                attrs = attrs.family(Family::Name(f));
            }
            if style.tracking != 0.0 {
                attrs = attrs.letter_spacing(style.tracking);
            }
            buf.set_text(&mut self.fs, text, &attrs, Shaping::Advanced);
            buf.shape_until_scroll(&mut self.fs, false);
            self.buffers.insert(key.clone(), buf);
        }
        self.buffers.get(&key)
    }
}

/// Whether a colour emoji font uses a colour table the rasteriser can draw:
/// bitmaps (`CBDT`, `sbix`) or COLR version 0. COLRv1-only fonts (the current
/// Noto Color Emoji) would shape fine but draw as blanks.
fn emoji_font_is_drawable(data: &[u8]) -> bool {
    let be16 = |o: usize| data.get(o..o + 2).map(|b| u16::from_be_bytes([b[0], b[1]]));
    if data.get(..4) == Some(b"ttcf") {
        return true;
    }
    let Some(n) = be16(4) else { return true };
    for i in 0..usize::from(n) {
        let rec = 12 + i * 16;
        let Some(tag) = data.get(rec..rec + 4) else {
            break;
        };
        match tag {
            b"CBDT" | b"sbix" => return true,
            b"COLR" => {
                let off = data
                    .get(rec + 8..rec + 12)
                    .map_or(0, |b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize);
                return be16(off) == Some(0);
            }
            _ => {}
        }
    }
    // No colour table at all: a plain outline font, drawable.
    true
}

/// Drop colour emoji faces we cannot draw, so shaping falls through to a
/// monochrome emoji or symbol font instead of picking a face that renders
/// nothing.
fn prune_unrenderable_emoji(fs: &mut FontSystem) {
    let ids: Vec<_> = fs
        .db()
        .faces()
        .filter(|f| {
            f.families
                .iter()
                .any(|(n, _)| n.to_ascii_lowercase().contains("color emoji"))
        })
        .map(|f| f.id)
        .collect();
    for id in ids {
        let drawable = fs
            .db()
            .with_face_data(id, |data, _| emoji_font_is_drawable(data))
            .unwrap_or(true);
        if !drawable {
            fs.db_mut().remove_face(id);
        }
    }
}

fn engine() -> std::sync::MutexGuard<'static, Engine> {
    static ENGINE: OnceLock<Mutex<Engine>> = OnceLock::new();
    ENGINE
        .get_or_init(|| Mutex::new(Engine::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Load the system fonts now, on the calling thread, so the first banner does
/// not pay for it. Safe to call any number of times.
pub fn warm_up() {
    drop(engine());
}

/// Name of the UI family in use, if any of the preferred ones is installed.
pub fn family_in_use() -> Option<String> {
    engine().family.clone()
}

/// Width of `text` on one line.
pub fn measure(text: &str, style: Style) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let mut e = engine();
    let Some(buf) = e.buffer(text, style, None) else {
        return 0.0;
    };
    buf.layout_runs().map(|r| r.line_w).fold(0.0, f32::max)
}

/// Height of one line box.
pub fn line_height(style: Style) -> f32 {
    style.size * LINE_HEIGHT
}

/// `text` cut to fit `max_w`, with an ellipsis when it had to be shortened.
pub fn ellipsize(text: &str, style: Style, max_w: f32) -> String {
    let text = text.trim_end();
    if measure(text, style) <= max_w {
        return text.to_owned();
    }
    let ends: Vec<usize> = text
        .grapheme_indices(true)
        .map(|(i, g)| i + g.len())
        .collect();
    let fits = |n: usize| {
        let cut = text[..ends[n]].trim_end();
        measure(&format!("{cut}\u{2026}"), style) <= max_w
    };
    // Largest prefix (in graphemes) that still fits together with the ellipsis.
    let (mut lo, mut hi) = (0usize, ends.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if fits(mid - 1) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    if lo == 0 {
        return "\u{2026}".to_owned();
    }
    format!("{}\u{2026}", text[..ends[lo - 1]].trim_end())
}

/// Word-wrap `text` into at most `max_lines` lines of width `max_w`; the last
/// line is ellipsized when the text does not fit.
pub fn wrap_lines(text: &str, style: Style, max_w: f32, max_lines: usize) -> Vec<String> {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let flat = flat.trim();
    if flat.is_empty() || max_lines == 0 {
        return Vec::new();
    }
    let ranges: Vec<(usize, usize)> = {
        let mut e = engine();
        let Some(buf) = e.buffer(flat, style, Some(max_w)) else {
            return vec![flat.to_owned()];
        };
        buf.layout_runs()
            .filter_map(|run| {
                let start = run.glyphs.iter().map(|g| g.start).min()?;
                let end = run.glyphs.iter().map(|g| g.end).max()?;
                Some((start, end))
            })
            .collect()
    };
    if ranges.is_empty() {
        return vec![ellipsize(flat, style, max_w)];
    }
    let mut out = Vec::new();
    for (i, &(s, e)) in ranges.iter().enumerate() {
        if i + 1 == max_lines && ranges.len() > max_lines {
            out.push(ellipsize(&flat[s..], style, max_w));
            break;
        }
        out.push(flat[s..e].trim().to_owned());
        if out.len() == max_lines {
            break;
        }
    }
    out
}

/// Draw one line of text with its top-left at (`x`, `y`) in pixels, blending
/// into `pm` with extra opacity `opacity`.
pub fn draw(pm: &mut Pixmap, text: &str, x: f32, y: f32, style: Style, color: Rgba, opacity: f32) {
    if text.is_empty() || opacity <= 0.0 || color.3 == 0 {
        return;
    }
    let mut e = engine();
    if e.buffer(text, style, None).is_none() {
        return;
    }
    let Engine {
        fs, swash, buffers, ..
    } = &mut *e;
    let key = BufKey {
        text: text.to_owned(),
        size: style.size.to_bits(),
        weight: style.weight,
        tracking: style.tracking.to_bits(),
        width: None,
    };
    let Some(buf) = buffers.get(&key) else {
        return;
    };
    let ox = x.round() as i32;
    let oy = y.round() as i32;
    let (pw, ph) = (pm.width() as i32, pm.height() as i32);
    let k = (opacity.clamp(0.0, 1.0) * f32::from(color.3)).round() as u32;
    let data = pm.data_mut();
    let div255 = |v: u32| (v + 128 + ((v + 128) >> 8)) >> 8;
    buf.draw(
        fs,
        swash,
        CtColor::rgba(color.0, color.1, color.2, 255),
        |gx, gy, gw, gh, c| {
            let a = div255(u32::from(c.a()) * k);
            if a == 0 {
                return;
            }
            let inv = 255 - a;
            let (cr, cg, cb) = (u32::from(c.r()), u32::from(c.g()), u32::from(c.b()));
            let (x0, y0) = (gx + ox, gy + oy);
            for py in y0.max(0)..(y0 + gh as i32).min(ph) {
                for px in x0.max(0)..(x0 + gw as i32).min(pw) {
                    let i = (py as usize * pw as usize + px as usize) * 4;
                    let px4 = &mut data[i..i + 4];
                    px4[0] = (div255(cr * a) + div255(u32::from(px4[0]) * inv)).min(255) as u8;
                    px4[1] = (div255(cg * a) + div255(u32::from(px4[1]) * inv)).min(255) as u8;
                    px4[2] = (div255(cb * a) + div255(u32::from(px4[2]) * inv)).min(255) as u8;
                    px4[3] = (a + div255(u32::from(px4[3]) * inv)).min(255) as u8;
                }
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style() -> Style {
        Style::new(14.0, Wt::Regular)
    }

    #[test]
    fn measure_never_panics_and_is_monotonic() {
        assert_eq!(measure("", style()), 0.0);
        let a = measure("hello", style());
        let b = measure("hello world", style());
        assert!(b >= a);
        // Degenerate sizes do not panic.
        assert_eq!(measure("x", Style::new(0.0, Wt::Regular)), 0.0);
        assert_eq!(measure("x", Style::new(f32::NAN, Wt::Regular)), 0.0);
    }

    #[test]
    fn ellipsize_fits_or_marks() {
        let s = style();
        let long = "The quick brown fox jumps over the lazy dog and keeps running far away";
        let w = measure("The quick brown", s).max(40.0);
        let out = ellipsize(long, s, w);
        assert!(out.chars().count() < long.chars().count());
        if measure(long, s) > 0.0 {
            assert!(out.ends_with('\u{2026}'));
            assert!(measure(&out, s) <= w + 0.5);
        }
        assert_eq!(ellipsize("hi", s, 10_000.0), "hi");
    }

    #[test]
    fn wrap_limits_lines_and_handles_unicode() {
        let s = style();
        let body = "Héllo wörld, this is a longer message with emoji \u{1F600} and more text that must wrap";
        let lines = wrap_lines(body, s, 120.0, 2);
        assert!(lines.len() <= 2);
        assert!(!lines.is_empty());
        assert!(wrap_lines("   ", s, 120.0, 2).is_empty());
        assert!(wrap_lines("x", s, 120.0, 0).is_empty());
    }

    #[test]
    fn draw_blends_into_pixmap() {
        let mut pm = Pixmap::new(120, 40).expect("pixmap");
        draw(
            &mut pm,
            "Hello",
            4.0,
            4.0,
            Style::new(20.0, Wt::Semibold),
            Rgba::hex(0xFFFFFF),
            1.0,
        );
        let lit = pm.data().chunks(4).filter(|p| p[3] > 0).count();
        // With no fonts installed nothing is drawn, but it must not panic.
        if family_in_use().is_some() {
            assert!(lit > 20, "text should have drawn pixels, got {lit}");
        }
    }
}
