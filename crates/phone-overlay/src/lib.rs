// SPDX-License-Identifier: GPL-3.0-or-later

//! macOS-style notification banners for GeekBridge Phone: incoming and active
//! calls with Answer / Decline / Hang up, and text messages with an inline
//! quick reply.
//!
//! The crate is split into a platform-neutral core and thin per-OS presenters:
//!
//! * **Core** (always built, no display needed): the banner [`model`], [`layout`]
//!   and hit-testing, the [`state`] machine with its animations, the
//!   single-line [`edit`] field, and a CPU [`render`]er that produces
//!   premultiplied RGBA [`tiny_skia::Pixmap`]s. Time is passed in everywhere,
//!   so all of it is unit-testable.
//! * **Linux presenter** (cargo feature `wayland`, default on Linux): a
//!   `zwlr_layer_shell_v1` client in the top-right corner of the screen.
//! * **Windows presenter**: a topmost, non-activating, layered window in the top-right corner
//!   of the work area ([`win32`]).
//!
//! ```no_run
//! use phone_overlay::{spawn, Banner, CallBanner, CallState, OverlayConfig};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let (overlay, actions) = spawn(OverlayConfig::default())?;
//! overlay.show(Banner::Call(CallBanner {
//!     id: 1,
//!     name: "Ada Lovelace".into(),
//!     number: "+1 555 0100".into(),
//!     state: CallState::Incoming,
//! }));
//! for action in actions {
//!     println!("{action:?}");
//! }
//! # Ok(()) }
//! ```
//!
//! If [`spawn`] returns [`OverlayError::Unsupported`] the caller should fall
//! back to a freedesktop notification.

#![warn(missing_docs)]

pub mod anim;
pub mod dotfont;
pub mod edit;
pub mod error;
pub mod gfx;
pub mod icons;
pub mod input;
pub mod layout;
pub mod model;
pub mod render;
pub mod state;
pub mod text;
pub mod theme;

#[cfg(all(feature = "wayland", target_os = "linux"))]
pub mod wayland;
#[cfg(windows)]
pub mod win32;

pub use error::OverlayError;
pub use input::{ButtonState, Key};
pub use model::{
    Action, Banner, CallBanner, CallState, Command, OverlayConfig, OverlayHandle, ReplyStatus,
    TextBanner, ThemePref,
};
pub use render::render;
pub use state::{OverlayState, Wake};
pub use theme::Theme;

use std::sync::mpsc::Receiver;

/// Start the overlay presenter for this platform.
///
/// Returns a [`OverlayHandle`] to show and remove banners and a receiver for
/// the user's [`Action`]s. The presenter runs on its own thread, uses no CPU
/// while nothing is shown, and shuts down when every handle is dropped.
///
/// # Errors
///
/// [`OverlayError::Unsupported`] when there is no presenter for this platform
/// or the compositor lacks `zwlr_layer_shell_v1`;
/// [`OverlayError::Presenter`] when the display connection fails.
pub fn spawn(config: OverlayConfig) -> Result<(OverlayHandle, Receiver<Action>), OverlayError> {
    #[cfg(all(feature = "wayland", target_os = "linux"))]
    {
        wayland::spawn(config)
    }
    #[cfg(windows)]
    {
        win32::spawn(config)
    }
    #[cfg(not(any(all(feature = "wayland", target_os = "linux"), windows)))]
    {
        let _ = config;
        Err(OverlayError::Unsupported)
    }
}
