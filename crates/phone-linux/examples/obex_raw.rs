// SPDX-License-Identifier: GPL-3.0-or-later
//! Dumps the raw OBEX Connect reply of a phone's MAP or PBAP service, as hex (protocol bytes
//! only, no personal data), to debug phones that answer in an unusual way.
//!
//!   cargo run -p phone-linux --example obex_raw -- <address> map|pbap

use phone_linux::sdp::{find_channel, service};
use phone_obex::packet::{encode_request, Header, OP_CONNECT};
use std::io::{Read, Write};

fn main() {
    let mut a = std::env::args().skip(1);
    let addr = a.next().expect("address");
    let which = a.next().unwrap_or_else(|| "map".into());
    let (uuid, target) = match which.as_str() {
        "pbap" => (service::PBAP_PSE, phone_obex::pbap::PSE_TARGET),
        _ => (service::MAP_MAS, phone_obex::map::MAS_TARGET),
    };
    let channel = find_channel(&addr, uuid)
        .expect("sdp")
        .expect("service not offered");
    println!("{which}: RFCOMM channel {channel}");
    let mut s = phone_linux::rfcomm_connect(&addr, channel).expect("rfcomm");
    let mut fixed = vec![0x10, 0x00];
    fixed.extend_from_slice(&0x2000u16.to_be_bytes());
    let req = encode_request(OP_CONNECT, &fixed, &[Header::Target(target.to_vec())]);
    println!("sent    ({} bytes): {}", req.len(), hex(&req));
    s.write_all(&req).expect("write");
    let mut buf = [0u8; 4096];
    let n = s.read(&mut buf).expect("read");
    println!("reply   ({} bytes): {}", n, hex(&buf[..n]));
}

fn hex(b: &[u8]) -> String {
    b.iter()
        .map(|x| format!("{x:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}
