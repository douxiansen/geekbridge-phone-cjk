// SPDX-License-Identifier: GPL-3.0-or-later
//! A minimal SDP client: asks a paired phone which RFCOMM channel a service
//! lives on (the channel differs per phone and per boot).

use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{FromRawFd, OwnedFd};

const AF_BLUETOOTH: libc::c_int = 31;
const BTPROTO_L2CAP: libc::c_int = 0;
const SDP_PSM: u16 = 1;

const PDU_SEARCH_ATTR_REQ: u8 = 0x06;
const PDU_SEARCH_ATTR_RSP: u8 = 0x07;
const PDU_ERROR: u8 = 0x01;
const ATTR_PROTOCOL_DESCRIPTOR_LIST: u16 = 0x0004;
const UUID_RFCOMM: u16 = 0x0003;

/// 16-bit service class ids.
pub mod service {
    pub const MAP_MAS: u16 = 0x1132;
    pub const PBAP_PSE: u16 = 0x112F;
    pub const HANDSFREE_AG: u16 = 0x111F;
}

#[repr(C)]
struct SockaddrL2 {
    family: libc::sa_family_t,
    psm: u16,
    bdaddr: [u8; 6],
    cid: u16,
    bdaddr_type: u8,
}

/// An SDP data element, as much as we need to walk a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Element {
    Nil,
    Uint(u64),
    Uuid(u16),
    Text(Vec<u8>),
    Seq(Vec<Element>),
    Other,
}

/// Parses one data element, returning it and the bytes consumed.
pub fn parse_element(data: &[u8]) -> Option<(Element, usize)> {
    let head = *data.first()?;
    let (ty, size_idx) = (head >> 3, head & 7);
    let (len, hdr) = match size_idx {
        0 => (if ty == 0 { 0 } else { 1 }, 1),
        1 => (2, 1),
        2 => (4, 1),
        3 => (8, 1),
        4 => (16, 1),
        5 => (*data.get(1)? as usize, 2),
        6 => (
            u16::from_be_bytes([*data.get(1)?, *data.get(2)?]) as usize,
            3,
        ),
        _ => (
            u32::from_be_bytes(data.get(1..5)?.try_into().ok()?) as usize,
            5,
        ),
    };
    let body = data.get(hdr..hdr + len)?;
    let element = match ty {
        0 => Element::Nil,
        1 | 2 if len <= 8 => Element::Uint(body.iter().fold(0u64, |a, b| (a << 8) | *b as u64)),
        3 if len == 2 => Element::Uuid(u16::from_be_bytes([body[0], body[1]])),
        4 | 8 => Element::Text(body.to_vec()),
        6 | 7 => {
            let mut items = Vec::new();
            let mut rest = body;
            while !rest.is_empty() {
                let (e, used) = parse_element(rest)?;
                items.push(e);
                rest = &rest[used..];
            }
            Element::Seq(items)
        }
        _ => Element::Other,
    };
    Some((element, hdr + len))
}

/// Finds the RFCOMM channel in a ProtocolDescriptorList value.
fn rfcomm_channel(list: &Element) -> Option<u8> {
    let Element::Seq(protocols) = list else {
        return None;
    };
    protocols.iter().find_map(|p| match p {
        Element::Seq(parts) if parts.first() == Some(&Element::Uuid(UUID_RFCOMM)) => {
            match parts.get(1) {
                Some(Element::Uint(ch)) => u8::try_from(*ch).ok(),
                _ => None,
            }
        }
        _ => None,
    })
}

/// Walks the attribute lists of a response and returns the first RFCOMM channel.
pub fn channel_from_attribute_lists(data: &[u8]) -> Option<u8> {
    let (Element::Seq(records), _) = parse_element(data)? else {
        return None;
    };
    for record in records {
        let Element::Seq(attrs) = record else {
            continue;
        };
        for pair in attrs.chunks(2) {
            if let [Element::Uint(id), value] = pair {
                if *id == ATTR_PROTOCOL_DESCRIPTOR_LIST as u64 {
                    if let Some(ch) = rfcomm_channel(value) {
                        return Some(ch);
                    }
                }
            }
        }
    }
    None
}

fn build_request(tid: u16, service: u16, continuation: &[u8]) -> Vec<u8> {
    let mut params = Vec::new();
    // ServiceSearchPattern: a sequence holding one 16-bit UUID.
    params.extend_from_slice(&[0x35, 0x03, 0x19]);
    params.extend_from_slice(&service.to_be_bytes());
    params.extend_from_slice(&0xFFFFu16.to_be_bytes()); // max attribute bytes
                                                        // AttributeIDList: a sequence holding the one attribute id we want.
    params.extend_from_slice(&[0x35, 0x03, 0x09]);
    params.extend_from_slice(&ATTR_PROTOCOL_DESCRIPTOR_LIST.to_be_bytes());
    params.push(continuation.len() as u8);
    params.extend_from_slice(continuation);
    let mut out = vec![PDU_SEARCH_ATTR_REQ];
    out.extend_from_slice(&tid.to_be_bytes());
    out.extend_from_slice(&(params.len() as u16).to_be_bytes());
    out.extend_from_slice(&params);
    out
}

fn connect_l2cap(bdaddr: [u8; 6]) -> io::Result<File> {
    // SAFETY: plain socket(2)/connect(2) calls; the fd is owned from here on.
    unsafe {
        let fd = libc::socket(
            AF_BLUETOOTH,
            libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
            BTPROTO_L2CAP,
        );
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let owned = OwnedFd::from_raw_fd(fd);
        let addr = SockaddrL2 {
            family: AF_BLUETOOTH as libc::sa_family_t,
            psm: SDP_PSM.to_le(),
            bdaddr,
            cid: 0,
            bdaddr_type: 0,
        };
        crate::connect_with_timeout(
            fd,
            &addr as *const SockaddrL2 as *const libc::sockaddr,
            std::mem::size_of::<SockaddrL2>() as libc::socklen_t,
        )?;
        Ok(File::from(owned))
    }
}

/// Asks the phone at `address` for the RFCOMM channel of `service`.
pub fn find_channel(address: &str, service: u16) -> io::Result<Option<u8>> {
    let bdaddr = crate::parse_address(address)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "bad Bluetooth address"))?;
    let mut sock = connect_l2cap(bdaddr)?;
    let mut collected: Vec<u8> = Vec::new();
    let mut continuation: Vec<u8> = Vec::new();
    for tid in 1u16..=16 {
        sock.write_all(&build_request(tid, service, &continuation))?;
        let mut buf = vec![0u8; 4096];
        let n = sock.read(&mut buf)?;
        let rsp = &buf[..n];
        if rsp.len() < 5 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "short SDP response",
            ));
        }
        match rsp[0] {
            PDU_SEARCH_ATTR_RSP => {}
            PDU_ERROR => return Ok(None),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unexpected SDP PDU",
                ))
            }
        }
        let count = u16::from_be_bytes([rsp[5], rsp[6]]) as usize;
        let lists = rsp.get(7..7 + count).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "truncated SDP attributes")
        })?;
        collected.extend_from_slice(lists);
        let cont_len = *rsp.get(7 + count).unwrap_or(&0) as usize;
        if cont_len == 0 {
            return Ok(channel_from_attribute_lists(&collected));
        }
        continuation = rsp[8 + count..8 + count + cont_len].to_vec();
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_rfcomm_channel_in_a_record() {
        // [ [ attr 0x0004 = [ [L2CAP], [RFCOMM, 7], [OBEX] ] ] ]
        let l2cap = [0x35, 0x03, 0x19, 0x01, 0x00];
        let rfcomm = [0x35, 0x05, 0x19, 0x00, 0x03, 0x08, 0x07];
        let obex = [0x35, 0x03, 0x19, 0x00, 0x08];
        let mut protos = vec![];
        protos.extend_from_slice(&l2cap);
        protos.extend_from_slice(&rfcomm);
        protos.extend_from_slice(&obex);
        let mut record = vec![0x09, 0x00, 0x04, 0x35, protos.len() as u8];
        record.extend(protos);
        let mut lists = vec![0x35, (record.len() + 2) as u8, 0x35, record.len() as u8];
        lists.extend(record);
        assert_eq!(channel_from_attribute_lists(&lists), Some(7));
    }

    #[test]
    fn no_rfcomm_means_none() {
        let record = [0x35, 0x08, 0x09, 0x00, 0x04, 0x35, 0x03, 0x19, 0x01, 0x00];
        let mut lists = vec![0x35, (record.len() + 2) as u8, 0x35, record.len() as u8];
        lists.extend_from_slice(&record);
        // Fix the inner length byte for the record we built by hand.
        assert_eq!(channel_from_attribute_lists(&lists), None);
        assert_eq!(channel_from_attribute_lists(&[]), None);
        assert_eq!(channel_from_attribute_lists(&[0x35, 0xFF]), None);
    }

    #[test]
    fn request_encodes_the_service_uuid() {
        let r = build_request(1, service::MAP_MAS, &[]);
        assert_eq!(r[0], PDU_SEARCH_ATTR_REQ);
        assert!(r.windows(2).any(|w| w == [0x11, 0x32]));
        assert_eq!(u16::from_be_bytes([r[3], r[4]]) as usize, r.len() - 5);
    }
}
