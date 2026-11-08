// SPDX-License-Identifier: GPL-3.0-or-later
//! A fake phone with invented people, texts and a scriptable call, so the UI
//! can be developed, demonstrated and screenshotted without hardware.

use crate::calls::{CallSnapshot, CallState};
use crate::platform::{PhoneInfo, Platform, PlatformError, Result, Stream};
use phone_obex::fake::{FakeMessage, FakePhoneServer};
use std::sync::{Arc, Mutex};

const CONTACTS: &str = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Alí Boudjemaa\r\nTEL:+213550123456\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Zoé Martin\r\nTEL:+33612345678\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Mara Ionescu\r\nTEL:+40722123456\r\nTEL:+40722123457\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Jonas Weber\r\nTEL:+4915112345678\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Priya Nair\r\nTEL:+919876543210\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Tomás Ribeiro\r\nTEL:+351912345678\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Dr. Keller\r\nTEL:+41441234567\r\nEND:VCARD\r\n";

/// A scriptable demo phone.
pub struct DemoPlatform {
    pub server: FakePhoneServer,
    calls: Mutex<Vec<CallSnapshot>>,
    pub connected: Mutex<bool>,
}

fn now_stamp(offset_secs: i64) -> String {
    let t = chrono::Utc::now() + chrono::Duration::seconds(offset_secs);
    t.format("%Y%m%dT%H%M%SZ").to_string()
}

impl DemoPlatform {
    pub fn new() -> Arc<Self> {
        let server = FakePhoneServer::new();
        {
            let mut st = server.state.lock().unwrap();
            st.contacts_vcf = CONTACTS.into();
            st.clock = now_stamp(0);
            let m = |from: &str, to: &str, text: &str, ago: i64, read: bool| FakeMessage { from: from.into(), to: to.into(), text: text.into(), datetime: now_stamp(-ago), read };
            st.inbox = vec![
                m("+213550123456", "", "Are we still on for tonight?", 3600, true),
                m("+213550123456", "", "I booked the table for 8 🍽️", 1800, false),
                m("+33612345678", "", "Merci pour hier, c'était génial !", 86400, true),
                m("+40722123456", "", "Call me when you land", 7200, true),
                m("+4915112345678", "", "Running 10 minutes late, sorry", 400, false),
                m("+41441234567", "", "Reminder: appointment Tuesday 09:30. Reply C to confirm.", 172800, true),
            ];
            st.sent = vec![
                m("", "+213550123456", "Yes! See you at 8", 3000, true),
                m("", "+40722123456", "Landing at 18:40", 6000, true),
            ];
        }
        Arc::new(Self { server, calls: Mutex::new(vec![]), connected: Mutex::new(true) })
    }

    /// Makes the phone ring.
    pub fn ring(&self, number: &str) {
        *self.calls.lock().unwrap() = vec![CallSnapshot { id: "/demo/call1".into(), state: CallState::Incoming, number: number.into(), name: None }];
    }

    /// Delivers a new text.
    pub fn text(&self, from: &str, body: &str) {
        let mut st = self.server.state.lock().unwrap();
        let datetime = now_stamp(0);
        st.inbox.push(FakeMessage { from: from.into(), to: String::new(), text: body.into(), datetime, read: false });
    }

    /// Ends whatever call there is.
    pub fn hang_up_now(&self) {
        self.calls.lock().unwrap().clear();
    }
}

impl Platform for DemoPlatform {
    fn phones(&self) -> Result<Vec<PhoneInfo>> {
        Ok(vec![PhoneInfo { address: "DE:MO:00:00:00:01".into(), name: "Demo Phone".into(), connected: *self.connected.lock().unwrap(), battery: Some(82) }])
    }
    fn open_map(&self, _: &str) -> Result<Box<dyn Stream>> {
        Ok(Box::new(self.server.connection()))
    }
    fn open_pbap(&self, _: &str) -> Result<Box<dyn Stream>> {
        Ok(Box::new(self.server.connection()))
    }
    fn calls(&self, _: &str) -> Result<Vec<CallSnapshot>> {
        Ok(self.calls.lock().unwrap().clone())
    }
    fn dial(&self, _: &str, number: &str) -> Result<()> {
        *self.calls.lock().unwrap() = vec![CallSnapshot { id: "/demo/call1".into(), state: CallState::Dialing, number: number.into(), name: None }];
        Ok(())
    }
    fn answer(&self, _: &str, id: &str) -> Result<()> {
        let mut c = self.calls.lock().unwrap();
        for call in c.iter_mut().filter(|c| c.id == id) {
            call.state = CallState::Active;
        }
        Ok(())
    }
    fn hang_up(&self, _: &str, _: &str) -> Result<()> {
        self.hang_up_now();
        Ok(())
    }
    fn hang_up_all(&self, _: &str) -> Result<()> {
        self.hang_up_now();
        Ok(())
    }
    fn send_tones(&self, _: &str, _: &str) -> Result<()> {
        Err(PlatformError::Other("tones are not simulated in the demo".into()))
    }
}
