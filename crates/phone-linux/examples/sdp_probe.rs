// SPDX-License-Identifier: GPL-3.0-or-later
use phone_linux::sdp::{find_channel, service};

fn main() {
    let addr = std::env::args().nth(1).expect("usage: sdp_probe <address>");
    for (name, uuid) in [("MAP message service", service::MAP_MAS), ("PBAP phonebook", service::PBAP_PSE), ("Hands-free gateway", service::HANDSFREE_AG)] {
        println!("{name}: {:?}", find_channel(&addr, uuid));
    }
}
