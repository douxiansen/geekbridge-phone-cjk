// SPDX-License-Identifier: GPL-3.0-or-later

//! Put the real overlay on the real screen and print every Action.
//!
//! ```text
//! cargo run --example demo [seconds]
//! ```
//!
//! Shows an incoming call, then a text banner with quick reply, switches the
//! call to active, and exits after `seconds` (default 8) leaving nothing on
//! screen.

use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use phone_overlay::{
    spawn, Action, Banner, CallBanner, CallState, OverlayConfig, OverlayError, ReplyStatus,
    TextBanner,
};

fn main() {
    let total = std::env::args()
        .nth(1)
        .and_then(|s| s.parse::<f32>().ok())
        .map_or(Duration::from_secs(8), Duration::from_secs_f32);

    let (overlay, actions) = match spawn(OverlayConfig::default()) {
        Ok(v) => v,
        Err(OverlayError::Unsupported) => {
            eprintln!("no layer-shell compositor here: fall back to a desktop notification");
            return;
        }
        Err(e) => {
            eprintln!("overlay failed to start: {e}");
            std::process::exit(1);
        }
    };

    let start = Instant::now();
    let call = |state| {
        Banner::Call(CallBanner {
            id: 1,
            name: "Ada Lovelace".into(),
            number: "+1 555 0100".into(),
            state,
        })
    };
    overlay.show(call(CallState::Incoming));
    println!(
        "[{:>5.2}s] showed incoming call",
        start.elapsed().as_secs_f32()
    );

    let mut stage = 0;
    loop {
        let t = start.elapsed();
        if t >= total {
            break;
        }
        // A tiny script driven by elapsed time.
        if stage == 0 && t >= Duration::from_millis(1800) {
            overlay.show(Banner::Text(TextBanner {
                id: 2,
                name: "Grace Hopper".into(),
                number: "+1 555 0199".into(),
                body: "Are we still on for lunch? I found a great place near the harbour.".into(),
                can_reply: true,
            }));
            println!("[{:>5.2}s] showed text banner", t.as_secs_f32());
            stage = 1;
        }
        if stage == 1 && t >= Duration::from_millis(3600) {
            overlay.show(call(CallState::Active {
                since: Instant::now(),
            }));
            println!("[{:>5.2}s] call -> active", t.as_secs_f32());
            stage = 2;
        }
        if stage == 2 && t + Duration::from_millis(1400) >= total {
            overlay.remove(1);
            overlay.remove(2);
            println!("[{:>5.2}s] removing banners", t.as_secs_f32());
            stage = 3;
        }
        match actions.recv_timeout(Duration::from_millis(50)) {
            Ok(action) => {
                println!(
                    "[{:>5.2}s] action: {action:?}",
                    start.elapsed().as_secs_f32()
                );
                if let Action::Reply { id, .. } = action {
                    overlay.reply_status(id, ReplyStatus::Sending);
                    std::thread::sleep(Duration::from_millis(400));
                    overlay.reply_status(id, ReplyStatus::Sent);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    // Let the slide-out finish (DEMO_LINGER seconds, default 0.5), then drop
    // the handle: the presenter exits.
    let linger = std::env::var("DEMO_LINGER")
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .map_or(Duration::from_millis(500), Duration::from_secs_f32);
    std::thread::sleep(linger);
    drop(overlay);
    println!("done");
}
