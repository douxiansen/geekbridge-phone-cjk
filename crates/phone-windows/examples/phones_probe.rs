// SPDX-License-Identifier: GPL-3.0-or-later
//! Spike 1: list the phones Windows has paired.
#[cfg(windows)]
fn main() {
    use phone_core::platform::Platform;
    match phone_windows::WindowsPlatform.phones() {
        Ok(p) if p.is_empty() => println!("no paired Bluetooth devices"),
        Ok(p) => p.iter().for_each(|d| println!("{d:?}")),
        Err(e) => eprintln!("error: {e}"),
    }
}
#[cfg(not(windows))]
fn main() {}
