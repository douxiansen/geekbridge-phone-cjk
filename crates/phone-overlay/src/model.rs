// SPDX-License-Identifier: GPL-3.0-or-later

//! Public data model: banners, actions, configuration and the command sink.

use std::sync::Arc;
use std::time::{Duration, Instant};

/// Where a phone call currently is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallState {
    /// Ringing: the remote party is calling us.
    Incoming,
    /// We are calling out and the far end has not answered yet.
    Dialing,
    /// Connected; `since` is when the call was answered (drives the timer).
    Active {
        /// Instant the call became active.
        since: Instant,
    },
    /// Connected but on hold.
    Held,
}

/// A phone call banner. It stays on screen until it is removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallBanner {
    /// Caller-chosen identifier; showing the same id again replaces the banner.
    pub id: u64,
    /// Contact name, or an empty string when unknown (the number is shown big).
    pub name: String,
    /// Phone number as the phone reports it.
    pub number: String,
    /// Current state of the call.
    pub state: CallState,
}

/// An incoming text message banner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextBanner {
    /// Caller-chosen identifier; showing the same id again replaces the banner.
    pub id: u64,
    /// Sender name, or an empty string when unknown.
    pub name: String,
    /// Sender number.
    pub number: String,
    /// Message text. Shown as a two line preview.
    pub body: String,
    /// Whether the inline quick-reply field is offered.
    pub can_reply: bool,
}

/// Anything the overlay can show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Banner {
    /// A phone call.
    Call(CallBanner),
    /// A text message.
    Text(TextBanner),
}

impl Banner {
    /// The banner's identifier.
    pub fn id(&self) -> u64 {
        match self {
            Banner::Call(c) => c.id,
            Banner::Text(t) => t.id,
        }
    }

    /// The name to display: the contact name, falling back to the number.
    pub fn display_name(&self) -> &str {
        let (name, number) = match self {
            Banner::Call(c) => (&c.name, &c.number),
            Banner::Text(t) => (&t.name, &t.number),
        };
        if name.trim().is_empty() {
            number
        } else {
            name
        }
    }

    /// The number shown under the name, or `""` when the number already is
    /// the displayed name.
    pub fn subtitle(&self) -> &str {
        let (name, number) = match self {
            Banner::Call(c) => (&c.name, &c.number),
            Banner::Text(t) => (&t.name, &t.number),
        };
        if name.trim().is_empty() || name == number {
            ""
        } else {
            number
        }
    }
}

/// What the user did on a banner. Delivered through the channel returned by
/// [`spawn`](crate::spawn).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Answer button on a ringing call.
    Answer(u64),
    /// Decline button on a ringing call.
    Decline(u64),
    /// Hang up button on an active, held or dialing call.
    HangUp(u64),
    /// A text banner went away without a reply: timeout or the close button.
    Dismiss(u64),
    /// A quick reply was submitted.
    Reply {
        /// The text banner's id.
        id: u64,
        /// The number to reply to.
        number: String,
        /// The reply text, never empty.
        text: String,
    },
    /// The banner body was clicked: bring the app forward.
    Open(u64),
}

/// Progress of a quick reply, reported by the app via
/// [`OverlayHandle::reply_status`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplyStatus {
    /// The message is on its way.
    Sending,
    /// The message was delivered; the banner closes shortly after.
    Sent,
    /// Sending failed; the reason is shown in red and the field stays usable.
    Failed(String),
}

/// Light or dark preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemePref {
    /// Follow the desktop (freedesktop settings portal); dark if unknown.
    #[default]
    System,
    /// Always dark.
    Dark,
    /// Always light.
    Light,
}

/// Overlay configuration.
#[derive(Debug, Clone)]
pub struct OverlayConfig {
    /// Light/dark choice.
    pub theme: ThemePref,
    /// How long a text banner stays up. Paused while hovered or typing.
    pub text_timeout: Duration,
    /// Name shown in the banner's app chip, e.g. `"Phone"`.
    pub app_name: String,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            theme: ThemePref::System,
            text_timeout: Duration::from_secs(8),
            app_name: "Phone".to_owned(),
        }
    }
}

/// A request from the app to the presenter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Add or replace a banner.
    Show(Banner),
    /// Animate a banner out.
    Remove(u64),
    /// Update a text banner's reply status.
    ReplyStatus(u64, ReplyStatus),
}

/// Cloneable, thread-safe handle the app keeps to drive the overlay.
///
/// Sending never blocks and never fails loudly: if the presenter is gone the
/// commands are dropped.
#[derive(Clone)]
pub struct OverlayHandle {
    sink: Arc<dyn Fn(Command) + Send + Sync>,
}

impl std::fmt::Debug for OverlayHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OverlayHandle")
    }
}

impl OverlayHandle {
    /// Build a handle around a presenter's command sink. Presenters for other
    /// platforms use this to hand out handles.
    pub fn from_sink(sink: impl Fn(Command) + Send + Sync + 'static) -> Self {
        Self {
            sink: Arc::new(sink),
        }
    }

    /// Show a banner, or replace the one with the same id.
    pub fn show(&self, banner: Banner) {
        (self.sink)(Command::Show(banner));
    }

    /// Animate a banner out. Unknown ids are ignored.
    pub fn remove(&self, id: u64) {
        (self.sink)(Command::Remove(id));
    }

    /// Report quick-reply progress: "Sending…", then a check, or the error.
    pub fn reply_status(&self, id: u64, status: ReplyStatus) {
        (self.sink)(Command::ReplyStatus(id, status));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name_falls_back_to_number() {
        let b = Banner::Text(TextBanner {
            id: 1,
            name: "  ".into(),
            number: "+49 1".into(),
            body: String::new(),
            can_reply: true,
        });
        assert_eq!(b.display_name(), "+49 1");
        assert_eq!(b.subtitle(), "");
    }

    #[test]
    fn handle_forwards_commands() {
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = std::sync::Mutex::new(tx);
        let h = OverlayHandle::from_sink(move |c| {
            if let Ok(tx) = tx.lock() {
                let _ = tx.send(c);
            }
        });
        h.remove(7);
        assert_eq!(rx.try_recv(), Ok(Command::Remove(7)));
    }
}
