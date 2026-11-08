// SPDX-License-Identifier: GPL-3.0-or-later
//! Messages: the conversation list on the left, the open thread on the right (one at a time on a
//! narrow window), a composer, and a "new message" flow with contact suggestions.

mod list;
mod outbox;
mod thread;

use super::Cx;
use crate::model::compose;
use crate::shared::TextOutcome;
use crate::state::UiState;
use crate::ui::vlist::VListState;
use eframe::egui::{self, Rect, Sense, Ui, UiBuilder, vec2};
use outbox::Outbox;
use phone_core::contacts::number_key;
use std::collections::HashMap;

/// Below this width the list and the thread take turns instead of sitting side by side.
pub const NARROW_BELOW: f32 = 680.0;
const LIST_WIDTH: f32 = 304.0;

/// The conversation being read. One value, never a flag per row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub key: String,
    /// The number as the phone wrote it (a new conversation has no history to read it from).
    pub number: String,
}

#[derive(Default)]
pub struct MessagesState {
    selected: Option<Selection>,
    /// The "To:" text while writing a new message; `None` when not composing a new one.
    new_to: Option<String>,
    list: VListState,
    drafts: HashMap<String, String>,
    outbox: Outbox,
    focus_composer: bool,
    focus_to: bool,
}

impl MessagesState {
    /// Shows the conversation with `number`, existing or not, and puts the cursor in the composer.
    pub fn open_conversation(&mut self, number: &str) {
        self.selected = Some(Selection { key: number_key(number), number: number.to_string() });
        self.new_to = None;
        self.focus_composer = true;
    }

    /// Starts a new message. With a usable `to`, goes straight to that thread.
    pub fn new_message(&mut self, to: Option<&str>) {
        match to.and_then(compose::recipient_number) {
            Some(number) => self.open_conversation(&number),
            None => {
                self.new_to = Some(to.unwrap_or("").to_string());
                self.selected = None;
                self.focus_to = true;
            }
        }
    }

    /// Escape: leave the new-message form, or the open thread. Returns true when it did.
    pub fn escape(&mut self) -> bool {
        if self.new_to.take().is_some() {
            return true;
        }
        self.selected.take().is_some()
    }

    /// Whether a thread or the new-message form is showing (a narrow window then hides the list).
    pub fn has_detail(&self) -> bool {
        self.selected.is_some() || self.new_to.is_some()
    }

    pub fn selected_key(&self) -> Option<&str> {
        self.selected.as_ref().map(|s| s.key.as_str())
    }

    /// Records the outcome of sent texts. Returns the failure text of those this view did not send
    /// (a quick reply from an alert), for the app to show as a notice.
    pub fn apply_outcomes(&mut self, outcomes: Vec<TextOutcome>) -> Vec<String> {
        outcomes
            .into_iter()
            .filter_map(|o| match self.outbox.settle(o.token, &o.result) {
                true => None,
                false => o.result.err().map(|e| format!("Could not send the text to {}: {e}", o.number)),
            })
            .collect()
    }

    pub fn show(&mut self, ui: &mut Ui, cx: &mut Cx, ui_state: &mut UiState) {
        let full = ui.available_rect_before_wrap();
        let narrow = full.width() < NARROW_BELOW;
        let (list_rect, detail_rect) = if narrow {
            if self.has_detail() {
                (None, Some(full))
            } else {
                (Some(full), None)
            }
        } else {
            let split = full.left() + LIST_WIDTH;
            (Some(Rect::from_min_max(full.min, egui::pos2(split, full.bottom()))), Some(Rect::from_min_max(egui::pos2(split + 1.0, full.top()), full.max)))
        };
        if let Some(r) = list_rect {
            ui.scope_builder(UiBuilder::new().max_rect(r), |ui| {
                ui.set_clip_rect(r.intersect(ui.clip_rect()));
                self.show_list(ui, cx, ui_state);
            });
        }
        if let (Some(l), Some(_)) = (list_rect, detail_rect) {
            ui.painter().vline(l.right() + 0.5, full.y_range(), egui::Stroke::new(1.0_f32, cx.pal.separator));
        }
        if let Some(r) = detail_rect {
            ui.scope_builder(UiBuilder::new().max_rect(r), |ui| {
                ui.set_clip_rect(r.intersect(ui.clip_rect()));
                self.show_detail(ui, cx, ui_state, narrow);
            });
        }
        ui.allocate_rect(full, Sense::hover());
        let _ = vec2(0.0, 0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_a_conversation_selects_it_by_key_and_focuses_the_composer() {
        let mut s = MessagesState::default();
        s.new_message(None);
        assert!(s.new_to.is_some());
        s.open_conversation("+213 550 123 456");
        assert_eq!(s.selected_key(), Some(number_key("+213550123456").as_str()));
        assert!(s.new_to.is_none());
        assert!(s.focus_composer);
    }

    #[test]
    fn a_new_message_to_a_usable_number_goes_straight_to_the_thread() {
        let mut s = MessagesState::default();
        s.new_message(Some("+1 (555) 010-0100"));
        assert!(s.new_to.is_none());
        assert_eq!(s.selected.as_ref().map(|x| x.number.as_str()), Some("+15550100100"));
    }

    #[test]
    fn a_new_message_to_nonsense_opens_the_form() {
        let mut s = MessagesState::default();
        s.new_message(Some("Alice"));
        assert_eq!(s.new_to.as_deref(), Some("Alice"));
        assert!(s.selected.is_none());
    }

    #[test]
    fn escape_leaves_the_form_then_the_thread() {
        let mut s = MessagesState::default();
        s.open_conversation("+1555");
        s.new_message(None);
        assert!(s.escape());
        assert!(!s.has_detail());
        s.open_conversation("+1555");
        assert!(s.escape());
        assert!(!s.escape());
    }

    #[test]
    fn failed_sends_from_elsewhere_become_notices() {
        let mut s = MessagesState::default();
        let token = s.outbox.add_with_token(7, "k", "+1", "hi", 1);
        let out = vec![
            TextOutcome { token, number: "+1".into(), result: Err("no link".into()) },
            TextOutcome { token: 999, number: "+2".into(), result: Err("no phone".into()) },
            TextOutcome { token: 998, number: "+3".into(), result: Ok(()) },
        ];
        let notices = s.apply_outcomes(out);
        assert_eq!(notices, vec!["Could not send the text to +2: no phone".to_string()]);
        assert!(s.outbox.entries("k").any(|o| o.failure().is_some()));
    }
}
