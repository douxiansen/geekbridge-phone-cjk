// SPDX-License-Identifier: GPL-3.0-or-later
//! Contacts: search, an alphabetical list with section letters, and a row that opens to show
//! each number with Call and Message buttons.

use super::Cx;
use crate::model::keypad::can_dial;
use crate::model::list::{section_letter, Offsets};
use crate::ui::icons::Icon;
use crate::ui::theme::{self, ButtonKind, PillSize, R_ROW, S3, S4};
use crate::ui::vlist::{self, RowCtx, VListState};
use crate::ui::widgets;
use eframe::egui::{self, Align, CornerRadius, FontFamily, FontId, Id, Key, Layout, Modifiers, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, vec2};
use phone_core::contacts::Contact;
use phone_core::service::{Command, LinkStatus};

const HEADER_H: f32 = 30.0;
const ROW_H: f32 = 58.0;
const NUMBER_H: f32 = 44.0;
const SEARCH_ID: &str = "contacts-search";

/// One line of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Item {
    Header(char),
    Contact(usize),
}

/// Identifies a contact across frames and searches.
fn key_of(c: &Contact) -> String {
    format!("{}|{}", c.name, c.numbers.first().map_or("", String::as_str))
}

#[derive(Default)]
pub struct ContactsState {
    query: String,
    /// The one open contact. Never a per-row flag.
    selected: Option<String>,
    filtered: Vec<Contact>,
    filtered_for: Option<(String, u64)>,
    list: VListState,
}

impl ContactsState {
    /// Puts the cursor in the search box (Ctrl/Cmd+F).
    pub fn focus_search(&self, ctx: &egui::Context) {
        ctx.memory_mut(|m| m.request_focus(Id::new(SEARCH_ID)));
    }

    /// Opens a contact's row, as if it had been clicked.
    pub fn open(&mut self, name: &str, first_number: &str) {
        self.selected = Some(format!("{name}|{first_number}"));
    }

    /// Escape: close the open contact, then clear the search. Returns true when it did something.
    pub fn escape(&mut self) -> bool {
        if self.selected.take().is_some() {
            return true;
        }
        if !self.query.is_empty() {
            self.query.clear();
            return true;
        }
        false
    }

    fn refilter(&mut self, cx: &Cx) {
        let stamp = (self.query.clone(), cx.data.generations.contacts);
        if self.filtered_for.as_ref() == Some(&stamp) {
            return;
        }
        self.filtered = cx.data.book.search(&self.query).into_iter().cloned().collect();
        self.filtered_for = Some(stamp);
        if let Some(sel) = &self.selected {
            if !self.filtered.iter().any(|c| &key_of(c) == sel) {
                self.selected = None;
            }
        }
    }

    fn items(&self) -> Vec<Item> {
        let mut out = Vec::with_capacity(self.filtered.len() + 27);
        let mut last = None;
        for (i, c) in self.filtered.iter().enumerate() {
            let letter = section_letter(&c.name);
            if last != Some(letter) {
                out.push(Item::Header(letter));
                last = Some(letter);
            }
            out.push(Item::Contact(i));
        }
        out
    }

    fn heights(&self, items: &[Item]) -> Vec<f32> {
        items
            .iter()
            .map(|item| match item {
                Item::Header(_) => HEADER_H,
                Item::Contact(i) => {
                    let c = &self.filtered[*i];
                    if self.selected.as_deref() == Some(key_of(c).as_str()) {
                        ROW_H + NUMBER_H * c.numbers.len() as f32 + 6.0
                    } else {
                        ROW_H
                    }
                }
            })
            .collect()
    }

    pub fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let pal = cx.pal;
        self.refilter(cx);
        ui.add_space(S3);
        ui.horizontal(|ui| {
            ui.add_space(S4);
            let w = (ui.available_width() - S4).min(520.0);
            widgets::field(ui, &pal, &mut self.query, "Search name or number", Id::new(SEARCH_ID), w, Some(Icon::Search));
        });
        ui.add_space(S3 - 4.0);

        if self.filtered.is_empty() {
            self.empty(ui, cx);
            return;
        }
        let items = self.items();
        let offsets = Offsets::new(self.heights(&items));
        let mut clicked: Option<String> = None;
        let selected = self.selected.clone();
        let filtered = &self.filtered;
        let mut actions: Vec<(bool, String)> = Vec::new();
        vlist::show(ui, "contacts-list", &mut self.list, &offsets, |ui, row| match &items[row.index] {
            Item::Header(letter) => {
                let g = ui.painter().layout_no_wrap(letter.to_string(), FontId::new(13.0, theme::semibold_family()), pal.text3);
                ui.painter().galley(egui::pos2(row.rect.left() + S4 + 6.0, row.rect.bottom() - g.size().y - 4.0), g, pal.text3);
            }
            Item::Contact(i) => {
                let c = &filtered[*i];
                let is_selected = selected.as_deref() == Some(key_of(c).as_str());
                if contact_row(ui, cx, row, c, is_selected, &mut actions) {
                    clicked = Some(key_of(c));
                }
            }
        });
        if let Some(k) = clicked {
            self.selected = if self.selected.as_deref() == Some(k.as_str()) { None } else { Some(k) };
        }
        for (call, number) in actions {
            if call {
                cx.dial(&number);
            } else {
                cx.new_message_to(&number);
            }
        }
    }

    fn empty(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let pal = cx.pal;
        let live = cx.live;
        let reload = |ui: &mut Ui, cx: &Cx, label: &str| {
            if theme::pill_button(ui, &cx.pal, label, ButtonKind::Plain, cx.live.phone_connected()).clicked() {
                cx.handle.send(Command::ReloadContacts);
            }
        };
        if !self.query.trim().is_empty() {
            let q = self.query.trim().to_string();
            widgets::empty_state(ui, &pal, "NO MATCHES", &format!("Nothing matches “{q}”."), |ui| {
                if can_dial(&q) && theme::pill_button(ui, &pal, "Call this number", ButtonKind::Plain, true).clicked() {
                    cx.dial(&q);
                }
            });
            return;
        }
        match (&live.phone, &live.contacts_status) {
            (None, _) => widgets::empty_state(ui, &pal, "NO PHONE", "Pair a phone in your system's Bluetooth settings to see its contacts.", |_| {}),
            (Some(p), _) if !p.connected => widgets::empty_state(ui, &pal, "PHONE OFFLINE", &format!("Connect {} to load its contacts.", p.name), |_| {}),
            (_, LinkStatus::Failed(why)) => widgets::empty_state(ui, &pal, "COULD NOT LOAD", why, |ui| reload(ui, cx, "Try again")),
            (_, LinkStatus::Connecting) => widgets::empty_state(ui, &pal, "LOADING...", "Reading contacts from your phone.", |_| {}),
            (_, LinkStatus::Ready) => widgets::empty_state(ui, &pal, "NO CONTACTS", "Your phone has no contacts with a phone number.", |ui| reload(ui, cx, "Reload")),
            (_, _) if !live.settings.load_contacts_on_connect => {
                widgets::empty_state(ui, &pal, "NOT LOADED", "Contacts are loaded only when you ask.", |ui| reload(ui, cx, "Load contacts"))
            }
            _ => widgets::empty_state(ui, &pal, "LOADING...", "Waiting for your phone.", |_| {}),
        }
    }
}

/// Draws one contact. Returns true when the header was clicked; pushes (is_call, number) for
/// buttons pressed in the open part.
fn contact_row(ui: &mut Ui, cx: &Cx, row: RowCtx, c: &Contact, open: bool, actions: &mut Vec<(bool, String)>) -> bool {
    let pal = cx.pal;
    let card = Rect::from_min_max(row.rect.min + vec2(S3, 1.0), row.rect.max - vec2(S3, 1.0));
    let head = Rect::from_min_size(card.min, vec2(card.width(), ROW_H - 2.0));
    let radius = CornerRadius::same(R_ROW);
    // Only the open row and the one row under the pointer have a background.
    if open {
        ui.painter().rect_filled(card, radius, pal.tile);
        ui.painter().rect_stroke(card, radius, Stroke::new(1.0_f32, pal.separator), StrokeKind::Inside);
    } else if row.hovered {
        ui.painter().rect_filled(card, radius, pal.tile_hover);
    }
    let id = ui.id().with(("contact", key_of(c)));
    let response = ui.interact(head, id, Sense::click());
    if response.has_focus() {
        theme::focus_ring(ui, &pal, head, radius);
    }
    let number0 = c.numbers.first().map_or("", String::as_str);
    widgets::avatar(ui.painter(), egui::pos2(head.left() + 26.0, head.center().y), 19.0, &c.name, &c.name);
    let text_x = head.left() + 56.0;
    let max_w = (head.right() - text_x - 36.0).max(40.0);
    let name = widgets::ellipsized(ui, &c.name, FontId::new(14.5, theme::semibold_family()), pal.text, max_w);
    let sub_text = if c.numbers.len() > 1 { format!("{}  +{} more", number0, c.numbers.len() - 1) } else { number0.to_string() };
    let sub = widgets::ellipsized(ui, &sub_text, FontId::new(12.5, FontFamily::Proportional), pal.text2, max_w);
    let total = name.size().y + sub.size().y + 2.0;
    let top = head.center().y - total / 2.0;
    let name_h = name.size().y;
    ui.painter().galley(egui::pos2(text_x, top), name, pal.text);
    ui.painter().galley(egui::pos2(text_x, top + name_h + 2.0), sub, pal.text2);
    crate::ui::icons::paint(
        ui.painter(),
        Rect::from_center_size(egui::pos2(head.right() - 16.0, head.center().y), vec2(12.0, 12.0)),
        if open { Icon::ChevronUp } else { Icon::ChevronDown },
        pal.text3,
    );
    let label = format!("{}, {}", c.name, if open { "open" } else { "closed" });
    response.widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone()));
    let clicked = response.clicked();

    if open {
        for (k, number) in c.numbers.iter().enumerate() {
            let r = Rect::from_min_size(egui::pos2(card.left(), card.top() + ROW_H - 2.0 + NUMBER_H * k as f32), vec2(card.width(), NUMBER_H));
            let g = ui.painter().layout_no_wrap(number.clone(), FontId::new(14.0, FontFamily::Proportional), pal.text);
            ui.painter().galley(egui::pos2(text_x, r.center().y - g.size().y / 2.0), g, pal.text);
            ui.painter().hline(egui::Rangef::new(text_x, r.right() - 12.0), r.top(), Stroke::new(1.0_f32, pal.separator));
            ui.scope_builder(UiBuilder::new().max_rect(r.shrink2(vec2(12.0, 8.0))).layout(Layout::right_to_left(Align::Center)), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let can_call = cx.live.phone_connected() && !cx.live.in_call();
                if theme::pill(ui, &pal, "Call", ButtonKind::Inverse, can_call, PillSize::Small, 64.0).clicked() {
                    actions.push((true, number.clone()));
                }
                if theme::pill(ui, &pal, "Message", ButtonKind::Plain, true, PillSize::Small, 0.0).clicked() {
                    actions.push((false, number.clone()));
                }
            });
        }
    }
    let _ = (Key::A, Modifiers::NONE);
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(name: &str, n: &[&str]) -> Contact {
        Contact { name: name.into(), numbers: n.iter().map(|s| s.to_string()).collect() }
    }

    #[test]
    fn items_get_a_header_per_letter_run() {
        let st = ContactsState { filtered: vec![c("Alí", &["1"]), c("Anna", &["2"]), c("Zoé", &["3"]), c("123 Pizza", &["4"])], ..Default::default() };
        let items = st.items();
        assert_eq!(items, vec![Item::Header('A'), Item::Contact(0), Item::Contact(1), Item::Header('Z'), Item::Contact(2), Item::Header('#'), Item::Contact(3)]);
    }

    #[test]
    fn the_open_contact_grows_by_its_numbers() {
        let mut st = ContactsState { filtered: vec![c("Mara", &["1", "2"]), c("Zoé", &["3"])], ..Default::default() };
        let items = st.items();
        let closed = st.heights(&items);
        st.selected = Some(key_of(&st.filtered[0].clone()));
        let open = st.heights(&items);
        assert_eq!(closed, vec![HEADER_H, ROW_H, HEADER_H, ROW_H]);
        assert_eq!(open[1], ROW_H + 2.0 * NUMBER_H + 6.0);
        assert_eq!(open[3], ROW_H);
    }

    #[test]
    fn escape_closes_then_clears() {
        let mut st = ContactsState { query: "al".into(), selected: Some("x".into()), ..Default::default() };
        assert!(st.escape());
        assert!(st.selected.is_none());
        assert_eq!(st.query, "al");
        assert!(st.escape());
        assert!(st.query.is_empty());
        assert!(!st.escape());
    }

    #[test]
    fn contacts_with_the_same_name_have_distinct_keys() {
        assert_ne!(key_of(&c("Sam", &["1"])), key_of(&c("Sam", &["2"])));
    }
}
