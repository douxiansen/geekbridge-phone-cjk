// SPDX-License-Identifier: GPL-3.0-or-later
//! Spike 2: open the phone's message service and print counts only, never message text.
//!   cargo run -p phone-windows --example map_probe -- AA:BB:CC:DD:EE:FF
#[cfg(windows)]
fn main() {
    use phone_core::platform::Platform;
    let addr = std::env::args().nth(1).expect("usage: map_probe <address>");
    eprintln!("opening message service...");
    let stream = match phone_windows::WindowsPlatform.open_map(&addr) {
        Ok(s) => s,
        Err(e) => return eprintln!("open failed: {e}"),
    };
    eprintln!("connected; opening MAP session...");
    let mut map = match phone_obex::MapClient::connect(stream) {
        Ok(m) => m,
        Err(e) => {
            return eprintln!(
                "MAP connect failed: {e} (access denied: {})",
                e.is_access_denied()
            )
        }
    };
    map.set_folder("/telecom/msg").expect("set folder");
    println!("folders: {:?}", map.list_folders().expect("folders"));
    for f in ["inbox", "sent"] {
        map.set_folder(&format!("/telecom/msg/{f}"))
            .expect("folder");
        let msgs = map.list_messages(100, 255).expect("list");
        println!("{f}: {} messages", msgs.len());
    }
    let _ = map.close();
}
#[cfg(not(windows))]
fn main() {}
