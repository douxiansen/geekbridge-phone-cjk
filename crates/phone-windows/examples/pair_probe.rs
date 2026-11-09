// SPDX-License-Identifier: GPL-3.0-or-later
//! Spike 1b: scan for nearby devices, pick one, pair. Put the phone's Bluetooth settings page
//! on screen first so it is discoverable.
#[cfg(windows)]
fn main() {
    use std::io::{stdin, stdout, Write};
    println!("Bluetooth: {:?}", phone_windows::bluetooth_state());
    println!("Scanning for 15 seconds...");
    let found = match phone_windows::scan(std::time::Duration::from_secs(15)) {
        Ok(f) => f,
        Err(e) => return eprintln!("scan failed: {e}"),
    };
    if found.is_empty() {
        return println!("nothing found (is the phone's Bluetooth page open?)");
    }
    for (i, d) in found.iter().enumerate() {
        println!("{i}: {}{}", d.name, if d.paired { " (paired)" } else { "" });
    }
    print!("Pair with which number? ");
    stdout().flush().unwrap();
    let mut line = String::new();
    stdin().read_line(&mut line).unwrap();
    let Some(d) = line.trim().parse::<usize>().ok().and_then(|i| found.get(i)) else {
        return println!("no such device");
    };
    let r = phone_windows::pair(&d.id, |pin| {
        match pin {
            Some(p) => println!("Check the phone shows {p}, then it will be accepted."),
            None => println!("Accept the request on the phone."),
        }
        true
    });
    println!("{r:?}");
}
#[cfg(not(windows))]
fn main() {}
