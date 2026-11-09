// SPDX-License-Identifier: GPL-3.0-or-later
//! Spike 3: pull the phonebook and call history; counts and sizes only, no names or numbers.
//!   cargo run -p phone-windows --example pbap_probe -- AA:BB:CC:DD:EE:FF
#[cfg(windows)]
fn main() {
    use phone_core::platform::Platform;
    use phone_obex::{Book, PbapClient};
    let addr = std::env::args()
        .nth(1)
        .expect("usage: pbap_probe <address>");
    let stream = match phone_windows::WindowsPlatform.open_pbap(&addr) {
        Ok(s) => s,
        Err(e) => return eprintln!("open failed: {e}"),
    };
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
            Ok(t) => println!(
                "{name}: {} bytes, {} cards",
                t.len(),
                t.matches("BEGIN:VCARD").count()
            ),
            Err(e) => println!("{name}: failed: {e}"),
        }
    }
}
#[cfg(not(windows))]
fn main() {}
