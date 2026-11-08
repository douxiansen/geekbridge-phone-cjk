// SPDX-License-Identifier: GPL-3.0-or-later
//! Look of the app: macOS-style structure with Nothing-OS colours. True-black or warm off-white
//! surfaces, grey ramps and a single red.
//!
//! Adapted from the GeekBridge design system (`bridge-apps/src/theme.rs` in the GeekBridge
//! repository, same licence). The `AppKind` and `crate::state` coupling is removed, the palette
//! is the one the GeekBridge apps share, and only the controls this app uses are kept.

use super::icons::{self, Icon};
use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame, Margin,
    Response, RichText, Sense, Stroke, StrokeKind, TextStyle, Theme, Ui, Visuals, WidgetInfo,
    WidgetType, vec2,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Corner radius of a list row or sidebar item.
pub const R_ROW: u8 = 12;
/// Corner radius of a card.
pub const R_CARD: u8 = 16;
/// Corner radius of a tile.
pub const R_TILE: u8 = 22;

/// 4-pt spacing scale.
pub const S1: f32 = 4.0;
pub const S2: f32 = 8.0;
pub const S3: f32 = 12.0;
pub const S4: f32 = 16.0;
pub const S5: f32 = 24.0;

/// The colours of one theme. Pick it with [`current`].
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub dark: bool,
    pub toolbar: Color32,
    pub sidebar: Color32,
    pub content: Color32,
    /// Raised areas inside content: inputs, keys, cards.
    pub surface: Color32,
    pub text: Color32,
    pub text2: Color32,
    pub text3: Color32,
    pub separator: Color32,
    pub control: Color32,
    pub control_hover: Color32,
    pub control_active: Color32,
    /// Text-safe red.
    pub accent: Color32,
    /// Brand red #D71921 for fills, dots and indicators.
    pub accent_fill: Color32,
    pub on_accent: Color32,
    pub selected_bg: Color32,
    pub selected_fg: Color32,
    pub ok: Color32,
    pub warn: Color32,
    pub err: Color32,
    /// Recessed display (the status readout in the toolbar).
    pub lcd_bg: Color32,
    pub lcd_off: Color32,
    /// Rounded-square tile and card fill, and its hover shade.
    pub tile: Color32,
    pub tile_hover: Color32,
    /// Unlit dot-matrix dots.
    pub dot_off: Color32,
}

/// The outgoing-bubble blue, as on a phone's own messaging app.
pub const BUBBLE_BLUE: Color32 = rgb(0x0A84FF);

/// One palette for every window: true-black / warm off-white, grey ramps and a single red.
pub fn palette(dark: bool) -> Palette {
    let base = base_palette(dark);
    if high_contrast() {
        high_contrast_of(base)
    } else {
        base
    }
}

static HIGH_CONTRAST: AtomicBool = AtomicBool::new(false);

pub fn set_high_contrast(on: bool) {
    HIGH_CONTRAST.store(on, Ordering::Relaxed);
}

pub fn high_contrast() -> bool {
    HIGH_CONTRAST.load(Ordering::Relaxed)
}

/// `GEEKBRIDGE_HIGH_CONTRAST=1`, or a GTK theme whose name says high contrast, asks for it.
pub fn high_contrast_requested() -> bool {
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    env("GEEKBRIDGE_HIGH_CONTRAST") == "1" || env("GTK_THEME").to_lowercase().contains("highcontrast")
}

/// Same layout, but secondary text becomes full ink and every edge and unlit dot a strong mid
/// grey, so nothing relies on a faint tint.
fn high_contrast_of(mut p: Palette) -> Palette {
    p.text2 = p.text;
    p.text3 = p.text;
    p.separator = if p.dark { rgb(0x8C8C86) } else { rgb(0x5F5F5A) };
    p.dot_off = p.separator;
    p.lcd_off = p.separator;
    p
}

fn base_palette(dark: bool) -> Palette {
    if dark {
        Palette {
            dark,
            toolbar: rgb(0x000000),
            sidebar: rgb(0x0A0A0A),
            content: rgb(0x000000),
            surface: rgb(0x121212),
            text: rgb(0xF4F4F1),
            text2: rgb(0xA3A39D),
            text3: rgb(0x9A9A94),
            separator: rgb(0x242422),
            control: rgb(0x1E1E1D),
            control_hover: rgb(0x2A2A28),
            control_active: rgb(0x383835),
            accent: rgb(0xFF5A5A),
            accent_fill: rgb(0xD71921),
            on_accent: Color32::WHITE,
            selected_bg: rgb(0xF4F4F1),
            selected_fg: rgb(0x0C0C0C),
            ok: rgb(0x58C486),
            warn: rgb(0xE0A030),
            err: rgb(0xFF5A5A),
            lcd_bg: rgb(0x0A0A0A),
            lcd_off: rgb(0x242422),
            tile: rgb(0x141414),
            tile_hover: rgb(0x1C1C1B),
            dot_off: rgb(0x2E2E2C),
        }
    } else {
        Palette {
            dark,
            toolbar: rgb(0xF4F3EF),
            sidebar: rgb(0xFBFAF7),
            content: rgb(0xF4F3EF),
            surface: rgb(0xFFFFFF),
            text: rgb(0x0C0C0C),
            text2: rgb(0x52524D),
            text3: rgb(0x5F5F5A),
            separator: rgb(0xDEDDD6),
            control: rgb(0xE8E7E1),
            control_hover: rgb(0xDDDCD5),
            control_active: rgb(0xCFCEC6),
            accent: rgb(0xB8121A),
            accent_fill: rgb(0xD71921),
            on_accent: Color32::WHITE,
            selected_bg: rgb(0x0C0C0C),
            selected_fg: rgb(0xFAFAF8),
            ok: rgb(0x1A6E3E),
            warn: rgb(0x8F5500),
            err: rgb(0xB8121A),
            lcd_bg: rgb(0xE8E7E1),
            lcd_off: rgb(0xD6D5CE),
            tile: rgb(0xFFFFFF),
            tile_hover: rgb(0xF7F6F2),
            dot_off: rgb(0xD3D2CB),
        }
    }
}

/// The palette of the theme the context is showing right now.
pub fn current(ctx: &egui::Context) -> Palette {
    palette(ctx.style().visuals.dark_mode)
}

// ---------------------------------------------------------------- fonts

const SEMIBOLD: &str = "geekbridge-semibold";

pub fn semibold_family() -> FontFamily {
    FontFamily::Name(SEMIBOLD.into())
}

struct Face {
    regular: Vec<PathBuf>,
    bold: Vec<PathBuf>,
}

fn paths(list: &[&str]) -> Vec<PathBuf> {
    list.iter().map(PathBuf::from).collect()
}

/// System UI font families in order of preference. If none exists the embedded Noto Sans is used.
fn candidate_faces() -> Vec<Face> {
    vec![
        Face {
            regular: paths(&["C:\\Windows\\Fonts\\segoeui.ttf"]),
            bold: paths(&["C:\\Windows\\Fonts\\segoeuib.ttf", "C:\\Windows\\Fonts\\seguisb.ttf"]),
        },
        // macOS: San Francisco ships as one variable file; the bold slot reuses it.
        Face {
            regular: paths(&["/System/Library/Fonts/SFNS.ttf"]),
            bold: paths(&["/System/Library/Fonts/SFNS.ttf"]),
        },
        Face {
            regular: paths(&[
                "/usr/share/fonts/noto/NotoSans-Regular.ttf",
                "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
                "/usr/share/fonts/google-noto/NotoSans-Regular.ttf",
                "/usr/share/fonts/NotoSans-Regular.ttf",
            ]),
            bold: paths(&[
                "/usr/share/fonts/noto/NotoSans-Bold.ttf",
                "/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf",
                "/usr/share/fonts/google-noto/NotoSans-Bold.ttf",
                "/usr/share/fonts/NotoSans-Bold.ttf",
            ]),
        },
        Face {
            regular: paths(&[
                "/usr/share/fonts/liberation/LiberationSans-Regular.ttf",
                "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
            ]),
            bold: paths(&[
                "/usr/share/fonts/liberation/LiberationSans-Bold.ttf",
                "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
            ]),
        },
        Face {
            regular: paths(&["/usr/share/fonts/TTF/DejaVuSans.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"]),
            bold: paths(&["/usr/share/fonts/TTF/DejaVuSans-Bold.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"]),
        },
    ]
}

fn read_font(candidates: &[PathBuf]) -> Option<Vec<u8>> {
    candidates.iter().find_map(|path| {
        let meta = std::fs::metadata(path).ok()?;
        // Skip anything that is not a plausibly sized font file.
        (meta.is_file() && meta.len() < 32 * 1024 * 1024).then(|| std::fs::read(path).ok()).flatten()
    })
}

/// Fonts embedded in the binary (licences next to them in `assets/fonts`).
const EMBEDDED_FONTS: [(&str, &[u8]); 3] = [
    ("noto-sans", include_bytes!("../../assets/fonts/NotoSans-Regular-subset.ttf")),
    ("noto-emoji", include_bytes!("../../assets/fonts/NotoEmoji-Regular.ttf")),
    ("hack", include_bytes!("../../assets/fonts/Hack-Regular.ttf")),
];

fn embed_fonts(fonts: &mut FontDefinitions) {
    for (name, bytes) in EMBEDDED_FONTS {
        fonts.font_data.insert(name.into(), Arc::new(FontData::from_static(bytes)));
    }
    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    proportional.extend(["noto-sans", "noto-emoji"].map(String::from));
    let monospace = fonts.families.entry(FontFamily::Monospace).or_default();
    monospace.extend(["hack", "noto-sans", "noto-emoji"].map(String::from));
}

/// Registers the embedded fonts behind the first system UI face that exists.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    embed_fonts(&mut fonts);
    let mut bold_name = None;
    // Golden-image runs use the embedded fonts only so output does not depend on the machine.
    let bundled_only = std::env::var_os("GEEKBRIDGE_BUNDLED_FONTS_ONLY").is_some();
    for face in candidate_faces().into_iter().filter(|_| !bundled_only) {
        let Some(regular) = read_font(&face.regular) else {
            continue;
        };
        fonts.font_data.insert("system-regular".into(), Arc::new(FontData::from_owned(regular)));
        if let Some(bold) = read_font(&face.bold) {
            fonts.font_data.insert("system-bold".into(), Arc::new(FontData::from_owned(bold)));
            bold_name = Some("system-bold".to_owned());
        }
        fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "system-regular".into());
        break;
    }
    // Semibold falls back to the regular face when no bold exists.
    let mut semibold: Vec<String> = bold_name.into_iter().collect();
    semibold.extend(fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default());
    fonts.families.insert(semibold_family(), semibold);
    ctx.set_fonts(fonts);
}

// ---------------------------------------------------------------- style

/// Installs fonts and both theme styles. Palettes are picked per frame from the active theme.
pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    set_high_contrast(high_contrast_requested());
    ctx.options_mut(|o| o.fallback_theme = Theme::Light);
    for (theme, dark) in [(Theme::Light, false), (Theme::Dark, true)] {
        let pal = palette(dark);
        ctx.style_mut_of(theme, |style| {
            let mut v = if dark { Visuals::dark() } else { Visuals::light() };
            v.panel_fill = pal.content;
            v.window_fill = pal.content;
            v.extreme_bg_color = pal.surface;
            v.faint_bg_color = pal.control;
            v.override_text_color = Some(pal.text);
            v.hyperlink_color = pal.accent;
            v.selection.bg_fill = pal.accent_fill.gamma_multiply(if dark { 0.5 } else { 0.28 });
            v.selection.stroke = Stroke::new(1.0_f32, pal.accent);
            v.text_cursor.stroke = Stroke::new(1.5_f32, pal.accent);
            let radius = CornerRadius::same(12);
            for (w, fill) in [
                (&mut v.widgets.inactive, pal.control),
                (&mut v.widgets.hovered, pal.control_hover),
                (&mut v.widgets.active, pal.control_active),
            ] {
                w.bg_fill = fill;
                w.weak_bg_fill = fill;
                w.bg_stroke = Stroke::NONE;
                w.corner_radius = CornerRadius::same(8);
                w.fg_stroke = Stroke::new(1.0_f32, pal.text);
            }
            let outline = if high_contrast() {
                pal.text
            } else if dark {
                rgb(0x666662)
            } else {
                rgb(0x8A8A84)
            };
            v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, outline);
            v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, pal.text3);
            v.widgets.active.bg_stroke = Stroke::new(2.0_f32, focus_color(&pal));
            v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, pal.separator);
            v.widgets.noninteractive.corner_radius = radius;
            v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, pal.text2);
            v.widgets.open.corner_radius = radius;
            v.window_corner_radius = CornerRadius::same(18);
            v.window_stroke = Stroke::new(1.0_f32, pal.separator);
            style.visuals = v;
            // Reduced motion: widgets change state instantly instead of fading.
            style.animation_time = 0.0;
            style.spacing.item_spacing = vec2(8.0, 8.0);
            style.spacing.button_padding = vec2(12.0, 5.0);
            style.spacing.interact_size.y = 28.0;
            style.spacing.scroll.bar_width = 8.0;
            style.spacing.scroll.floating = true;
            style.text_styles = [
                (TextStyle::Heading, FontId::new(22.0, semibold_family())),
                (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
                (TextStyle::Button, FontId::new(14.0, FontFamily::Proportional)),
                (TextStyle::Small, FontId::new(12.0, FontFamily::Proportional)),
                (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
            ]
            .into();
        });
    }
}

// ---------------------------------------------------------------- text helpers

pub fn semibold(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text.into()).font(FontId::new(size, semibold_family()))
}

pub fn secondary(pal: &Palette, text: impl Into<String>) -> RichText {
    RichText::new(text.into()).color(pal.text2)
}

/// Placeholder text of a stock text field, in the secondary colour (held to 4.5:1).
pub fn hint(pal: &Palette, text: &str) -> RichText {
    RichText::new(text).color(pal.text3)
}

pub fn caption(pal: &Palette, text: impl Into<String>) -> RichText {
    secondary(pal, text).size(12.0)
}

/// Small uppercase section label.
pub fn overline(pal: &Palette, text: &str) -> RichText {
    RichText::new(text.to_uppercase()).font(FontId::new(12.0, semibold_family())).color(pal.text3)
}

// ---------------------------------------------------------------- structure

/// Unified toolbar across the top of the content area with a hairline underneath.
pub fn toolbar<R>(ctx: &egui::Context, pal: &Palette, height: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::TopBottomPanel::top("toolbar")
        .exact_height(height)
        .frame(Frame::new().fill(pal.toolbar).stroke(Stroke::NONE).inner_margin(Margin::symmetric(12, 0)))
        .show(ctx, |ui| {
            let r = ui.max_rect();
            ui.painter().hline(r.x_range().expand(12.0), r.bottom() + 0.5, Stroke::new(1.0_f32, pal.separator));
            add(ui)
        })
        .inner
}

pub fn hairline(ui: &mut Ui, pal: &Palette) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, pal.separator);
}

/// Filled dot, or a ring when `hollow`.
pub fn dot(ui: &mut Ui, color: Color32, hollow: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
    paint_dot(ui.painter(), rect.center(), color, hollow);
}

pub fn paint_dot(painter: &egui::Painter, center: egui::Pos2, color: Color32, hollow: bool) {
    if hollow {
        painter.circle_stroke(center, 3.5, Stroke::new(1.4_f32, color));
    } else {
        painter.circle_filled(center, 3.5, color);
    }
}

// ---------------------------------------------------------------- controls

/// Keyboard focus ring colour: full ink, at least 3:1 on every surface in both themes, never red.
pub fn focus_color(pal: &Palette) -> Color32 {
    pal.text
}

/// 2 pt ring one point outside `rect`.
pub fn focus_ring(ui: &Ui, pal: &Palette, rect: egui::Rect, radius: impl Into<CornerRadius>) {
    ui.painter().rect_stroke(rect.expand(1.0), radius, Stroke::new(2.0_f32, focus_color(pal)), StrokeKind::Outside);
}

/// [`focus_ring`] for round controls.
pub fn focus_circle(ui: &Ui, pal: &Palette, center: egui::Pos2, radius: f32) {
    ui.painter().circle_stroke(center, radius + 2.0, Stroke::new(2.0_f32, focus_color(pal)));
}

/// Tells screen readers what a custom-painted control is.
pub fn describe(response: &Response, label: &str, enabled: bool) {
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
}

/// Flat toolbar button: icon with an optional label; shaded on hover.
pub fn tool_button(ui: &mut Ui, pal: &Palette, icon: Icon, label: Option<&str>, enabled: bool, tip: &str) -> Response {
    let font = FontId::new(13.0, FontFamily::Proportional);
    let tint = if enabled { pal.text } else { pal.text3 };
    let galley = label.map(|l| ui.painter().layout_no_wrap(l.to_owned(), font, tint));
    let width = 30.0 + galley.as_ref().map_or(0.0, |g| g.size().x + 6.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(vec2(width, 28.0), sense);
    if enabled && (response.hovered() || response.is_pointer_button_down_on()) {
        let fill = if response.is_pointer_button_down_on() { pal.control_active } else { pal.control_hover.gamma_multiply(0.7) };
        ui.painter().rect_filled(rect, CornerRadius::same(7), fill);
    }
    if response.has_focus() {
        focus_ring(ui, pal, rect, CornerRadius::same(7));
    }
    let icon_rect = egui::Rect::from_center_size(egui::pos2(rect.left() + 15.0, rect.center().y), vec2(16.0, 16.0));
    icons::paint(ui.painter(), icon_rect, icon, tint);
    if let Some(galley) = galley {
        let pos = egui::pos2(rect.left() + 30.0, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(pos, galley, tint);
    }
    describe(&response, label.unwrap_or(tip), enabled);
    response.on_hover_text(tip)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    /// Filled with the brand red (decline, hang up).
    Primary,
    /// Quiet grey pill.
    Plain,
    /// High-contrast ink pill (white on black in dark, black on white in light).
    Inverse,
}

/// Pill height.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PillSize {
    Regular,
    Small,
}

/// Rounded pill button. Disabled buttons stay visible but muted.
pub fn pill_button(ui: &mut Ui, pal: &Palette, text: &str, kind: ButtonKind, enabled: bool) -> Response {
    pill(ui, pal, text, kind, enabled, PillSize::Regular, 0.0)
}

/// [`pill_button`] at least `min_width` wide (for side-by-side decisions).
pub fn pill_button_sized(ui: &mut Ui, pal: &Palette, text: &str, kind: ButtonKind, enabled: bool, min_width: f32) -> Response {
    pill(ui, pal, text, kind, enabled, PillSize::Regular, min_width)
}

pub fn pill(ui: &mut Ui, pal: &Palette, text: &str, kind: ButtonKind, enabled: bool, size: PillSize, min_width: f32) -> Response {
    let (fill, fg) = match (kind, enabled) {
        (ButtonKind::Primary, true) => (pal.accent_fill, pal.on_accent),
        (ButtonKind::Inverse, true) => (pal.selected_bg, pal.selected_fg),
        (ButtonKind::Primary | ButtonKind::Inverse, false) => (pal.control, pal.text3),
        (ButtonKind::Plain, true) => (pal.control, pal.text),
        (ButtonKind::Plain, false) => (pal.control.gamma_multiply(0.6), pal.text3),
    };
    let (height, font_size, pad) = match size {
        PillSize::Regular => (32.0, 13.5, 28.0),
        PillSize::Small => (26.0, 12.5, 22.0),
    };
    let galley = ui.painter().layout_no_wrap(text.to_owned(), FontId::new(font_size, semibold_family()), fg);
    let size = vec2((galley.size().x + pad).max(min_width), height);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let fill = if !enabled {
        fill
    } else if response.is_pointer_button_down_on() {
        fill.gamma_multiply(0.82)
    } else if response.hovered() {
        match kind {
            ButtonKind::Primary | ButtonKind::Inverse => fill.gamma_multiply(0.92),
            ButtonKind::Plain => pal.control_hover,
        }
    } else {
        fill
    };
    let radius = CornerRadius::same((height / 2.0) as u8);
    ui.painter().rect_filled(rect, radius, fill);
    if response.has_focus() {
        focus_ring(ui, pal, rect, radius);
    }
    ui.painter().galley(egui::pos2(rect.center().x - galley.size().x / 2.0, rect.center().y - galley.size().y / 2.0), galley, fg);
    describe(&response, text, enabled);
    response
}

/// Round icon button used for Send, Call and similar single actions.
pub fn round_button(ui: &mut Ui, pal: &Palette, diameter: f32, fill: Color32, icon: Icon, enabled: bool, label: &str) -> Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(vec2(diameter, diameter), sense);
    let fill = if !enabled {
        pal.control
    } else if response.is_pointer_button_down_on() {
        fill.gamma_multiply(0.8)
    } else if response.hovered() {
        fill.gamma_multiply(0.92)
    } else {
        fill
    };
    ui.painter().circle_filled(rect.center(), diameter / 2.0, fill);
    if response.has_focus() {
        focus_circle(ui, pal, rect.center(), diameter / 2.0);
    }
    let tint = if enabled { Color32::WHITE } else { pal.text3 };
    let icon_rect = egui::Rect::from_center_size(rect.center(), vec2(diameter * 0.46, diameter * 0.46));
    if icon == Icon::Phone {
        icons::paint_handset(ui.painter(), rect.center(), diameter * 0.34, tint, 0.0);
    } else if icon == Icon::HangUp {
        icons::paint_handset(ui.painter(), rect.center(), diameter * 0.34, tint, 135.0);
    } else {
        icons::paint(ui.painter(), icon_rect, icon, tint);
    }
    describe(&response, label, enabled);
    response.on_hover_text(label)
}

/// A small round icon button on a quiet fill, for row actions (call, message).
pub fn icon_button(ui: &mut Ui, pal: &Palette, icon: Icon, label: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::click());
    let fill = if response.is_pointer_button_down_on() {
        pal.control_active
    } else if response.hovered() {
        pal.control_hover
    } else {
        pal.control
    };
    ui.painter().circle_filled(rect.center(), 15.0, fill);
    if response.has_focus() {
        focus_circle(ui, pal, rect.center(), 15.0);
    }
    if icon == Icon::Phone {
        icons::paint_handset(ui.painter(), rect.center(), 8.0, pal.text, 0.0);
    } else {
        icons::paint(ui.painter(), egui::Rect::from_center_size(rect.center(), vec2(15.0, 15.0)), icon, pal.text);
    }
    describe(&response, label, true);
    response.on_hover_text(label)
}

/// macOS-style segmented control. Returns true when the selection changed.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, pal: &Palette, value: &mut T, options: &[(T, &str)]) -> bool {
    let font = FontId::new(13.0, FontFamily::Proportional);
    let galleys: Vec<_> = options.iter().map(|(_, label)| ui.painter().layout_no_wrap((*label).to_owned(), font.clone(), pal.text)).collect();
    let seg_w = galleys.iter().map(|g| g.size().x).fold(0.0_f32, f32::max) + 26.0;
    let total = vec2(seg_w * options.len() as f32 + 4.0, 30.0);
    let (rect, _) = ui.allocate_exact_size(total, Sense::hover());
    let track = CornerRadius::same(15);
    ui.painter().rect_filled(rect, track, pal.control);
    let mut changed = false;
    for (i, ((option, label), galley)) in options.iter().zip(galleys).enumerate() {
        let seg = egui::Rect::from_min_size(egui::pos2(rect.left() + 2.0 + seg_w * i as f32, rect.top() + 2.0), vec2(seg_w, 26.0));
        let id = ui.id().with(("segment", i));
        let response = ui.interact(seg, id, Sense::click());
        let selected = *value == *option;
        if selected {
            let fill = if pal.dark { pal.control_active } else { Color32::WHITE };
            ui.painter().rect_filled(seg, CornerRadius::same(13), fill);
            ui.painter().rect_stroke(seg, CornerRadius::same(13), Stroke::new(1.0_f32, pal.separator), StrokeKind::Inside);
        } else if response.hovered() {
            ui.painter().rect_filled(seg, CornerRadius::same(13), pal.control_hover.gamma_multiply(0.6));
        }
        if response.has_focus() {
            focus_ring(ui, pal, seg, CornerRadius::same(13));
        }
        let color = if selected { pal.text } else { pal.text2 };
        ui.painter().galley(egui::pos2(seg.center().x - galley.size().x / 2.0, seg.center().y - galley.size().y / 2.0), galley, color);
        response.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, selected, *label));
        if response.clicked() && !selected {
            *value = *option;
            changed = true;
        }
    }
    changed
}

/// A selectable sidebar row. `badge` is a short right-aligned count.
pub fn sidebar_item(ui: &mut Ui, pal: &Palette, icon: Icon, label: &str, badge: Option<&str>, selected: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
    let radius = CornerRadius::same(R_ROW);
    if selected {
        ui.painter().rect_filled(rect, radius, pal.selected_bg);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, radius, pal.control_hover.gamma_multiply(0.55));
    }
    if response.has_focus() {
        focus_ring(ui, pal, rect, radius);
    }
    let fg = if selected { pal.selected_fg } else { pal.text };
    let icon_tint = if selected { pal.selected_fg } else { pal.text2 };
    icons::paint(ui.painter(), egui::Rect::from_center_size(egui::pos2(rect.left() + 18.0, rect.center().y), vec2(16.0, 16.0)), icon, icon_tint);
    let galley = ui.painter().layout_no_wrap(label.to_owned(), FontId::new(13.5, FontFamily::Proportional), fg);
    ui.painter().galley(egui::pos2(rect.left() + 36.0, rect.center().y - galley.size().y / 2.0), galley, fg);
    if let Some(badge) = badge {
        let g = ui.painter().layout_no_wrap(badge.to_owned(), FontId::new(12.0, semibold_family()), pal.on_accent);
        let w = (g.size().x + 12.0).max(20.0);
        let pill = egui::Rect::from_center_size(egui::pos2(rect.right() - 10.0 - w / 2.0, rect.center().y), vec2(w, 18.0));
        ui.painter().rect_filled(pill, CornerRadius::same(9), pal.accent_fill);
        ui.painter().galley(pill.center() - g.size() / 2.0, g, pal.on_accent);
    }
    describe(&response, label, true);
    response
}

/// A card for grouped rows: one step less round than a tile.
pub fn card<R>(ui: &mut Ui, pal: &Palette, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .fill(pal.tile)
        .stroke(if pal.dark { Stroke::NONE } else { Stroke::new(1.0_f32, pal.separator) })
        .corner_radius(CornerRadius::same(R_CARD))
        .inner_margin(Margin::same(S4 as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// Section label above a group.
pub fn section(ui: &mut Ui, pal: &Palette, text: &str) {
    ui.add_space(S2);
    ui.label(overline(pal, text));
    ui.add_space(S1);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lin(c: u8) -> f64 {
        let v = f64::from(c) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }
    fn lum(c: Color32) -> f64 {
        0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
    }
    pub fn ratio(a: Color32, b: Color32) -> f64 {
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn text_and_red_accent_meet_4_5_to_1_on_every_surface_in_both_themes() {
        for dark in [false, true] {
            let p = base_palette(dark);
            for (bg_name, bg) in [("content", p.content), ("sidebar", p.sidebar), ("tile", p.tile), ("control", p.control), ("control_hover", p.control_hover)] {
                for (fg_name, fg) in [("text", p.text), ("text2", p.text2), ("text3", p.text3), ("accent", p.accent)] {
                    let r = ratio(fg, bg);
                    assert!(r >= 4.5, "dark={dark} {fg_name} on {bg_name}: {r:.2}");
                }
            }
            for (fg_name, fg) in [("ok", p.ok), ("warn", p.warn), ("err", p.err)] {
                for bg in [p.content, p.tile, p.control] {
                    assert!(ratio(fg, bg) >= 4.5, "dark={dark} {fg_name}");
                }
            }
            assert!(ratio(p.on_accent, p.accent_fill) >= 4.5);
            assert!(ratio(p.selected_fg, p.selected_bg) >= 4.5);
        }
    }

    #[test]
    fn focus_ring_is_visible_and_never_the_brand_red() {
        for dark in [false, true] {
            let p = base_palette(dark);
            for bg in [p.toolbar, p.sidebar, p.content, p.surface, p.tile, p.control] {
                assert!(ratio(focus_color(&p), bg) >= 3.0);
            }
            assert_ne!(focus_color(&p), p.accent_fill);
        }
    }

    #[test]
    fn high_contrast_makes_secondary_text_and_edges_strong() {
        for dark in [false, true] {
            let p = high_contrast_of(base_palette(dark));
            for bg in [p.content, p.sidebar, p.tile, p.control] {
                assert!(ratio(p.text2, bg) >= 7.0 && ratio(p.text3, bg) >= 7.0);
                assert!(ratio(p.separator, bg) >= 3.0);
            }
        }
    }

    #[test]
    fn embedded_fonts_cover_latin_greek_cyrillic_and_monospace() {
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        let _ = ctx.run(Default::default(), |ctx| {
            ctx.fonts(|f| {
                for family in [FontFamily::Proportional, FontFamily::Monospace] {
                    for c in ['a', 'É', 'ž', 'Ж', 'ω', '€', '—'] {
                        assert!(f.has_glyph(&FontId::new(14.0, family.clone()), c), "{c} in {family:?}");
                    }
                }
            });
        });
    }
}
