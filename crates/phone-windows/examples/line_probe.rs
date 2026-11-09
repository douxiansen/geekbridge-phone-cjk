// SPDX-License-Identifier: GPL-3.0-or-later
//! Does Windows have a phone line for this phone right now? (Read only.)
//!   cargo run -p phone-windows --example line_probe -- AA:BB:CC:DD:EE:FF
#[cfg(windows)]
fn main() {
    let addr = std::env::args()
        .nth(1)
        .expect("usage: line_probe <address>");
    println!("packaged app: {}", phone_windows::is_packaged());
    println!(
        "phone line exists: {}",
        phone_windows::phone_line_exists(&addr)
    );
    println!(
        "registering: {:?}",
        phone_windows::ensure_registered(&addr).map_err(|e| e.to_string())
    );
}
#[cfg(not(windows))]
fn main() {}
