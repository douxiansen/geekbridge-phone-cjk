// SPDX-License-Identifier: GPL-3.0-or-later
//! Pulls a phone's phonebook and call history and prints only counts and sizes (no names or
//! numbers), to see what a phone returns.
//!
//!   cargo run -p phone-linux --example pbap_probe -- <address>

use phone_linux::sdp::{find_channel, service};
use phone_obex::{Book, PbapClient};

fn main() {
    let addr = std::env::args().nth(1).expect("address");
    let ch = find_channel(&addr, service::PBAP_PSE)
        .expect("sdp")
        .expect("phonebook service not offered");
    let stream = phone_linux::rfcomm_connect(&addr, ch).expect("rfcomm");
    let mut pbap = match PbapClient::connect(stream) {
        Ok(p) => p,
        Err(e) => {
            return println!(
                "connect failed: {e} (access denied: {})",
                e.is_access_denied()
            )
        }
    };
    for (name, book) in [
        ("contacts", Book::Contacts),
        ("received", Book::Received),
        ("dialed", Book::Dialed),
        ("missed", Book::Missed),
    ] {
        match pbap.pull(book, 500) {
            Ok(text) => {
                let cards = text.matches("BEGIN:VCARD").count();
                println!("{name}: {} bytes, {cards} cards", text.len());
            }
            Err(e) => println!("{name}: failed: {e}"),
        }
    }
}
