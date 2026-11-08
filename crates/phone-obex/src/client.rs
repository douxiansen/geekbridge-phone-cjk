// SPDX-License-Identifier: GPL-3.0-or-later
//! An OBEX client session over any byte stream.

use crate::packet::*;
use std::io::{Read, Write};

#[derive(Debug, thiserror::Error)]
pub enum ObexError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("malformed OBEX data: {0}")]
    Malformed(&'static str),
    #[error("the phone refused the request (OBEX response 0x{0:02X})")]
    Refused(u8),
    #[error("not connected")]
    NotConnected,
    #[error("unexpected response: {0}")]
    Protocol(&'static str),
}

impl ObexError {
    /// True for the codes a phone returns when the user has not allowed
    /// access (Forbidden 0xC3, Unauthorized 0xC1).
    pub fn is_access_denied(&self) -> bool {
        matches!(self, ObexError::Refused(0xC3) | ObexError::Refused(0xC1))
    }
}

/// The largest packet we are willing to receive.
const OUR_MAX_PACKET: u16 = 0x2000;

pub struct ObexClient<S: Read + Write> {
    stream: S,
    connection_id: Option<u32>,
    /// The largest packet the phone accepts from us.
    peer_max: usize,
}

impl<S: Read + Write> ObexClient<S> {
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            connection_id: None,
            peer_max: 255,
        }
    }

    pub fn into_inner(self) -> S {
        self.stream
    }

    fn read_packet(&mut self) -> Result<Vec<u8>, ObexError> {
        let mut head = [0u8; 3];
        self.stream.read_exact(&mut head)?;
        let len = u16::from_be_bytes([head[1], head[2]]) as usize;
        if len < 3 {
            return Err(ObexError::Malformed("packet shorter than its header"));
        }
        let mut packet = head.to_vec();
        packet.resize(len, 0);
        self.stream.read_exact(&mut packet[3..])?;
        Ok(packet)
    }

    fn exchange(&mut self, request: &[u8], fixed_len: usize) -> Result<Response, ObexError> {
        self.stream.write_all(request)?;
        self.stream.flush()?;
        let packet = self.read_packet()?;
        decode_response(&packet, fixed_len)
    }

    fn with_connection(&self, mut headers: Vec<Header>) -> Vec<Header> {
        if let Some(id) = self.connection_id {
            headers.insert(0, Header::ConnectionId(id));
        }
        headers
    }

    /// Opens an OBEX session to the service named by `target` (a 16-byte UUID).
    pub fn connect(&mut self, target: &[u8; 16]) -> Result<(), ObexError> {
        let mut fixed = vec![0x10, 0x00];
        fixed.extend_from_slice(&OUR_MAX_PACKET.to_be_bytes());
        let request = encode_request(OP_CONNECT, &fixed, &[Header::Target(target.to_vec())]);
        let rsp = self.exchange(&request, 4)?;
        if rsp.code != RSP_SUCCESS {
            return Err(ObexError::Refused(rsp.code));
        }
        if rsp.fixed.len() == 4 {
            let max = u16::from_be_bytes([rsp.fixed[2], rsp.fixed[3]]) as usize;
            // Leave room for packet and header overhead.
            self.peer_max = max.clamp(255, 0xFFFF);
        }
        self.connection_id = rsp.headers.iter().find_map(|h| match h {
            Header::ConnectionId(id) => Some(*id),
            _ => None,
        });
        Ok(())
    }

    pub fn disconnect(&mut self) -> Result<(), ObexError> {
        let request = encode_request(OP_DISCONNECT, &[], &self.with_connection(vec![]));
        let _ = self.exchange(&request, 0)?;
        self.connection_id = None;
        Ok(())
    }

    /// Changes folder. An empty `name` with `backup == false` goes to the root.
    pub fn set_path(&mut self, name: &str, backup: bool) -> Result<(), ObexError> {
        // Flags: bit 0 = back up one level, bit 1 = do not create. Never create.
        let flags = 0x02 | u8::from(backup);
        let headers = if backup {
            vec![]
        } else {
            vec![Header::Name(name.to_string())]
        };
        let request = encode_request(OP_SETPATH, &[flags, 0x00], &self.with_connection(headers));
        let rsp = self.exchange(&request, 0)?;
        if rsp.code != RSP_SUCCESS {
            return Err(ObexError::Refused(rsp.code));
        }
        Ok(())
    }

    /// Fetches an object, following Continue responses until it is whole.
    pub fn get(
        &mut self,
        type_: &str,
        name: Option<&str>,
        app_params: Option<Vec<u8>>,
    ) -> Result<Vec<u8>, ObexError> {
        let mut headers = vec![Header::Type(type_.to_string())];
        if let Some(n) = name {
            headers.push(Header::Name(n.to_string()));
        }
        if let Some(p) = app_params {
            headers.push(Header::AppParams(p));
        }
        let mut request = encode_request(OP_GET_FINAL, &[], &self.with_connection(headers));
        let mut data = Vec::new();
        loop {
            let rsp = self.exchange(&request, 0)?;
            for h in &rsp.headers {
                if let Header::Body(b) | Header::EndOfBody(b) = h {
                    data.extend_from_slice(b);
                }
            }
            match rsp.code {
                RSP_CONTINUE => {
                    // Ask for the next part: only the connection id.
                    request = encode_request(OP_GET_FINAL, &[], &self.with_connection(vec![]));
                }
                RSP_SUCCESS => return Ok(data),
                code => return Err(ObexError::Refused(code)),
            }
        }
    }

    /// Sends an object, splitting it into packets the phone will accept.
    pub fn put(
        &mut self,
        type_: &str,
        name: &str,
        app_params: Option<Vec<u8>>,
        body: &[u8],
    ) -> Result<(), ObexError> {
        let mut first = vec![
            Header::Name(name.to_string()),
            Header::Type(type_.to_string()),
        ];
        if let Some(p) = app_params {
            first.push(Header::AppParams(p));
        }
        let first = self.with_connection(first);
        let mut overhead = encode_request(OP_PUT, &[], &first).len() + 3;
        let mut rest = body;
        let mut is_first = true;
        loop {
            let room = self.peer_max.saturating_sub(overhead.max(8)).max(1);
            let take = room.min(rest.len());
            let (chunk, tail) = rest.split_at(take);
            let last = tail.is_empty();
            let mut headers = if is_first {
                first.clone()
            } else {
                self.with_connection(vec![])
            };
            headers.push(if last {
                Header::EndOfBody(chunk.to_vec())
            } else {
                Header::Body(chunk.to_vec())
            });
            let request = encode_request(if last { OP_PUT_FINAL } else { OP_PUT }, &[], &headers);
            let rsp = self.exchange(&request, 0)?;
            match (last, rsp.code) {
                (true, RSP_SUCCESS) => return Ok(()),
                (false, RSP_CONTINUE) => {}
                (_, code) => return Err(ObexError::Refused(code)),
            }
            rest = tail;
            is_first = false;
            overhead = 3 + 8;
        }
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use std::collections::VecDeque;
    use std::io::{Read, Write};

    /// A phone that answers with canned packets and records what it was sent.
    /// Answers one request packet with one response packet.
    pub type Responder = Box<dyn FnMut(&[u8]) -> Vec<u8>>;

    pub struct FakePhone {
        pub replies: VecDeque<Vec<u8>>,
        pub pending: Vec<u8>,
        pub sent: Vec<Vec<u8>>,
        /// When set, answers each request itself instead of using `replies`.
        pub responder: Option<Responder>,
    }

    impl FakePhone {
        pub fn new(replies: Vec<Vec<u8>>) -> Self {
            Self {
                replies: replies.into(),
                pending: vec![],
                sent: vec![],
                responder: None,
            }
        }
    }

    impl Read for FakePhone {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.pending.is_empty() {
                match self.replies.pop_front() {
                    Some(r) => self.pending = r,
                    None => return Ok(0),
                }
            }
            let n = buf.len().min(self.pending.len());
            buf[..n].copy_from_slice(&self.pending[..n]);
            self.pending.drain(..n);
            Ok(n)
        }
    }

    impl Write for FakePhone {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.sent.push(buf.to_vec());
            if let Some(r) = self.responder.as_mut() {
                let reply = r(buf);
                self.replies.push_back(reply);
            }
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::FakePhone;
    use super::*;

    const TARGET: [u8; 16] = [
        0xBB, 0x58, 0x2B, 0x40, 0x42, 0x0C, 0x11, 0xDB, 0xB0, 0xDE, 0x08, 0x00, 0x20, 0x0C, 0x9A,
        0x66,
    ];

    fn connect_reply(conn_id: u32, max: u16) -> Vec<u8> {
        let mut body = vec![0x10, 0x00];
        body.extend_from_slice(&max.to_be_bytes());
        body.push(0xCB);
        body.extend_from_slice(&conn_id.to_be_bytes());
        let mut p = vec![RSP_SUCCESS];
        p.extend_from_slice(&((body.len() + 3) as u16).to_be_bytes());
        p.extend(body);
        p
    }

    fn reply(code: u8, headers: &[Header]) -> Vec<u8> {
        encode_request(code, &[], headers)
    }

    #[test]
    fn connect_stores_connection_id_and_uses_it() {
        let phone = FakePhone::new(vec![connect_reply(0x1234, 0x1000), reply(RSP_SUCCESS, &[])]);
        let mut c = ObexClient::new(phone);
        c.connect(&TARGET).unwrap();
        c.set_path("telecom", false).unwrap();
        let sent = c.into_inner().sent;
        assert_eq!(sent[0][0], OP_CONNECT);
        // The folder change carries the connection id first.
        let hdrs = decode_headers(&sent[1][5..]).unwrap();
        assert_eq!(hdrs[0], Header::ConnectionId(0x1234));
        assert_eq!(hdrs[1], Header::Name("telecom".into()));
        assert_eq!(sent[1][3], 0x02, "flags: never create folders");
    }

    #[test]
    fn get_follows_continue_and_joins_body() {
        let phone = FakePhone::new(vec![
            connect_reply(1, 0x1000),
            reply(RSP_CONTINUE, &[Header::Body(b"hello ".to_vec())]),
            reply(RSP_SUCCESS, &[Header::EndOfBody(b"world".to_vec())]),
        ]);
        let mut c = ObexClient::new(phone);
        c.connect(&TARGET).unwrap();
        assert_eq!(c.get("x-bt/test", None, None).unwrap(), b"hello world");
    }

    #[test]
    fn a_phone_that_refuses_the_connection_is_reported_as_access_denied() {
        // Exactly the bytes an iPhone sends before message access is allowed.
        let phone = FakePhone::new(vec![vec![0xC3, 0x00, 0x07, 0x10, 0x00, 0x0F, 0xA0]]);
        let mut c = ObexClient::new(phone);
        let e = c.connect(&TARGET).unwrap_err();
        assert!(matches!(e, ObexError::Refused(0xC3)), "{e}");
        assert!(e.is_access_denied());
    }

    #[test]
    fn refused_get_reports_the_code() {
        let phone = FakePhone::new(vec![connect_reply(1, 0x1000), reply(0xC3, &[])]);
        let mut c = ObexClient::new(phone);
        c.connect(&TARGET).unwrap();
        let err = c.get("x", None, None).unwrap_err();
        assert!(err.is_access_denied());
    }

    #[test]
    fn put_splits_a_large_body_across_packets() {
        let body = vec![7u8; 1000];
        // The peer accepts 300-byte packets and, like a real phone, answers
        // Continue to a non-final Put and Success to the final one.
        let mut phone = FakePhone::new(vec![]);
        phone.responder = Some(Box::new(|req: &[u8]| match req[0] {
            OP_CONNECT => connect_reply(1, 300),
            OP_PUT_FINAL => reply(RSP_SUCCESS, &[]),
            _ => reply(RSP_CONTINUE, &[]),
        }));
        let mut c = ObexClient::new(phone);
        c.connect(&TARGET).unwrap();
        c.put("x-bt/message", "outbox", None, &body).unwrap();
        let sent = c.into_inner().sent;
        let puts = &sent[1..];
        assert!(puts.len() >= 4);
        assert!(puts.iter().all(|p| p.len() <= 300));
        assert_eq!(puts.last().unwrap()[0], OP_PUT_FINAL);
        let total: usize = puts
            .iter()
            .map(|p| {
                decode_headers(&p[3..])
                    .unwrap()
                    .iter()
                    .map(|h| match h {
                        Header::Body(b) | Header::EndOfBody(b) => b.len(),
                        _ => 0,
                    })
                    .sum::<usize>()
            })
            .sum();
        assert_eq!(total, 1000);
    }
}
