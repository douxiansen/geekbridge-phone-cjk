// SPDX-License-Identifier: GPL-3.0-or-later
//! User settings, stored as JSON. Unknown or missing fields fall back to
//! defaults, so an older file never stops the app from starting.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemePref {
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub notify_calls: bool,
    pub notify_missed: bool,
    pub notify_texts: bool,
    pub quick_reply: bool,
    /// Seconds a text banner stays when untouched.
    pub text_banner_secs: u32,
    pub theme: ThemePref,
    /// The phone to use when several are paired.
    pub preferred_phone: Option<String>,
    pub keep_running_in_tray: bool,
    pub start_minimised: bool,
    pub load_contacts_on_connect: bool,
    /// How often the inbox is checked, in seconds (2 to 60).
    pub inbox_poll_secs: u32,
    /// Look for a newer release on GitHub now and then (about once a day). This is the only
    /// network access the app makes.
    pub check_updates: bool,
    /// Use the Bluetooth audio link for calls only: the phone's music, videos and other media
    /// are not played through this computer. A phone otherwise treats the computer as a
    /// Bluetooth speaker as soon as they are paired.
    pub calls_audio_only: bool,
    /// Record calls to audio files. Off by default: recording a call needs
    /// the other person's consent in many places.
    pub record_calls: bool,
    /// Where recordings go; `None` means `~/Music/GeekBridge Calls` (or the
    /// platform's equivalent).
    pub recordings_dir: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            notify_calls: true,
            notify_missed: true,
            notify_texts: true,
            quick_reply: true,
            text_banner_secs: 8,
            theme: ThemePref::System,
            preferred_phone: None,
            keep_running_in_tray: true,
            start_minimised: false,
            load_contacts_on_connect: true,
            inbox_poll_secs: 5,
            check_updates: true,
            calls_audio_only: true,
            record_calls: false,
            recordings_dir: None,
        }
    }
}

impl Settings {
    /// Clamps values that would break the app if hand-edited.
    pub fn sanitised(mut self) -> Self {
        self.inbox_poll_secs = self.inbox_poll_secs.clamp(2, 60);
        self.text_banner_secs = self.text_banner_secs.clamp(2, 60);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_take_defaults() {
        let s: Settings = serde_json::from_str(r#"{"notify_texts": false}"#).unwrap();
        assert!(!s.notify_texts);
        assert!(s.notify_calls);
        assert_eq!(s.inbox_poll_secs, 5);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let s: Settings = serde_json::from_str(r#"{"from_the_future": 1}"#).unwrap();
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn silly_values_are_clamped() {
        let s = Settings {
            inbox_poll_secs: 0,
            text_banner_secs: 9999,
            ..Settings::default()
        }
        .sanitised();
        assert_eq!(s.inbox_poll_secs, 2);
        assert_eq!(s.text_banner_secs, 60);
    }
}
