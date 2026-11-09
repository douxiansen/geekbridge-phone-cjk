// SPDX-License-Identifier: GPL-3.0-or-later
//! The screens. Each has a small state struct and a `show` function; they share a [`Cx`] and
//! ask the app to navigate by pushing [`Cmd`]s.

pub mod call_card;
pub mod contacts;
pub mod guide;
pub mod keypad;
pub mod messages;
pub mod pairing;
pub mod recents;
pub mod settings;

use crate::data::{Live, ViewData};
use crate::model::tab::Tab;
use crate::ui::theme::Palette;
use chrono::FixedOffset;
use phone_core::service::Handle;

/// A request from a screen to the app shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd {
    /// Go to a tab.
    Tab(Tab),
    /// Start a new text, optionally to a number already chosen.
    NewMessage { to: Option<String> },
    /// Open the conversation with a number.
    OpenConversation(String),
    /// Open the iPhone setup guide, whether or not it was dismissed.
    OpenGuide,
    /// Open the "Pair a phone" window.
    OpenPairing,
    /// Quit the whole app.
    Quit,
}

/// What a screen needs to draw and act.
pub struct Cx<'a> {
    pub pal: Palette,
    pub handle: &'a Handle,
    pub data: &'a ViewData,
    pub live: &'a Live,
    /// Unix seconds now, and the local zone: passed in so drawing is repeatable.
    pub now: i64,
    pub off: FixedOffset,
    pub cmds: &'a mut Vec<Cmd>,
    pub updater: &'a crate::updater::Updater,
}

impl Cx<'_> {
    /// Places a call to `number`, unless one is already in progress.
    pub fn dial(&mut self, number: &str) {
        if !self.live.in_call() && crate::model::keypad::can_dial(number) {
            self.handle.dial(number);
        }
    }

    pub fn open_conversation(&mut self, number: &str) {
        self.cmds.push(Cmd::OpenConversation(number.to_string()));
    }

    pub fn new_message_to(&mut self, number: &str) {
        self.cmds.push(Cmd::NewMessage {
            to: Some(number.to_string()),
        });
    }
}
