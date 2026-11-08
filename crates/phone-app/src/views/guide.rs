// SPDX-License-Identifier: GPL-3.0-or-later
//! The iPhone setup guide: a pop-up with four steps that tick themselves off as the service sees
//! the iPhone's links come up. See `model::guide` for what each step means.

use super::Cx;
use crate::model::guide::{self, AutoOpen, Facts, Progress, Step, StepKind, StepState};
use crate::ui::icons::Icon;
use crate::ui::nothing;
use crate::ui::theme::{self, ButtonKind, Palette, PillSize, R_ROW, S2, S3, S4};
use crate::ui::widgets;
use eframe::egui::{
    self, pos2, vec2, Align, Color32, CornerRadius, Frame, Id, Layout, Margin, Rect, ScrollArea,
    Sense, Stroke, Ui,
};
use phone_core::service::Command;
use std::collections::BTreeSet;
use std::time::Duration;

const MAX_WIDTH: f32 = 540.0;
const GLYPH: f32 = 28.0;

/// What the pop-up asked the app to do this frame.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// The user asked not to see the guide again for this phone (its address).
    pub dismiss: Option<String>,
}

#[derive(Debug, Default)]
pub struct GuideState {
    open: bool,
    closed_this_run: bool,
    dont_show: bool,
}

impl GuideState {
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens it on request (Settings, the Help menu). Ignores dismissals and an earlier close.
    pub fn open_manually(&mut self) {
        self.open = true;
        self.dont_show = false;
    }

    /// Opens it by itself when the rules say so. Returns true when it did.
    pub fn auto_open(&mut self, cx: &Cx, dismissed: &BTreeSet<String>) -> bool {
        let steps = guide::steps(&progress(cx));
        let due = guide::should_open(&AutoOpen {
            phone: cx.live.phone.as_ref(),
            dismissed,
            steps: &steps,
            in_call: cx.live.in_call(),
            open: self.open,
            closed_this_run: self.closed_this_run,
        });
        if due {
            self.open_manually();
        }
        due
    }

    /// Closes it, as the Close button does (without the "don't show again" choice).
    pub fn close(&mut self) {
        self.open = false;
        self.closed_this_run = true;
    }

    /// Draws the pop-up when it is open. It steps aside while a call is on, so the call card
    /// behind it can be answered.
    pub fn show(&mut self, ctx: &egui::Context, cx: &Cx) -> Outcome {
        let mut out = Outcome::default();
        if !self.open || cx.live.in_call() {
            return out;
        }
        let pal = cx.pal;
        let steps = guide::steps(&progress(cx));
        let finished = guide::all_done(&steps);
        let screen = ctx.screen_rect();
        let width = (screen.width() - 48.0).clamp(280.0, MAX_WIDTH);
        let mut action = Action::None;
        let modal = egui::Modal::new(Id::new("iphone-setup-guide"))
            .backdrop_color(Color32::from_black_alpha(if pal.dark { 170 } else { 96 }))
            .frame(
                Frame::new()
                    .fill(pal.surface)
                    .stroke(Stroke::new(
                        1.0_f32,
                        if pal.dark {
                            pal.control_active
                        } else {
                            pal.separator
                        },
                    ))
                    .corner_radius(CornerRadius::same(20))
                    .inner_margin(Margin::same(22)),
            )
            .show(ctx, |ui| {
                ui.set_width(width - 44.0);
                ui.spacing_mut().item_spacing.y = S2;
                if finished {
                    action = success(ui, &pal);
                } else {
                    let room = (screen.height() - 190.0).max(160.0);
                    ScrollArea::vertical()
                        .max_height(room)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            header(ui, &pal, &mut action);
                            ui.add_space(S3);
                            steps_list(ui, cx, &steps);
                        });
                    ui.add_space(S2);
                    footer(ui, &pal, &mut self.dont_show, &mut action);
                }
            });
        if modal.should_close() && action == Action::None {
            action = Action::Close;
        }
        match action {
            Action::None => {}
            Action::CheckAgain => {
                cx.handle.retry_messages();
                cx.handle.send(Command::ReloadContacts);
            }
            Action::Close => {
                if self.dont_show {
                    out.dismiss = cx.live.phone.as_ref().map(|p| p.address.clone());
                }
                self.close();
            }
            Action::Done => self.close(),
        }
        // The steps tick over without an event of their own: look again soon.
        ctx.request_repaint_after(Duration::from_millis(500));
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    None,
    CheckAgain,
    Close,
    Done,
}

fn progress<'a>(cx: &'a Cx) -> Progress<'a> {
    Progress {
        phone: cx.live.phone.as_ref(),
        messages: &cx.live.messages_status,
        contacts: &cx.live.contacts_status,
        contacts_len: cx.live.contact_count,
        calls_ready: cx.live.calls_ready,
    }
}

fn header(ui: &mut Ui, pal: &Palette, action: &mut Action) {
    ui.horizontal(|ui| {
        ui.label(theme::semibold("Set up your iPhone", 21.0));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if theme::icon_button(ui, pal, Icon::Close, "Close").clicked() {
                *action = Action::Close;
            }
        });
    });
    ui.add(
        egui::Label::new(theme::secondary(
            pal,
            "An iPhone shares texts and contacts with this computer like a car kit, once you switch them on. Four short steps; each one ticks itself off.",
        ))
        .wrap(),
    );
}

fn steps_list(ui: &mut Ui, cx: &Cx, steps: &[Step; 4]) {
    let pal = cx.pal;
    let computer = guide::computer_label(cx.live.computer_name.as_deref());
    let phone_name = cx
        .live
        .phone
        .as_ref()
        .map_or("your iPhone", |p| p.name.as_str());
    let facts = Facts {
        computer: &computer,
        phone_name,
        contacts_len: cx.live.contact_count,
    };
    let current = guide::current(steps);
    for (i, step) in steps.iter().enumerate() {
        if i > 0 {
            theme::hairline(ui, &pal);
        }
        let is_current = current == Some(step.kind);
        ui.add_space(S2);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = S3;
            glyph(ui, &pal, step, is_current);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                let title_color = if step.state == StepState::Done {
                    pal.text2
                } else {
                    pal.text
                };
                ui.label(theme::semibold(guide::instruction(step.kind), 15.0).color(title_color));
                ui.add(
                    egui::Label::new(
                        theme::secondary(&pal, guide::detail(step.kind, step.state, &facts))
                            .size(13.0),
                    )
                    .wrap(),
                );
                if step.kind == StepKind::Call {
                    if let Some(note) = guide::call_note(step.state) {
                        ui.label(theme::caption(&pal, note));
                    }
                }
                if is_current {
                    match step.kind {
                        StepKind::Texts => mock(
                            ui,
                            &pal,
                            &[
                                Mock::Row(&computer, Right::Info),
                                Mock::Row("Show Notifications", Right::Switch),
                            ],
                        ),
                        StepKind::Contacts => {
                            mock(ui, &pal, &[Mock::Row("Sync Contacts", Right::Switch)])
                        }
                        StepKind::Pair | StepKind::Call => {}
                    }
                }
            });
        });
        ui.add_space(S2);
    }
}

/// The status mark at the left of a step: a green check, a numbered ring, or a red exclamation.
fn glyph(ui: &mut Ui, pal: &Palette, step: &Step, current: bool) {
    let (rect, response) = ui.allocate_exact_size(vec2(GLYPH, GLYPH), Sense::hover());
    let c = rect.center();
    let p = ui.painter();
    let label = match step.state {
        StepState::Done => "done",
        StepState::Waiting => "waiting",
        StepState::Problem => "problem",
    };
    match step.state {
        StepState::Done => {
            p.circle_filled(c, GLYPH / 2.0, pal.ok);
            check(
                p,
                c,
                6.5,
                if pal.dark {
                    Color32::BLACK
                } else {
                    Color32::WHITE
                },
            );
        }
        StepState::Problem => {
            p.circle_filled(c, GLYPH / 2.0, pal.accent_fill);
            let g = p.layout_no_wrap(
                "!".into(),
                egui::FontId::new(16.0, theme::semibold_family()),
                Color32::WHITE,
            );
            p.galley(c - g.size() / 2.0, g, Color32::WHITE);
        }
        StepState::Waiting => {
            let (ring, width) = if current {
                (pal.text, 2.0_f32)
            } else {
                (pal.text3, 1.4_f32)
            };
            p.circle_stroke(c, GLYPH / 2.0 - 1.0, Stroke::new(width, ring));
            let n = step.kind.number().to_string();
            let g = p.layout_no_wrap(n, egui::FontId::new(13.0, theme::semibold_family()), ring);
            p.galley(c - g.size() / 2.0, g, ring);
        }
    }
    let text = format!("Step {}: {label}", step.kind.number());
    response.widget_info(move || {
        egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text.clone())
    });
}

fn check(p: &egui::Painter, c: egui::Pos2, s: f32, color: Color32) {
    let stroke = Stroke::new(2.4_f32, color);
    p.line_segment(
        [
            pos2(c.x - s, c.y + 0.5),
            pos2(c.x - s * 0.35, c.y + s * 0.65),
        ],
        stroke,
    );
    p.line_segment(
        [
            pos2(c.x - s * 0.35, c.y + s * 0.65),
            pos2(c.x + s, c.y - s * 0.6),
        ],
        stroke,
    );
}

enum Right {
    /// A circled (i), highlighted.
    Info,
    /// A switch that is on.
    Switch,
}

enum Mock<'a> {
    Row(&'a str, Right),
}

/// A tiny picture of the iPhone's settings row the step is about, drawn with shapes.
fn mock(ui: &mut Ui, pal: &Palette, rows: &[Mock]) {
    ui.add_space(4.0);
    let w = ui.available_width().min(330.0);
    let row_h = 38.0;
    let (rect, response) =
        ui.allocate_exact_size(vec2(w, row_h * rows.len() as f32), Sense::hover());
    let radius = CornerRadius::same(R_ROW);
    ui.painter().rect_filled(rect, radius, pal.control);
    let mut spoken = Vec::new();
    for (i, Mock::Row(label, right)) in rows.iter().enumerate() {
        let r = Rect::from_min_size(
            pos2(rect.left(), rect.top() + row_h * i as f32),
            vec2(w, row_h),
        );
        if i > 0 {
            ui.painter().hline(
                egui::Rangef::new(r.left() + 12.0, r.right()),
                r.top(),
                Stroke::new(1.0_f32, pal.separator),
            );
        }
        let max_w = w - 86.0;
        let g = widgets::ellipsized(
            ui,
            label,
            egui::FontId::new(13.5, egui::FontFamily::Proportional),
            pal.text,
            max_w,
        );
        ui.painter().galley(
            pos2(r.left() + 12.0, r.center().y - g.size().y / 2.0),
            g,
            pal.text,
        );
        spoken.push((*label).to_string());
        match right {
            Right::Info => {
                let c = pos2(r.right() - 20.0, r.center().y);
                // The (i) the user has to tap, ringed in red.
                ui.painter()
                    .circle_stroke(c, 8.0, Stroke::new(1.4_f32, pal.text2));
                let i = ui.painter().layout_no_wrap(
                    "i".into(),
                    egui::FontId::new(11.5, theme::semibold_family()),
                    pal.text2,
                );
                ui.painter().galley(c - i.size() / 2.0, i, pal.text2);
                ui.painter()
                    .circle_stroke(c, 13.0, Stroke::new(2.0_f32, pal.accent_fill));
            }
            Right::Switch => {
                let track =
                    Rect::from_center_size(pos2(r.right() - 32.0, r.center().y), vec2(40.0, 24.0));
                ui.painter()
                    .rect_filled(track, CornerRadius::same(12), pal.ok);
                ui.painter().circle_filled(
                    pos2(track.right() - 12.0, track.center().y),
                    9.5,
                    Color32::WHITE,
                );
            }
        }
    }
    let text = format!("Picture of the iPhone's settings: {}", spoken.join(", "));
    response.widget_info(move || {
        egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text.clone())
    });
}

fn footer(ui: &mut Ui, pal: &Palette, dont_show: &mut bool, action: &mut Action) {
    theme::hairline(ui, pal);
    ui.add_space(S2);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = S3;
        ui.horizontal(|ui| {
            widgets::toggle(ui, pal, dont_show, "Don't show this again");
            ui.label(theme::caption(pal, "Don't show this again"));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if theme::pill(
                ui,
                pal,
                "Close",
                ButtonKind::Inverse,
                true,
                PillSize::Regular,
                76.0,
            )
            .clicked()
            {
                *action = Action::Close;
            }
            if theme::pill(
                ui,
                pal,
                "Check again",
                ButtonKind::Plain,
                true,
                PillSize::Regular,
                0.0,
            )
            .clicked()
            {
                *action = Action::CheckAgain;
            }
        });
    });
}

fn success(ui: &mut Ui, pal: &Palette) -> Action {
    let mut action = Action::None;
    ui.vertical_centered(|ui| {
        ui.add_space(S3);
        let (rect, response) = ui.allocate_exact_size(vec2(56.0, 56.0), Sense::hover());
        ui.painter().circle_filled(rect.center(), 28.0, pal.ok);
        check(
            ui.painter(),
            rect.center(),
            13.0,
            if pal.dark {
                Color32::BLACK
            } else {
                Color32::WHITE
            },
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "All four steps are done")
        });
        ui.add_space(S3);
        let pitch = nothing::fit_pitch("ALL SET", 4.4, 2.4, ui.available_width());
        nothing::dot_text(ui, pal, "ALL SET", pitch, pal.text, true);
        ui.add_space(S2);
        ui.add(
            egui::Label::new(
                theme::secondary(
                    pal,
                    "All set. Your iPhone works like a car kit: calls, texts and contacts.",
                )
                .size(14.5),
            )
            .wrap(),
        );
        ui.add_space(S4);
        if theme::pill(
            ui,
            pal,
            "Done",
            ButtonKind::Inverse,
            true,
            PillSize::Regular,
            120.0,
        )
        .clicked()
        {
            action = Action::Done;
        }
        ui.add_space(S2);
    });
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_open_ignores_an_earlier_close_and_clears_the_tick_box() {
        let mut g = GuideState {
            closed_this_run: true,
            dont_show: true,
            ..GuideState::default()
        };
        g.open_manually();
        assert!(g.is_open());
        assert!(!g.dont_show);
    }

    #[test]
    fn closing_remembers_it_for_this_run() {
        let mut g = GuideState::default();
        g.open_manually();
        g.close();
        assert!(!g.is_open());
        assert!(g.closed_this_run);
    }
}
