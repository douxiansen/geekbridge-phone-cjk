// SPDX-License-Identifier: GPL-3.0-or-later
//! GeekBridge Phone desktop app: a Bluetooth phone dialler and SMS client for Android phones with
//! nothing installed on the phone.
//!
//! The program is `geekbridge-phone` (see `main.rs`). This library exists so the embedding
//! program, and the tests, can reach the seams:
//!
//! * [`notify`]: the [`notify::Notifier`] trait the event loop calls to show alerts, and the
//!   [`notify::NotifierHost`] a notifier uses to act on them.
//! * [`action`]: [`action::UiAction`], requests from outside the window (second launch, alerts).
//! * [`instance`]: one app per user.
//! * [`run`] and [`RunOptions`].
//!
//! Layout: `model` is plain logic with tests, `ui` is the visual kit, `views` are the screens,
//! `app` is the shell; `pump` is the event loop that runs while the window is hidden.

pub mod action;
pub mod app;
pub mod banners;
pub mod boot;
pub mod chrome;
pub mod data;
#[cfg(feature = "demo")]
pub mod demo;
pub mod icon;
pub mod instance;
pub mod model;
pub mod notify;
#[cfg(target_os = "linux")]
pub mod notify_freedesktop;
#[cfg(target_os = "linux")]
pub mod overlay_notifier;
pub mod platform_ui;
pub mod pump;
#[cfg(target_os = "linux")]
pub mod raise_wayland;
pub mod shared;
pub mod state;
pub mod ui;
pub mod updater;
pub mod views;
pub mod window;

use crate::app::{App, Boot};
use crate::instance::{Request, Role};
use crate::model::tab::Tab;
use crate::notify::NotifierFactory;
use eframe::egui;
use phone_core::settings::{Settings, ThemePref};
use phone_core::store::{self, Paths};

/// The desktop identity of the window: matches `StartupWMClass` in the `.desktop` file.
pub const APP_ID: &str = "dev.geekbridge.Phone";
/// The demo's own identity, so it can never be mistaken for the real app.
pub const DEMO_APP_ID: &str = "dev.geekbridge.Phone.Demo";

/// How to start the app.
#[derive(Default)]
pub struct RunOptions {
    /// Run against the fake phone with a throwaway data folder (needs the `demo` feature).
    pub demo: bool,
    /// Alerts: supply the overlay here. `None` uses the built-in notifier (freedesktop
    /// notifications on Linux).
    pub notifier: Option<NotifierFactory>,
    /// What a second launch asks the running instance to do.
    pub request: Option<Request>,
    /// Start on this tab.
    pub tab: Option<Tab>,
    /// Force a theme for this run, without saving it.
    pub theme: Option<ThemePref>,
    /// Initial window size, for screenshots.
    pub size: Option<[f32; 2]>,
    /// Demo only: put the fake phone in a situation at start-up.
    #[cfg(feature = "demo")]
    pub scene: Option<demo::Scene>,
    /// Demo only: visit every screen in turn (for screenshots).
    #[cfg(feature = "demo")]
    pub tour: bool,
    /// Demo only: post alerts on the real desktop instead of writing them to the log.
    pub demo_real_alerts: bool,
    /// Start minimised whatever the setting says.
    pub minimised: bool,
    /// Scale the whole interface, for checking narrow layouts on a wide screen.
    pub zoom: Option<f32>,
}

/// Runs the app until the window is closed. Returns an error text when it cannot start.
pub fn run(opts: RunOptions) -> Result<(), String> {
    #[cfg(feature = "demo")]
    let demo_dir = if opts.demo {
        Some(demo::DemoDir::create().map_err(|e| format!("cannot create the demo folder: {e}"))?)
    } else {
        None
    };
    #[cfg(not(feature = "demo"))]
    if opts.demo {
        return Err("this build has no demo mode".into());
    }

    #[cfg(feature = "demo")]
    let paths = match &demo_dir {
        Some(d) => Paths::under(d.path()),
        None => Paths::standard(),
    };
    #[cfg(not(feature = "demo"))]
    let paths = Paths::standard();

    // One app per user. The demo has its own socket so it never meets the real app.
    let _ = std::fs::create_dir_all(&paths.data);
    let name = if opts.demo {
        "geekbridge-phone-demo"
    } else {
        "geekbridge-phone"
    };
    let request = opts.request.clone().unwrap_or(Request::Show);
    let guard = match instance::acquire(&instance::socket_dir(paths.data.clone()), name, &request) {
        Role::Primary(guard) => Some(guard),
        Role::Secondary => return Ok(()),
        Role::Unresponsive(why) => {
            return Err(format!(
                "GeekBridge Phone seems to be running already but does not answer ({why})."
            ))
        }
    };

    let settings: Settings =
        store::load::<Settings>(&paths.config.join("settings.json")).sanitised();
    let start_minimised = opts.minimised || settings.start_minimised;

    #[cfg(feature = "demo")]
    let (platform, demo_control): (
        std::sync::Arc<dyn phone_core::platform::Platform>,
        Option<demo::DemoControl>,
    ) = if opts.demo {
        let p = phone_core::demo::DemoPlatform::new();
        let control = demo::DemoControl::new(p.clone());
        if let Some(scene) = opts.scene {
            control.prepare(scene);
        }
        (p, Some(control))
    } else {
        (boot::live_platform(), None)
    };
    #[cfg(not(feature = "demo"))]
    let platform = boot::live_platform();

    let app_id = if opts.demo { DEMO_APP_ID } else { APP_ID };
    let [w, h] = opts.size.unwrap_or([940.0, 660.0]);
    let viewport = egui::ViewportBuilder::default()
        .with_title("GeekBridge Phone")
        .with_app_id(app_id)
        .with_inner_size([w, h])
        .with_min_inner_size([720.0, 520.0])
        .with_icon(icon::app_icon());
    let native = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    let mut boot = Some(Boot {
        paths,
        platform,
        notifier: opts.notifier,
        guard,
        tab: opts.tab,
        theme: opts.theme,
        start_minimised,
        app_id,
        zoom: opts.zoom,
        initial: match &opts.request {
            Some(Request::Open(n)) => {
                Some(action::UiAction::OpenConversation { number: n.clone() })
            }
            Some(Request::Dial(n)) => Some(action::UiAction::PrefillDial { number: n.clone() }),
            _ => None,
        },
        #[cfg(feature = "demo")]
        demo: demo_control,
        #[cfg(feature = "demo")]
        demo_scene: opts.scene,
        #[cfg(feature = "demo")]
        demo_tour: opts.tour,
        demo_real_alerts: opts.demo_real_alerts,
        #[cfg(feature = "demo")]
        demo_dir,
    });
    eframe::run_native(
        "GeekBridge Phone",
        native,
        Box::new(move |cc| match boot.take() {
            Some(b) => Ok(Box::new(App::new(cc, b))),
            None => Err("the window was created twice".into()),
        }),
    )
    .map_err(|e| e.to_string())
}
