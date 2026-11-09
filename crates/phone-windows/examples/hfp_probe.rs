// SPDX-License-Identifier: GPL-3.0-or-later
//! Spike 4c: open our own hands-free link and watch the phone's calls for 60 seconds.
//! Read-only: it never answers, hangs up or dials. Numbers are masked to the last 3 digits.
//!   cargo run -p phone-windows --example hfp_probe -- AA:BB:CC:DD:EE:FF
#[cfg(windows)]
fn main() {
    use phone_core::platform::Platform;
    use std::time::{Duration, Instant};
    let addr = std::env::args().nth(1).expect("usage: hfp_probe <address>");
    let p = phone_windows::WindowsPlatform;
    println!("opening the hands-free link...");
    let end = Instant::now() + Duration::from_secs(60);
    let mut last = String::new();
    let mut first = true;
    while Instant::now() < end {
        match p.calls(&addr) {
            Ok(calls) => {
                if first {
                    println!("link is up; call the phone now (60 s)");
                    first = false;
                }
                let now: String = calls
                    .iter()
                    .map(|c| {
                        let n: String = c
                            .number
                            .chars()
                            .rev()
                            .take(3)
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect();
                        format!("[{:?} ...{n}]", c.state)
                    })
                    .collect();
                if now != last {
                    println!("calls: {}", if now.is_empty() { "none" } else { &now });
                    last = now;
                }
            }
            Err(e) => {
                let m = format!("error: {e}");
                if m != last {
                    println!("{m}");
                    last = m;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(700));
    }
}
#[cfg(not(windows))]
fn main() {}
