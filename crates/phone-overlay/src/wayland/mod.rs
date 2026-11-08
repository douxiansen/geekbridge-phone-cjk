// SPDX-License-Identifier: GPL-3.0-or-later

//! Linux presenter: a `zwlr_layer_shell_v1` client in the top-right corner.
//!
//! One thread owns the Wayland connection and a calloop event loop. It sleeps
//! in `poll` until a command, a Wayland event or the next timed wake-up
//! arrives; with nothing on screen there is no layer surface and no wake-up
//! at all. Rendering is the platform-neutral core's CPU renderer; this
//! module only moves its pixels into `wl_shm` buffers and its input back.

mod app;
mod keys;
mod portal;

use std::fmt::Debug;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Mutex;

use smithay_client_toolkit::reexports::calloop::channel::{self, Event};
use smithay_client_toolkit::reexports::calloop::{ping, EventLoop};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::globals::registry_queue_init;
use smithay_client_toolkit::reexports::client::{ConnectError, Connection};

use self::app::{App, BindFailure};
use self::portal::SchemeCell;
use crate::{Action, Command, OverlayConfig, OverlayError, OverlayHandle, ThemePref};

/// Set `PHONE_OVERLAY_DEBUG=1` to print scale and theme decisions to stderr.
fn debug_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("PHONE_OVERLAY_DEBUG").is_some())
}

fn presenter_err<E: Debug>(e: E) -> OverlayError {
    OverlayError::Presenter(format!("{e:?}"))
}

struct Presenter {
    conn: Connection,
    event_loop: EventLoop<'static, App>,
    app: App,
}

impl Presenter {
    fn init(
        config: &OverlayConfig,
        actions: Sender<Action>,
    ) -> Result<(Self, channel::Sender<Command>), OverlayError> {
        let conn = Connection::connect_to_env().map_err(|e| match e {
            ConnectError::NoWaylandLib | ConnectError::NoCompositor => OverlayError::Unsupported,
            other => presenter_err(other),
        })?;
        let (globals, event_queue) = registry_queue_init::<App>(&conn).map_err(presenter_err)?;
        let qh = event_queue.handle();
        let event_loop: EventLoop<'static, App> = EventLoop::try_new().map_err(presenter_err)?;
        let handle = event_loop.handle();

        let (ping_tx, ping_src) = ping::make_ping().map_err(presenter_err)?;
        let scheme = if config.theme == ThemePref::System {
            portal::watch(ping_tx)
        } else {
            SchemeCell::default()
        };

        let app = App::new(&globals, &qh, handle.clone(), config, scheme, actions).map_err(
            |f| match f {
                BindFailure::Missing => OverlayError::Unsupported,
                BindFailure::Other(s) => OverlayError::Presenter(s),
            },
        )?;

        WaylandSource::new(conn.clone(), event_queue)
            .insert(handle.clone())
            .map_err(presenter_err)?;
        let (cmd_tx, cmd_rx) = channel::channel::<Command>();
        handle
            .insert_source(cmd_rx, |event, _, app: &mut App| match event {
                Event::Msg(cmd) => app.on_command(cmd),
                Event::Closed => app.exit = true,
            })
            .map_err(presenter_err)?;
        handle
            .insert_source(ping_src, |_, _, app: &mut App| app.on_theme_changed())
            .map_err(presenter_err)?;

        Ok((
            Self {
                conn,
                event_loop,
                app,
            },
            cmd_tx,
        ))
    }

    fn run(mut self) {
        loop {
            let timeout = self.app.timeout();
            if self.event_loop.dispatch(timeout, &mut self.app).is_err() {
                break;
            }
            self.app.after_dispatch();
            if self.conn.flush().is_err() || self.app.exit {
                break;
            }
        }
    }
}

/// Start the Wayland presenter.
///
/// # Errors
///
/// [`OverlayError::Unsupported`] without a Wayland session or without
/// `zwlr_layer_shell_v1`; [`OverlayError::Presenter`] for other failures.
pub fn spawn(config: OverlayConfig) -> Result<(OverlayHandle, Receiver<Action>), OverlayError> {
    // Font discovery takes a moment on systems with many fonts: do it in the
    // background so the first banner appears immediately.
    let _ = std::thread::Builder::new()
        .name("phone-overlay-fonts".into())
        .spawn(crate::text::warm_up);
    let (act_tx, act_rx) = mpsc::channel();
    let (init_tx, init_rx) =
        mpsc::sync_channel::<Result<channel::Sender<Command>, OverlayError>>(1);
    std::thread::Builder::new()
        .name("phone-overlay".into())
        .spawn(move || match Presenter::init(&config, act_tx) {
            Ok((presenter, cmd_tx)) => {
                if init_tx.send(Ok(cmd_tx)).is_ok() {
                    presenter.run();
                }
            }
            Err(e) => {
                let _ = init_tx.send(Err(e));
            }
        })
        .map_err(presenter_err)?;
    let cmd_tx = init_rx
        .recv()
        .map_err(|_| OverlayError::Presenter("presenter thread exited during start-up".into()))??;
    let cmd_tx = Mutex::new(cmd_tx);
    let handle = OverlayHandle::from_sink(move |cmd| {
        if let Ok(tx) = cmd_tx.lock() {
            let _ = tx.send(cmd);
        }
    });
    Ok((handle, act_rx))
}
