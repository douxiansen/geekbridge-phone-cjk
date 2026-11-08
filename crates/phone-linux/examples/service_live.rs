// SPDX-License-Identifier: GPL-3.0-or-later
//! Runs the whole service against the real phone and prints what happens,
//! without message text. Uses a throwaway data folder.
//!
//!   cargo run -p phone-linux --example service_live -- [seconds]

use phone_core::service::{Event, Options, Service};
use phone_core::store::Paths;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let secs: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(25);
    let platform = Arc::new(phone_linux::LinuxPlatform::new().expect("buses"));
    let root = std::env::temp_dir().join("gbp-live");
    let _ = std::fs::remove_dir_all(&root);
    let (svc, rx) = Service::start(platform, Options::new(Paths::under(&root)));
    let h = svc.handle();
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if let Ok(e) = rx.recv_timeout(Duration::from_millis(500)) {
            match e {
                Event::Phone(p) => {
                    println!("phone: {:?}", p.map(|p| (p.name, p.connected, p.battery)))
                }
                Event::Call(c) => println!("call event: {c:?}"),
                Event::TextReceived(m) => println!(
                    "TEXT RECEIVED from {} ({} chars)",
                    m.number,
                    m.text.chars().count()
                ),
                Event::Notice(n) => println!("notice: {n}"),
                Event::ContactsChanged => {
                    let st = h.state();
                    println!("contacts: {} ({:?})", st.contacts.len(), st.contacts_status);
                }
                Event::ConversationsChanged => {
                    let st = h.state();
                    println!(
                        "conversations: {} threads, {} unread ({:?})",
                        st.conversations.conversations().len(),
                        st.conversations.unread_total(),
                        st.messages_status
                    );
                }
                Event::RecentsChanged => println!("recents: {}", h.state().recents.len()),
                _ => {}
            }
        }
    }
    {
        let st = h.state();
        println!(
            "final: messages {:?}, contacts {:?} ({} contacts), recents {}",
            st.messages_status,
            st.contacts_status,
            st.contacts.len(),
            st.recents.len()
        );
    }
    svc.stop();
}
