// SPDX-License-Identifier: GPL-3.0-or-later
//! The application shell: owns the service, the event thread and every screen, and lays out the
//! window (sidebar, toolbar, banners, call cards, the current screen).

use crate::action::{ActionSender, UiAction};
use crate::banners::{self, Notices};
use crate::chrome;
use crate::data::{Live, ViewData};
use crate::instance::Guard;
use crate::model::format::{local_offset, unix_now};
use crate::model::status::{self, lcd_text, phone_chip, problems};
use crate::model::tab::Tab;
use crate::notify::{self, LogNotifier, NotifierFactory, NotifierHost};
use crate::pump;
use crate::shared::Shared;
use crate::state::{UiState, UiStore};
use crate::ui::theme;
use crate::views::{call_card, contacts::ContactsState, keypad::KeypadState, messages::MessagesState, recents::RecentsState, settings::SettingsState, Cmd, Cx};
use crate::window::{self, CloseAction};
use eframe::egui::{self, Frame, Key, Margin, Modifiers, ThemePreference, ViewportCommand};
use phone_core::platform::Platform;
use phone_core::service::{Command, Handle, Options, Service};
use phone_core::settings::ThemePref;
use phone_core::store::Paths;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Everything the shell needs to start.
pub struct Boot {
    pub paths: Paths,
    pub platform: Arc<dyn Platform>,
    pub notifier: Option<NotifierFactory>,
    /// Holds the single-instance socket for as long as the app runs.
    pub guard: Option<Guard>,
    /// Start on this tab instead of the remembered one.
    pub tab: Option<Tab>,
    /// Use this theme instead of the setting (not saved).
    pub theme: Option<ThemePref>,
    /// Scale the interface (screenshots of narrow layouts).
    pub zoom: Option<f32>,
    /// The window's desktop identity, to bring it back when minimised.
    pub app_id: &'static str,
    /// Minimise the window as soon as it exists.
    pub start_minimised: bool,
    /// An action to apply once the window is up (`--open NUMBER`).
    pub initial: Option<UiAction>,
    #[cfg(feature = "demo")]
    pub demo: Option<crate::demo::DemoControl>,
    #[cfg(feature = "demo")]
    pub demo_scene: Option<crate::demo::Scene>,
    /// Demo: visit every screen in turn, a few seconds each.
    #[cfg(feature = "demo")]
    pub demo_tour: bool,
    /// Keep the demo's alerts on the real desktop instead of in the log.
    pub demo_real_alerts: bool,
    /// Removes the demo data folder on exit.
    #[cfg(feature = "demo")]
    pub demo_dir: Option<crate::demo::DemoDir>,
}

pub struct App {
    handle: Handle,
    service: Option<Service>,
    shared: Arc<Shared>,
    actions: Receiver<UiAction>,
    pump: Option<JoinHandle<()>>,
    _guard: Option<Guard>,
    data: ViewData,
    ui_state: UiState,
    saved_state: UiState,
    ui_store: UiStore,
    tab: Tab,
    keypad: KeypadState,
    contacts: ContactsState,
    recents: RecentsState,
    messages: MessagesState,
    settings: SettingsState,
    notices: Notices,
    quitting: bool,
    pending_minimise: bool,
    theme_override: Option<ThemePref>,
    applied_theme: Option<ThemePref>,
    #[cfg(feature = "demo")]
    demo: Option<crate::demo::DemoControl>,
    /// Demo screenshots: moves through the screens by itself.
    #[cfg(feature = "demo")]
    tour: Option<(Instant, usize)>,
    #[cfg(feature = "demo")]
    _demo_dir: Option<crate::demo::DemoDir>,
}

fn theme_preference(pref: ThemePref) -> ThemePreference {
    match pref {
        ThemePref::System => ThemePreference::System,
        ThemePref::Dark => ThemePreference::Dark,
        ThemePref::Light => ThemePreference::Light,
    }
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, boot: Boot) -> Self {
        let ctx = cc.egui_ctx.clone();
        theme::apply(&ctx);
        if let Some(z) = boot.zoom {
            ctx.set_zoom_factor(z.clamp(0.5, 3.0));
        }
        let wake: Arc<dyn Fn() + Send + Sync> = {
            let ctx = ctx.clone();
            Arc::new(move || ctx.request_repaint())
        };
        let mut options = Options::new(boot.paths.clone());
        options.wake = Some(wake.clone());
        let (service, events) = Service::start(boot.platform, options);
        let handle = service.handle();
        let shared = Arc::new(Shared::default());
        let app_id = boot.app_id;
        let (actions_tx, actions) = ActionSender::new(wake.clone());
        let actions_tx = actions_tx.with_raise(Arc::new(move || window::raise_in_background(app_id)));
        let mut guard = boot.guard;
        if let Some(g) = guard.as_mut() {
            g.serve(actions_tx.clone());
        }

        if let Some(action) = boot.initial {
            actions_tx.send(action);
        }
        let host = NotifierHost { handle: handle.clone(), actions: actions_tx };
        #[allow(unused_mut)]
        let mut quiet_demo = false;
        #[cfg(feature = "demo")]
        {
            quiet_demo = boot.demo.is_some() && !boot.demo_real_alerts;
        }
        let notifier = match boot.notifier {
            Some(factory) => factory(host),
            None if quiet_demo => Box::new(LogNotifier),
            None => notify::default_notifier(host),
        };
        let pump = match pump::spawn(events, handle.clone(), shared.clone(), notifier, wake) {
            Ok(h) => Some(h),
            Err(e) => {
                log::error!("cannot start the event thread: {e}");
                None
            }
        };

        #[cfg(feature = "demo")]
        if let (Some(demo), Some(scene)) = (&boot.demo, boot.demo_scene) {
            demo.play(scene, handle.clone());
        }
        #[cfg(feature = "demo")]
        if let (Some(demo), true) = (&boot.demo, boot.demo_tour) {
            demo.seed_history(handle.clone());
        }

        let ui_store = UiStore::new(&boot.paths.config);
        let ui_state = ui_store.load();
        let tab = boot.tab.unwrap_or(ui_state.tab);
        Self {
            handle,
            service: Some(service),
            shared,
            actions,
            pump,
            _guard: guard,
            data: ViewData::default(),
            saved_state: ui_state.clone(),
            ui_state,
            ui_store,
            tab,
            keypad: KeypadState::default(),
            contacts: ContactsState::default(),
            recents: RecentsState::default(),
            messages: MessagesState::default(),
            settings: SettingsState::default(),
            notices: Notices::default(),
            quitting: false,
            pending_minimise: boot.start_minimised,
            theme_override: boot.theme,
            applied_theme: None,
            #[cfg(feature = "demo")]
            demo: boot.demo,
            #[cfg(feature = "demo")]
            tour: boot.demo_tour.then(|| (Instant::now(), 0)),
            #[cfg(feature = "demo")]
            _demo_dir: boot.demo_dir,
        }
    }

    fn goto(&mut self, tab: Tab) {
        if self.tab != tab {
            self.tab = tab;
            self.ui_state.tab = tab;
        }
    }

    fn quit(&mut self, ctx: &egui::Context) {
        self.quitting = true;
        ctx.send_viewport_cmd(ViewportCommand::Close);
    }

    /// Requests from outside the window.
    fn apply_actions(&mut self, ctx: &egui::Context) {
        while let Ok(action) = self.actions.try_recv() {
            match action {
                UiAction::Show => window::bring_forward(ctx),
                UiAction::OpenConversation { number } => {
                    self.goto(Tab::Messages);
                    self.messages.open_conversation(&number);
                    window::bring_forward(ctx);
                }
                UiAction::OpenTab(tab) => {
                    self.goto(tab);
                    window::bring_forward(ctx);
                }
                UiAction::PrefillDial { number } => {
                    self.goto(Tab::Keypad);
                    self.keypad.set_number(&number);
                    window::bring_forward(ctx);
                }
                UiAction::Quit => self.quit(ctx),
            }
        }
    }

    fn apply_theme(&mut self, ctx: &egui::Context, live: &Live) {
        let pref = self.theme_override.unwrap_or(live.settings.theme);
        if self.applied_theme != Some(pref) {
            ctx.set_theme(theme_preference(pref));
            self.applied_theme = Some(pref);
            ctx.request_repaint();
        }
    }

    /// Shortcuts and keys that work on every screen.
    fn shortcuts(&mut self, ctx: &egui::Context, live: &Live) {
        const NUMBERS: [Key; 5] = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5];
        for (i, key) in NUMBERS.into_iter().enumerate() {
            if ctx.input_mut(|inp| inp.consume_key(Modifiers::COMMAND, key)) {
                if let Some(tab) = Tab::from_number(i + 1) {
                    self.goto(tab);
                }
            }
        }
        if ctx.input_mut(|inp| inp.consume_key(Modifiers::COMMAND, Key::N)) {
            self.goto(Tab::Messages);
            self.messages.new_message(None);
        }
        if ctx.input_mut(|inp| inp.consume_key(Modifiers::COMMAND, Key::F)) {
            self.goto(Tab::Contacts);
            self.contacts.focus_search(ctx);
        }
        if ctx.input_mut(|inp| inp.consume_key(Modifiers::COMMAND, Key::Q)) {
            self.quit(ctx);
        }
        // Enter answers a ringing call when nothing else is using the key.
        if let Some(ringing) = live.ringing() {
            if !ctx.wants_keyboard_input() && ctx.input_mut(|inp| inp.consume_key(Modifiers::NONE, Key::Enter)) {
                self.handle.answer(&ringing.call.id);
            }
        }
        // Escape closes the innermost thing the current screen has open.
        if ctx.input(|inp| inp.key_pressed(Key::Escape)) {
            let handled = match self.tab {
                Tab::Contacts => self.contacts.escape(),
                Tab::Recents => self.recents.escape(),
                Tab::Messages => self.messages.escape(),
                Tab::Keypad | Tab::Settings => false,
            };
            if handled {
                ctx.input_mut(|inp| inp.consume_key(Modifiers::NONE, Key::Escape));
            }
        }
    }

    /// Steps the screenshot tour: every few seconds, the next screen.
    #[cfg(feature = "demo")]
    fn tour_step(&mut self, ctx: &egui::Context) {
        let Some((since, step)) = self.tour else { return };
        let secs: u64 = std::env::var("GBP_TOUR_STEP_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(3);
        // The first step waits for the demo phone's data to arrive.
        if since.elapsed() < Duration::from_secs(secs * (step as u64 + 1)) {
            ctx.request_repaint_after(Duration::from_millis(300));
            return;
        }
        let known = crate::demo::KNOWN_NUMBER;
        match step {
            0 => {
                self.goto(Tab::Keypad);
                self.keypad.set_number("+21355012");
            }
            1 => self.goto(Tab::Contacts),
            2 => self.contacts.open("Mara Ionescu", "+40722123456"),
            3 => {
                self.goto(Tab::Messages);
                self.messages.escape();
            }
            4 => {
                self.goto(Tab::Messages);
                self.messages.open_conversation(known);
            }
            5 => {
                self.goto(Tab::Messages);
                self.messages.new_message(None);
            }
            6 => self.goto(Tab::Settings),
            7 => self.goto(Tab::Recents),
            _ => {
                self.tour = None;
                return;
            }
        }
        self.tour = Some((since, step + 1));
        ctx.request_repaint_after(Duration::from_millis(300));
    }

    #[cfg(feature = "demo")]
    fn demo_menu(&mut self, ctx: &egui::Context) {
        use crate::demo::{KNOWN_NUMBER, UNKNOWN_NUMBER};
        let Some(demo) = self.demo.clone() else { return };
        let pal = theme::current(ctx);
        egui::TopBottomPanel::top("demo-menu").exact_height(26.0).frame(Frame::new().fill(pal.toolbar).inner_margin(Margin::symmetric(8, 0))).show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("Help", |ui| {
                    if ui.button("Simulate incoming call (contact)").clicked() {
                        demo.ring(KNOWN_NUMBER);
                        ui.close_menu();
                    }
                    if ui.button("Simulate incoming call (unknown number)").clicked() {
                        demo.ring(UNKNOWN_NUMBER);
                        ui.close_menu();
                    }
                    if ui.button("Simulate missed call").clicked() {
                        demo.miss(KNOWN_NUMBER);
                        ui.close_menu();
                    }
                    if ui.button("End call").clicked() {
                        demo.end_call();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Simulate text (contact)").clicked() {
                        demo.text(KNOWN_NUMBER, "Table is ready, come whenever you like");
                        ui.close_menu();
                    }
                    if ui.button("Simulate text (unknown number)").clicked() {
                        demo.text(UNKNOWN_NUMBER, "Your verification code is 482913. It expires in 10 minutes.");
                        ui.close_menu();
                    }
                });
                ui.label(theme::caption(&pal, "Demo mode: a fake phone, nothing is real"));
            });
        });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now_instant = Instant::now();
        let live = Live::read(&self.handle);
        if std::mem::take(&mut self.pending_minimise) {
            ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
        }
        self.apply_theme(ctx, &live);
        self.apply_actions(ctx);
        self.data.refresh(&self.handle, &self.shared);
        for notice in self.shared.take_notices() {
            self.notices.push(notice, now_instant);
        }
        for failure in self.messages.apply_outcomes(self.shared.take_outcomes()) {
            self.notices.push(failure, now_instant);
        }

        // Closing minimises while the app keeps running.
        if ctx.input(|i| i.viewport().close_requested()) {
            match window::on_close_requested(live.settings.keep_running_in_tray, self.quitting) {
                CloseAction::Minimise => window::minimise(ctx),
                CloseAction::Quit => self.quitting = true,
            }
        }

        self.shortcuts(ctx, &live);
        let now = unix_now();
        let off = local_offset(now);
        let pal = theme::current(ctx);
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        self.shared.set_watching_messages(focused && self.tab == Tab::Messages);

        #[cfg(feature = "demo")]
        {
            self.tour_step(ctx);
            self.demo_menu(ctx);
        }

        let unread = self.ui_state.unread_total(&self.data.conversations);
        let chip = phone_chip(live.phone.as_ref(), &live.messages_status);
        if let Some(tab) = chrome::sidebar(ctx, &pal, self.tab, unread, &chip) {
            self.goto(tab);
        }
        let calls: Vec<_> = live.calls.iter().map(|c| c.call.clone()).collect();
        let lcd = lcd_text(live.phone.as_ref(), &live.messages_status, &calls);
        let alert = live.ringing().is_some();
        let tools = chrome::toolbar(ctx, &pal, self.tab, &lcd, alert, live.phone_connected());
        if tools.new_message {
            self.messages.new_message(None);
        }
        if tools.reload_contacts {
            self.handle.send(Command::ReloadContacts);
        }

        let active_problems = problems(live.phone.as_ref(), &live.messages_status, &live.contacts_status);
        self.notices.expire(now_instant, &active_problems);

        let mut cmds: Vec<Cmd> = Vec::new();
        egui::CentralPanel::default().frame(Frame::new().fill(pal.content).inner_margin(Margin::ZERO)).show(ctx, |ui| {
            banners::show(ui, &pal, &self.handle, &active_problems, &mut self.notices);
            let mut cx = Cx { pal, handle: &self.handle, data: &self.data, live: &live, now, off, cmds: &mut cmds };
            if !live.calls.is_empty() {
                Frame::new().inner_margin(Margin::symmetric(theme::S3 as i8, 0)).show(ui, |ui| {
                    ui.add_space(theme::S3);
                    for call in &live.calls {
                        call_card::show(ui, &mut cx, call, self.tab == Tab::Keypad);
                        ui.add_space(theme::S2);
                    }
                });
            }
            match self.tab {
                Tab::Keypad => self.keypad.show(ui, &mut cx),
                Tab::Contacts => self.contacts.show(ui, &mut cx),
                Tab::Recents => self.recents.show(ui, &mut cx),
                Tab::Messages => self.messages.show(ui, &mut cx, &mut self.ui_state),
                Tab::Settings => self.settings.show(ui, &mut cx),
            }
        });

        for cmd in cmds {
            match cmd {
                Cmd::Tab(tab) => self.goto(tab),
                Cmd::NewMessage { to } => {
                    self.goto(Tab::Messages);
                    self.messages.new_message(to.as_deref());
                }
                Cmd::OpenConversation(number) => {
                    self.goto(Tab::Messages);
                    self.messages.open_conversation(&number);
                }
                Cmd::Quit => self.quit(ctx),
            }
        }

        if self.ui_state != self.saved_state {
            self.ui_store.save(&self.ui_state);
            self.saved_state = self.ui_state.clone();
        }

        // Link statuses and timers change without an event: look again soon.
        let mut next = Duration::from_secs(1);
        if let Some(expiry) = self.notices.next_expiry(now_instant) {
            next = next.min(expiry + Duration::from_millis(50));
        }
        ctx.request_repaint_after(next);
        let _ = status::Dot::Green;
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.shared.stop();
        self.ui_store.save(&self.ui_state);
        if let Some(service) = self.service.take() {
            service.stop();
        }
        if let Some(pump) = self.pump.take() {
            let _ = pump.join();
        }
    }
}
