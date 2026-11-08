// SPDX-License-Identifier: GPL-3.0-or-later
//! The seam between the app and whatever shows alerts: freedesktop notifications today, the
//! `phone-overlay` banners tomorrow, native toasts on other systems later.
//!
//! The event loop (see `pump`) decides *whether* something deserves an alert, honouring the
//! user's settings, and calls the [`Notifier`]. The notifier decides *how* it looks. When the
//! user acts on an alert, the notifier uses the [`NotifierHost`] it was built with to answer a
//! call, send a quick reply or ask the window to show a conversation.

use crate::action::{ActionSender, UiAction};
use phone_core::service::Handle;

/// What an alert is about, so it can be withdrawn or replaced later.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NotifyId {
    /// A call, by the id of [`phone_core::service::ActiveCall`].
    Call(String),
    /// A conversation, by its [`phone_core::contacts::number_key`].
    Text(String),
}

/// A call is ringing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallNotice {
    /// The call's id: pass it to [`NotifierHost::answer`] or [`NotifierHost::decline`].
    pub call_id: String,
    pub number: String,
    /// The contact's name, when the number is in the phonebook.
    pub name: Option<String>,
}

impl CallNotice {
    pub fn id(&self) -> NotifyId {
        NotifyId::Call(self.call_id.clone())
    }
}

/// A call is over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallEndedNotice {
    pub call_id: String,
    pub number: String,
    pub name: Option<String>,
    /// It rang and was never answered.
    pub missed: bool,
    pub seconds: u64,
}

/// A text arrived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextNotice {
    pub number: String,
    pub name: Option<String>,
    pub body: String,
    /// Unix seconds.
    pub at: i64,
    /// The user wants a reply box on the alert (`Settings::quick_reply`).
    pub quick_reply: bool,
    /// How long the alert stays when untouched (`Settings::text_banner_secs`).
    pub timeout_secs: u32,
}

impl TextNotice {
    pub fn id(&self) -> NotifyId {
        NotifyId::Text(phone_core::contacts::number_key(&self.number))
    }
}

/// Shows alerts. Called from the event thread, never from the UI thread, so it may block briefly,
/// but should not for long: alerts queue behind it.
pub trait Notifier: Send {
    fn incoming_call(&self, call: &CallNotice);
    fn call_ended(&self, call: &CallEndedNotice);
    fn text(&self, text: &TextNotice);
    /// Withdraws an alert that is no longer relevant (the call was answered elsewhere, the
    /// conversation was opened in the window).
    fn clear(&self, id: &NotifyId);
}

/// Shows nothing. The default off Linux until a native notifier exists, and for tests.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullNotifier;

impl Notifier for NullNotifier {
    fn incoming_call(&self, _: &CallNotice) {}
    fn call_ended(&self, _: &CallEndedNotice) {}
    fn text(&self, _: &TextNotice) {}
    fn clear(&self, _: &NotifyId) {}
}

/// Writes alerts to the log instead of showing them. Used by `--demo`, so that simulated calls
/// never pop up on the real desktop.
#[derive(Debug, Default, Clone, Copy)]
pub struct LogNotifier;

impl Notifier for LogNotifier {
    fn incoming_call(&self, c: &CallNotice) {
        log::info!("notify: incoming call from {} ({})", c.name.as_deref().unwrap_or("unknown"), c.number);
    }
    fn call_ended(&self, c: &CallEndedNotice) {
        log::info!("notify: call with {} ended, missed={}, {}s", c.number, c.missed, c.seconds);
    }
    fn text(&self, t: &TextNotice) {
        log::info!("notify: text from {} ({}): {}", t.name.as_deref().unwrap_or("unknown"), t.number, t.body);
    }
    fn clear(&self, id: &NotifyId) {
        log::info!("notify: clear {id:?}");
    }
}

/// What a notifier needs to act on the user's behalf: the phone (to answer, hang up, reply) and
/// the window (to show or navigate). Cheap to clone; give a copy to every callback.
#[derive(Clone)]
pub struct NotifierHost {
    pub handle: Handle,
    pub actions: ActionSender,
}

impl NotifierHost {
    /// Answers the ringing call.
    pub fn answer(&self, call_id: &str) {
        self.handle.answer(call_id);
    }

    /// Declines a ringing call.
    pub fn decline(&self, call_id: &str) {
        self.handle.hang_up(call_id);
    }

    /// Ends a call in progress.
    pub fn hang_up(&self, call_id: &str) {
        self.handle.hang_up(call_id);
    }

    /// Sends a quick reply. The outcome comes back as a banner in the window if it fails.
    pub fn reply(&self, number: &str, text: &str) -> u64 {
        self.handle.send_text(number, text)
    }

    /// Brings the window forward on the conversation with `number`.
    pub fn open_conversation(&self, number: &str) {
        self.actions.send(UiAction::OpenConversation { number: number.to_string() });
    }

    /// Brings the window forward as it is.
    pub fn show_window(&self) {
        self.actions.send(UiAction::Show);
    }
}

/// Builds a notifier once the app has its [`NotifierHost`].
pub type NotifierFactory = Box<dyn FnOnce(NotifierHost) -> Box<dyn Notifier> + Send>;

/// The notifier used when the embedding program supplies none: freedesktop notifications on
/// Linux, nothing elsewhere.
pub fn default_notifier(host: NotifierHost) -> Box<dyn Notifier> {
    #[cfg(target_os = "linux")]
    {
        Box::new(crate::notify_freedesktop::FreedesktopNotifier::new(host))
    }
    #[cfg(not(target_os = "linux"))]
    {
        // TODO(windows, macos): native toast notifications (WinRT ToastNotification,
        // UNUserNotificationCenter) behind this same trait.
        let _ = host;
        Box::new(NullNotifier)
    }
}
