// SPDX-License-Identifier: GPL-3.0-or-later
//! Small controls the screens share: avatars, switches, text fields, banners, empty states.

use super::icons::{self, Icon};
use super::nothing;
use super::theme::{self, ButtonKind, Palette, PillSize, S2, S3, S4};
use crate::model::avatar;
use eframe::egui::{
    self, epaint::text::TextWrapMode, pos2, vec2, Align, Color32, CornerRadius, FontFamily, FontId,
    Frame, Layout, Margin, Painter, Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind,
    TextEdit, Ui, WidgetInfo, WidgetType,
};
use std::sync::Arc;

/// Avatar fills. Each takes white initials at 4.5:1 or better.
pub const AVATAR_FILLS: [Color32; avatar::COLOURS] = [
    Color32::from_rgb(0x4F, 0x62, 0x86),
    Color32::from_rgb(0x3F, 0x6F, 0x59),
    Color32::from_rgb(0x8A, 0x55, 0x3F),
    Color32::from_rgb(0x74, 0x55, 0x86),
    Color32::from_rgb(0x7A, 0x5F, 0x1C),
    Color32::from_rgb(0x28, 0x6D, 0x79),
    Color32::from_rgb(0x95, 0x45, 0x59),
    Color32::from_rgb(0x58, 0x58, 0x52),
];

/// Draws a contact's avatar: a coloured circle with initials, or a silhouette for a bare number.
/// `key` picks the colour (use the same string everywhere for the same person); `label` is what
/// the initials come from.
pub fn avatar(painter: &Painter, center: Pos2, radius: f32, key: &str, label: &str) {
    let fill = AVATAR_FILLS[avatar::colour_index(key)];
    painter.circle_filled(center, radius, fill);
    match avatar::initials(label) {
        Some(text) => {
            let size = (radius * 0.86).max(12.0);
            let g = painter.layout_no_wrap(
                text,
                FontId::new(size, theme::semibold_family()),
                Color32::WHITE,
            );
            painter.galley(center - g.size() / 2.0, g, Color32::WHITE);
        }
        None => icons::paint(
            painter,
            Rect::from_center_size(center, vec2(radius * 1.05, radius * 1.05)),
            Icon::Person,
            Color32::WHITE,
        ),
    }
}

/// Text on one line, cut with an ellipsis when it would pass `max_width`.
pub fn ellipsized(
    ui: &Ui,
    text: &str,
    font: FontId,
    color: Color32,
    max_width: f32,
) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap.max_width = max_width.max(8.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    ui.painter().layout_job(job)
}

/// macOS-style switch. Returns the response; `value` flips on click.
pub fn toggle(ui: &mut Ui, pal: &Palette, value: &mut bool, label: &str) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(vec2(44.0, 26.0), Sense::click());
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    let on = *value;
    let track = if on {
        pal.selected_bg
    } else {
        pal.control_active
    };
    let radius = CornerRadius::same(13);
    ui.painter().rect_filled(rect, radius, track);
    if !on {
        ui.painter().rect_stroke(
            rect,
            radius,
            Stroke::new(1.0_f32, pal.separator),
            StrokeKind::Inside,
        );
    }
    let knob_x = if on {
        rect.right() - 13.0
    } else {
        rect.left() + 13.0
    };
    let knob = if on {
        pal.selected_fg
    } else if pal.dark {
        pal.text
    } else {
        Color32::WHITE
    };
    ui.painter()
        .circle_filled(pos2(knob_x, rect.center().y), 9.5, knob);
    if response.has_focus() {
        theme::focus_ring(ui, pal, rect, radius);
    }
    let label = label.to_owned();
    response
        .widget_info(move || WidgetInfo::selected(WidgetType::Checkbox, true, on, label.clone()));
    response
}

/// A single-line text field in the app's style: rounded, outlined, with a leading icon and a
/// clear button once there is text. Returns the text edit's response.
pub fn field(
    ui: &mut Ui,
    pal: &Palette,
    text: &mut String,
    hint_text: &str,
    id: egui::Id,
    width: f32,
    icon: Option<Icon>,
) -> Response {
    let mut clear = false;
    let inner = Frame::new()
        .fill(pal.surface)
        .stroke(Stroke::new(
            1.0_f32,
            if pal.dark {
                pal.control_active
            } else {
                pal.separator
            },
        ))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.set_width(width - 20.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if let Some(icon) = icon {
                    let (r, _) = ui.allocate_exact_size(vec2(16.0, 20.0), Sense::hover());
                    icons::paint(
                        ui.painter(),
                        Rect::from_center_size(r.center(), vec2(14.0, 14.0)),
                        icon,
                        pal.text3,
                    );
                }
                let reserve = if text.is_empty() { 0.0 } else { 26.0 };
                let edit = ui.add(
                    TextEdit::singleline(text)
                        .id(id)
                        .frame(false)
                        .hint_text(theme::hint(pal, hint_text))
                        .desired_width((ui.available_width() - reserve).max(20.0))
                        .margin(Margin::symmetric(0, 4)),
                );
                if !text.is_empty() {
                    let (r, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
                    if resp.hovered() {
                        ui.painter()
                            .circle_filled(r.center(), 9.0, pal.control_hover);
                    }
                    icons::paint(
                        ui.painter(),
                        Rect::from_center_size(r.center(), vec2(11.0, 11.0)),
                        Icon::Close,
                        pal.text2,
                    );
                    theme::describe(&resp, "Clear", true);
                    clear = resp.clicked();
                }
                edit
            })
            .inner
        });
    let response = inner.inner;
    if clear {
        text.clear();
        response.request_focus();
    }
    if response.has_focus() {
        theme::focus_ring(ui, pal, inner.response.rect, CornerRadius::same(10));
    }
    response
}

/// How a banner is coloured.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// A problem to fix: red dot.
    Alert,
    /// Something to know: neutral.
    Info,
}

/// What was pressed on a banner.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BannerAction {
    Primary,
    Dismiss,
}

/// A full-width notice with an optional action button and a dismiss cross.
pub fn banner(
    ui: &mut Ui,
    pal: &Palette,
    tone: Tone,
    title: &str,
    detail: &str,
    action: Option<&str>,
    dismissable: bool,
) -> Option<BannerAction> {
    let mut out = None;
    Frame::new()
        .fill(pal.tile)
        .stroke(Stroke::new(
            1.0_f32,
            if tone == Tone::Alert {
                pal.accent_fill
            } else {
                pal.separator
            },
        ))
        .corner_radius(CornerRadius::same(theme::R_ROW))
        .inner_margin(Margin::symmetric(S3 as i8, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = S3;
                let (r, _) = ui.allocate_exact_size(vec2(16.0, 18.0), Sense::hover());
                let tint = if tone == Tone::Alert {
                    pal.accent
                } else {
                    pal.text2
                };
                icons::paint(
                    ui.painter(),
                    Rect::from_center_size(r.center(), vec2(16.0, 16.0)),
                    Icon::Warning,
                    tint,
                );
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let reserve =
                        34.0 + action.map_or(0.0, |_| 96.0) + if dismissable { 30.0 } else { 0.0 };
                    ui.set_max_width((ui.available_width() - reserve).max(120.0));
                    ui.label(theme::semibold(title, 13.5));
                    if !detail.is_empty() {
                        ui.add(egui::Label::new(theme::caption(pal, detail)).wrap());
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if dismissable {
                        let (r, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
                        if resp.hovered() {
                            ui.painter()
                                .circle_filled(r.center(), 11.0, pal.control_hover);
                        }
                        if resp.has_focus() {
                            theme::focus_circle(ui, pal, r.center(), 11.0);
                        }
                        icons::paint(
                            ui.painter(),
                            Rect::from_center_size(r.center(), vec2(12.0, 12.0)),
                            Icon::Close,
                            pal.text2,
                        );
                        theme::describe(&resp, "Dismiss", true);
                        if resp.clicked() {
                            out = Some(BannerAction::Dismiss);
                        }
                    }
                    if let Some(label) = action {
                        if theme::pill(
                            ui,
                            pal,
                            label,
                            ButtonKind::Plain,
                            true,
                            PillSize::Small,
                            0.0,
                        )
                        .clicked()
                        {
                            out = Some(BannerAction::Primary);
                        }
                    }
                });
            });
        });
    out
}

/// The centred "nothing here" block: a dot-matrix title, one line of explanation and whatever
/// `extra` adds (a button).
pub fn empty_state(
    ui: &mut Ui,
    pal: &Palette,
    title: &str,
    detail: &str,
    extra: impl FnOnce(&mut Ui),
) {
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() * 0.28).clamp(16.0, 140.0));
        let pitch = nothing::fit_pitch(
            title,
            nothing::PITCH_TITLE,
            2.0,
            (ui.available_width() - 32.0).max(80.0),
        );
        nothing::dot_text(ui, pal, title, pitch, pal.text2, true);
        ui.add_space(S2);
        ui.add(egui::Label::new(theme::secondary(pal, detail)).wrap());
        ui.add_space(S4);
        extra(ui);
    });
}

/// A row of a settings card: label and hint on the left, `control` on the right.
pub fn setting_row(
    ui: &mut Ui,
    pal: &Palette,
    label: &str,
    hint_text: &str,
    control: impl FnOnce(&mut Ui),
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        let controls_w = 0.0;
        ui.vertical(|ui| {
            ui.set_max_width((ui.available_width() - 150.0 - controls_w).max(160.0));
            ui.label(RichText::new(label).size(14.0));
            if !hint_text.is_empty() {
                ui.add(egui::Label::new(theme::caption(pal, hint_text)).wrap());
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), control);
    });
}

/// A minus/plus stepper for a bounded whole number. Returns true when it changed.
pub fn stepper(
    ui: &mut Ui,
    pal: &Palette,
    value: &mut u32,
    min: u32,
    max: u32,
    unit: &str,
) -> bool {
    let before = *value;
    // Right-to-left layout: draw the plus first.
    if theme::pill(
        ui,
        pal,
        "+",
        ButtonKind::Plain,
        *value < max,
        PillSize::Small,
        30.0,
    )
    .clicked()
    {
        *value = (*value + 1).min(max);
    }
    let text = format!("{} {}", *value, unit);
    let g = ui.painter().layout_no_wrap(
        text.clone(),
        FontId::new(13.5, FontFamily::Proportional),
        pal.text,
    );
    let (r, resp) = ui.allocate_exact_size(vec2(g.size().x.max(48.0), 26.0), Sense::hover());
    ui.painter()
        .galley(r.center() - g.size() / 2.0, g, pal.text);
    resp.widget_info(move || WidgetInfo::labeled(WidgetType::Label, true, text.clone()));
    if theme::pill(
        ui,
        pal,
        "−",
        ButtonKind::Plain,
        *value > min,
        PillSize::Small,
        30.0,
    )
    .clicked()
    {
        *value = value.saturating_sub(1).max(min);
    }
    *value != before
}

/// A selectable row with a radio dot, for "use this phone". Returns the response.
pub fn radio_row(ui: &mut Ui, pal: &Palette, selected: bool, title: &str, sub: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(theme::R_ROW), pal.tile_hover);
    }
    if response.has_focus() {
        theme::focus_ring(ui, pal, rect, CornerRadius::same(theme::R_ROW));
    }
    let c = pos2(rect.left() + 18.0, rect.center().y);
    ui.painter().circle_stroke(
        c,
        8.0,
        Stroke::new(1.5_f32, if selected { pal.text } else { pal.text3 }),
    );
    if selected {
        ui.painter().circle_filled(c, 4.5, pal.text);
    }
    let name = ellipsized(
        ui,
        title,
        FontId::new(14.0, FontFamily::Proportional),
        pal.text,
        rect.width() - 50.0,
    );
    let subg = ellipsized(
        ui,
        sub,
        FontId::new(12.0, FontFamily::Proportional),
        pal.text2,
        rect.width() - 50.0,
    );
    let total = name.size().y + subg.size().y + 2.0;
    let top = rect.center().y - total / 2.0;
    ui.painter()
        .galley(pos2(rect.left() + 38.0, top), name.clone(), pal.text);
    ui.painter().galley(
        pos2(rect.left() + 38.0, top + name.size().y + 2.0),
        subg,
        pal.text2,
    );
    let label = title.to_owned();
    response.widget_info(move || {
        WidgetInfo::selected(WidgetType::RadioButton, true, selected, label.clone())
    });
    response
}

/// One line of wrapped text that never grows past its parent, for notes and captions.
pub fn wrapped_caption(ui: &mut Ui, pal: &Palette, text: &str) {
    ui.add(egui::Label::new(theme::caption(pal, text)).wrap_mode(TextWrapMode::Wrap));
}

/// A rectangle sized for the card edge insets.
pub fn inset(rect: Rect, by: f32) -> Rect {
    rect.shrink(by)
}

/// Keeps the "Primary" ink pill visually the same height as a field.
pub const CONTROL_H: f32 = 32.0;

/// Light separator row inside a card.
pub fn card_divider(ui: &mut Ui, pal: &Palette) {
    ui.add_space(S3 - 4.0);
    theme::hairline(ui, pal);
    ui.add_space(S3 - 4.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::{palette, Palette};

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

    #[test]
    fn white_initials_are_legible_on_every_avatar_fill() {
        for (i, fill) in AVATAR_FILLS.iter().enumerate() {
            let ratio = 1.05 / (lum(*fill) + 0.05);
            assert!(ratio >= 4.5, "avatar fill {i} is only {ratio:.2}:1");
        }
    }

    #[test]
    fn avatars_stay_readable_against_both_themes() {
        // The circle must stand out from the surface it sits on, in light and dark.
        for dark in [false, true] {
            let p: Palette = palette(dark);
            for fill in AVATAR_FILLS {
                let (a, b) = (lum(fill) + 0.05, lum(p.content) + 0.05);
                let r = a.max(b) / a.min(b);
                assert!(r >= 1.8, "dark={dark}: {r:.2}");
            }
        }
    }
}
