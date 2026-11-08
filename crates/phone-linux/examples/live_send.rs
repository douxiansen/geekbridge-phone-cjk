// SPDX-License-Identifier: GPL-3.0-or-later
//! Sends one text through a real phone. Only for a number you own or have
//! permission to text.
//!
//!   cargo run -p phone-linux --example live_send -- <address> <mas-channel> <number> <text>

use phone_obex::MapClient;

fn main() {
    let mut a = std::env::args().skip(1);
    let (addr, channel, number, text) = (
        a.next().expect("address"),
        a.next().expect("channel").parse::<u8>().expect("channel"),
        a.next().expect("number"),
        a.next().expect("text"),
    );
    let stream = phone_linux::connect(&addr, channel).expect("RFCOMM connect");
    let mut map = MapClient::connect(stream).expect("MAP connect");
    map.send_sms(&number, &text).expect("send");
    println!("pushed to the phone's outbox");
    // Show the phone's own record of it (count only).
    map.set_folder("/telecom/msg/sent").expect("folder");
    let sent = map.list_messages(5, 40).expect("list");
    println!("sent folder now has {} recent message(s)", sent.len());
    let _ = map.close();
}
