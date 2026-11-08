// SPDX-License-Identifier: GPL-3.0-or-later
//! The open conversation: header, bubbles, composer, and the new-message form.

use super::outbox::{Delivery, Outgoing};
use super::{MessagesState, Selection};
use crate::model::compose::{self, Counter};
use crate::model::format::local_offset;
use crate::model::thread::{rows, Row, Slot};
use crate::state::UiState;
use crate::ui::icons::Icon;
use crate::ui::theme::{self, ButtonKind, PillSize, BUBBLE_BLUE, R_ROW, S2, S3, S4};
use crate::ui::widgets;
use crate::views::Cx;
use eframe::egui::{
    self, Align, Color32, CornerRadius, FontFamily, FontId, Frame, Id, Key, KeyboardShortcut, Layout, Margin, Modifiers, RichText, ScrollArea, Sense, Stroke, TextEdit, Ui, vec2,
};
use phone_core::conversations::Message;

const HEADER_H: f32 = 60.0;
const NEW_TO_ID: &str = "new-message-to";

/// One thing in the thread: a text from the phone's list, or one of ours still on its way.
enum Entry<'a> {
    Message(&'a Message),
    Local(&'a Outgoing),
}

impl Entry<'_> {
    fn slot(&self) -> Slot {
        match self {
            Entry::Message(m) => Slot { at: m.at, outgoing: !m.incoming },
            Entry::Local(o) => Slot { at: o.at, outgoing: true },
        }
    }
}

impl MessagesState {
    pub(super) fn show_detail(&mut self, ui: &mut Ui, cx: &mut Cx, ui_state: &mut UiState, narrow: bool) {
        if self.new_to.is_some() {
            self.new_message_form(ui, cx, narrow);
            return;
        }
        let Some(sel) = self.selected.clone() else {
            let pal = cx.pal;
            let mut start = false;
            let list_empty = cx.data.conversations.is_empty();
            widgets::empty_state(ui, &pal, "NO CHAT OPEN", "Choose a conversation, or start a new message.", |ui| {
                // The empty list beside this already offers the button.
                if !list_empty {
                    start = theme::pill_button(ui, &pal, "New message", ButtonKind::Inverse, true).clicked();
                }
            });
            if start {
                self.new_message(None);
            }
            return;
        };
        let mut back = false;
        // Header on top, composer at the bottom, the thread filling what is left.
        egui::TopBottomPanel::bottom(Id::new("thread-composer")).frame(Frame::new()).show_separator_line(false).resizable(false).show_inside(ui, |ui| {
            self.composer(ui, cx, &sel);
        });
        back = self.header(ui, cx, &sel, narrow);
        egui::CentralPanel::default().frame(Frame::new()).show_inside(ui, |ui| {
            self.body(ui, cx, ui_state, &sel);
        });
        if back {
            self.selected = None;
        }
    }

    /// Name, number and a call button. Returns true when "back" was pressed.
    fn header(&mut self, ui: &mut Ui, cx: &mut Cx, sel: &Selection, narrow: bool) -> bool {
        let pal = cx.pal;
        let mut back = false;
        let name = cx.data.display(&sel.number);
        ui.allocate_ui_with_layout(vec2(ui.available_width(), HEADER_H), Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = S3;
            ui.add_space(S3);
            if narrow && theme::tool_button(ui, &pal, Icon::ChevronLeft, Some("Messages"), true, "Back to conversations").clicked() {
                back = true;
            }
            let (r, _) = ui.allocate_exact_size(vec2(38.0, 38.0), Sense::hover());
            widgets::avatar(ui.painter(), r.center(), 19.0, &sel.key, &name);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                let room = (ui.available_width() - 70.0).max(80.0);
                let g = widgets::ellipsized(ui, &name, FontId::new(15.5, theme::semibold_family()), pal.text, room);
                let (rr, _) = ui.allocate_exact_size(g.size(), Sense::hover());
                ui.painter().galley(rr.min, g, pal.text);
                if name != sel.number {
                    let g = widgets::ellipsized(ui, &sel.number, FontId::new(12.5, FontFamily::Proportional), pal.text2, room);
                    let (rr, _) = ui.allocate_exact_size(g.size(), Sense::hover());
                    ui.painter().galley(rr.min, g, pal.text2);
                }
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(S3);
                let can = cx.live.phone_connected() && !cx.live.in_call();
                ui.add_enabled_ui(can, |ui| {
                    if theme::icon_button(ui, &pal, Icon::Phone, &format!("Call {name}")).clicked() {
                        let n = sel.number.clone();
                        cx.dial(&n);
                    }
                });
            });
        });
        theme::hairline(ui, &pal);
        back
    }

    fn body(&mut self, ui: &mut Ui, cx: &mut Cx, ui_state: &mut UiState, sel: &Selection) {
        let pal = cx.pal;
        let conv = cx.data.conversation(&sel.key);
        let mut entries: Vec<Entry> = Vec::new();
        if let Some(c) = conv {
            entries.extend(c.messages.iter().map(Entry::Message));
        }
        for o in self.outbox.entries(&sel.key) {
            // The service's own faded copy replaces ours as soon as the phone accepted the text.
            let has_pending_copy = conv.is_some_and(|c| c.messages.iter().any(|m| m.pending && m.text == o.text));
            if !(matches!(o.delivery, Delivery::Sending) && has_pending_copy) {
                entries.push(Entry::Local(o));
            }
        }
        entries.sort_by_key(|e| e.slot().at);

        // Reading it here marks it read here.
        if ui.input(|i| i.focused) {
            if let Some(last) = conv.and_then(|c| c.messages.last()) {
                ui_state.mark_read(&sel.key, last.at);
            }
        }

        let slots: Vec<Slot> = entries.iter().map(Entry::slot).collect();
        let layout = rows(&slots, cx.now, local_offset(cx.now));
        let width = ui.available_width();
        let max_bubble = ((width - 2.0 * S4) * 0.74).clamp(180.0, 520.0);
        let mut retry: Option<u64> = None;
        let mut dismiss: Option<u64> = None;
        ScrollArea::vertical().id_salt(("thread", &sel.key)).stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if entries.is_empty() {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| {
                    ui.label(theme::caption(&pal, "No messages yet. Say hello."));
                });
            }
            ui.add_space(S2);
            Frame::new().inner_margin(Margin::symmetric(S4 as i8, 0)).show(ui, |ui| {
                ui.set_width(width - 2.0 * S4);
                for row in &layout {
                    match row {
                        Row::Stamp(text) => {
                            ui.add_space(12.0);
                            ui.vertical_centered(|ui| ui.label(theme::caption(&pal, text.as_str())));
                            ui.add_space(6.0);
                        }
                        Row::Bubble { index, first, last } => {
                            ui.add_space(if *first { 6.0 } else { 2.0 });
                            let (text, outgoing, faded, failed) = match &entries[*index] {
                                Entry::Message(m) => (m.text.as_str(), !m.incoming, m.pending, false),
                                Entry::Local(o) => (o.text.as_str(), true, matches!(o.delivery, Delivery::Sending), o.failure().is_some()),
                            };
                            bubble(ui, &pal, text, outgoing, faded, failed, *first, max_bubble);
                            if let Entry::Local(o) = &entries[*index] {
                                match &o.delivery {
                                    Delivery::Sending => {
                                        ui.with_layout(Layout::top_down(Align::Max), |ui| ui.label(theme::caption(&pal, "Sending…")));
                                    }
                                    Delivery::Failed(why) => {
                                        let (r, d) = failure_line(ui, &pal, why);
                                        if r {
                                            retry = Some(o.token);
                                        }
                                        if d {
                                            dismiss = Some(o.token);
                                        }
                                    }
                                }
                            }
                            let _ = last;
                        }
                    }
                }
            });
            ui.add_space(S3);
        });
        if let Some(token) = retry {
            if let Some(o) = self.outbox.remove(token) {
                self.send(cx, &o.key, &o.number, &o.text);
            }
        }
        if let Some(token) = dismiss {
            self.outbox.remove(token);
        }
    }

    fn send(&mut self, cx: &Cx, key: &str, number: &str, text: &str) {
        let token = cx.handle.send_text(number, text);
        self.outbox.add_with_token(token, key, number, text, cx.now);
    }

    fn composer(&mut self, ui: &mut Ui, cx: &mut Cx, sel: &Selection) {
        let pal = cx.pal;
        let id = Id::new(("composer", &sel.key));
        let mut text = self.drafts.remove(&sel.key).unwrap_or_default();
        let focused = ui.memory(|m| m.has_focus(id));
        let mut send = false;
        let can_send = compose::can_send(&text);
        Frame::new().fill(pal.content).inner_margin(Margin { left: S3 as i8, right: S3 as i8, top: 10, bottom: 12 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let counter = Counter::of(&text);
            if counter.visible() {
                ui.with_layout(Layout::top_down(Align::Max), |ui| {
                    let color = if counter.at_limit() { pal.accent } else if counter.emphasised() { pal.warn } else { pal.text3 };
                    ui.label(RichText::new(counter.label()).size(12.0).color(color));
                });
                ui.add_space(2.0);
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = S2;
                let send_d = 38.0;
                let field_w = ui.available_width() - send_d - S2;
                let stroke = if focused { Stroke::new(2.0_f32, theme::focus_color(&pal)) } else { Stroke::new(1.0_f32, if pal.dark { pal.control_active } else { pal.separator }) };
                Frame::new().fill(pal.surface).stroke(stroke).corner_radius(CornerRadius::same(19)).inner_margin(Margin::symmetric(14, 6)).show(ui, |ui| {
                    ui.set_width(field_w - 28.0);
                    ScrollArea::vertical().id_salt(("composer-scroll", &sel.key)).max_height(112.0).min_scrolled_height(20.0).auto_shrink([false, true]).show(ui, |ui| {
                        let edit = ui.add(
                            TextEdit::multiline(&mut text)
                                .id(id)
                                .frame(false)
                                .hint_text(theme::hint(&pal, "Message"))
                                .desired_rows(1)
                                .desired_width(f32::INFINITY)
                                .char_limit(compose::LIMIT)
                                .margin(Margin::symmetric(0, 4))
                                // Shift+Enter is the newline; a plain Enter is ours, to send.
                                .return_key(Some(KeyboardShortcut::new(Modifiers::SHIFT, Key::Enter))),
                        );
                        if edit.has_focus() && ui.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.shift) {
                            send = true;
                        }
                    });
                });
                if theme::round_button(ui, &pal, send_d, pal.selected_bg, pal.selected_fg, Icon::Send, can_send, "Send").clicked() {
                    send = true;
                }
            });
        });
        theme::hairline(ui, &pal);
        compose::clamp(&mut text);
        if self.focus_composer {
            ui.memory_mut(|m| m.request_focus(id));
            self.focus_composer = false;
        }
        if send && compose::can_send(&text) {
            let out = compose::outgoing_text(&text);
            text.clear();
            self.send(cx, &sel.key, &sel.number, &out);
            ui.memory_mut(|m| m.request_focus(id));
        }
        if !text.is_empty() {
            self.drafts.insert(sel.key.clone(), text);
        }
    }

    fn new_message_form(&mut self, ui: &mut Ui, cx: &mut Cx, narrow: bool) {
        let pal = cx.pal;
        let mut input = self.new_to.take().unwrap_or_default();
        let mut close = false;
        ui.allocate_ui_with_layout(vec2(ui.available_width(), HEADER_H), Layout::left_to_right(Align::Center), |ui| {
            ui.add_space(S4);
            if narrow && theme::tool_button(ui, &pal, Icon::ChevronLeft, Some("Messages"), true, "Back to conversations").clicked() {
                close = true;
            }
            ui.label(theme::semibold("New message", 16.0));
        });
        theme::hairline(ui, &pal);
        ui.add_space(S3);
        let mut chosen: Option<String> = None;
        let id = Id::new(NEW_TO_ID);
        ui.horizontal(|ui| {
            ui.add_space(S4);
            ui.label(theme::secondary(&pal, "To:"));
            let w = (ui.available_width() - S4).min(480.0);
            let response = widgets::field(ui, &pal, &mut input, "Name or phone number", id, w, Some(Icon::Person));
            if self.focus_to {
                response.request_focus();
                self.focus_to = false;
            }
            if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                chosen = suggestions(cx, &input).into_iter().next().map(|s| s.number);
            }
        });
        ui.add_space(S2);
        let list = suggestions(cx, &input);
        if !list.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(S4);
                ui.label(theme::overline(&pal, if input.trim().is_empty() { "Recent" } else { "Suggestions" }));
            });
            ui.add_space(4.0);
        }
        // One row is lit: the one the pointer is on, worked out from the rows' rectangles.
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let mut rects = Vec::new();
        for s in &list {
            let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width() - 2.0 * S3, 52.0), Sense::click());
            let rect = rect.translate(vec2(S3, 0.0));
            rects.push((rect, response, s));
        }
        let lit = pointer.and_then(|p| rects.iter().position(|(r, _, _)| r.contains(p)));
        for (i, (rect, response, s)) in rects.iter().enumerate() {
            let radius = CornerRadius::same(R_ROW);
            if lit == Some(i) {
                ui.painter().rect_filled(*rect, radius, pal.tile_hover);
            }
            if response.has_focus() {
                theme::focus_ring(ui, &pal, *rect, radius);
            }
            widgets::avatar(ui.painter(), egui::pos2(rect.left() + 28.0, rect.center().y), 17.0, &s.number, &s.title);
            let g1 = widgets::ellipsized(ui, &s.title, FontId::new(14.0, theme::semibold_family()), pal.text, rect.width() - 70.0);
            let g2 = widgets::ellipsized(ui, &s.detail, FontId::new(12.5, FontFamily::Proportional), pal.text2, rect.width() - 70.0);
            let top = rect.center().y - (g1.size().y + g2.size().y + 2.0) / 2.0;
            let h1 = g1.size().y;
            ui.painter().galley(egui::pos2(rect.left() + 56.0, top), g1, pal.text);
            ui.painter().galley(egui::pos2(rect.left() + 56.0, top + h1 + 2.0), g2, pal.text2);
            let label = format!("{}, {}", s.title, s.detail);
            response.widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone()));
            if response.clicked() {
                chosen = Some(s.number.clone());
            }
        }
        if list.is_empty() && !input.trim().is_empty() {
            ui.add_space(S4);
            ui.horizontal(|ui| {
                ui.add_space(S4);
                ui.label(theme::caption(&pal, "Type a name from your contacts, or a full phone number."));
            });
        }
        if let Some(number) = chosen {
            self.open_conversation(&number);
            return;
        }
        if close {
            return;
        }
        self.new_to = Some(input);
    }
}

/// A person or number offered under "To:".
struct Suggestion {
    title: String,
    detail: String,
    number: String,
}

/// Who to offer for what has been typed: the number itself when it is one, then matching
/// contacts; with nothing typed, the people texted most recently.
fn suggestions(cx: &Cx, input: &str) -> Vec<Suggestion> {
    let mut out = Vec::new();
    let query = input.trim();
    if query.is_empty() {
        for c in cx.data.conversations.iter().take(6) {
            out.push(Suggestion { title: cx.data.display(&c.number), detail: c.number.clone(), number: c.number.clone() });
        }
        return out;
    }
    if let Some(n) = compose::recipient_number(query) {
        let detail = match cx.data.name_for(&n) {
            Some(name) => format!("{name}"),
            None => "Send to this number".to_string(),
        };
        out.push(Suggestion { title: n.clone(), detail, number: n });
    }
    for c in cx.data.book.search(query).into_iter().take(6) {
        if let Some(number) = c.numbers.first() {
            if !out.iter().any(|s| phone_core::contacts::number_key(&s.number) == phone_core::contacts::number_key(number)) {
                out.push(Suggestion { title: c.name.clone(), detail: number.clone(), number: number.clone() });
            }
        }
    }
    out
}

/// One bubble. Outgoing ones are blue and on the right, incoming grey and on the left.
fn bubble(ui: &mut Ui, pal: &theme::Palette, text: &str, outgoing: bool, faded: bool, failed: bool, first: bool, max_width: f32) {
    let (mut fill, mut fg) = if outgoing { (BUBBLE_BLUE, Color32::WHITE) } else { (pal.control_hover, pal.text) };
    if faded {
        fill = fill.gamma_multiply(0.55);
        fg = fg.gamma_multiply(0.85);
    }
    let big = 18;
    let small = 5;
    let radius = if outgoing {
        CornerRadius { nw: big, sw: big, ne: if first { big } else { small }, se: small }
    } else {
        CornerRadius { ne: big, se: big, nw: if first { big } else { small }, sw: small }
    };
    let stroke = if failed { Stroke::new(1.5_f32, pal.accent_fill) } else { Stroke::NONE };
    let align = if outgoing { Align::Max } else { Align::Min };
    ui.with_layout(Layout::top_down(align), |ui| {
        Frame::new().fill(fill).stroke(stroke).corner_radius(radius).inner_margin(Margin::symmetric(13, 8)).show(ui, |ui| {
            ui.set_max_width(max_width);
            let shown = crate::model::text::display_text(text);
            ui.add(egui::Label::new(RichText::new(shown.as_ref()).size(14.5).color(fg)).selectable(true));
        });
    });
}

/// "Not delivered: why", with Retry and Delete. Returns (retry, delete) pressed.
fn failure_line(ui: &mut Ui, pal: &theme::Palette, why: &str) -> (bool, bool) {
    let (mut retry, mut delete) = (false, false);
    ui.add_space(4.0);
    ui.with_layout(Layout::top_down(Align::Max), |ui| {
        ui.add(egui::Label::new(RichText::new(format!("Not delivered: {why}")).size(12.0).color(pal.accent)).wrap());
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = S2;
            // Right-aligned by the parent's layout: draw in reading order.
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::pill(ui, pal, "Delete", ButtonKind::Plain, true, PillSize::Small, 0.0).clicked() {
                    delete = true;
                }
                if theme::pill(ui, pal, "Retry", ButtonKind::Inverse, true, PillSize::Small, 0.0).clicked() {
                    retry = true;
                }
            });
        });
    });
    (retry, delete)
}
