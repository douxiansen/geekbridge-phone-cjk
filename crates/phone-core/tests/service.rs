// SPDX-License-Identifier: GPL-3.0-or-later
//! The service against a fake phone: no hardware, real threads.

use phone_core::calls::{CallEvent, CallSnapshot, CallState, Direction};
use phone_core::platform::{PhoneInfo, Platform, PlatformError, Result, Stream};
use phone_core::service::{Event, LinkStatus, Options, Service};
use phone_core::store::Paths;
use phone_obex::fake::{FakeMessage, FakePhoneServer};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct FakePlatform {
    server: FakePhoneServer,
    calls: Mutex<Vec<CallSnapshot>>,
    log: Mutex<Vec<String>>,
    connected: Mutex<bool>,
}

impl FakePlatform {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            server: FakePhoneServer::new(),
            calls: Mutex::new(vec![]),
            log: Mutex::new(vec![]),
            connected: Mutex::new(true),
        })
    }
}

impl Platform for FakePlatform {
    fn phones(&self) -> Result<Vec<PhoneInfo>> {
        Ok(vec![PhoneInfo {
            address: "AA:BB:CC:DD:EE:FF".into(),
            name: "Test Phone".into(),
            connected: *self.connected.lock().unwrap(),
            battery: Some(77),
        }])
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
        self.log.lock().unwrap().push(format!("dial {number}"));
        Ok(())
    }
    fn answer(&self, _: &str, id: &str) -> Result<()> {
        self.log.lock().unwrap().push(format!("answer {id}"));
        Ok(())
    }
    fn hang_up(&self, _: &str, id: &str) -> Result<()> {
        self.log.lock().unwrap().push(format!("hangup {id}"));
        Ok(())
    }
    fn hang_up_all(&self, _: &str) -> Result<()> {
        Err(PlatformError::Other("nope".into()))
    }
    fn send_tones(&self, _: &str, _: &str) -> Result<()> {
        Ok(())
    }
}

fn fast(dir: &str) -> Options {
    let root = std::env::temp_dir().join(format!("gbp-svc-{dir}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mut o = Options::new(Paths::under(&root));
    o.phones_every = Duration::from_millis(40);
    o.calls_every = Duration::from_millis(30);
    o.reconnect_min = Duration::from_millis(50);
    o.reconnect_max = Duration::from_millis(200);
    o.denied_retry = Duration::from_millis(100);
    o
}

/// Waits for an event matching `f`, up to 5 s.
fn wait_for<T>(rx: &Receiver<Event>, mut f: impl FnMut(&Event) -> Option<T>) -> T {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let left = end.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(e) => {
                if let Some(v) = f(&e) {
                    return v;
                }
            }
            Err(_) => panic!("timed out waiting for an event"),
        }
    }
}

/// Polls `f` until it is true, up to 5 s.
fn eventually(mut f: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !f() {
        assert!(Instant::now() < end, "condition never became true");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn msg(from: &str, to: &str, text: &str, dt: &str, read: bool) -> FakeMessage {
    FakeMessage {
        from: from.into(),
        to: to.into(),
        text: text.into(),
        datetime: dt.into(),
        read,
    }
}

const VCF: &str =
    "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Alí Boudjemaa\r\nTEL:+213550123456\r\nEND:VCARD\r\n";

#[test]
fn connects_loads_contacts_and_receives_and_sends_texts() {
    let p = FakePlatform::new();
    {
        let mut st = p.server.state.lock().unwrap();
        st.contacts_vcf = VCF.into();
        st.inbox.push(msg(
            "+213550123456",
            "",
            "old news",
            "20260101T100000Z",
            true,
        ));
    }
    let (svc, rx) = Service::start(p.clone(), fast("texts"));
    let h = svc.handle();

    wait_for(&rx, |e| {
        matches!(e, Event::Phone(Some(ph)) if ph.name == "Test Phone").then_some(())
    });
    eventually(|| h.state().contacts_status == LinkStatus::Ready);
    assert_eq!(h.state().display_name("0550 12 34 56"), "Alí Boudjemaa");

    // The first listing is a baseline: the old text is history, not news.
    eventually(|| h.state().conversations.thread("+213550123456").len() == 1);
    assert_eq!(h.state().messages_status, LinkStatus::Ready);

    // A new text arrives.
    p.server.state.lock().unwrap().inbox.push(msg(
        "+213550123456",
        "",
        "are you free?",
        "20260101T110000Z",
        false,
    ));
    let m = wait_for(&rx, |e| {
        if let Event::TextReceived(m) = e {
            Some(m.clone())
        } else {
            None
        }
    });
    assert_eq!(m.text, "are you free?");
    assert_eq!(h.state().conversations.unread_total(), 1);

    // We reply.
    let token = h.send_text("+213550123456", "yes, call me");
    let (t, result) = wait_for(&rx, |e| {
        if let Event::TextResult { token, result, .. } = e {
            Some((*token, result.clone()))
        } else {
            None
        }
    });
    assert_eq!(t, token);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        p.server.state.lock().unwrap().sent.last().unwrap().text,
        "yes, call me"
    );
    let thread = h.state().conversations.thread("0550123456");
    assert_eq!(thread.last().unwrap().text, "yes, call me");
    svc.stop();
}

#[test]
fn tracks_a_call_from_ring_to_end_and_records_it() {
    let p = FakePlatform::new();
    let (svc, rx) = Service::start(p.clone(), fast("calls"));
    let h = svc.handle();
    wait_for(&rx, |e| matches!(e, Event::Phone(Some(_))).then_some(()));

    let call = |state| {
        vec![CallSnapshot {
            id: "/c1".into(),
            state,
            number: "+49151".into(),
            name: None,
        }]
    };
    *p.calls.lock().unwrap() = call(CallState::Incoming);
    wait_for(&rx, |e| {
        matches!(e, Event::Call(CallEvent::Incoming { number, .. }) if number == "+49151")
            .then_some(())
    });
    assert_eq!(h.state().calls.len(), 1);

    h.answer("/c1");
    *p.calls.lock().unwrap() = call(CallState::Active);
    wait_for(&rx, |e| {
        matches!(
            e,
            Event::Call(CallEvent::Changed {
                state: CallState::Active,
                ..
            })
        )
        .then_some(())
    });
    assert!(h.state().calls[0].since.is_some());
    assert!(p.log.lock().unwrap().contains(&"answer /c1".to_string()));

    p.calls.lock().unwrap().clear();
    let dir = wait_for(&rx, |e| {
        if let Event::Call(CallEvent::Ended { direction, .. }) = e {
            Some(direction.clone())
        } else {
            None
        }
    });
    assert_eq!(dir, Direction::Incoming);
    assert_eq!(h.state().recents.len(), 1);
    assert!(h.state().calls.is_empty());
    svc.stop();
}

#[test]
fn a_refused_message_link_is_reported_and_recovers() {
    let p = FakePlatform::new();
    p.server.state.lock().unwrap().refuse_with = Some(0xC3);
    let (svc, rx) = Service::start(p.clone(), fast("denied"));
    let h = svc.handle();
    wait_for(&rx, |e| {
        matches!(e, Event::Notice(n) if n.contains("Allow message access")).then_some(())
    });
    assert_eq!(h.state().messages_status, LinkStatus::AccessDenied);
    // The user taps Allow on the phone.
    p.server.state.lock().unwrap().refuse_with = None;
    let end = Instant::now() + Duration::from_secs(20);
    while h.state().messages_status != LinkStatus::Ready {
        assert!(Instant::now() < end, "never recovered");
        std::thread::sleep(Duration::from_millis(50));
    }
    svc.stop();
}

#[test]
fn sending_with_no_phone_fails_clearly() {
    let p = FakePlatform::new();
    *p.connected.lock().unwrap() = false;
    let (svc, rx) = Service::start(p, fast("nophone"));
    let h = svc.handle();
    wait_for(&rx, |e| {
        matches!(e, Event::Phone(Some(ph)) if !ph.connected).then_some(())
    });
    h.send_text("+1", "hi");
    let r = wait_for(&rx, |e| {
        if let Event::TextResult { result, .. } = e {
            Some(result.clone())
        } else {
            None
        }
    });
    assert!(r.is_err());
    svc.stop();
}

#[test]
fn failed_command_becomes_a_notice() {
    let p = FakePlatform::new();
    let (svc, rx) = Service::start(p, fast("cmd"));
    let h = svc.handle();
    wait_for(&rx, |e| matches!(e, Event::Phone(Some(_))).then_some(()));
    h.send(phone_core::service::Command::HangUpAll);
    wait_for(&rx, |e| {
        matches!(e, Event::Notice(n) if n == "nope").then_some(())
    });
    svc.stop();
}
