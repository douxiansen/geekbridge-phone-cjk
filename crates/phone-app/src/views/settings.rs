// SPDX-License-Identifier: GPL-3.0-or-later
//! Settings: every field of `phone_core::settings::Settings`, the paired phones, diagnostics and
//! About. Changes apply at once.

use super::{Cmd, Cx};
use crate::model::status::link_text;
use crate::platform_ui;
use crate::ui::nothing;
use crate::ui::theme::{self, ButtonKind, PillSize, S2, S3, S4, S5};
use crate::ui::widgets;
use eframe::egui::{self, vec2, Id, RichText, ScrollArea, Ui, UiBuilder};
use phone_core::service::Command;
use phone_core::settings::{Settings, ThemePref};
use std::time::{Duration, Instant};

const COLUMN: f32 = 640.0;
/// How long a change the service has not echoed back yet is shown as if it had.
const PENDING_FOR: Duration = Duration::from_millis(2500);
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Default)]
pub struct SettingsState {
    pending: Option<(Settings, Instant)>,
    recordings_text: String,
    folder_note: Option<String>,
}

/// The settings to show: our own just-made change until the service reports it, else the service's.
fn displayed(live: &Settings, pending: &Option<(Settings, Instant)>) -> Settings {
    match pending {
        Some((p, at)) if at.elapsed() < PENDING_FOR && p != live => p.clone(),
        _ => live.clone(),
    }
}

impl SettingsState {
    pub fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let mut s = displayed(&cx.live.settings, &self.pending);
        let before = s.clone();

        let avail = ui.available_rect_before_wrap();
        let w = avail.width().min(COLUMN) - 2.0 * S4;
        let x = avail.center().x - w / 2.0;
        ScrollArea::vertical()
            .id_salt("settings-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let rect =
                    egui::Rect::from_min_size(egui::pos2(x, ui.cursor().min.y + S4), vec2(w, 10.0));
                ui.scope_builder(
                    UiBuilder::new().max_rect(egui::Rect::from_min_max(
                        rect.min,
                        egui::pos2(rect.max.x, f32::INFINITY),
                    )),
                    |ui| {
                        ui.set_width(w);
                        self.appearance(ui, cx, &mut s);
                        self.notifications(ui, cx, &mut s);
                        self.calls(ui, cx, &mut s);
                        self.phones(ui, cx, &mut s);
                        self.background(ui, cx, &mut s);
                        self.updates(ui, cx, &mut s);
                        self.data(ui, cx, &mut s);
                        self.diagnostics(ui, cx);
                        self.about(ui, cx);
                        ui.add_space(S5);
                    },
                );
            });

        if s != before {
            cx.handle.set_settings(s.clone());
            self.pending = Some((s, Instant::now()));
            ui.ctx().request_repaint_after(Duration::from_millis(200));
        }
    }

    fn appearance(&mut self, ui: &mut Ui, cx: &Cx, s: &mut Settings) {
        let pal = cx.pal;
        theme::section(ui, &pal, "Appearance");
        theme::card(ui, &pal, |ui| {
            widgets::setting_row(
                ui,
                &pal,
                "Theme",
                "System follows your desktop's light or dark setting.",
                |ui| {
                    theme::segmented(
                        ui,
                        &pal,
                        &mut s.theme,
                        &[
                            (ThemePref::System, "System"),
                            (ThemePref::Light, "Light"),
                            (ThemePref::Dark, "Dark"),
                        ],
                    );
                },
            );
        });
    }

    fn notifications(&mut self, ui: &mut Ui, cx: &Cx, s: &mut Settings) {
        let pal = cx.pal;
        theme::section(ui, &pal, "Notifications");
        theme::card(ui, &pal, |ui| {
            toggle_row(
                ui,
                &pal,
                "Incoming calls",
                "Show an alert while the phone rings.",
                &mut s.notify_calls,
            );
            widgets::card_divider(ui, &pal);
            toggle_row(
                ui,
                &pal,
                "Missed calls",
                "Show an alert when you miss a call.",
                &mut s.notify_missed,
            );
            widgets::card_divider(ui, &pal);
            toggle_row(
                ui,
                &pal,
                "New texts",
                "Show an alert when a text arrives.",
                &mut s.notify_texts,
            );
            widgets::card_divider(ui, &pal);
            toggle_row(
                ui,
                &pal,
                "Quick reply",
                "Offer a reply box on text alerts.",
                &mut s.quick_reply,
            );
            widgets::card_divider(ui, &pal);
            widgets::setting_row(
                ui,
                &pal,
                "Text alerts stay for",
                "When you do not touch them.",
                |ui| {
                    widgets::stepper(ui, &pal, &mut s.text_banner_secs, 2, 60, "s");
                },
            );
        });
    }

    fn calls(&mut self, ui: &mut Ui, cx: &Cx, s: &mut Settings) {
        let pal = cx.pal;
        theme::section(ui, &pal, "Calls");
        theme::card(ui, &pal, |ui| {
            toggle_row(
                ui,
                &pal,
                "Calls only",
                "Use Bluetooth audio for calls only. Music and videos from the phone do not play on this computer.",
                &mut s.calls_audio_only,
            );
            widgets::card_divider(ui, &pal);
            toggle_row(
                ui,
                &pal,
                "Record calls",
                "Save the audio of calls on this computer.",
                &mut s.record_calls,
            );
            ui.add_space(S2);
            ui.add(
                egui::Label::new(theme::caption(
                    &pal,
                    "Recording a call may require the other person's consent where you live.",
                ))
                .wrap(),
            );
            widgets::card_divider(ui, &pal);
            ui.label(RichText::new("Recordings folder").size(14.0));
            ui.add_space(4.0);
            let default = platform_ui::default_recordings_dir();
            let default_text = platform_ui::tilde(&default);
            let id = Id::new("recordings-dir");
            let focused = ui.memory(|m| m.has_focus(id));
            if !focused {
                self.recordings_text = s
                    .recordings_dir
                    .as_deref()
                    .map(|d| platform_ui::tilde(&platform_ui::expand_tilde(d)))
                    .unwrap_or_default();
            }
            ui.horizontal(|ui| {
                let field_w = (ui.available_width() - 190.0).max(160.0);
                let response = widgets::field(
                    ui,
                    &pal,
                    &mut self.recordings_text,
                    &default_text,
                    id,
                    field_w,
                    None,
                );
                if response.lost_focus() {
                    let typed = self.recordings_text.trim();
                    s.recordings_dir = (!typed.is_empty())
                        .then(|| platform_ui::expand_tilde(typed).display().to_string());
                }
                let folder = s
                    .recordings_dir
                    .as_deref()
                    .map_or_else(|| default.clone(), platform_ui::expand_tilde);
                if theme::pill(
                    ui,
                    &pal,
                    "Open folder",
                    ButtonKind::Plain,
                    true,
                    PillSize::Regular,
                    0.0,
                )
                .clicked()
                {
                    self.folder_note = platform_ui::open_folder(&folder)
                        .err()
                        .map(|e| format!("Could not open {}: {e}", platform_ui::tilde(&folder)));
                }
                if s.recordings_dir.is_some()
                    && theme::pill(
                        ui,
                        &pal,
                        "Reset",
                        ButtonKind::Plain,
                        true,
                        PillSize::Regular,
                        0.0,
                    )
                    .on_hover_text("Use the default folder")
                    .clicked()
                {
                    s.recordings_dir = None;
                    self.recordings_text.clear();
                }
            });
            let shown = s.recordings_dir.as_deref().map_or_else(
                || default_text.clone(),
                |d| platform_ui::tilde(&platform_ui::expand_tilde(d)),
            );
            ui.add_space(4.0);
            ui.add(
                egui::Label::new(theme::caption(
                    &pal,
                    self.folder_note
                        .clone()
                        .unwrap_or_else(|| format!("Saved to {shown}")),
                ))
                .wrap(),
            );
        });
    }

    fn phones(&mut self, ui: &mut Ui, cx: &Cx, s: &mut Settings) {
        let pal = cx.pal;
        theme::section(ui, &pal, "Phone");
        theme::card(ui, &pal, |ui| {
            let auto = s.preferred_phone.is_none();
            if widgets::radio_row(
                ui,
                &pal,
                auto,
                "Automatic",
                "Use the connected phone, or the first paired one.",
            )
            .clicked()
            {
                s.preferred_phone = None;
            }
            if cx.live.phones.is_empty() {
                ui.add_space(S2);
                ui.add(
                    egui::Label::new(theme::caption(
                        &pal,
                        "No paired phones. Pair your phone in your system's Bluetooth settings.",
                    ))
                    .wrap(),
                );
            }
            for p in &cx.live.phones {
                let selected = s.preferred_phone.as_deref() == Some(p.address.as_str());
                let mut sub = format!(
                    "{} · {}",
                    p.address,
                    if p.connected {
                        "Connected"
                    } else {
                        "Not connected"
                    }
                );
                if let Some(b) = p.battery.filter(|b| *b <= 100) {
                    sub.push_str(&format!(" · {b}%"));
                }
                if widgets::radio_row(ui, &pal, selected, &p.name, &sub).clicked() {
                    s.preferred_phone = Some(p.address.clone());
                }
            }
        });
    }

    fn background(&mut self, ui: &mut Ui, cx: &Cx, s: &mut Settings) {
        let pal = cx.pal;
        theme::section(ui, &pal, "Background");
        theme::card(ui, &pal, |ui| {
            toggle_row(
                ui,
                &pal,
                "Keep running when the window is closed",
                "Closing minimises the window so calls and texts still arrive. Quit with Ctrl+Q.",
                &mut s.keep_running_in_tray,
            );
            widgets::card_divider(ui, &pal);
            toggle_row(
                ui,
                &pal,
                "Start minimised",
                "Open in the background when you log in.",
                &mut s.start_minimised,
            );
        });
    }

    fn updates(&mut self, ui: &mut Ui, cx: &Cx, s: &mut Settings) {
        use crate::updater::{UpdateStatus, CURRENT_VERSION};
        let pal = cx.pal;
        theme::section(ui, &pal, "Updates");
        theme::card(ui, &pal, |ui| {
            toggle_row(
                ui,
                &pal,
                "Check for updates",
                "Looks on GitHub about once a day. This is the only time the app uses the network.",
                &mut s.check_updates,
            );
            widgets::card_divider(ui, &pal);
            let status = cx.updater.status();
            let line = match &status {
                UpdateStatus::Idle => "Not checked yet.".to_string(),
                UpdateStatus::Checking => "Checking…".to_string(),
                UpdateStatus::UpToDate => "You have the latest version.".to_string(),
                UpdateStatus::Available(u) => format!("Version {} is available.", u.version),
                UpdateStatus::Installing(v) => format!("Installing version {v}…"),
                UpdateStatus::Installed(v) => format!("Installed version {v}; restarting."),
                UpdateStatus::Failed(e) => format!("Could not check: {e}"),
                UpdateStatus::ManagedElsewhere => {
                    "Installed by a package manager: update it there.".to_string()
                }
            };
            widgets::setting_row(
                ui,
                &pal,
                &format!("Version {CURRENT_VERSION}"),
                &line,
                |ui| {
                    let busy = matches!(
                        status,
                        UpdateStatus::Checking
                            | UpdateStatus::Installing(_)
                            | UpdateStatus::Installed(_)
                    );
                    let (label, available) = match &status {
                        UpdateStatus::Available(_) => ("Update now", true),
                        _ => ("Check now", false),
                    };
                    if theme::pill(
                        ui,
                        &pal,
                        label,
                        ButtonKind::Plain,
                        !busy,
                        PillSize::Small,
                        0.0,
                    )
                    .clicked()
                    {
                        if available {
                            cx.updater.install();
                        } else {
                            cx.updater.check_now();
                        }
                    }
                },
            );
        });
    }

    fn data(&mut self, ui: &mut Ui, cx: &Cx, s: &mut Settings) {
        let pal = cx.pal;
        theme::section(ui, &pal, "Contacts and messages");
        theme::card(ui, &pal, |ui| {
            toggle_row(
                ui,
                &pal,
                "Load contacts when the phone connects",
                "Reads your phone's contacts so names show instead of numbers.",
                &mut s.load_contacts_on_connect,
            );
            widgets::card_divider(ui, &pal);
            widgets::setting_row(
                ui,
                &pal,
                "Check for new texts every",
                "Shorter is quicker, longer saves your phone's battery.",
                |ui| {
                    widgets::stepper(ui, &pal, &mut s.inbox_poll_secs, 2, 60, "s");
                },
            );
            widgets::card_divider(ui, &pal);
            widgets::setting_row(
                ui,
                &pal,
                "Contacts",
                "Read them from the phone again now.",
                |ui| {
                    if theme::pill(
                        ui,
                        &pal,
                        "Reload contacts",
                        ButtonKind::Plain,
                        cx.live.phone_connected(),
                        PillSize::Regular,
                        0.0,
                    )
                    .clicked()
                    {
                        cx.handle.send(Command::ReloadContacts);
                    }
                },
            );
        });
    }

    fn diagnostics(&mut self, ui: &mut Ui, cx: &Cx) {
        let pal = cx.pal;
        theme::section(ui, &pal, "Diagnostics");
        theme::card(ui, &pal, |ui| {
            let live = cx.live;
            let phone = live.phone.as_ref().map_or("none".to_string(), |p| {
                format!("{} ({})", p.name, p.address)
            });
            kv(ui, &pal, "Phone", &phone);
            kv(ui, &pal, "Messages link", &link_text(&live.messages_status));
            kv(ui, &pal, "Contacts link", &link_text(&live.contacts_status));
            kv(ui, &pal, "Contacts", &live.contact_count.to_string());
            kv(
                ui,
                &pal,
                "Conversations",
                &format!(
                    "{} ({} texts)",
                    cx.data.conversations.len(),
                    cx.data.message_count
                ),
            );
            kv(ui, &pal, "Recent calls", &live.recent_count.to_string());
            kv(ui, &pal, "Paired phones", &live.phones.len().to_string());
            ui.add_space(S2);
            ui.horizontal(|ui| {
                if theme::pill(
                    ui,
                    &pal,
                    "Retry message link",
                    ButtonKind::Plain,
                    live.phone_connected(),
                    PillSize::Regular,
                    0.0,
                )
                .clicked()
                {
                    cx.handle.retry_messages();
                }
            });
        });
    }

    fn about(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let pal = cx.pal;
        theme::section(ui, &pal, "About");
        theme::card(ui, &pal, |ui| {
            ui.horizontal(|ui| {
                nothing::dot_text(
                    ui,
                    &pal,
                    "GEEKBRIDGE",
                    nothing::PITCH_LABEL + 0.4,
                    pal.text,
                    false,
                );
                ui.label(theme::semibold("Phone", 16.0));
                ui.label(theme::caption(&pal, format!("Version {VERSION}")));
            });
            ui.add_space(S2);
            ui.add(egui::Label::new(theme::secondary(&pal, "Make and take calls and texts from your computer, over Bluetooth, with no app on the phone.")).wrap());
            ui.add_space(S3);
            ui.label(theme::caption(
                &pal,
                "Licence: GNU General Public License v3.0 or later.",
            ));
            ui.add_space(4.0);
            ui.add(egui::Label::new(theme::caption(&pal, "Credits: a from-scratch Rust app. The look (palette, dot-matrix type, controls) comes from the GeekBridge design system. Phone messages and contacts are read with the OBEX MAP and PBAP protocols.")).wrap());
            ui.add_space(S3);
            ui.horizontal(|ui| {
                if theme::pill(
                    ui,
                    &pal,
                    "Quit GeekBridge Phone",
                    ButtonKind::Primary,
                    true,
                    PillSize::Regular,
                    0.0,
                )
                .clicked()
                {
                    cx.cmds.push(Cmd::Quit);
                }
                ui.label(theme::caption(&pal, "Ctrl+Q"));
            });
        });
    }
}

fn toggle_row(ui: &mut Ui, pal: &theme::Palette, label: &str, hint: &str, value: &mut bool) {
    widgets::setting_row(ui, pal, label, hint, |ui| {
        widgets::toggle(ui, pal, value, label);
    });
}

fn kv(ui: &mut Ui, pal: &theme::Palette, key: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.add_sized(
            vec2(130.0, 20.0),
            egui::Label::new(theme::secondary(pal, key)),
        );
        ui.add(egui::Label::new(RichText::new(value).size(13.0)).wrap());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn our_own_change_shows_until_the_service_echoes_it() {
        let live = Settings::default();
        let mine = Settings {
            notify_texts: false,
            ..Settings::default()
        };
        let pending = Some((mine.clone(), Instant::now()));
        assert_eq!(displayed(&live, &pending), mine);
        // Once the service reports the same thing there is nothing left to override.
        assert_eq!(displayed(&mine, &pending), mine);
    }

    #[test]
    fn an_old_change_stops_overriding() {
        let live = Settings::default();
        let mine = Settings {
            notify_texts: false,
            ..Settings::default()
        };
        let old = Instant::now()
            .checked_sub(PENDING_FOR + Duration::from_millis(100))
            .unwrap_or_else(Instant::now);
        assert_eq!(displayed(&live, &Some((mine, old))), live);
        assert_eq!(displayed(&live, &None), live);
    }
}
