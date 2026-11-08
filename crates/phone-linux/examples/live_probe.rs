// SPDX-License-Identifier: GPL-3.0-or-later
//! Talks to a real phone: connects to its message service and prints only
//! counts and field shapes, never message text.
//!
//!   cargo run -p phone-linux --example live_probe -- AA:BB:CC:DD:EE:FF 7

use phone_obex::MapClient;

fn main() {
    let mut args = std::env::args().skip(1);
    let addr = args
        .next()
        .expect("usage: live_probe <address> <mas-channel>");
    let channel: u8 = args
        .next()
        .expect("channel")
        .parse()
        .expect("channel number");

    eprintln!("connecting RFCOMM channel {channel}…");
    let stream = phone_linux::connect(&addr, channel).expect("RFCOMM connect");
    eprintln!("connected; opening MAP session…");
    let mut map = match MapClient::connect(stream) {
        Ok(m) => m,
        Err(e) => {
            eprintln!(
                "MAP connect failed: {e} (access denied: {})",
                e.is_access_denied()
            );
            std::process::exit(1);
        }
    };
    println!("folders under /telecom/msg:");
    map.set_folder("/telecom/msg").expect("set folder");
    println!("  {:?}", map.list_folders().expect("folders"));
    for folder in ["inbox", "sent"] {
        map.set_folder(&format!("/telecom/msg/{folder}"))
            .expect("folder");
        let msgs = map.list_messages(100, 255).expect("list");
        let with_text = msgs.iter().filter(|m| !m.subject.is_empty()).count();
        let longest = msgs
            .iter()
            .map(|m| m.subject.chars().count())
            .max()
            .unwrap_or(0);
        println!(
            "{folder}: {} messages, {with_text} with text, longest preview {longest} chars",
            msgs.len()
        );
    }
    let _ = map.close();
}
