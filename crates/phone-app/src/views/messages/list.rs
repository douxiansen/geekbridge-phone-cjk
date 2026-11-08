// SPDX-License-Identifier: GPL-3.0-or-later
//! The conversation list.

use super::{MessagesState, Selection};
use crate::model::format::list_time;
use crate::model::list::Offsets;
use crate::model::thread::preview;
use crate::state::UiState;
use crate::ui::theme::{self, ButtonKind, R_ROW, S3};
use crate::ui::vlist;
use crate::ui::widgets;
use crate::views::Cx;
use eframe::egui::{self, vec2, Color32, CornerRadius, FontFamily, FontId, Rect, Sense, Ui};

const ROW_H: f32 = 70.0;

impl MessagesState {
    pub(super) fn show_list(&mut self, ui: &mut Ui, cx: &mut Cx, ui_state: &UiState) {
        let pal = cx.pal;
        let convs = &cx.data.conversations;
        if convs.is_empty() {
            let mut start = false;
            widgets::empty_state(
                ui,
                &pal,
                "NO MESSAGES",
                "Texts you send and receive will show up here.",
                |ui| {
                    start = theme::pill_button(ui, &pal, "New message", ButtonKind::Inverse, true)
                        .clicked();
                },
            );
            if start {
                self.new_message(None);
            }
            return;
        }
        ui.add_space(S3 - 4.0);
        let offsets = Offsets::new(convs.iter().map(|_| ROW_H));
        let selected = self.selected_key().map(str::to_string);
        let mut clicked: Option<Selection> = None;
        vlist::show(ui, "conversations", &mut self.list, &offsets, |ui, row| {
            let conv = &convs[row.index];
            let is_selected = selected.as_deref() == Some(conv.key.as_str());
            let card = Rect::from_min_max(
                row.rect.min + vec2(S3 - 2.0, 1.0),
                row.rect.max - vec2(S3 - 2.0, 1.0),
            );
            let radius = CornerRadius::same(R_ROW);
            if is_selected {
                ui.painter().rect_filled(card, radius, pal.selected_bg);
            } else if row.hovered {
                ui.painter().rect_filled(card, radius, pal.tile_hover);
            }
            let response = ui.interact(
                card,
                ui.id().with(("conversation", &conv.key)),
                Sense::click(),
            );
            if response.has_focus() {
                theme::focus_ring(ui, &pal, card, radius);
            }
            if response.clicked() {
                clicked = Some(Selection {
                    key: conv.key.clone(),
                    number: conv.number.clone(),
                });
            }
            let last = conv.last();
            let unread = ui_state.unread_in(conv);
            let name = cx.data.display(&conv.number);
            let (fg, fg2) = if is_selected {
                (pal.selected_fg, pal.selected_fg.gamma_multiply(0.72))
            } else {
                (pal.text, pal.text2)
            };

            widgets::avatar(
                ui.painter(),
                egui::pos2(card.left() + 30.0, card.center().y),
                22.0,
                &cx.data.avatar_key(&conv.number),
                &name,
            );

            let time = list_time(last.at, cx.now, cx.off);
            let tg = ui.painter().layout_no_wrap(
                time,
                FontId::new(12.0, FontFamily::Proportional),
                if unread > 0 && !is_selected {
                    pal.text
                } else {
                    fg2
                },
            );
            let right = card.right() - 12.0;
            let text_x = card.left() + 64.0;
            let name_w = (right - tg.size().x - 10.0 - text_x).max(40.0);
            let ng = widgets::ellipsized(
                ui,
                &name,
                FontId::new(14.5, theme::semibold_family()),
                fg,
                name_w,
            );
            let badge_room = if unread > 0 { 30.0 } else { 0.0 };
            let pg = widgets::ellipsized(
                ui,
                &preview(
                    &crate::model::text::display_text(&last.text),
                    !last.incoming,
                ),
                FontId::new(13.0, FontFamily::Proportional),
                if unread > 0 && !is_selected {
                    pal.text
                } else {
                    fg2
                },
                (right - text_x - badge_room).max(40.0),
            );
            let top = card.center().y - (ng.size().y + pg.size().y + 3.0) / 2.0;
            let nh = ng.size().y;
            let tcolor = if unread > 0 && !is_selected {
                pal.text
            } else {
                fg2
            };
            ui.painter()
                .galley(egui::pos2(right - tg.size().x, top + 1.0), tg, tcolor);
            ui.painter().galley(egui::pos2(text_x, top), ng, fg);
            ui.painter()
                .galley(egui::pos2(text_x, top + nh + 3.0), pg, fg2);
            if unread > 0 {
                let label = if unread > 99 {
                    "99+".to_string()
                } else {
                    unread.to_string()
                };
                let g = ui.painter().layout_no_wrap(
                    label,
                    FontId::new(12.0, theme::semibold_family()),
                    Color32::WHITE,
                );
                let w = (g.size().x + 12.0).max(20.0);
                let pill = Rect::from_center_size(
                    egui::pos2(right - w / 2.0, top + nh + 3.0 + 9.0),
                    vec2(w, 18.0),
                );
                ui.painter()
                    .rect_filled(pill, CornerRadius::same(9), pal.accent_fill);
                ui.painter()
                    .galley(pill.center() - g.size() / 2.0, g, Color32::WHITE);
            }
            let spoken = if unread > 0 {
                format!("{name}, {unread} unread: {}", last.text)
            } else {
                format!("{name}: {}", last.text)
            };
            response.widget_info(move || {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, spoken.clone())
            });
        });
        if let Some(sel) = clicked {
            self.new_to = None;
            self.selected = Some(sel);
            self.focus_composer = true;
        }
    }
}
