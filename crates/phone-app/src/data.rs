// SPDX-License-Identifier: GPL-3.0-or-later
//! What the screens read. Each frame takes one short look at the service state ([`Live`]); the
//! bigger pieces (contacts, conversations, recents) are copied only when the event thread says
//! they changed ([`ViewData`]), so drawing never holds the service's lock.

use crate::shared::{Generations, Shared};
use phone_core::calls::CallState;
use phone_core::contacts::{number_key, Contact, ContactBook};
use phone_core::conversations::Conversation;
use phone_core::platform::PhoneInfo;
use phone_core::recents::Recent;
use phone_core::service::{ActiveCall, Handle, LinkStatus};
use phone_core::settings::Settings;
use std::collections::HashMap;
use std::time::Instant;

/// A call as the screens draw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallView {
    pub call: ActiveCall,
    /// The call is being recorded. The service does not report this yet, so it is always false;
    /// the card already draws the indicator.
    pub recording: bool,
}

impl CallView {
    pub fn seconds(&self) -> Option<u64> {
        self.call.since.map(|s| s.elapsed().as_secs())
    }

    pub fn is_active(&self) -> bool {
        matches!(self.call.state, CallState::Active | CallState::Held)
    }

    pub fn is_ringing(&self) -> bool {
        self.call.state.is_ringing()
    }
}

/// The small, always-needed part of the service state, copied once per frame.
#[derive(Debug, Clone, Default)]
pub struct Live {
    pub phone: Option<PhoneInfo>,
    pub phones: Vec<PhoneInfo>,
    pub calls: Vec<CallView>,
    pub contacts_status: LinkStatus,
    pub messages_status: LinkStatus,
    pub settings: Settings,
    pub contact_count: usize,
    pub recent_count: usize,
}

impl Live {
    /// Takes the state lock once, copies, and lets go.
    pub fn read(handle: &Handle) -> Self {
        let st = handle.state();
        Self {
            phone: st.phone.clone(),
            phones: st.phones.clone(),
            calls: st
                .calls
                .iter()
                .cloned()
                .map(|call| CallView {
                    call,
                    recording: false,
                })
                .collect(),
            contacts_status: st.contacts_status.clone(),
            messages_status: st.messages_status.clone(),
            settings: st.settings.clone(),
            contact_count: st.contacts.len(),
            recent_count: st.recents.len(),
        }
    }

    pub fn phone_connected(&self) -> bool {
        self.phone.as_ref().is_some_and(|p| p.connected)
    }

    pub fn in_call(&self) -> bool {
        !self.calls.is_empty()
    }

    /// A call that is active or on hold (tones can be sent into it).
    pub fn active_call(&self) -> Option<&CallView> {
        self.calls.iter().find(|c| c.is_active())
    }

    pub fn ringing(&self) -> Option<&CallView> {
        self.calls.iter().find(|c| c.is_ringing())
    }
}

/// Copies of the larger pieces, refreshed on demand.
#[derive(Default)]
pub struct ViewData {
    seen: Option<Generations>,
    pub generations: Generations,
    pub book: ContactBook,
    names: HashMap<String, String>,
    pub conversations: Vec<Conversation>,
    pub recents: Vec<Recent>,
    /// Number of texts across all conversations, for diagnostics.
    pub message_count: usize,
    pub loaded_at: Option<Instant>,
}

impl ViewData {
    /// Re-reads whatever changed since the last call. Returns true when something did.
    pub fn refresh(&mut self, handle: &Handle, shared: &Shared) -> bool {
        let now = shared.generations();
        let seen = self.seen.unwrap_or(Generations {
            contacts: u64::MAX,
            conversations: u64::MAX,
            recents: u64::MAX,
        });
        if seen == now && self.seen.is_some() {
            return false;
        }
        let (contacts, conversations, recents) = {
            let st = handle.state();
            (
                (seen.contacts != now.contacts).then(|| st.contacts.clone()),
                (seen.conversations != now.conversations).then(|| st.conversations.conversations()),
                (seen.recents != now.recents).then(|| st.recents.entries().to_vec()),
            )
        };
        if let Some(book) = contacts {
            self.names = name_index(book.contacts());
            self.book = book;
        }
        if let Some(c) = conversations {
            self.message_count = c.iter().map(|c| c.messages.len()).sum();
            self.conversations = c;
        }
        if let Some(r) = recents {
            self.recents = r;
        }
        self.seen = Some(now);
        self.generations = now;
        self.loaded_at = Some(Instant::now());
        true
    }

    /// The contact's name for a number, if it is in the phonebook.
    pub fn name_for(&self, number: &str) -> Option<&str> {
        self.names.get(&number_key(number)).map(String::as_str)
    }

    /// The name for a number, or the number itself.
    pub fn display(&self, number: &str) -> String {
        self.name_for(number)
            .map_or_else(|| number.to_string(), str::to_string)
    }

    /// What picks a person's avatar colour: their name when the number is in the phonebook, else
    /// the number. The same person then has the same colour on every screen.
    pub fn avatar_key(&self, number: &str) -> String {
        self.name_for(number)
            .map_or_else(|| number_key(number), str::to_string)
    }

    pub fn conversation(&self, key: &str) -> Option<&Conversation> {
        self.conversations.iter().find(|c| c.key == key)
    }
}

/// Index of every number's key to its contact's name (first contact wins, as in the core).
pub fn name_index(contacts: &[Contact]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for c in contacts {
        for n in &c.numbers {
            map.entry(number_key(n)).or_insert_with(|| c.name.clone());
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contact(name: &str, numbers: &[&str]) -> Contact {
        Contact {
            name: name.into(),
            numbers: numbers.iter().map(|n| n.to_string()).collect(),
        }
    }

    #[test]
    fn names_are_found_however_the_number_is_written() {
        let idx = name_index(&[contact("Mara", &["+40 722 123 456", "0722123457"])]);
        let data = ViewData {
            names: idx,
            ..ViewData::default()
        };
        assert_eq!(data.name_for("+40722123456"), Some("Mara"));
        assert_eq!(data.name_for("0722-123-457"), Some("Mara"));
        assert_eq!(data.name_for("+40722000000"), None);
        assert_eq!(data.display("+40722000000"), "+40722000000");
        assert_eq!(data.display("+40722123456"), "Mara");
    }

    #[test]
    fn a_person_has_one_avatar_key_however_their_number_is_written() {
        let idx = name_index(&[contact("Mara", &["+40 722 123 456"])]);
        let data = ViewData {
            names: idx,
            ..ViewData::default()
        };
        assert_eq!(data.avatar_key("0722123456"), "Mara");
        assert_eq!(data.avatar_key("+40722123456"), "Mara");
        // A stranger is keyed by number, again in any notation.
        assert_eq!(
            data.avatar_key("+1 (555) 010-0199"),
            data.avatar_key("5550100199")
        );
    }

    #[test]
    fn the_first_contact_keeps_a_shared_number() {
        let idx = name_index(&[contact("A", &["+1555010"]), contact("B", &["+1555010"])]);
        assert_eq!(
            idx.get(&number_key("+1555010")).map(String::as_str),
            Some("A")
        );
    }

    #[test]
    fn calls_know_their_state() {
        let call = |state| CallView {
            call: ActiveCall {
                id: "c".into(),
                state,
                number: "1".into(),
                name: None,
                since: None,
            },
            recording: false,
        };
        assert!(call(CallState::Incoming).is_ringing());
        assert!(call(CallState::Active).is_active());
        assert!(call(CallState::Held).is_active());
        assert!(!call(CallState::Dialing).is_active());
        let live = Live {
            calls: vec![call(CallState::Dialing), call(CallState::Active)],
            ..Live::default()
        };
        assert!(live.in_call());
        assert_eq!(
            live.active_call().map(|c| c.call.state),
            Some(CallState::Active)
        );
        assert!(live.ringing().is_none());
    }
}
