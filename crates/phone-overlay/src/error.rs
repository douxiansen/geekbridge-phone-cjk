// SPDX-License-Identifier: GPL-3.0-or-later

//! Errors from starting a presenter.

/// Why the overlay could not be started.
#[derive(Debug, thiserror::Error)]
pub enum OverlayError {
    /// No presenter can run here: not Linux, the `wayland` feature is off, there is
    /// no Wayland session, or the compositor lacks `zwlr_layer_shell_v1`. The caller
    /// should fall back to a desktop notification.
    #[error("overlay not supported here (no layer-shell compositor)")]
    Unsupported,
    /// The display connection or a protocol request failed.
    #[error("overlay presenter failed: {0}")]
    Presenter(String),
}
