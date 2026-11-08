// SPDX-License-Identifier: GPL-3.0-or-later
//! The dial pad: a dot-matrix readout, twelve keys, Call, Text and Delete. During a call the
//! same keys send tones into the call instead.

use super::Cx;
use crate::model::keypad::{self, KEYS};
use crate::ui::icons::Icon;
use crate::ui::nothing;
use crate::ui::theme::{self, ButtonKind, PillSize, S2, S3};
use crate::ui::widgets;
use eframe::egui::{
    self, vec2, Align, Color32, CornerRadius, Event, FontFamily, FontId, Key, Layout, Rect,
    Response, Sense, Stroke, StrokeKind, Ui, UiBuilder, ViewportCommand,
};
use phone_core::service::Command;
use std::time::Duration;

/// How long a press on 0 must last to type `+`.
const LONG_PRESS_SECS: f64 = 0.5;
const KEY_GAP: f32 = 14.0;

/// A contact the typed digits point at.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Hint {
    name: String,
    number: String,
}

/// Suggestions for the digits typed so far, remembered until the digits or the phonebook change.
#[derive(Debug, Default)]
struct Suggestions {
    for_number: String,
    generation: u64,
    computed: bool,
    exact: Option<String>,
    similar: Vec<Hint>,
}

#[derive(Default)]
pub struct KeypadState {
    /// The number being typed.
    number: String,
    /// Tones sent during the current call, shown in the readout.
    tones: String,
    zero_down_since: Option<f64>,
    zero_fired: bool,
    suggestions: Suggestions,
}

impl KeypadState {
    /// Fills in a number (from a contact or a notification) and keeps what was typed otherwise.
    pub fn set_number(&mut self, number: &str) {
        self.number.clear();
        keypad::push_str(&mut self.number, number);
    }

    fn type_chars(&mut self, cx: &Cx, text: &str) {
        if cx.live.in_call() {
            for ch in text
                .chars()
                .filter(|c| c.is_ascii_digit() || matches!(c, '*' | '#'))
            {
                self.send_tone(cx, ch);
            }
        } else {
            keypad::push_str(&mut self.number, text);
        }
    }

    fn send_tone(&mut self, cx: &Cx, ch: char) {
        if cx.live.active_call().is_some() {
            cx.handle.send(Command::Tones(ch.to_string()));
            if self.tones.chars().count() < keypad::MAX_LEN {
                self.tones.push(ch);
            }
        }
    }

    fn dial(&mut self, cx: &mut Cx) {
        if keypad::can_dial(&self.number) {
            let number = self.number.clone();
            cx.dial(&number);
            self.number.clear();
        }
    }

    fn handle_keyboard(&mut self, ui: &Ui, cx: &mut Cx) {
        if ui.ctx().wants_keyboard_input() {
            return;
        }
        // Enter on a focused button activates that button; it must not also dial.
        let something_focused = ui.memory(|m| m.focused().is_some());
        let events = ui.input(|i| i.events.clone());
        for event in events {
            match event {
                Event::Text(t) => self.type_chars(cx, &t),
                Event::Paste(t) => self.type_chars(cx, &keypad::sanitise_paste(&t)),
                Event::Key {
                    key: Key::Backspace,
                    pressed: true,
                    ..
                } if !cx.live.in_call() => {
                    keypad::backspace(&mut self.number);
                }
                Event::Key {
                    key: Key::Enter,
                    pressed: true,
                    ..
                } if !cx.live.in_call() && !something_focused => self.dial(cx),
                _ => {}
            }
        }
    }

    fn refresh_suggestions(&mut self, cx: &Cx) {
        let s = &mut self.suggestions;
        if s.computed && s.for_number == self.number && s.generation == cx.data.generations.contacts
        {
            return;
        }
        s.computed = true;
        s.for_number = self.number.clone();
        s.generation = cx.data.generations.contacts;
        s.exact = cx
            .data
            .name_for(&self.number)
            .map(str::to_string)
            .filter(|_| keypad::can_dial(&self.number));
        s.similar.clear();
        if s.exact.is_none() && self.number.chars().filter(char::is_ascii_digit).count() >= 2 {
            s.similar = cx
                .data
                .book
                .search(&self.number)
                .into_iter()
                .take(3)
                .filter_map(|c| {
                    c.numbers.first().map(|n| Hint {
                        name: c.name.clone(),
                        number: n.clone(),
                    })
                })
                .collect();
        }
    }

    pub fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let in_call = cx.live.in_call();
        if !in_call {
            self.tones.clear();
        }
        self.handle_keyboard(ui, cx);
        self.refresh_suggestions(cx);

        let avail = ui.available_size();
        let col_w = avail.x.min(380.0);
        let readout_h = 62.0;
        let hint_h = 40.0;
        let d = ((avail.y - readout_h - hint_h - 24.0) / 5.0 - KEY_GAP).clamp(46.0, 76.0);
        let stack_h = readout_h + hint_h + 5.0 * d + 4.0 * KEY_GAP;
        let top_pad = ((avail.y - stack_h) / 2.0 - 8.0).max(4.0);

        let origin = ui.cursor().min + vec2((avail.x - col_w) / 2.0, top_pad);
        let area = Rect::from_min_size(origin, vec2(col_w, stack_h + 8.0));
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(area)
                .layout(Layout::top_down(Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing = vec2(KEY_GAP, KEY_GAP);
                self.readout(ui, cx, col_w, readout_h);
                self.hints(ui, cx, col_w, hint_h);
                let grid_w = 3.0 * d + 2.0 * KEY_GAP;
                for row in KEYS.chunks(3) {
                    ui.allocate_ui_with_layout(
                        vec2(grid_w, d),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            for key in row {
                                if self.key(ui, cx, d, key.ch, key.letters).clicked()
                                    && key.ch != '0'
                                {
                                    self.press(cx, key.ch);
                                }
                            }
                        },
                    );
                }
                self.actions(ui, cx, d, grid_w);
            },
        );
        // Keep the whole pane as this view's area so the parent does not collapse it.
        ui.allocate_rect(
            Rect::from_min_size(ui.cursor().min, vec2(avail.x, 0.0)),
            Sense::hover(),
        );
    }

    fn press(&mut self, cx: &Cx, ch: char) {
        if cx.live.in_call() {
            self.send_tone(cx, ch);
        } else {
            keypad::push_str(&mut self.number, &ch.to_string());
        }
    }

    fn readout(&mut self, ui: &mut Ui, cx: &Cx, width: f32, height: f32) {
        let pal = cx.pal;
        let in_call = cx.live.in_call();
        let shown = if in_call { &self.tones } else { &self.number };
        let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        let paste_w = 40.0;
        let text_area =
            Rect::from_min_max(rect.min + vec2(paste_w, 0.0), rect.max - vec2(paste_w, 0.0));
        if shown.is_empty() {
            let hint = if in_call {
                "Press keys to send tones"
            } else {
                "Enter a number"
            };
            let g = ui.painter().layout_no_wrap(
                hint.to_owned(),
                FontId::new(22.0, theme::semibold_family()),
                pal.text3,
            );
            ui.painter()
                .galley(text_area.center() - g.size() / 2.0, g, pal.text3);
        } else {
            let max_pitch = nothing::scaled(ui, nothing::PITCH_NUMERAL);
            let min_pitch = nothing::scaled(ui, 2.8);
            let fit_chars = nothing::chars_fitting(min_pitch, text_area.width());
            let text = keypad::readout(shown, fit_chars.max(4));
            let pitch = nothing::fit_pitch(&text, max_pitch, min_pitch, text_area.width());
            let size = nothing::dot_size(&text, pitch);
            let origin = egui::pos2(
                text_area.center().x - size.x / 2.0 + pitch * 0.4,
                text_area.center().y - size.y / 2.0 + pitch * 0.4,
            );
            nothing::dot_text_at(ui, &pal, origin, &text, pitch, pal.text, true);
            // One accessible name for the whole number.
            let r = ui.interact(text_area, ui.id().with("readout"), Sense::hover());
            let spoken = format!("Number {shown}");
            r.widget_info(move || {
                egui::WidgetInfo::labeled(egui::WidgetType::Label, true, spoken.clone())
            });
        }
        if !in_call {
            let paste_rect = Rect::from_center_size(
                egui::pos2(rect.right() - 16.0, rect.center().y),
                vec2(32.0, 32.0),
            );
            let r = ui.interact(paste_rect, ui.id().with("paste"), Sense::click());
            let fill = if r.hovered() {
                pal.control_hover
            } else {
                pal.control
            };
            ui.painter().circle_filled(paste_rect.center(), 15.0, fill);
            if r.has_focus() {
                theme::focus_circle(ui, &pal, paste_rect.center(), 15.0);
            }
            crate::ui::icons::paint(
                ui.painter(),
                Rect::from_center_size(paste_rect.center(), vec2(15.0, 15.0)),
                Icon::Clipboard,
                pal.text,
            );
            theme::describe(&r, "Paste a number", true);
            if r.on_hover_text("Paste a number (Ctrl+V)").clicked() {
                ui.ctx().send_viewport_cmd(ViewportCommand::RequestPaste);
            }
        }
    }

    fn hints(&mut self, ui: &mut Ui, cx: &Cx, width: f32, height: f32) {
        let pal = cx.pal;
        let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        let mut inner = ui.new_child(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::top_down(Align::Center)),
        );
        inner.spacing_mut().item_spacing = vec2(S2, 0.0);
        let s = &self.suggestions;
        if let Some(name) = &s.exact {
            inner.add_space(8.0);
            inner.label(theme::semibold(name, 15.0));
        } else if !s.similar.is_empty() {
            let mut picked = None;
            inner.add_space(6.0);
            inner.horizontal(|ui| {
                // Centre the chips: leave room on the left for what is not used.
                let widths: f32 = s
                    .similar
                    .iter()
                    .map(|h| (h.name.chars().count() as f32 * 7.0 + 28.0).min(150.0))
                    .sum();
                ui.add_space(((width - widths - S2 * s.similar.len() as f32) / 2.0).max(0.0));
                for hint in &s.similar {
                    let label = widgets::ellipsized(
                        ui,
                        &hint.name,
                        FontId::new(12.5, theme::semibold_family()),
                        pal.text,
                        120.0,
                    );
                    let text = label.text().to_string();
                    if theme::pill(
                        ui,
                        &pal,
                        &text,
                        ButtonKind::Plain,
                        true,
                        PillSize::Small,
                        0.0,
                    )
                    .on_hover_text(&hint.number)
                    .clicked()
                    {
                        picked = Some(hint.number.clone());
                    }
                }
            });
            if let Some(n) = picked {
                self.set_number(&n);
            }
        } else if self.number.is_empty() && !cx.live.in_call() {
            inner.add_space(10.0);
            inner.label(theme::caption(&pal, "Type, paste or tap a number"));
        }
    }

    fn key(&mut self, ui: &mut Ui, cx: &Cx, d: f32, ch: char, letters: &str) -> Response {
        let pal = cx.pal;
        let (rect, response) = ui.allocate_exact_size(vec2(d, d), Sense::click());
        let down = response.is_pointer_button_down_on();
        let fill = if down {
            pal.control_active
        } else if response.hovered() {
            if pal.dark {
                pal.control_hover
            } else {
                pal.tile_hover
            }
        } else if pal.dark {
            pal.control
        } else {
            pal.surface
        };
        ui.painter().circle_filled(rect.center(), d / 2.0, fill);
        if !pal.dark {
            ui.painter().circle_stroke(
                rect.center(),
                d / 2.0 - 0.5,
                Stroke::new(1.0_f32, pal.separator),
            );
        }
        if response.has_focus() {
            theme::focus_circle(ui, &pal, rect.center(), d / 2.0);
        }
        let digit_size = (d * 0.38).max(20.0);
        let g = ui.painter().layout_no_wrap(
            ch.to_string(),
            FontId::new(digit_size, theme::semibold_family()),
            pal.text,
        );
        let has_letters = !letters.is_empty() && ch != '0';
        let shift = if has_letters { digit_size * 0.18 } else { 0.0 };
        // '*' and '#' sit low in their glyph boxes: nudge them to the optical centre.
        let nudge = if ch == '*' { digit_size * 0.16 } else { 0.0 };
        ui.painter().galley(
            rect.center() - g.size() / 2.0 + vec2(0.0, nudge - shift),
            g,
            pal.text,
        );
        if has_letters {
            let l = ui.painter().layout_no_wrap(
                letters.to_owned(),
                FontId::new(12.0, FontFamily::Proportional),
                pal.text3,
            );
            ui.painter().galley(
                egui::pos2(
                    rect.center().x - l.size().x / 2.0,
                    rect.center().y + digit_size * 0.28,
                ),
                l,
                pal.text3,
            );
        } else if ch == '0' {
            let l = ui.painter().layout_no_wrap(
                "+".to_owned(),
                FontId::new(12.0, FontFamily::Proportional),
                pal.text3,
            );
            ui.painter().galley(
                egui::pos2(
                    rect.center().x - l.size().x / 2.0,
                    rect.center().y + digit_size * 0.28,
                ),
                l,
                pal.text3,
            );
        }
        let name = match ch {
            '*' => "star".to_string(),
            '#' => "pound".to_string(),
            c if !letters.is_empty() && c != '0' => format!("{c} {letters}"),
            c => c.to_string(),
        };
        theme::describe(&response, &name, true);
        if ch == '0' {
            self.long_press_zero(ui, cx, &response);
        }
        response
    }

    /// 0 types `0`; held down it types `+` instead.
    fn long_press_zero(&mut self, ui: &Ui, cx: &Cx, response: &Response) {
        let now = ui.input(|i| i.time);
        if response.is_pointer_button_down_on() {
            let since = *self.zero_down_since.get_or_insert(now);
            if !self.zero_fired && now - since >= LONG_PRESS_SECS {
                self.zero_fired = true;
                if !cx.live.in_call() {
                    keypad::push_str(&mut self.number, "+");
                }
            }
            ui.ctx().request_repaint_after(Duration::from_millis(80));
        }
        if response.clicked() && !self.zero_fired {
            self.press(cx, '0');
        }
        if !response.is_pointer_button_down_on() {
            self.zero_down_since = None;
            self.zero_fired = false;
        }
    }

    fn actions(&mut self, ui: &mut Ui, cx: &mut Cx, d: f32, grid_w: f32) {
        let pal = cx.pal;
        let in_call = cx.live.in_call();
        let side = (d * 0.76).max(40.0);
        ui.allocate_ui_with_layout(
            vec2(grid_w, d),
            Layout::left_to_right(Align::Center),
            |ui| {
                // Text
                cell(ui, d, side, |ui| {
                    let can = !self.number.is_empty() && !in_call;
                    let r = theme::round_button(
                        ui,
                        &pal,
                        side,
                        pal.control,
                        pal.text,
                        Icon::Chat,
                        can,
                        "Text this number",
                    );
                    if r.clicked() {
                        let n = self.number.clone();
                        cx.new_message_to(&n);
                    }
                });
                // Call / Hang up
                cell(ui, d, d, |ui| {
                    if let Some(call) = cx.live.calls.first() {
                        let id = call.call.id.clone();
                        if theme::round_button(
                            ui,
                            &pal,
                            d,
                            pal.accent_fill,
                            Color32::WHITE,
                            Icon::HangUp,
                            true,
                            "Hang up",
                        )
                        .clicked()
                        {
                            cx.handle.hang_up(&id);
                        }
                    } else {
                        let can = keypad::can_dial(&self.number) && cx.live.phone_connected();
                        let tip = if cx.live.phone_connected() {
                            "Call"
                        } else {
                            "Connect your phone to call"
                        };
                        if theme::round_button(
                            ui,
                            &pal,
                            d,
                            pal.selected_bg,
                            pal.selected_fg,
                            Icon::Phone,
                            can,
                            tip,
                        )
                        .clicked()
                        {
                            self.dial(cx);
                        }
                    }
                });
                // Delete
                cell(ui, d, side, |ui| {
                    let can = !self.number.is_empty() && !in_call;
                    if theme::round_button(
                        ui,
                        &pal,
                        side,
                        pal.control,
                        pal.text,
                        Icon::Backspace,
                        can,
                        "Delete",
                    )
                    .clicked()
                    {
                        keypad::backspace(&mut self.number);
                    }
                });
            },
        );
        let _ = (CornerRadius::ZERO, StrokeKind::Inside, S3);
    }
}

/// A square cell of side `d` with `inner` (of side `size`) centred in it.
fn cell(ui: &mut Ui, d: f32, size: f32, add: impl FnOnce(&mut Ui)) {
    let (rect, _) = ui.allocate_exact_size(vec2(d, d), Sense::hover());
    let inner = Rect::from_center_size(rect.center(), vec2(size, size));
    ui.scope_builder(UiBuilder::new().max_rect(inner), add);
}
