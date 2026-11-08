// SPDX-License-Identifier: GPL-3.0-or-later
//! Linux transport: an RFCOMM stream to a paired phone, straight from the
//! kernel's Bluetooth sockets. BlueZ must be running and the phone paired.

use std::fs::File;
use std::io;
use std::os::fd::{FromRawFd, OwnedFd};

const AF_BLUETOOTH: libc::c_int = 31;
const BTPROTO_RFCOMM: libc::c_int = 3;

#[repr(C)]
struct SockaddrRc {
    family: libc::sa_family_t,
    bdaddr: [u8; 6], // little-endian: the last byte of the text address first
    channel: u8,
}

/// Parses `AA:BB:CC:DD:EE:FF` into the kernel's reversed byte order.
pub fn parse_address(text: &str) -> Option<[u8; 6]> {
    let parts: Vec<&str> = text.split(':').collect();
    if parts.len() != 6 {
        return None;
    }
    let mut out = [0u8; 6];
    for (i, p) in parts.iter().enumerate() {
        out[5 - i] = u8::from_str_radix(p, 16).ok()?;
    }
    Some(out)
}

/// Connects to `channel` on the phone at `address`.
pub fn connect(address: &str, channel: u8) -> io::Result<File> {
    let bdaddr = parse_address(address)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "bad Bluetooth address"))?;
    // SAFETY: plain socket(2)/connect(2) calls; the fd is owned from here on.
    unsafe {
        let fd = libc::socket(AF_BLUETOOTH, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, BTPROTO_RFCOMM);
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let owned = OwnedFd::from_raw_fd(fd);
        let addr = SockaddrRc { family: AF_BLUETOOTH as libc::sa_family_t, bdaddr, channel };
        let rc = libc::connect(
            fd,
            &addr as *const SockaddrRc as *const libc::sockaddr,
            std::mem::size_of::<SockaddrRc>() as libc::socklen_t,
        );
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(File::from(owned))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_is_reversed_for_the_kernel() {
        assert_eq!(parse_address("AA:BB:CC:DD:EE:FF"), Some([0xDA, 0x46, 0x5C, 0xC2, 0xAC, 0x6C]));
        assert_eq!(parse_address("nope"), None);
        assert_eq!(parse_address("AA:BB:CC:DD:EE"), None);
    }
}
