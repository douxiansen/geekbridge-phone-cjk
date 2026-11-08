// SPDX-License-Identifier: GPL-3.0-or-later
use phone_core::platform::Platform;

fn main() {
    let p = phone_linux::LinuxPlatform::new().expect("buses");
    let phones = p.phones().expect("phones");
    for ph in &phones {
        println!("phone: {} [{}] connected={} battery={:?}", ph.name, ph.address, ph.connected, ph.battery);
        match p.calls(&ph.address) {
            Ok(c) => println!("  calls now: {}", c.len()),
            Err(e) => println!("  calls: {e}"),
        }
    }
}
