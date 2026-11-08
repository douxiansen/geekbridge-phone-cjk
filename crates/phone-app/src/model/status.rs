// SPDX-License-Identifier: GPL-3.0-or-later
//! How the phone link and the calls are put into words: the phone chip, the toolbar readout,
//! banners and call labels. Pure functions of the service state.

use phone_core::calls::CallState;
use phone_core::platform::PhoneInfo;
use phone_core::service::{ActiveCall, LinkStatus};

/// The status dot next to the phone's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dot {
    /// Connected and everything works.
    Green,
    /// Not connected, or something needs attention.
    Red,
    /// Still connecting.
    Hollow,
}

/// What the phone chip at the bottom of the sidebar shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhoneChip {
    pub name: String,
    pub dot: Dot,
    pub status: String,
    pub battery: Option<u8>,
}

/// Summarises the selected phone and its message link.
pub fn phone_chip(phone: Option<&PhoneInfo>, messages: &LinkStatus) -> PhoneChip {
    let Some(phone) = phone else {
        return PhoneChip { name: "No phone".into(), dot: Dot::Red, status: "Pair a phone in Bluetooth settings".into(), battery: None };
    };
    let (dot, status) = if !phone.connected {
        (Dot::Red, "Not connected")
    } else {
        match messages {
            LinkStatus::Ready => (Dot::Green, "Connected"),
            LinkStatus::Idle | LinkStatus::Connecting => (Dot::Hollow, "Connecting…"),
            LinkStatus::AccessDenied => (Dot::Red, "Allow on phone"),
            LinkStatus::Failed(_) => (Dot::Red, "Messages offline"),
        }
    };
    PhoneChip { name: phone.name.clone(), dot, status: status.into(), battery: phone.battery.filter(|b| *b <= 100) }
}

/// The short upper-case line on the toolbar's dot-matrix display.
pub fn lcd_text(phone: Option<&PhoneInfo>, messages: &LinkStatus, calls: &[ActiveCall]) -> String {
    if let Some(call) = calls.first() {
        return match call.state {
            CallState::Incoming | CallState::Waiting => "INCOMING CALL".into(),
            CallState::Dialing | CallState::Alerting => "CALLING".into(),
            CallState::Held => "ON HOLD".into(),
            CallState::Active => "IN CALL".into(),
            CallState::Disconnected => "CALL ENDED".into(),
        };
    }
    match (phone, messages) {
        (None, _) => "NO PHONE".into(),
        (Some(p), _) if !p.connected => "PHONE OFFLINE".into(),
        (Some(_), LinkStatus::Ready) => "PHONE CONNECTED".into(),
        (Some(_), LinkStatus::AccessDenied) => "ALLOW ACCESS".into(),
        (Some(_), LinkStatus::Failed(_)) => "MESSAGES OFFLINE".into(),
        (Some(_), _) => "CONNECTING".into(),
    }
}

/// What a call is doing, for the label on the call card ("Incoming call", "Calling").
pub fn call_state_label(state: CallState) -> &'static str {
    match state {
        CallState::Incoming => "Incoming call",
        CallState::Waiting => "Call waiting",
        CallState::Dialing => "Calling…",
        CallState::Alerting => "Ringing…",
        CallState::Active => "On call",
        CallState::Held => "On hold",
        CallState::Disconnected => "Call ended",
    }
}

/// Something wrong with a link that a banner should offer to fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// The phone has not allowed this computer to read and send texts.
    MessageAccess,
    MessagesFailed(String),
    ContactsFailed(String),
}

impl Problem {
    pub fn title(&self) -> String {
        match self {
            Problem::MessageAccess => "Allow message access on your phone".into(),
            Problem::MessagesFailed(_) => "Messages are offline".into(),
            Problem::ContactsFailed(_) => "Contacts could not be loaded".into(),
        }
    }

    pub fn detail(&self) -> String {
        match self {
            Problem::MessageAccess => "Your phone is asking whether this computer may use your messages. Tap Allow on the phone, then press Retry.".into(),
            Problem::MessagesFailed(why) | Problem::ContactsFailed(why) => why.clone(),
        }
    }
}

/// The link problems worth a banner right now. Nothing is reported while no phone is connected:
/// the phone chip already says so.
pub fn problems(phone: Option<&PhoneInfo>, messages: &LinkStatus, contacts: &LinkStatus) -> Vec<Problem> {
    if !phone.is_some_and(|p| p.connected) {
        return Vec::new();
    }
    let mut out = Vec::new();
    match messages {
        LinkStatus::AccessDenied => out.push(Problem::MessageAccess),
        LinkStatus::Failed(why) => out.push(Problem::MessagesFailed(why.clone())),
        _ => {}
    }
    if let LinkStatus::Failed(why) = contacts {
        out.push(Problem::ContactsFailed(why.clone()));
    }
    out
}

/// A service notice that only repeats a banner already on screen.
pub fn notice_is_redundant(notice: &str, active: &[Problem]) -> bool {
    let lower = notice.to_lowercase();
    active.iter().any(|p| match p {
        Problem::MessageAccess => lower.contains("message access"),
        Problem::ContactsFailed(_) => lower.contains("read contacts"),
        Problem::MessagesFailed(_) => false,
    })
}

/// A link status in words, for diagnostics.
pub fn link_text(status: &LinkStatus) -> String {
    match status {
        LinkStatus::Idle => "Idle".into(),
        LinkStatus::Connecting => "Connecting".into(),
        LinkStatus::Ready => "Ready".into(),
        LinkStatus::AccessDenied => "Access not allowed on the phone".into(),
        LinkStatus::Failed(why) => format!("Failed: {why}"),
    }
}

/// The name to show for a call: the contact's name, else the number, else "Unknown".
pub fn call_title(call: &ActiveCall) -> String {
    match (&call.name, call.number.trim()) {
        (Some(name), _) if !name.trim().is_empty() => name.clone(),
        (_, number) if !number.is_empty() => number.to_string(),
        _ => "Unknown caller".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phone(connected: bool, battery: Option<u8>) -> PhoneInfo {
        PhoneInfo { address: "AA:BB".into(), name: "Pixel".into(), connected, battery }
    }

    fn call(state: CallState, name: Option<&str>, number: &str) -> ActiveCall {
        ActiveCall { id: "c".into(), state, number: number.into(), name: name.map(str::to_string), since: None }
    }

    #[test]
    fn the_dot_is_green_red_or_hollow() {
        assert_eq!(phone_chip(Some(&phone(true, None)), &LinkStatus::Ready).dot, Dot::Green);
        assert_eq!(phone_chip(Some(&phone(true, None)), &LinkStatus::Connecting).dot, Dot::Hollow);
        assert_eq!(phone_chip(Some(&phone(true, None)), &LinkStatus::Idle).dot, Dot::Hollow);
        assert_eq!(phone_chip(Some(&phone(true, None)), &LinkStatus::AccessDenied).dot, Dot::Red);
        assert_eq!(phone_chip(Some(&phone(true, None)), &LinkStatus::Failed("x".into())).dot, Dot::Red);
        assert_eq!(phone_chip(Some(&phone(false, None)), &LinkStatus::Ready).dot, Dot::Red);
        assert_eq!(phone_chip(None, &LinkStatus::Idle).dot, Dot::Red);
    }

    #[test]
    fn an_implausible_battery_is_hidden() {
        assert_eq!(phone_chip(Some(&phone(true, Some(82))), &LinkStatus::Ready).battery, Some(82));
        assert_eq!(phone_chip(Some(&phone(true, Some(255))), &LinkStatus::Ready).battery, None);
    }

    #[test]
    fn the_display_puts_calls_first() {
        let p = phone(true, None);
        assert_eq!(lcd_text(Some(&p), &LinkStatus::Ready, &[]), "PHONE CONNECTED");
        assert_eq!(lcd_text(Some(&p), &LinkStatus::Ready, &[call(CallState::Incoming, None, "1")]), "INCOMING CALL");
        assert_eq!(lcd_text(Some(&p), &LinkStatus::Ready, &[call(CallState::Active, None, "1")]), "IN CALL");
        assert_eq!(lcd_text(None, &LinkStatus::Idle, &[]), "NO PHONE");
        assert_eq!(lcd_text(Some(&phone(false, None)), &LinkStatus::Idle, &[]), "PHONE OFFLINE");
        assert_eq!(lcd_text(Some(&p), &LinkStatus::Connecting, &[]), "CONNECTING");
    }

    #[test]
    fn display_text_only_uses_glyphs_the_dot_font_has() {
        let p = phone(true, None);
        for t in [
            lcd_text(Some(&p), &LinkStatus::Ready, &[]),
            lcd_text(Some(&p), &LinkStatus::AccessDenied, &[]),
            lcd_text(Some(&p), &LinkStatus::Failed("x".into()), &[]),
            lcd_text(None, &LinkStatus::Idle, &[]),
        ] {
            assert!(t.chars().all(|c| c.is_ascii_uppercase() || c == ' '), "{t}");
        }
    }

    #[test]
    fn problems_need_a_connected_phone() {
        let denied = LinkStatus::AccessDenied;
        assert!(problems(None, &denied, &LinkStatus::Idle).is_empty());
        assert!(problems(Some(&phone(false, None)), &denied, &LinkStatus::Idle).is_empty());
        assert_eq!(problems(Some(&phone(true, None)), &denied, &LinkStatus::Idle), vec![Problem::MessageAccess]);
        let both = problems(Some(&phone(true, None)), &LinkStatus::Failed("a".into()), &LinkStatus::Failed("b".into()));
        assert_eq!(both, vec![Problem::MessagesFailed("a".into()), Problem::ContactsFailed("b".into())]);
        assert!(problems(Some(&phone(true, None)), &LinkStatus::Ready, &LinkStatus::Connecting).is_empty());
    }

    #[test]
    fn notices_that_repeat_a_banner_are_dropped() {
        let active = vec![Problem::MessageAccess];
        assert!(notice_is_redundant("Allow message access for this computer on your phone, then it will connect.", &active));
        assert!(!notice_is_redundant("No phone is connected.", &active));
        assert!(!notice_is_redundant("Allow message access for this computer", &[]));
    }

    #[test]
    fn call_titles_fall_back_gracefully() {
        assert_eq!(call_title(&call(CallState::Active, Some("Alí"), "+1")), "Alí");
        assert_eq!(call_title(&call(CallState::Active, None, "+1 555")), "+1 555");
        assert_eq!(call_title(&call(CallState::Active, Some("  "), "+1 555")), "+1 555");
        assert_eq!(call_title(&call(CallState::Incoming, None, "")), "Unknown caller");
    }

    #[test]
    fn every_call_state_has_a_label() {
        for s in [CallState::Incoming, CallState::Waiting, CallState::Dialing, CallState::Alerting, CallState::Active, CallState::Held, CallState::Disconnected] {
            assert!(!call_state_label(s).is_empty());
        }
    }
}
