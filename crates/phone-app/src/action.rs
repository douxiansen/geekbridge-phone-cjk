// SPDX-License-Identifier: GPL-3.0-or-later
//! Requests from outside the window: a second launch, a notification, the overlay.

use crate::model::tab::Tab;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

/// Something that should happen in the window. Sent through an [`ActionSender`]; the UI applies
/// them in the next frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiAction {
    /// Show the window and give it focus.
    Show,
    /// Show the window on the conversation with `number`.
    OpenConversation { number: String },
    /// Show the window on a tab.
    OpenTab(Tab),
    /// Quit the whole app, even when it would otherwise keep running in the background.
    Quit,
}

/// A cloneable way to send [`UiAction`]s from any thread. Sending also wakes the UI, which may
/// be idle or hidden.
#[derive(Clone)]
pub struct ActionSender {
    tx: Sender<UiAction>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl ActionSender {
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> (Self, Receiver<UiAction>) {
        let (tx, rx) = mpsc::channel();
        (Self { tx, wake }, rx)
    }

    /// Queues `action`. Silently does nothing once the window is gone.
    pub fn send(&self, action: UiAction) {
        if self.tx.send(action).is_ok() {
            (self.wake)();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn sending_queues_the_action_and_wakes_the_ui() {
        let woken = Arc::new(AtomicUsize::new(0));
        let w = woken.clone();
        let (tx, rx) = ActionSender::new(Arc::new(move || {
            w.fetch_add(1, Ordering::SeqCst);
        }));
        tx.send(UiAction::Show);
        tx.clone().send(UiAction::OpenConversation { number: "+1".into() });
        assert_eq!(rx.try_recv(), Ok(UiAction::Show));
        assert_eq!(rx.try_recv(), Ok(UiAction::OpenConversation { number: "+1".into() }));
        assert_eq!(woken.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn sending_after_the_window_is_gone_is_harmless() {
        let woken = Arc::new(AtomicUsize::new(0));
        let w = woken.clone();
        let (tx, rx) = ActionSender::new(Arc::new(move || {
            w.fetch_add(1, Ordering::SeqCst);
        }));
        drop(rx);
        tx.send(UiAction::Quit);
        assert_eq!(woken.load(Ordering::SeqCst), 0);
    }
}
