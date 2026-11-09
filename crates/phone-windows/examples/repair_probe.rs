// SPDX-License-Identifier: GPL-3.0-or-later
//! Removes the pairing with a phone and pairs it again, so the phone shows its permission
//! prompts afresh. Keep the phone's Bluetooth page open.
//!   cargo run -p phone-windows --example repair_probe -- AA:BB:CC:DD:EE:FF
#[cfg(windows)]
fn main() {
    use std::time::Duration;
    let addr = std::env::args()
        .nth(1)
        .expect("usage: repair_probe <address>");
    match phone_windows::paired_id(&addr) {
        Ok(Some(id)) => println!("unpair: {:?}", phone_windows::unpair(&id)),
        other => println!("not paired now: {other:?}"),
    }
    let want = addr.to_lowercase();
    for attempt in 1..=4 {
        println!("scan {attempt}...");
        let found = phone_windows::scan(Duration::from_secs(15)).unwrap_or_default();
        if let Some(d) = found.iter().find(|d| d.id.to_lowercase().contains(&want)) {
            println!("found {}; pairing, check the number on the phone", d.name);
            let r = phone_windows::pair(&d.id, |pin| {
                println!("number: {pin:?}");
                true
            });
            return println!("pair: {r:?}");
        }
    }
    println!("phone not found; open its Bluetooth page and try again");
}
#[cfg(not(windows))]
fn main() {}
