// SPDX-License-Identifier: GPL-3.0-or-later
//! The "Pair a phone" window: scans for phones in range, pairs with the one the user picks and
//! unpairs one that is paired. Only offered where the platform can pair from inside the app.
//! It opens by itself when no phone is paired at all.

use crate::data::Live;
use crate::ui::theme::{self, ButtonKind, Palette, PillSize, S2, S3};
use eframe::egui::{
    self, Align, Color32, CornerRadius, Frame, Id, Layout, Margin, RichText, ScrollArea, Stroke, Ui,
};
use phone_core::platform::{NearbyDevice, Platform};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

const MAX_WIDTH: f32 = 460.0;
/// How long the scan listens.
const SCAN_SECONDS: u32 = 15;
/// How long the app waits with no phone paired before opening the window by itself.
const AUTO_OPEN_AFTER: Duration = Duration::from_secs(4);

#[derive(Debug)]
enum Stage {
    Closed,
    Scanning,
    List(Vec<NearbyDevice>),
    Pairing { name: String, code: Option<String> },
    Unpairing { name: String },
    Done { name: String },
    Failed(String),
}

enum Msg {
    Found(Result<Vec<NearbyDevice>, String>),
    Code(Option<String>),
    Paired(Result<(), String>),
    Unpaired(Result<(), String>),
}

#[derive(Debug, PartialEq, Eq)]
enum Action {
    None,
    Rescan,
    Close,
    Pair(String, String),
    Unpair(String, String),
}

pub struct PairingState {
    stage: Stage,
    rx: Option<Receiver<Msg>>,
    /// The window already opened by itself this run.
    auto_opened: bool,
    /// Since when no phone has been paired.
    none_since: Option<Instant>,
}

impl Default for PairingState {
    fn default() -> Self {
        Self {
            stage: Stage::Closed,
            rx: None,
            auto_opened: false,
            none_since: None,
        }
    }
}

impl std::fmt::Debug for PairingState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PairingState({:?})", self.stage)
    }
}

impl PairingState {
    pub fn is_open(&self) -> bool {
        !matches!(self.stage, Stage::Closed)
    }

    /// Opens the window and starts scanning.
    pub fn open(&mut self, platform: &Arc<dyn Platform>, ctx: &egui::Context) {
        self.scan(platform, ctx);
    }

    /// Opens it by itself once, after no phone has been paired for a few seconds.
    pub fn auto_open(
        &mut self,
        platform: &Arc<dyn Platform>,
        ctx: &egui::Context,
        no_phone_paired: bool,
        in_call: bool,
        now: Instant,
    ) {
        if !platform.can_pair() || self.auto_opened || self.is_open() || in_call {
            return;
        }
        if !no_phone_paired {
            self.none_since = None;
            return;
        }
        let since = *self.none_since.get_or_insert(now);
        if now.saturating_duration_since(since) >= AUTO_OPEN_AFTER {
            self.auto_opened = true;
            self.open(platform, ctx);
        } else {
            ctx.request_repaint_after(AUTO_OPEN_AFTER);
        }
    }

    fn scan(&mut self, platform: &Arc<dyn Platform>, ctx: &egui::Context) {
        let (tx, rx) = channel();
        self.rx = Some(rx);
        self.stage = Stage::Scanning;
        let (platform, ctx) = (platform.clone(), ctx.clone());
        std::thread::spawn(move || {
            let r = platform
                .scan_nearby(SCAN_SECONDS)
                .map_err(|e| e.to_string());
            let _ = tx.send(Msg::Found(r));
            ctx.request_repaint();
        });
    }

    fn pair(
        &mut self,
        platform: &Arc<dyn Platform>,
        ctx: &egui::Context,
        id: String,
        name: String,
    ) {
        let (tx, rx) = channel();
        self.rx = Some(rx);
        self.stage = Stage::Pairing { name, code: None };
        let (platform, ctx) = (platform.clone(), ctx.clone());
        std::thread::spawn(move || {
            let (code_tx, code_ctx) = (tx.clone(), ctx.clone());
            let show: Arc<dyn Fn(Option<String>) + Send + Sync> = Arc::new(move |code| {
                let _ = code_tx.send(Msg::Code(code));
                code_ctx.request_repaint();
            });
            let r = platform.pair(&id, show).map_err(|e| e.to_string());
            let _ = tx.send(Msg::Paired(r));
            ctx.request_repaint();
        });
    }

    fn unpair(
        &mut self,
        platform: &Arc<dyn Platform>,
        ctx: &egui::Context,
        address: String,
        name: String,
    ) {
        let (tx, rx) = channel();
        self.rx = Some(rx);
        self.stage = Stage::Unpairing { name };
        let (platform, ctx) = (platform.clone(), ctx.clone());
        std::thread::spawn(move || {
            let r = platform.unpair(&address).map_err(|e| e.to_string());
            let _ = tx.send(Msg::Unpaired(r));
            ctx.request_repaint();
        });
    }

    fn poll(&mut self, platform: &Arc<dyn Platform>, ctx: &egui::Context) {
        let Some(rx) = &self.rx else { return };
        let mut msgs = Vec::new();
        loop {
            match rx.try_recv() {
                Ok(m) => msgs.push(m),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.rx = None;
                    break;
                }
            }
        }
        for m in msgs {
            match (m, &mut self.stage) {
                (Msg::Found(Ok(list)), Stage::Scanning) => self.stage = Stage::List(list),
                (Msg::Found(Err(e)), Stage::Scanning) => self.stage = Stage::Failed(e),
                (Msg::Code(code), Stage::Pairing { code: slot, .. }) => *slot = code,
                (Msg::Paired(Ok(())), Stage::Pairing { name, .. }) => {
                    self.stage = Stage::Done { name: name.clone() }
                }
                (Msg::Paired(Err(e)), Stage::Pairing { .. }) => self.stage = Stage::Failed(e),
                // After an unpair, look again so the phone can be paired afresh.
                (Msg::Unpaired(Ok(())), Stage::Unpairing { .. }) => self.scan(platform, ctx),
                (Msg::Unpaired(Err(e)), Stage::Unpairing { .. }) => self.stage = Stage::Failed(e),
                _ => {}
            }
        }
    }

    /// Draws the window when it is open.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        pal: &Palette,
        platform: &Arc<dyn Platform>,
        live: &Live,
    ) {
        self.poll(platform, ctx);
        if !self.is_open() {
            return;
        }
        let width = (ctx.screen_rect().width() - 48.0).clamp(280.0, MAX_WIDTH);
        let mut action = Action::None;
        let modal = egui::Modal::new(Id::new("pair-a-phone"))
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
                ui.label(RichText::new("Pair a phone").size(20.0).strong());
                ui.add_space(S2);
                match &self.stage {
                    Stage::Closed => {}
                    Stage::Scanning => scanning(ui, pal),
                    Stage::List(list) => action = list_view(ui, pal, list, live),
                    Stage::Pairing { name, code } => pairing(ui, pal, name, code.as_deref()),
                    Stage::Unpairing { name } => busy(ui, pal, &format!("Removing {name}\u{2026}")),
                    Stage::Done { name } => {
                        let apple = live.phones.iter().any(|p| p.apple && &p.name == name);
                        action = done(ui, pal, name, live.calls_ready, apple)
                    }
                    Stage::Failed(why) => action = failed(ui, pal, why),
                }
            });
        let working = matches!(
            self.stage,
            Stage::Scanning | Stage::Pairing { .. } | Stage::Unpairing { .. }
        ) || (matches!(self.stage, Stage::Done { .. }) && !live.calls_ready);
        if modal.should_close() && action == Action::None && !working {
            action = Action::Close;
        }
        match action {
            Action::None => {}
            Action::Rescan => self.scan(platform, ctx),
            Action::Close => {
                self.stage = Stage::Closed;
                self.rx = None;
            }
            Action::Pair(id, name) => self.pair(platform, ctx, id, name),
            Action::Unpair(address, name) => self.unpair(platform, ctx, address, name),
        }
        if working {
            ctx.request_repaint_after(Duration::from_millis(300));
        }
    }
}

fn busy(ui: &mut Ui, pal: &Palette, text: &str) {
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(theme::caption(pal, text));
    });
}

fn scanning(ui: &mut Ui, pal: &Palette) {
    busy(ui, pal, "Looking for phones nearby\u{2026}");
    ui.add_space(S2);
    ui.label(theme::caption(
        pal,
        "On your phone, open Settings > Bluetooth and keep that screen open, so the phone can be found.",
    ));
}

fn pairing(ui: &mut Ui, pal: &Palette, name: &str, code: Option<&str>) {
    busy(ui, pal, &format!("Pairing with {name}\u{2026}"));
    ui.add_space(S3);
    match code {
        Some(code) => {
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(code).size(34.0).monospace().strong());
            });
            ui.add_space(S2);
            ui.label(theme::caption(
                pal,
                "Your phone should show the same number. Check it, then tap Pair on the phone.",
            ));
        }
        None => {
            ui.label(theme::caption(
                pal,
                "Watch your phone: it will ask you to pair.",
            ));
        }
    }
}

fn list_view(ui: &mut Ui, pal: &Palette, list: &[NearbyDevice], live: &Live) -> Action {
    let mut action = Action::None;
    if list.is_empty() {
        ui.label(theme::caption(
            pal,
            "No phone found. Open Settings > Bluetooth on the phone, keep that screen open, and scan again.",
        ));
    } else {
        ui.label(theme::caption(pal, "Pick your phone:"));
        ScrollArea::vertical()
            .max_height(220.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for d in list {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&d.name).size(15.0));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if theme::pill(
                                ui,
                                pal,
                                "Pair",
                                ButtonKind::Inverse,
                                true,
                                PillSize::Regular,
                                70.0,
                            )
                            .clicked()
                            {
                                action = Action::Pair(d.id.clone(), d.name.clone());
                            }
                        });
                    });
                }
            });
    }
    if !live.phones.is_empty() {
        ui.add_space(S3);
        theme::hairline(ui, pal);
        ui.add_space(S2);
        ui.label(theme::caption(pal, "Paired phones:"));
        for p in &live.phones {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&p.name).size(15.0));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::pill(
                        ui,
                        pal,
                        "Unpair",
                        ButtonKind::Plain,
                        true,
                        PillSize::Regular,
                        0.0,
                    )
                    .clicked()
                    {
                        action = Action::Unpair(p.address.clone(), p.name.clone());
                    }
                });
            });
        }
    }
    ui.add_space(S3);
    ui.horizontal(|ui| {
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
                action = Action::Close;
            }
            if theme::pill(
                ui,
                pal,
                "Scan again",
                ButtonKind::Plain,
                true,
                PillSize::Regular,
                0.0,
            )
            .clicked()
            {
                action = Action::Rescan;
            }
        });
    });
    action
}

fn done(ui: &mut Ui, pal: &Palette, name: &str, calls_ready: bool, apple: bool) -> Action {
    ui.label(RichText::new(format!("Paired with {name}")).size(15.0));
    ui.add_space(S2);
    if apple {
        // An iPhone shares texts and contacts only after it is told to, and the setup guide that
        // opens next walks through it. Calls need nothing extra on the phone.
        ui.label(theme::caption(
            pal,
            "On the iPhone open Settings > Bluetooth, tap the i next to this computer, and switch on Show Notifications \
             and Sync Contacts. The setup guide shows each step as it comes true.",
        ));
    } else if calls_ready {
        ui.label(theme::caption(
            pal,
            "One more thing for calls: on the phone open Settings > Bluetooth, tap the gear next to this computer \
             and switch on Calls (call audio). Texts and contacts may also ask for permission on the phone: tap Allow.",
        ));
    } else {
        // The phone only keeps Calls switched on once this computer's call link is up.
        busy(ui, pal, "Setting up calls on this computer\u{2026}");
        ui.add_space(S2);
        ui.label(theme::caption(
            pal,
            "This takes a few seconds. Then this window tells you to switch on Calls on the phone.",
        ));
    }
    ui.add_space(S3);
    let mut action = Action::None;
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if theme::pill(
            ui,
            pal,
            "Done",
            ButtonKind::Inverse,
            true,
            PillSize::Regular,
            76.0,
        )
        .clicked()
        {
            action = Action::Close;
        }
    });
    action
}

fn failed(ui: &mut Ui, pal: &Palette, why: &str) -> Action {
    ui.label(RichText::new("That did not work").size(15.0));
    ui.add_space(S2);
    ui.label(theme::caption(pal, why));
    ui.add_space(S3);
    let mut action = Action::None;
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
            action = Action::Close;
        }
        if theme::pill(
            ui,
            pal,
            "Try again",
            ButtonKind::Plain,
            true,
            PillSize::Regular,
            0.0,
        )
        .clicked()
        {
            action = Action::Rescan;
        }
    });
    action
}
