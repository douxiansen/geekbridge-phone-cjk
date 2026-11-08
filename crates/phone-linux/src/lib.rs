// SPDX-License-Identifier: GPL-3.0-or-later
//! Linux transport: an RFCOMM stream to a paired phone, straight from the
//! kernel's Bluetooth sockets. BlueZ must be running and the phone paired.

pub mod dbus;
pub mod platform;
pub mod recorder;
pub mod sdp;

pub use platform::LinuxPlatform;

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

/// How long a Bluetooth connect may take.
const CONNECT_TIMEOUT_MS: libc::c_int = 15_000;
/// How long one read or write may stall before the link is considered dead.
const IO_TIMEOUT_SECS: libc::time_t = 12;

/// Connects `fd` to `addr` without hanging forever, then makes it blocking
/// with read/write timeouts.
///
/// # Safety
/// `fd` must be an open socket and `addr` must point to `len` valid bytes.
pub(crate) unsafe fn connect_with_timeout(
    fd: libc::c_int,
    addr: *const libc::sockaddr,
    len: libc::socklen_t,
) -> io::Result<()> {
    let flags = libc::fcntl(fd, libc::F_GETFL);
    if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
        return Err(io::Error::last_os_error());
    }
    if libc::connect(fd, addr, len) < 0 {
        let err = io::Error::last_os_error();
        if err.raw_os_error() != Some(libc::EINPROGRESS) {
            return Err(err);
        }
        let mut pfd = libc::pollfd {
            fd,
            events: libc::POLLOUT,
            revents: 0,
        };
        let n = libc::poll(&mut pfd, 1, CONNECT_TIMEOUT_MS);
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Bluetooth connect timed out",
            ));
        }
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut so_err: libc::c_int = 0;
        let mut so_len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
        if libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_ERROR,
            &mut so_err as *mut _ as *mut libc::c_void,
            &mut so_len,
        ) < 0
        {
            return Err(io::Error::last_os_error());
        }
        if so_err != 0 {
            return Err(io::Error::from_raw_os_error(so_err));
        }
    }
    if libc::fcntl(fd, libc::F_SETFL, flags) < 0 {
        return Err(io::Error::last_os_error());
    }
    let tv = libc::timeval {
        tv_sec: IO_TIMEOUT_SECS,
        tv_usec: 0,
    };
    for opt in [libc::SO_RCVTIMEO, libc::SO_SNDTIMEO] {
        if libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            opt,
            &tv as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::timeval>() as libc::socklen_t,
        ) < 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
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
    rfcomm_connect(address, channel)
}

/// Connects to `channel` on the phone at `address`.
pub fn rfcomm_connect(address: &str, channel: u8) -> io::Result<File> {
    let bdaddr = parse_address(address)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "bad Bluetooth address"))?;
    // SAFETY: plain socket(2)/connect(2) calls; the fd is owned from here on.
    unsafe {
        let fd = libc::socket(
            AF_BLUETOOTH,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC,
            BTPROTO_RFCOMM,
        );
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let owned = OwnedFd::from_raw_fd(fd);
        let addr = SockaddrRc {
            family: AF_BLUETOOTH as libc::sa_family_t,
            bdaddr,
            channel,
        };
        connect_with_timeout(
            fd,
            &addr as *const SockaddrRc as *const libc::sockaddr,
            std::mem::size_of::<SockaddrRc>() as libc::socklen_t,
        )?;
        Ok(File::from(owned))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_is_reversed_for_the_kernel() {
        assert_eq!(
            parse_address("AA:BB:CC:DD:EE:FF"),
            Some([0xDA, 0x46, 0x5C, 0xC2, 0xAC, 0x6C])
        );
        assert_eq!(parse_address("nope"), None);
        assert_eq!(parse_address("AA:BB:CC:DD:EE"), None);
    }
}
