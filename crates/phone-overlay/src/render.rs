// SPDX-License-Identifier: GPL-3.0-or-later

//! The CPU renderer: [`render`] turns an [`OverlayState`] into a
//! premultiplied RGBA [`Pixmap`], with no display and no clock involved.

use std::cell::RefCell;
use std::collections::HashMap;

use tiny_skia::{PathBuilder, Pixmap};
use unicode_segmentation::UnicodeSegmentation;

use crate::anim;
use crate::dotfont;
use crate::gfx::{shadow_layer, Canvas};
use crate::icons;
use crate::layout::{Hit, Rect, BANNER_W, RADIUS, SHADOW_PAD};
use crate::model::{Banner, CallState, ReplyStatus};
use crate::state::{body_style, BannerView, OverlayState, FIELD_FONT, FIELD_TEXT_X};
use crate::text::{self, Style, Wt};
use crate::theme::{Palette, Rgba, Theme};

/// Smallest and largest accepted scale factors.
pub const SCALE_RANGE: (f32, f32) = (0.5, 8.0);

/// Render the whole overlay surface at `scale` physical pixels per logical
/// pixel. The result is premultiplied RGBA, sized to
/// [`OverlayState::surface_size`]; it is 1x1 and transparent when nothing is
/// shown. Rendering is a pure function of its inputs.
pub fn render(state: &OverlayState, scale: f32, theme: Theme) -> Pixmap {
    let scale = if scale.is_finite() {
        scale.clamp(SCALE_RANGE.0, SCALE_RANGE.1)
    } else {
        1.0
    };
    let (w, h) = state.surface_size();
    let Some(mut surface) = Canvas::new(w, h, scale) else {
        return empty_pixmap();
    };
    BANNERS.with(|cache| {
        let mut cache = cache.borrow_mut();
        let mut seen = Vec::new();
        for v in state.views() {
            if v.alpha <= 0.0 {
                continue;
            }
            let id = v.banner.id();
            let sig = signature(&v, scale, theme);
            seen.push(id);
            let fresh = !matches!(cache.get(&id), Some((s, _)) if *s == sig);
            if fresh {
                if let Some(pm) = draw_banner(&v, scale, theme) {
                    cache.insert(id, (sig, pm));
                }
            }
            let Some((_, base)) = cache.get(&id) else {
                continue;
            };
            match v.banner {
                Banner::Call(c) if is_dynamic(c.state) => {
                    // Cached panel and text, plus this frame's ripple or timer.
                    let mut frame = Canvas {
                        pm: base.clone(),
                        s: scale,
                        origin: (SHADOW_PAD, SHADOW_PAD),
                    };
                    draw_call_status(&mut frame, &v, &theme.palette(), c.state);
                    surface.blit(&frame.pm, v.x - SHADOW_PAD, v.y - SHADOW_PAD, v.alpha, None);
                }
                _ => surface.blit(base, v.x - SHADOW_PAD, v.y - SHADOW_PAD, v.alpha, None),
            }
        }
        cache.retain(|id, _| seen.contains(id));
    });
    surface.pm
}

thread_local! {
    /// Last rendered pixmap per banner id, reused while nothing about the
    /// banner's appearance changed (a static text banner next to a ringing
    /// call is not repainted 25 times a second).
    static BANNERS: RefCell<HashMap<u64, (u64, Pixmap)>> = RefCell::new(HashMap::new());
}

/// A hash of everything that affects how one banner looks, except its
/// position and fade (those are applied when compositing) and the ripple
/// and timer (those are drawn over the cached result every frame).
fn signature(v: &BannerView<'_>, scale: f32, theme: Theme) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    format!("{:?}", v.banner).hash(&mut h);
    v.lines.hash(&mut h);
    v.app_name.hash(&mut h);
    scale.to_bits().hash(&mut h);
    theme.hash(&mut h);
    v.geo.height.to_bits().hash(&mut h);
    v.input.expand.to_bits().hash(&mut h);
    v.input.status.to_bits().hash(&mut h);
    format!("{:?}{:?}{:?}", v.hover, v.pressed, v.btn_hit).hash(&mut h);
    v.btn_hover.to_bits().hash(&mut h);
    v.btn_press.to_bits().hash(&mut h);
    v.hovered.hash(&mut h);
    v.field.text().hash(&mut h);
    v.field.cursor().hash(&mut h);
    v.field.scroll.to_bits().hash(&mut h);
    v.focused.hash(&mut h);
    (v.focused && v.caret_on).hash(&mut h);
    format!("{:?}", v.reply).hash(&mut h);
    age_label(v.age).hash(&mut h);
    h.finish()
}

fn empty_pixmap() -> Pixmap {
    // Only fails for zero or overflowing sizes; 1x1 is neither.
    #[allow(clippy::expect_used)]
    Pixmap::new(1, 1).expect("1x1 pixmap")
}

type ShadowKey = (u32, u32, u32, Theme);

thread_local! {
    static SHADOWS: RefCell<HashMap<ShadowKey, Pixmap>> = RefCell::new(HashMap::new());
}

/// The shadow plate for a banner of `h` logical height, cached.
fn shadow_for(h: f32, scale: f32, theme: Theme, pal: &Palette) -> Option<Pixmap> {
    let key = (h.round() as u32, scale.to_bits(), 0, theme);
    if let Some(hit) = SHADOWS.with(|c| c.borrow().get(&key).cloned()) {
        return Some(hit);
    }
    let (w, ph) = (BANNER_W + 2.0 * SHADOW_PAD, h + 2.0 * SHADOW_PAD);
    let rect = Rect::new(SHADOW_PAD, SHADOW_PAD, BANNER_W, h);
    let strength = f32::from(pal.shadow.3) / 255.0;
    let mut out = Canvas::new(w, ph, scale)?;
    // Broad ambient shadow and a tight contact shadow.
    let layers = [(10.0, 8.0, 4.0, 0.80), (3.0, 1.5, 2.0, 0.55)];
    for (sigma, dy, down, k) in layers {
        if let Some(l) = shadow_layer(
            w,
            ph,
            scale,
            rect,
            RADIUS,
            (0.0, dy),
            sigma,
            down,
            pal.shadow.with_alpha(strength * k),
        ) {
            out.blit(&l, 0.0, 0.0, 1.0, None);
        }
    }
    // The panel is translucent: keep the shadow out from underneath it.
    out.clear_rrect(rect, RADIUS);
    SHADOWS.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() > 24 {
            c.clear();
        }
        c.insert(key, out.pm.clone());
    });
    Some(out.pm)
}

/// Draw one banner into its own pixmap (banner size plus shadow room).
fn draw_banner(v: &BannerView<'_>, scale: f32, theme: Theme) -> Option<Pixmap> {
    let pal = theme.palette();
    let h = v.geo.height;
    let mut out = Canvas::new(BANNER_W + 2.0 * SHADOW_PAD, h + 2.0 * SHADOW_PAD, scale)?;
    if let Some(sh) = shadow_for(h, scale, theme, &pal) {
        out.pm = sh;
    }
    let panel = Rect::new(SHADOW_PAD, SHADOW_PAD, BANNER_W, h);
    out.fill_rrect(panel, RADIUS, pal.panel);
    let sheen = match theme {
        Theme::Dark => (Rgba(255, 255, 255, 12), Rgba(255, 255, 255, 0)),
        Theme::Light => (Rgba(255, 255, 255, 110), Rgba(255, 255, 255, 0)),
    };
    out.fill_rrect_gradient(
        Rect::new(
            panel.x,
            panel.y,
            panel.w,
            (h * 0.7).max(RADIUS * 2.0).min(h),
        ),
        RADIUS,
        sheen.0,
        sheen.1,
    );

    // Content only pokes outside the panel while the reply area is growing
    // or shrinking; clip then, and skip the costly mask otherwise.
    let settling = |t: f32| t > 0.0 && t < 1.0;
    if settling(v.input.expand) || settling(v.input.status) {
        let mut content = Canvas::new(BANNER_W, h, scale)?;
        draw_content(&mut content, v, &pal);
        let mask = out.rrect_mask(panel, RADIUS);
        out.blit(&content.pm, SHADOW_PAD, SHADOW_PAD, 1.0, mask.as_ref());
    } else {
        out.origin = (SHADOW_PAD, SHADOW_PAD);
        draw_content(&mut out, v, &pal);
        out.origin = (0.0, 0.0);
    }

    out.stroke_rrect(panel.inset(0.5), RADIUS - 0.5, 1.0, pal.border);
    Some(out.pm)
}

fn draw_content(cv: &mut Canvas, v: &BannerView<'_>, pal: &Palette) {
    draw_chip(cv, v, pal);
    match v.banner {
        Banner::Call(c) => draw_call(cv, v, pal, c.state),
        Banner::Text(_) => draw_text_banner(cv, v, pal),
    }
}

// ---- text helpers --------------------------------------------------------

fn put(cv: &mut Canvas, s: &str, x: f32, y: f32, style: Style, color: Rgba, opacity: f32) {
    let k = cv.s;
    let (x, y) = (x + cv.origin.0, y + cv.origin.1);
    text::draw(&mut cv.pm, s, x * k, y * k, style.scaled(k), color, opacity);
}

fn put_right(cv: &mut Canvas, s: &str, right: f32, y: f32, style: Style, color: Rgba, op: f32) {
    let w = text::measure(s, style);
    put(cv, s, right - w, y, style, color, op);
}

fn put_centered(cv: &mut Canvas, s: &str, cx: f32, y: f32, style: Style, color: Rgba) {
    let w = text::measure(s, style);
    put(cv, s, cx - w / 2.0, y, style, color, 1.0);
}

fn name_style() -> Style {
    Style::new(15.0, Wt::Semibold)
}

fn sub_style() -> Style {
    Style::new(13.0, Wt::Regular)
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = anim::clamp01((x - a) / (b - a));
    t * t * (3.0 - 2.0 * t)
}

// ---- chip row -------------------------------------------------------------

fn draw_chip(cv: &mut Canvas, v: &BannerView<'_>, pal: &Palette) {
    let g = &v.geo;
    icons::app_tile(cv, g.icon, Rgba(255, 255, 255, 36));
    let label_style = Style::new(11.0, Wt::Semibold).tracked(0.07);
    let ly = g.chip_row.y + (g.chip_row.h - text::line_height(label_style)) / 2.0;
    put(
        cv,
        &v.app_name.to_uppercase(),
        g.icon.right() + 8.0,
        ly,
        label_style,
        pal.text2,
        1.0,
    );

    match v.banner {
        Banner::Call(c) => {
            let (label, color) = match c.state {
                CallState::Incoming => ("INCOMING", pal.red_text),
                CallState::Dialing => ("CALLING", pal.red_text),
                CallState::Active { .. } => ("ON CALL", pal.text2),
                CallState::Held => ("ON HOLD", pal.text2),
            };
            let pitch = 2.0;
            let w = dotfont::columns(label) as f32 * pitch;
            let x = g.chip_row.right() - w;
            let y = g.chip_row.y + (g.chip_row.h - 7.0 * pitch) / 2.0;
            dots(cv, label, x, y, pitch, 0.42, color, None);
        }
        Banner::Text(_) => {
            let close_a = match v.input.kind {
                crate::layout::Kind::Text { can_reply: true } => v.input.expand,
                _ => f32::from(u8::from(v.hovered)),
            };
            let st = Style::new(12.0, Wt::Regular);
            let when = age_label(v.age);
            let ty = g.chip_row.y + (g.chip_row.h - text::line_height(st)) / 2.0;
            put_right(
                cv,
                &when,
                g.chip_row.right(),
                ty,
                st,
                pal.text2,
                1.0 - close_a,
            );
            if let (Some(close), true) = (g.close, close_a > 0.01) {
                let (cx, cy) = close.center();
                let hot = v.hover == Some(Hit::Close);
                let fill = if hot { pal.control_hover } else { pal.control };
                cv.circle(cx, cy, 9.0, fill.scaled_alpha(close_a));
                icons::cross(cv, cx, cy, 6.0, 1.6, pal.text2.scaled_alpha(close_a));
            }
        }
    }
}

fn age_label(age: std::time::Duration) -> String {
    let s = age.as_secs();
    if s < 60 {
        "now".to_owned()
    } else if s < 3600 {
        format!("{}m ago", s / 60)
    } else {
        format!("{}h ago", s / 3600)
    }
}

// ---- dot matrix -----------------------------------------------------------

/// Draw dot-matrix text with its top-left at (`x`, `y`). Returns the width.
/// Strings the 5x7 font cannot draw fall back to regular text.
#[allow(clippy::too_many_arguments)]
fn dots(
    cv: &mut Canvas,
    s: &str,
    x: f32,
    y: f32,
    pitch: f32,
    radius: f32,
    color: Rgba,
    off: Option<Rgba>,
) -> f32 {
    if !dotfont::supported(s) {
        let st = Style::new(pitch * 6.0, Wt::Semibold);
        put(cv, &s.to_uppercase(), x, y, st, color, 1.0);
        return text::measure(&s.to_uppercase(), st);
    }
    let r = pitch * radius;
    let at = |c: usize, row: usize| (x + (c as f32 + 0.5) * pitch, y + (row as f32 + 0.5) * pitch);
    // One path per colour: hundreds of dots cost one fill, not hundreds.
    if let Some(off) = off {
        let mut pb = PathBuilder::new();
        for (c, row, lit) in dotfont::cells(s) {
            if !lit {
                let (cx, cy) = at(c, row);
                pb.push_circle(cx, cy, r * 0.72);
            }
        }
        if let Some(p) = pb.finish() {
            cv.fill_path(&p, off);
        }
    }
    let mut pb = PathBuilder::new();
    for (c, row) in dotfont::lit_dots(s) {
        let (cx, cy) = at(c, row);
        pb.push_circle(cx, cy, r);
    }
    if let Some(p) = pb.finish() {
        cv.fill_path(&p, color);
    }
    dotfont::columns(s) as f32 * pitch
}

// ---- avatars --------------------------------------------------------------

fn fnv1a(s: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in s.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// A deterministic pastel colour for `key`.
pub fn avatar_color(key: &str) -> Rgba {
    let hue = (fnv1a(key) % 360) as f32;
    hsl(hue, 0.62, 0.76)
}

fn hsl(h: f32, s: f32, l: f32) -> Rgba {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let q = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Rgba(q(r), q(g), q(b), 255)
}

/// Up to two initials from a display name, or `None` for number-only names.
pub fn initials(name: &str) -> Option<String> {
    let mut out = String::new();
    for word in name.split_whitespace() {
        let first = word
            .graphemes(true)
            .find(|g| g.chars().any(char::is_alphabetic));
        if let Some(g) = first {
            out.push_str(&g.to_uppercase());
        }
        if out.graphemes(true).count() >= 2 {
            break;
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn draw_avatar(cv: &mut Canvas, r: Rect, name: &str, key: &str, pal: &Palette) {
    let (cx, cy) = r.center();
    let rad = r.w / 2.0;
    let base = avatar_color(key);
    let ink = Rgba::hex(0x1C1C1E);
    match initials(name) {
        Some(ini) => {
            cv.fill_rrect_gradient(r, rad, base.shift(0.14), base.shift(-0.08));
            let st = Style::new(rad * 0.82, Wt::Semibold);
            let y = cy - text::line_height(st) / 2.0;
            put_centered(cv, &ini, cx, y, st, ink);
        }
        None => {
            // Unknown caller: neutral disc with a silhouette.
            let disc = pal.text2.with_alpha(0.28);
            cv.circle(cx, cy, rad, disc);
            // Draw the silhouette clipped to the disc using a mask.
            if let Some(mask) = cv.rrect_mask(r, rad) {
                let mut tmp = Canvas::new(BANNER_W, r.bottom() + 2.0, cv.s);
                if let Some(t) = tmp.as_mut() {
                    icons::person(t, cx, cy, rad, pal.text2);
                    cv.blit(&t.pm, 0.0, 0.0, 1.0, Some(&mask));
                }
            }
        }
    }
}

// ---- call banner ----------------------------------------------------------

fn draw_call(cv: &mut Canvas, v: &BannerView<'_>, pal: &Palette, state: CallState) {
    let g = &v.geo;
    let name = v.banner.display_name();
    draw_avatar(cv, g.avatar, name, name, pal);

    let sub = call_subtitle(v.banner, state);
    let ns = name_style();
    let ss = sub_style();
    let (nh, sh) = (text::line_height(ns), text::line_height(ss));
    let two = !sub.is_empty();
    let block = if two { nh + sh } else { nh };
    let mut y = g.text_col.y + (g.text_col.h - block) / 2.0;
    put(
        cv,
        &text::ellipsize(name, ns, g.text_col.w),
        g.text_col.x,
        y,
        ns,
        pal.text,
        1.0,
    );
    y += nh;
    if two {
        put(
            cv,
            &text::ellipsize(&sub, ss, g.text_col.w),
            g.text_col.x,
            y,
            ss,
            pal.text2,
            1.0,
        );
    }

    if !is_dynamic(state) {
        draw_call_status(cv, v, pal, state);
    }

    for (hit, rect) in &g.buttons {
        draw_call_button(cv, v, pal, *hit, *rect);
    }
}

/// Whether a call state paints something that changes between frames: the
/// ripple while ringing, the timer while active. Those parts are drawn on top
/// of the cached static banner.
fn is_dynamic(state: CallState) -> bool {
    !matches!(state, CallState::Held)
}

/// Status art on the right of a call's main row: pulsing dots while ringing
/// or dialing, the running timer when active, "HELD" on hold.
fn draw_call_status(cv: &mut Canvas, v: &BannerView<'_>, pal: &Palette, state: CallState) {
    let st = v.geo.call_status;
    match state {
        CallState::Incoming | CallState::Dialing => {
            let n = 7;
            let pitch = 10.0;
            let total = (n - 1) as f32 * pitch;
            let x0 = st.right() - total - 3.4;
            let cy = st.center().1;
            for i in 0..n {
                let lvl = anim::ripple_level(i, n, v.ripple_phase);
                let base = pal.text2.with_alpha(0.38);
                let colour = base.mix(pal.red, lvl);
                cv.circle(x0 + i as f32 * pitch, cy, 2.0 + 2.1 * lvl, colour);
            }
        }
        CallState::Active { .. } => {
            let secs = v.call_elapsed.map_or(0, |d| d.as_secs());
            let label = timer_text(secs);
            let cols = dotfont::columns(&label) as f32;
            let pitch = (st.w / cols).min(3.4);
            let y = st.center().1 - 3.5 * pitch;
            let x = st.right() - cols * pitch;
            dots(
                cv,
                &label,
                x,
                y,
                pitch,
                0.42,
                pal.text,
                Some(pal.dot_off.with_alpha(0.7)),
            );
        }
        CallState::Held => {
            let label = "HELD";
            let pitch = 2.8;
            let cols = dotfont::columns(label) as f32;
            let y = st.center().1 - 3.5 * pitch;
            dots(
                cv,
                label,
                st.right() - cols * pitch,
                y,
                pitch,
                0.43,
                pal.text2,
                Some(pal.dot_off.with_alpha(0.5)),
            );
        }
    }
}

fn call_subtitle(b: &Banner, state: CallState) -> String {
    let number = b.subtitle();
    match (state, number.is_empty()) {
        (CallState::Incoming, true) => "Incoming call".to_owned(),
        (CallState::Incoming, false) => number.to_owned(),
        (CallState::Dialing, true) => "Calling\u{2026}".to_owned(),
        (CallState::Dialing, false) => format!("Calling\u{2026} {number}"),
        (CallState::Active { .. }, true) => "Connected".to_owned(),
        (CallState::Active { .. }, false) => number.to_owned(),
        (CallState::Held, true) => "On hold".to_owned(),
        (CallState::Held, false) => number.to_owned(),
    }
}

/// `mm:ss`, or `h:mm:ss` from an hour on.
pub fn timer_text(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs / 60) % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

fn draw_call_button(cv: &mut Canvas, v: &BannerView<'_>, pal: &Palette, hit: Hit, rect: Rect) {
    let (base, ink, label, hangup) = match hit {
        Hit::Answer => (pal.green, pal.green_ink, "Answer", false),
        Hit::Decline => (pal.red, pal.on_accent, "Decline", true),
        _ => (pal.red, pal.on_accent, "Hang up", true),
    };
    let (hover, press) = if v.btn_hit == Some(hit) {
        (v.btn_hover, v.btn_press)
    } else {
        (0.0, 0.0)
    };
    let r = rect.scale_about_center(1.0 - 0.03 * press);
    let fill = base.shift(0.10 * hover - 0.20 * press);
    cv.fill_rrect(r, r.h / 2.0, fill);
    // A hairline of light on top gives the pill a little body.
    let k = 1.0 - 0.03 * press;
    let st = Style::new(14.0 * k, Wt::Semibold);
    let icon = 15.0 * k;
    let gap = 7.0 * k;
    let lw = text::measure(label, st);
    let total = icon + gap + lw;
    let (cx, cy) = r.center();
    let x0 = cx - total / 2.0;
    icons::handset(cv, x0 + icon / 2.0, cy, icon, hangup, ink);
    put(
        cv,
        label,
        x0 + icon + gap,
        cy - text::line_height(st) / 2.0,
        st,
        ink,
        1.0,
    );
}

// ---- text banner ----------------------------------------------------------

fn draw_text_banner(cv: &mut Canvas, v: &BannerView<'_>, pal: &Palette) {
    let g = &v.geo;
    let name = v.banner.display_name();
    draw_avatar(cv, g.avatar, name, name, pal);
    let ns = name_style();
    put(
        cv,
        &text::ellipsize(name, ns, g.text_col.w),
        g.text_col.x,
        g.text_col.y + 0.5,
        ns,
        pal.text,
        1.0,
    );
    let bs = body_style();
    for (i, line) in v.lines.iter().enumerate() {
        put(
            cv,
            line,
            g.text_col.x,
            g.text_col.y + 21.0 + 17.0 * i as f32,
            bs,
            pal.text2.mix(pal.text, 0.35),
            1.0,
        );
    }

    let ea = smoothstep(0.2, 1.0, v.input.expand);
    if let (Some(field), true) = (g.field, ea > 0.0) {
        draw_field(cv, v, pal, field, ea);
    }
    if let (Some(line), true) = (g.status_line, v.input.status > 0.01) {
        draw_status(cv, v, pal, line, smoothstep(0.0, 1.0, v.input.status));
    }
}

fn draw_field(cv: &mut Canvas, v: &BannerView<'_>, pal: &Palette, field: Rect, a: f32) {
    let radius = field.h / 2.0;
    cv.fill_rrect(field, radius, pal.control.scaled_alpha(a));
    cv.stroke_rrect(
        field.inset(0.5),
        radius - 0.5,
        1.0,
        pal.border.scaled_alpha(a),
    );
    if v.focused {
        cv.stroke_rrect(
            field.inset(-0.5),
            radius + 0.5,
            2.0,
            pal.focus.scaled_alpha(a * 0.4),
        );
    }

    let style = Style::new(FIELD_FONT, Wt::Regular);
    let send = v.geo.send.unwrap_or_default();
    let tx = field.x + FIELD_TEXT_X;
    let tw = (send.x - 8.0 - tx).max(0.0);
    let th = text::line_height(style);
    let ty = field.y + (field.h - th) / 2.0;
    if v.field.is_empty() {
        put(cv, "Reply", tx, ty, style, pal.text2, a);
    } else if let Some(mut clip) = Canvas::new(tw, field.h, cv.s) {
        let k = cv.s;
        text::draw(
            &mut clip.pm,
            v.field.text(),
            -v.field.scroll * k,
            (field.h - th) / 2.0 * k,
            style.scaled(k),
            pal.text,
            1.0,
        );
        cv.blit(&clip.pm, tx, field.y, a, None);
    }
    if v.focused && v.caret_on {
        let cx = tx + text::measure(v.field.before_cursor(), style) - v.field.scroll;
        if cx >= tx - 1.0 && cx <= tx + tw + 1.0 {
            cv.line(
                (cx, ty + 1.5),
                (cx, ty + th - 1.5),
                1.5,
                pal.focus.scaled_alpha(a),
            );
        }
    }

    // Send button: blue once there is something to send.
    let (sx, sy) = send.center();
    let hot = v.hover == Some(Hit::Send);
    let ready = !v.field.is_empty();
    let press = if v.btn_hit == Some(Hit::Send) {
        v.btn_press
    } else {
        0.0
    };
    let hover = if v.btn_hit == Some(Hit::Send) {
        v.btn_hover
    } else {
        0.0
    };
    let base = if ready { pal.blue } else { pal.control_hover };
    let fill = if ready {
        base.shift(0.12 * hover - 0.2 * press)
    } else if hot {
        pal.control_hover.shift(0.1)
    } else {
        base
    };
    let rad = send.w / 2.0 * (1.0 - 0.03 * press);
    cv.circle(sx, sy, rad, fill.scaled_alpha(a));
    let ink = if ready { pal.on_accent } else { pal.text2 };
    icons::arrow_up(cv, sx, sy, 15.0, ink.scaled_alpha(a));
}

fn draw_status(cv: &mut Canvas, v: &BannerView<'_>, pal: &Palette, line: Rect, a: f32) {
    let st = Style::new(12.0, Wt::Medium);
    let y = line.y + (line.h - text::line_height(st)) / 2.0;
    let Some(status) = v.reply else { return };
    match status {
        ReplyStatus::Sending => put(cv, "Sending\u{2026}", line.x, y, st, pal.text2, a),
        ReplyStatus::Sent => {
            put(cv, "Sent", line.x, y, st, pal.ok_text, a);
            let w = text::measure("Sent", st);
            icons::check(
                cv,
                line.x + w + 6.0,
                line.center().1,
                9.0,
                1.7,
                pal.ok_text.scaled_alpha(a),
            );
        }
        ReplyStatus::Failed(msg) => {
            icons::alert(
                cv,
                line.x + 6.0,
                line.center().1,
                5.5,
                pal.red_text.scaled_alpha(a),
            );
            let text = text::ellipsize(msg, st, line.w - 18.0);
            put(cv, &text, line.x + 18.0, y, st, pal.red_text, a);
        }
    }
}

#[cfg(test)]
mod tests;
