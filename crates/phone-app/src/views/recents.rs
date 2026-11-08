// SPDX-License-Identifier: GPL-3.0-or-later
//! Recents: the call history, newest first, with a button to call back and one to text.

use super::Cx;
use crate::model::format::{duration_short, list_time};
use crate::model::list::Offsets;
use crate::ui::icons::{self, Icon};
use crate::ui::theme::{self, R_ROW, S3, S4};
use crate::ui::vlist::{self, VListState};
use crate::ui::widgets;
use eframe::egui::{self, Align, CornerRadius, FontFamily, FontId, Layout, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, vec2};
use phone_core::recents::{Kind, Recent};

const ROW_H: f32 = 62.0;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Filter {
    #[default]
    All,
    Missed,
}

#[derive(Default)]
pub struct RecentsState {
    filter: Filter,
    /// The one selected call. Never a per-row flag.
    selected: Option<String>,
    list: VListState,
}

fn key_of(r: &Recent) -> String {
    format!("{}|{}", r.at, r.number)
}

/// What a call is called in the list.
pub fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Missed => "Missed",
        Kind::Incoming => "Incoming",
        Kind::Outgoing => "Outgoing",
    }
}

/// The second line: "Incoming · 2:15", or just "Missed" (a missed call has no length).
pub fn subline(r: &Recent) -> String {
    if r.seconds > 0 && r.kind != Kind::Missed {
        format!("{} · {}", kind_label(r.kind), duration_short(r.seconds))
    } else {
        kind_label(r.kind).to_string()
    }
}

impl RecentsState {
    pub fn escape(&mut self) -> bool {
        self.selected.take().is_some()
    }

    pub fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let pal = cx.pal;
        ui.add_space(S3);
        ui.horizontal(|ui| {
            ui.add_space(S4);
            theme::segmented(ui, &pal, &mut self.filter, &[(Filter::All, "All"), (Filter::Missed, "Missed")]);
        });
        ui.add_space(S3 - 4.0);

        let rows: Vec<&Recent> = cx.data.recents.iter().filter(|r| self.filter == Filter::All || r.kind == Kind::Missed).collect();
        if rows.is_empty() {
            let (title, detail) = match (self.filter, cx.live.phone.is_some()) {
                (Filter::Missed, _) => ("NO MISSED CALLS", "Calls you miss will show up here."),
                (_, false) => ("NO PHONE", "Pair a phone in your system's Bluetooth settings."),
                _ => ("NO RECENT CALLS", "Calls you make and receive will show up here."),
            };
            widgets::empty_state(ui, &pal, title, detail, |_| {});
            return;
        }
        let offsets = Offsets::new(rows.iter().map(|_| ROW_H));
        let mut clicked = None;
        let mut act: Option<(bool, String)> = None;
        let selected = self.selected.clone();
        vlist::show(ui, "recents-list", &mut self.list, &offsets, |ui, row| {
            let r = rows[row.index];
            let open = selected.as_deref() == Some(key_of(r).as_str());
            let card = Rect::from_min_max(row.rect.min + vec2(S3, 1.0), row.rect.max - vec2(S3, 1.0));
            let radius = CornerRadius::same(R_ROW);
            if open {
                ui.painter().rect_filled(card, radius, pal.tile);
                ui.painter().rect_stroke(card, radius, Stroke::new(1.0_f32, pal.separator), StrokeKind::Inside);
            } else if row.hovered {
                ui.painter().rect_filled(card, radius, pal.tile_hover);
            }
            let response = ui.interact(card, ui.id().with(("recent", key_of(r))), Sense::click());
            if response.has_focus() {
                theme::focus_ring(ui, &pal, card, radius);
            }
            if response.clicked() {
                clicked = Some(key_of(r));
            }
            let name = cx.data.display(&r.number);
            let missed = r.kind == Kind::Missed;
            let title_color = if missed { pal.accent } else { pal.text };
            widgets::avatar(ui.painter(), egui::pos2(card.left() + 26.0, card.center().y), 19.0, &r.number, &name);

            let time = list_time(r.at, cx.now, cx.off);
            let tg = ui.painter().layout_no_wrap(time.clone(), FontId::new(12.5, FontFamily::Proportional), pal.text2);
            let buttons_w = 30.0 * 2.0 + 8.0 + 12.0;
            let time_x = card.right() - 12.0 - buttons_w - tg.size().x;
            ui.painter().galley(egui::pos2(time_x, card.center().y - tg.size().y / 2.0), tg, pal.text2);

            let text_x = card.left() + 56.0;
            let max_w = (time_x - text_x - 12.0).max(40.0);
            let ng = widgets::ellipsized(ui, &name, FontId::new(14.5, theme::semibold_family()), title_color, max_w);
            let glyph_w = 18.0;
            let sg = widgets::ellipsized(ui, &subline(r), FontId::new(12.5, FontFamily::Proportional), if missed { pal.accent } else { pal.text2 }, max_w - glyph_w);
            let total = ng.size().y + sg.size().y + 2.0;
            let top = card.center().y - total / 2.0;
            let nh = ng.size().y;
            ui.painter().galley(egui::pos2(text_x, top), ng, title_color);
            let sub_color = if missed { pal.accent } else { pal.text2 };
            let icon = if r.kind == Kind::Outgoing { Icon::ArrowUpRight } else { Icon::ArrowDownLeft };
            icons::paint(ui.painter(), Rect::from_center_size(egui::pos2(text_x + 6.0, top + nh + 2.0 + sg.size().y / 2.0), vec2(11.0, 11.0)), icon, sub_color);
            ui.painter().galley(egui::pos2(text_x + glyph_w, top + nh + 2.0), sg, sub_color);

            let group = Rect::from_min_max(egui::pos2(card.right() - 12.0 - buttons_w + 12.0, card.top()), egui::pos2(card.right() - 12.0, card.bottom()));
            ui.scope_builder(UiBuilder::new().max_rect(group).layout(Layout::right_to_left(Align::Center)), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if theme::icon_button(ui, &pal, Icon::Chat, &format!("Message {name}")).clicked() {
                    act = Some((false, r.number.clone()));
                }
                let can = cx.live.phone_connected() && !cx.live.in_call();
                ui.add_enabled_ui(can, |ui| {
                    if theme::icon_button(ui, &pal, Icon::Phone, &format!("Call {name}")).clicked() {
                        act = Some((true, r.number.clone()));
                    }
                });
            });
        });
        if let Some(k) = clicked {
            self.selected = if self.selected.as_deref() == Some(k.as_str()) { None } else { Some(k) };
        }
        match act {
            Some((true, n)) => cx.dial(&n),
            Some((false, n)) => cx.new_message_to(&n),
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(kind: Kind, seconds: u64) -> Recent {
        Recent { number: "+1".into(), kind, at: 100, seconds }
    }

    #[test]
    fn sublines_show_length_only_when_there_was_a_call() {
        assert_eq!(subline(&r(Kind::Incoming, 135)), "Incoming · 2:15");
        assert_eq!(subline(&r(Kind::Outgoing, 8)), "Outgoing · 0:08");
        assert_eq!(subline(&r(Kind::Outgoing, 0)), "Outgoing");
        assert_eq!(subline(&r(Kind::Missed, 0)), "Missed");
        assert_eq!(subline(&r(Kind::Missed, 30)), "Missed");
    }

    #[test]
    fn calls_at_the_same_time_to_different_numbers_have_different_keys() {
        let a = Recent { number: "1".into(), kind: Kind::Missed, at: 5, seconds: 0 };
        let b = Recent { number: "2".into(), kind: Kind::Missed, at: 5, seconds: 0 };
        assert_ne!(key_of(&a), key_of(&b));
    }

    #[test]
    fn escape_clears_the_selection() {
        let mut s = RecentsState { selected: Some("k".into()), ..Default::default() };
        assert!(s.escape());
        assert!(!s.escape());
    }
}
