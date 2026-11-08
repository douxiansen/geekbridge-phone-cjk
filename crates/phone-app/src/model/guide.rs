// SPDX-License-Identifier: GPL-3.0-or-later
//! The iPhone setup guide: which of its four steps are done, what each one says, and when the
//! guide opens by itself. Pure functions of the service state, so all of it is tested.
//!
//! An iPhone shares its texts and contacts with a "car kit" only after the user switches that on in
//! the iPhone's own Bluetooth settings, and opens its hands-free link when a call needs it. Until
//! then the app sees refused links and an empty phonebook; the guide explains why.

use phone_core::platform::PhoneInfo;
use phone_core::service::LinkStatus;
use std::collections::BTreeSet;

/// The four steps, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepKind {
    Pair,
    Texts,
    Contacts,
    Call,
}

impl StepKind {
    pub const ALL: [StepKind; 4] = [
        StepKind::Pair,
        StepKind::Texts,
        StepKind::Contacts,
        StepKind::Call,
    ];

    /// 1-based, as shown.
    pub fn number(self) -> usize {
        match self {
            StepKind::Pair => 1,
            StepKind::Texts => 2,
            StepKind::Contacts => 3,
            StepKind::Call => 4,
        }
    }
}

/// How one step is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepState {
    Done,
    /// Not yet; the user has something to do.
    Waiting,
    /// Something went wrong that is not just "not done yet".
    Problem,
}

/// What the steps are worked out from.
#[derive(Debug, Clone, Copy)]
pub struct Progress<'a> {
    pub phone: Option<&'a PhoneInfo>,
    pub messages: &'a LinkStatus,
    pub contacts: &'a LinkStatus,
    pub contacts_len: usize,
    pub calls_ready: bool,
}

/// One step with its state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub kind: StepKind,
    pub state: StepState,
}

/// The four steps and their states.
pub fn steps(p: &Progress) -> [Step; 4] {
    let paired = p.phone.is_some_and(|ph| ph.connected);
    // Everything after pairing waits for pairing: nothing can be shared with a phone that is away.
    let after_pairing = |state: StepState| if paired { state } else { StepState::Waiting };
    let texts = match p.messages {
        LinkStatus::Ready => StepState::Done,
        LinkStatus::Failed(_) => StepState::Problem,
        LinkStatus::Idle | LinkStatus::Connecting | LinkStatus::AccessDenied => StepState::Waiting,
    };
    // "Shared no contacts" is the normal state before Sync Contacts is on, so it only waits.
    let contacts = if matches!(p.contacts, LinkStatus::Ready) && p.contacts_len > 0 {
        StepState::Done
    } else {
        StepState::Waiting
    };
    let call = if p.calls_ready {
        StepState::Done
    } else {
        StepState::Waiting
    };
    [
        Step {
            kind: StepKind::Pair,
            state: if paired {
                StepState::Done
            } else {
                StepState::Waiting
            },
        },
        Step {
            kind: StepKind::Texts,
            state: after_pairing(texts),
        },
        Step {
            kind: StepKind::Contacts,
            state: after_pairing(contacts),
        },
        Step {
            kind: StepKind::Call,
            state: after_pairing(call),
        },
    ]
}

pub fn all_done(steps: &[Step; 4]) -> bool {
    steps.iter().all(|s| s.state == StepState::Done)
}

/// The step the user should do next: the first one not done.
pub fn current(steps: &[Step; 4]) -> Option<StepKind> {
    steps
        .iter()
        .find(|s| s.state != StepState::Done)
        .map(|s| s.kind)
}

/// How the iPhone names this computer in its Bluetooth list, or a stand-in.
pub fn computer_label(name: Option<&str>) -> String {
    match name.map(str::trim) {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => "this computer".to_string(),
    }
}

/// The bold line of a step.
pub fn instruction(kind: StepKind) -> &'static str {
    match kind {
        StepKind::Pair => "Pair your iPhone",
        StepKind::Texts => "Allow texts",
        StepKind::Contacts => "Share contacts",
        StepKind::Call => "Try a call",
    }
}

/// Everything the detail line may mention.
#[derive(Debug, Clone, Copy)]
pub struct Facts<'a> {
    pub computer: &'a str,
    pub phone_name: &'a str,
    pub contacts_len: usize,
}

/// The one-line explanation under a step's bold line.
pub fn detail(kind: StepKind, state: StepState, f: &Facts) -> String {
    match (kind, state) {
        (StepKind::Pair, StepState::Done) => format!("Connected to {}.", f.phone_name),
        (StepKind::Pair, _) => "Pair it in your computer's Bluetooth settings. You only do this once.".to_string(),
        (StepKind::Texts, StepState::Done) => "Texts are coming through.".to_string(),
        (StepKind::Texts, StepState::Problem) => format!(
            "The iPhone did not answer. Check Settings > Bluetooth > (i) next to \u{201c}{}\u{201d}, then press Check again.",
            f.computer
        ),
        (StepKind::Texts, StepState::Waiting) => format!(
            "On the iPhone: Settings > Bluetooth, tap (i) next to \u{201c}{}\u{201d}, then turn on Show Notifications.",
            f.computer
        ),
        (StepKind::Contacts, StepState::Done) => match f.contacts_len {
            1 => "1 contact loaded.".to_string(),
            n => format!("{n} contacts loaded."),
        },
        (StepKind::Contacts, _) => "On the same screen, turn on Sync Contacts, then press Check again.".to_string(),
        (StepKind::Call, StepState::Done) => "Calls are ready.".to_string(),
        (StepKind::Call, _) => format!(
            "Call the iPhone and answer from the banner at the top right of the screen. If you cannot hear it, tap Audio on the iPhone's call screen and pick \u{201c}{}\u{201d} once.",
            f.computer
        ),
    }
}

/// A second, smaller line for the call step while it waits.
pub fn call_note(state: StepState) -> Option<&'static str> {
    (state != StepState::Done).then_some("Your first call will finish this step.")
}

/// What decides whether the guide opens by itself.
#[derive(Debug, Clone, Copy)]
pub struct AutoOpen<'a> {
    pub phone: Option<&'a PhoneInfo>,
    pub dismissed: &'a BTreeSet<String>,
    pub steps: &'a [Step; 4],
    pub in_call: bool,
    /// The guide is on screen now.
    pub open: bool,
    /// The user closed it during this run.
    pub closed_this_run: bool,
}

/// Opens by itself for a connected Apple phone whose guide was not dismissed, while one of
/// steps 2 to 4 is still to do, never during a call and never again after the user closed it.
pub fn should_open(a: &AutoOpen) -> bool {
    let Some(phone) = a.phone else { return false };
    phone.apple
        && phone.connected
        && !a.open
        && !a.closed_this_run
        && !a.in_call
        && !a.dismissed.contains(&phone.address)
        && a.steps[1..].iter().any(|s| s.state != StepState::Done)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phone(connected: bool, apple: bool) -> PhoneInfo {
        PhoneInfo {
            address: "AA:BB".into(),
            name: "iPhone".into(),
            connected,
            battery: Some(80),
            apple,
        }
    }

    fn progress<'a>(
        phone: Option<&'a PhoneInfo>,
        messages: &'a LinkStatus,
        contacts: &'a LinkStatus,
        len: usize,
        calls_ready: bool,
    ) -> Progress<'a> {
        Progress {
            phone,
            messages,
            contacts,
            contacts_len: len,
            calls_ready,
        }
    }

    fn states(p: &Progress) -> [StepState; 4] {
        steps(p).map(|s| s.state)
    }

    const D: StepState = StepState::Done;
    const W: StepState = StepState::Waiting;
    const P: StepState = StepState::Problem;

    #[test]
    fn nothing_is_done_without_a_phone() {
        let p = progress(None, &LinkStatus::Idle, &LinkStatus::Idle, 0, false);
        assert_eq!(states(&p), [W, W, W, W]);
    }

    #[test]
    fn a_fresh_iphone_is_paired_and_nothing_else() {
        let ph = phone(true, true);
        let failed = LinkStatus::Failed("The phone shared no contacts".into());
        let p = progress(Some(&ph), &LinkStatus::AccessDenied, &failed, 0, false);
        assert_eq!(states(&p), [D, W, W, W]);
    }

    #[test]
    fn each_step_ticks_as_its_condition_comes_true() {
        let ph = phone(true, true);
        let denied = LinkStatus::AccessDenied;
        let ready = LinkStatus::Ready;
        let failed = LinkStatus::Failed("none".into());
        assert_eq!(
            states(&progress(Some(&ph), &ready, &failed, 0, false)),
            [D, D, W, W]
        );
        assert_eq!(
            states(&progress(Some(&ph), &ready, &ready, 12, false)),
            [D, D, D, W]
        );
        assert_eq!(
            states(&progress(Some(&ph), &ready, &ready, 12, true)),
            [D, D, D, D]
        );
        // Steps are independent of each other once paired: contacts can come before texts.
        assert_eq!(
            states(&progress(Some(&ph), &denied, &ready, 3, false)),
            [D, W, D, W]
        );
    }

    #[test]
    fn contacts_need_at_least_one_contact() {
        let ph = phone(true, true);
        let ready = LinkStatus::Ready;
        assert_eq!(states(&progress(Some(&ph), &ready, &ready, 0, false))[2], W);
        assert_eq!(
            states(&progress(
                Some(&ph),
                &ready,
                &LinkStatus::Connecting,
                5,
                false
            ))[2],
            W
        );
    }

    #[test]
    fn a_failed_message_link_is_a_problem_not_just_waiting() {
        let ph = phone(true, true);
        let failed = LinkStatus::Failed("timed out".into());
        assert_eq!(
            states(&progress(Some(&ph), &failed, &LinkStatus::Idle, 0, false))[1],
            P
        );
        // Refused is the expected state before Show Notifications is on.
        assert_eq!(
            states(&progress(
                Some(&ph),
                &LinkStatus::AccessDenied,
                &LinkStatus::Idle,
                0,
                false
            ))[1],
            W
        );
    }

    #[test]
    fn an_absent_phone_unticks_everything_after_pairing() {
        let ph = phone(false, true);
        let ready = LinkStatus::Ready;
        // Stale "ready" statuses from before the phone left must not look done.
        assert_eq!(
            states(&progress(Some(&ph), &ready, &ready, 9, true)),
            [W, W, W, W]
        );
    }

    #[test]
    fn the_current_step_is_the_first_not_done() {
        let ph = phone(true, true);
        let ready = LinkStatus::Ready;
        let s = steps(&progress(Some(&ph), &ready, &LinkStatus::Idle, 0, false));
        assert_eq!(current(&s), Some(StepKind::Contacts));
        assert!(!all_done(&s));
        let s = steps(&progress(Some(&ph), &ready, &ready, 2, true));
        assert_eq!(current(&s), None);
        assert!(all_done(&s));
    }

    #[test]
    fn text_names_the_computer_and_falls_back() {
        assert_eq!(computer_label(Some("my-laptop")), "my-laptop");
        assert_eq!(computer_label(Some("  ")), "this computer");
        assert_eq!(computer_label(None), "this computer");
        let f = Facts {
            computer: "my-laptop",
            phone_name: "Vali's iPhone",
            contacts_len: 1,
        };
        assert!(detail(StepKind::Texts, W, &f).contains("\u{201c}my-laptop\u{201d}"));
        assert!(detail(StepKind::Texts, W, &f).contains("Show Notifications"));
        assert!(detail(StepKind::Texts, P, &f).contains("Check again"));
        assert!(detail(StepKind::Contacts, W, &f).contains("Sync Contacts"));
        assert!(detail(StepKind::Call, W, &f).contains("Audio"));
        assert!(detail(StepKind::Call, W, &f).contains("\u{201c}my-laptop\u{201d}"));
        assert_eq!(detail(StepKind::Pair, D, &f), "Connected to Vali's iPhone.");
        assert_eq!(detail(StepKind::Contacts, D, &f), "1 contact loaded.");
        let many = Facts {
            contacts_len: 250,
            ..f
        };
        assert_eq!(detail(StepKind::Contacts, D, &many), "250 contacts loaded.");
    }

    #[test]
    fn every_step_has_a_line_in_every_state() {
        let f = Facts {
            computer: "pc",
            phone_name: "p",
            contacts_len: 2,
        };
        for kind in StepKind::ALL {
            assert!(!instruction(kind).is_empty());
            for state in [D, W, P] {
                assert!(!detail(kind, state, &f).is_empty());
            }
        }
        assert_eq!(StepKind::ALL.map(StepKind::number), [1, 2, 3, 4]);
    }

    #[test]
    fn the_call_note_goes_once_the_call_is_done() {
        assert!(call_note(W).is_some());
        assert!(call_note(D).is_none());
    }

    fn auto<'a>(
        ph: Option<&'a PhoneInfo>,
        dismissed: &'a BTreeSet<String>,
        s: &'a [Step; 4],
    ) -> AutoOpen<'a> {
        AutoOpen {
            phone: ph,
            dismissed,
            steps: s,
            in_call: false,
            open: false,
            closed_this_run: false,
        }
    }

    fn fresh() -> [Step; 4] {
        let ph = phone(true, true);
        steps(&progress(
            Some(&ph),
            &LinkStatus::AccessDenied,
            &LinkStatus::Idle,
            0,
            false,
        ))
    }

    #[test]
    fn it_opens_for_a_connected_iphone_that_is_not_set_up() {
        let ph = phone(true, true);
        let none = BTreeSet::new();
        let s = fresh();
        assert!(should_open(&auto(Some(&ph), &none, &s)));
    }

    #[test]
    fn it_never_opens_for_other_phones_or_no_phone() {
        let none = BTreeSet::new();
        let s = fresh();
        assert!(!should_open(&auto(None, &none, &s)));
        assert!(!should_open(&auto(Some(&phone(true, false)), &none, &s)));
        // Not connected: nothing to set up yet.
        assert!(!should_open(&auto(Some(&phone(false, true)), &none, &s)));
    }

    #[test]
    fn it_stays_quiet_when_dismissed_for_that_phone_only() {
        let ph = phone(true, true);
        let s = fresh();
        let mut dismissed = BTreeSet::new();
        dismissed.insert("AA:BB".to_string());
        assert!(!should_open(&auto(Some(&ph), &dismissed, &s)));
        let other: BTreeSet<String> = ["CC:DD".to_string()].into();
        assert!(should_open(&auto(Some(&ph), &other, &s)));
    }

    #[test]
    fn it_does_not_open_during_a_call_or_twice_or_while_open() {
        let ph = phone(true, true);
        let none = BTreeSet::new();
        let s = fresh();
        assert!(!should_open(&AutoOpen {
            in_call: true,
            ..auto(Some(&ph), &none, &s)
        }));
        assert!(!should_open(&AutoOpen {
            closed_this_run: true,
            ..auto(Some(&ph), &none, &s)
        }));
        assert!(!should_open(&AutoOpen {
            open: true,
            ..auto(Some(&ph), &none, &s)
        }));
    }

    #[test]
    fn it_does_not_open_when_steps_two_to_four_are_done() {
        let ph = phone(true, true);
        let none = BTreeSet::new();
        let ready = LinkStatus::Ready;
        let done = steps(&progress(Some(&ph), &ready, &ready, 4, true));
        assert!(!should_open(&auto(Some(&ph), &none, &done)));
        // One missing step is enough.
        let almost = steps(&progress(Some(&ph), &ready, &ready, 4, false));
        assert!(should_open(&auto(Some(&ph), &none, &almost)));
    }
}
