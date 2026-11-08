// SPDX-License-Identifier: GPL-3.0-or-later
//! The window's frame: the sidebar with its phone chip, and the unified toolbar.

use crate::model::status::{Dot, PhoneChip};
use crate::model::tab::Tab;
use crate::ui::icons::Icon;
use crate::ui::nothing;
use crate::ui::theme::{self, Palette, R_CARD, S2, S3};
use crate::ui::widgets;
use eframe::egui::{
    self, vec2, Align, CornerRadius, Frame, Layout, Margin, Rect, Sense, Stroke, StrokeKind, Ui,
};

pub const SIDEBAR_WIDTH: f32 = 208.0;
pub const TOOLBAR_HEIGHT: f32 = 50.0;
/// The readout is dropped when the toolbar is narrower than this.
const LCD_MIN_TOOLBAR: f32 = 640.0;

fn icon_of(tab: Tab) -> Icon {
    match tab {
        Tab::Keypad => Icon::Keypad,
        Tab::Contacts => Icon::Person,
        Tab::Recents => Icon::Clock,
        Tab::Messages => Icon::Chat,
        Tab::Settings => Icon::Sliders,
    }
}

/// Draws the sidebar. Returns the tab the user picked, if they picked one.
pub fn sidebar(
    ctx: &egui::Context,
    pal: &Palette,
    current: Tab,
    unread: usize,
    chip: &PhoneChip,
) -> Option<Tab> {
    let mut picked = None;
    egui::SidePanel::left("sidebar")
        .exact_width(SIDEBAR_WIDTH)
        .resizable(false)
        .frame(
            Frame::new()
                .fill(pal.sidebar)
                .inner_margin(Margin::symmetric(S3 as i8, 0)),
        )
        .show(ctx, |ui| {
            egui::TopBottomPanel::bottom("phone-chip")
                .frame(Frame::new().inner_margin(Margin {
                    left: 0,
                    right: 0,
                    top: 0,
                    bottom: S3 as i8,
                }))
                .show_separator_line(false)
                .resizable(false)
                .show_inside(ui, |ui| {
                    phone_chip(ui, pal, chip);
                });
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    nothing::dot_text(ui, pal, "GEEKBRIDGE", nothing::PITCH_LABEL, pal.text, false);
                    ui.label(theme::semibold("Phone", 19.0));
                });
            });
            ui.add_space(18.0);
            for tab in Tab::ALL {
                if tab == Tab::Settings {
                    ui.add_space(10.0);
                }
                let badge = (tab == Tab::Messages && unread > 0).then(|| {
                    if unread > 99 {
                        "99+".to_string()
                    } else {
                        unread.to_string()
                    }
                });
                if theme::sidebar_item(
                    ui,
                    pal,
                    icon_of(tab),
                    tab.label(),
                    badge.as_deref(),
                    tab == current,
                )
                .on_hover_text(format!("{} (Ctrl+{})", tab.label(), tab.number()))
                .clicked()
                {
                    picked = Some(tab);
                }
            }
        });
    picked
}

fn phone_chip(ui: &mut Ui, pal: &Palette, chip: &PhoneChip) {
    let stroke = if pal.dark {
        Stroke::NONE
    } else {
        Stroke::new(1.0_f32, pal.separator)
    };
    let response = Frame::new()
        .fill(pal.tile)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(R_CARD))
        .inner_margin(Margin::symmetric(S3 as i8, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = vec2(S2, 4.0);
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(12.0, 16.0), Sense::hover());
                let (color, hollow) = match chip.dot {
                    Dot::Green => (pal.ok, false),
                    Dot::Red => (pal.accent_fill, false),
                    Dot::Hollow => (pal.text2, true),
                };
                theme::paint_dot(ui.painter(), rect.center(), color, hollow);
                let g = widgets::ellipsized(
                    ui,
                    &chip.name,
                    egui::FontId::new(13.5, theme::semibold_family()),
                    pal.text,
                    ui.available_width(),
                );
                let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
                ui.painter().galley(r.min, g, pal.text);
            });
            ui.horizontal(|ui| {
                ui.add_space(20.0);
                let battery = chip.battery.map(|b| format!("{b}%"));
                let room = ui.available_width()
                    - battery
                        .as_ref()
                        .map_or(0.0, |b| nothing::dot_size(b, nothing::PITCH_LABEL).x + 8.0);
                let g = widgets::ellipsized(
                    ui,
                    &chip.status,
                    egui::FontId::new(12.0, egui::FontFamily::Proportional),
                    pal.text2,
                    room.max(40.0),
                );
                let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
                ui.painter().galley(r.min, g, pal.text2);
                if let Some(b) = battery {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        nothing::dot_text(ui, pal, &b, nothing::PITCH_LABEL, pal.text, false);
                    });
                }
            });
        });
    let battery = chip
        .battery
        .map(|b| format!(", battery {b}%"))
        .unwrap_or_default();
    let label = format!("Phone {}, {}{battery}", chip.name, chip.status);
    response.response.widget_info(move || {
        egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label.clone())
    });
}

/// What the toolbar's buttons asked for.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ToolbarOut {
    pub new_message: bool,
    pub reload_contacts: bool,
}

/// Draws the toolbar: the tab's title, the dot-matrix status readout in the middle and the tab's
/// own tools on the right.
pub fn toolbar(
    ctx: &egui::Context,
    pal: &Palette,
    tab: Tab,
    lcd: &str,
    lcd_alert: bool,
    phone_connected: bool,
) -> ToolbarOut {
    let mut out = ToolbarOut::default();
    theme::toolbar(ctx, pal, TOOLBAR_HEIGHT, |ui| {
        let rect = ui.max_rect();
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.label(theme::semibold(tab.label(), 17.0));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (icon, label, enabled, tip) = match tab {
                    Tab::Messages => (Icon::Compose, "New message", true, "New message (Ctrl+N)"),
                    Tab::Contacts => (
                        Icon::Refresh,
                        "Reload",
                        phone_connected,
                        "Reload contacts from the phone",
                    ),
                    _ => return,
                };
                if theme::tool_button(ui, pal, icon, Some(label), enabled, tip).clicked() {
                    match tab {
                        Tab::Messages => out.new_message = true,
                        _ => out.reload_contacts = true,
                    }
                }
            });
        });
        if rect.width() >= LCD_MIN_TOOLBAR {
            readout(ui, pal, rect, lcd, lcd_alert);
        }
    });
    out
}

/// The recessed dot-matrix display in the middle of the toolbar.
fn readout(ui: &mut Ui, pal: &Palette, toolbar: Rect, text: &str, alert: bool) {
    let pitch = nothing::scaled(ui, 2.0);
    let size = nothing::dot_size(text, pitch);
    let rect = Rect::from_center_size(toolbar.center(), vec2(size.x + 36.0, 28.0));
    let radius = CornerRadius::same(8);
    ui.painter().rect_filled(rect, radius, pal.lcd_bg);
    ui.painter().rect_stroke(
        rect,
        radius,
        Stroke::new(1.0_f32, pal.separator),
        StrokeKind::Inside,
    );
    let origin = egui::pos2(
        rect.center().x - size.x / 2.0 + pitch * 0.4,
        rect.center().y - size.y / 2.0 + pitch * 0.4,
    );
    nothing::dot_text_at(
        ui,
        pal,
        origin,
        text,
        pitch,
        if alert { pal.accent_fill } else { pal.text },
        false,
    );
    let r = ui.interact(rect, ui.id().with("lcd"), Sense::hover());
    let spoken = text.to_lowercase();
    r.widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Label, true, spoken.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tab_has_an_icon_and_the_readout_threshold_fits_the_minimum_window() {
        for t in Tab::ALL {
            let _ = icon_of(t);
        }
    }

    /// The readout is dropped in the narrowest window and kept at the default size.
    const _: () = assert!(720.0 - SIDEBAR_WIDTH < LCD_MIN_TOOLBAR);
    const _: () = assert!(940.0 - SIDEBAR_WIDTH >= LCD_MIN_TOOLBAR);
}
