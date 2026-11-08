// SPDX-License-Identifier: GPL-3.0-or-later
//! Closing, minimising and bringing the window back.
//!
//! There is no tray on every desktop, so "keep running in the background" means *minimised*: the
//! window stays in the taskbar and launching the app again brings it back. The decisions are plain
//! functions; the viewport commands are the only egui in here.

use eframe::egui::{self, UserAttentionType, ViewportCommand};

/// What to do when the user closes the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseAction {
    /// Let the window close and the app exit.
    Quit,
    /// Cancel the close and minimise instead.
    Minimise,
}

/// Closing minimises when the app is set to keep running, unless it was asked to quit
/// (Ctrl+Q, the menu, or a second launch asking for it).
pub fn on_close_requested(keep_running: bool, quitting: bool) -> CloseAction {
    if keep_running && !quitting {
        CloseAction::Minimise
    } else {
        CloseAction::Quit
    }
}

/// Brings a minimised window back from outside the window's own event loop (a minimised window
/// may not be redrawing). Runs on a helper thread; failures are only logged.
///
/// Wayland: the compositor's window-management protocol (see `raise_wayland`). Other systems
/// restore the window from the UI path in [`bring_forward`], where minimised windows still draw.
pub fn raise_in_background(app_id: &str) {
    #[cfg(target_os = "linux")]
    {
        let app_id = app_id.to_string();
        let spawned = std::thread::Builder::new().name("gbp-raise".into()).spawn(move || match crate::raise_wayland::raise(&app_id) {
            Ok(()) => log::debug!("window raised"),
            Err(e) => log::debug!("window not raised through the compositor: {e:?}"),
        });
        if let Err(e) = spawned {
            log::debug!("cannot raise the window: {e}");
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = app_id;
}

/// Cancels the close and minimises the window.
pub fn minimise(ctx: &egui::Context) {
    ctx.send_viewport_cmd(ViewportCommand::CancelClose);
    ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
}

/// Un-minimises, shows and focuses the window. Some desktops (Wayland compositors among them)
/// decide for themselves whether a program may take focus; then the taskbar entry flashes
/// instead, which is what asking for attention does.
pub fn bring_forward(ctx: &egui::Context) {
    ctx.send_viewport_cmd(ViewportCommand::Visible(true));
    // Wayland ignores this (and says so in the log); there the compositor path does the work.
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
    }
    ctx.send_viewport_cmd(ViewportCommand::Focus);
    ctx.send_viewport_cmd(ViewportCommand::RequestUserAttention(UserAttentionType::Informational));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_minimises_only_while_keep_running_is_on() {
        assert_eq!(on_close_requested(true, false), CloseAction::Minimise);
        assert_eq!(on_close_requested(false, false), CloseAction::Quit);
    }

    #[test]
    fn an_explicit_quit_always_quits() {
        assert_eq!(on_close_requested(true, true), CloseAction::Quit);
        assert_eq!(on_close_requested(false, true), CloseAction::Quit);
    }
}
