// SPDX-License-Identifier: GPL-3.0-or-later
//! The call card: pinned above every screen for as long as the phone has a call.

use super::{Cmd, Cx};
use crate::data::CallView;
use crate::model::format::call_timer;
use crate::model::status::{call_state_label, call_title};
use crate::model::tab::Tab;
use crate::ui::nothing;
use crate::ui::theme::{self, ButtonKind, PillSize, R_CARD, S2, S3};
use crate::ui::widgets;
use eframe::egui::{
    self, vec2, Align, Color32, CornerRadius, Frame, Layout, Margin, Sense, Stroke, Ui,
};
use phone_core::calls::CallState;
use std::time::Duration;

const MUTE_NOTE: &str = "Muting is not available: the call's audio is handled by your phone and Bluetooth, not by this app";
const AUDIO_NOTE: &str = "Move the call's voice to this computer's speakers and microphone";

/// Draws one call.
pub fn show(ui: &mut Ui, cx: &mut Cx, call: &CallView, on_keypad: bool) {
    let pal = cx.pal;
    let state = call.call.state;
    let ringing = call.is_ringing();
    let stroke = if ringing {
        Stroke::new(1.5_f32, pal.accent_fill)
    } else {
        Stroke::new(1.0_f32, pal.separator)
    };
    Frame::new()
        .fill(pal.tile)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(R_CARD))
        .inner_margin(Margin::symmetric(S3 as i8 + 2, S3 as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = vec2(S3, S2);
            let id = call.call.id.clone();
            let title = call_title(&call.call);
            let number = call.call.number.trim().to_string();
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(44.0, 44.0), Sense::hover());
                let key = if number.is_empty() {
                    id.clone()
                } else {
                    cx.data.avatar_key(&number)
                };
                widgets::avatar(ui.painter(), r.center(), 22.0, &key, &title);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let room =
                        (ui.available_width() - status_width(call) - buttons_width(call) - 24.0)
                            .max(120.0);
                    let g = widgets::ellipsized(
                        ui,
                        &title,
                        egui::FontId::new(16.0, theme::semibold_family()),
                        pal.text,
                        room,
                    );
                    let (rect, _) = ui.allocate_exact_size(g.size(), Sense::hover());
                    ui.painter().galley(rect.min, g, pal.text);
                    let sub = if !number.is_empty() && number != title {
                        format!("{} · {number}", call_state_label(state))
                    } else {
                        call_state_label(state).to_string()
                    };
                    let g = widgets::ellipsized(
                        ui,
                        &sub,
                        egui::FontId::new(12.5, egui::FontFamily::Proportional),
                        pal.text2,
                        room,
                    );
                    let (rect, _) = ui.allocate_exact_size(g.size(), Sense::hover());
                    ui.painter().galley(rect.min, g, pal.text2);
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if !call.is_active() {
                        buttons(ui, cx, call, on_keypad, &id);
                    }
                    status(ui, cx, call);
                });
            });
            if call.is_active() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = S2;
                    theme::pill(
                        ui,
                        &pal,
                        "Mute",
                        ButtonKind::Plain,
                        false,
                        PillSize::Regular,
                        0.0,
                    )
                    .on_disabled_hover_text(MUTE_NOTE);
                    if !on_keypad
                        && theme::pill(
                            ui,
                            &pal,
                            "Keypad",
                            ButtonKind::Plain,
                            true,
                            PillSize::Regular,
                            0.0,
                        )
                        .clicked()
                    {
                        cx.cmds.push(Cmd::Tab(Tab::Keypad));
                    }
                    if theme::pill(
                        ui,
                        &pal,
                        "Audio on this computer",
                        ButtonKind::Plain,
                        true,
                        PillSize::Regular,
                        0.0,
                    )
                    .on_hover_text(AUDIO_NOTE)
                    .clicked()
                    {
                        cx.handle.audio_here();
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if theme::pill_button_sized(
                            ui,
                            &pal,
                            "Hang up",
                            ButtonKind::Primary,
                            true,
                            104.0,
                        )
                        .clicked()
                        {
                            cx.handle.hang_up(&id);
                        }
                    });
                });
            }
        });
    if call.is_active() || state == CallState::Alerting {
        // The timer ticks every second.
        ui.ctx().request_repaint_after(Duration::from_millis(500));
    }
}

/// Answer / Decline for a ringing call, Cancel for one being placed.
fn buttons(ui: &mut Ui, cx: &mut Cx, call: &CallView, _on_keypad: bool, id: &str) {
    let pal = cx.pal;
    // Right-to-left: the last drawn is leftmost.
    if call.is_ringing() {
        if theme::pill_button_sized(ui, &pal, "Answer", ButtonKind::Inverse, true, 96.0).clicked() {
            cx.handle.answer(id);
        }
        if theme::pill_button_sized(ui, &pal, "Decline", ButtonKind::Primary, true, 96.0).clicked()
        {
            cx.handle.hang_up(id);
        }
    } else if theme::pill_button_sized(ui, &pal, "Cancel", ButtonKind::Primary, true, 96.0)
        .clicked()
    {
        cx.handle.hang_up(id);
    }
}

fn buttons_width(call: &CallView) -> f32 {
    if call.is_active() {
        0.0
    } else if call.is_ringing() {
        210.0
    } else {
        104.0
    }
}

/// The dot-matrix state on the right: a timer while talking, a word while ringing or dialling.
fn status(ui: &mut Ui, cx: &Cx, call: &CallView) {
    let pal = cx.pal;
    let (text, color) = match call.call.state {
        CallState::Incoming | CallState::Waiting => ("INCOMING".to_string(), pal.accent_fill),
        CallState::Dialing => ("CALLING".to_string(), pal.text),
        CallState::Alerting => ("RINGING".to_string(), pal.text),
        CallState::Held => ("ON HOLD".to_string(), pal.text2),
        CallState::Active => (
            call.seconds()
                .map_or_else(|| "--:--".to_string(), call_timer),
            pal.text,
        ),
        CallState::Disconnected => ("ENDED".to_string(), pal.text2),
    };
    let pitch = if call.is_active() { 3.4 } else { 2.6 };
    nothing::dot_text(ui, &pal, &text, pitch, color, true);
    if call.recording {
        recording_badge(ui, cx);
    }
}

fn status_width(call: &CallView) -> f32 {
    let len = if call.is_active() { 5.0 } else { 8.0 };
    let pitch = if call.is_active() { 3.4 } else { 2.6 };
    len * 6.0 * pitch + if call.recording { 48.0 } else { 0.0 }
}

/// A small red dot and "REC", shown while the call is being recorded.
fn recording_badge(ui: &mut Ui, cx: &Cx) {
    let pal = cx.pal;
    nothing::dot_text(ui, &pal, "REC", 2.2, pal.accent, false);
    let (r, resp) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
    ui.painter().circle_filled(r.center(), 4.5, pal.accent_fill);
    resp.on_hover_text("This call is being recorded");
    let _ = Color32::WHITE;
}

/// Width the card needs, for layout tests.
#[cfg(test)]
mod tests {
    use super::*;
    use phone_core::service::ActiveCall;

    fn view(state: CallState, recording: bool) -> CallView {
        CallView {
            call: ActiveCall {
                id: "c".into(),
                state,
                number: "+1".into(),
                name: None,
                since: None,
                recording: false,
            },
            recording,
        }
    }

    #[test]
    fn buttons_belong_to_the_state() {
        assert_eq!(buttons_width(&view(CallState::Active, false)), 0.0);
        assert!(
            buttons_width(&view(CallState::Incoming, false))
                > buttons_width(&view(CallState::Dialing, false))
        );
    }

    #[test]
    fn the_recording_badge_widens_the_status() {
        assert!(
            status_width(&view(CallState::Active, true))
                > status_width(&view(CallState::Active, false))
        );
    }
}
