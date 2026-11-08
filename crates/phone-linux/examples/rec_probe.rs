// SPDX-License-Identifier: GPL-3.0-or-later
//! Run this DURING a call to see whether the phone's voice stream is found,
//! and to record 8 seconds of it:
//!
//!   cargo run -p phone-linux --example rec_probe -- AA:BB:CC:DD:EE:FF

use phone_core::platform::Platform;

fn main() {
    let addr = std::env::args().nth(1).expect("usage: rec_probe <address>");
    let out = std::env::temp_dir().join("gbp-rec-probe");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).expect("temp dir");
    let platform = phone_linux::LinuxPlatform::new().expect("buses");
    let rec = platform.start_recording(&addr, &out).expect("start");
    println!("recording 8 s into {} ...", out.display());
    std::thread::sleep(std::time::Duration::from_secs(8));
    match rec.finish() {
        Ok(a) => println!("remote: {:?}\nlocal:  {:?}", a.remote, a.local),
        Err(e) => println!("failed: {e}"),
    }
}
