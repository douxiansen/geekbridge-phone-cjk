// SPDX-License-Identifier: GPL-3.0-or-later
//! A fake phone: an in-memory OBEX server that behaves like a phone's message
//! and phonebook services, for tests that must not need hardware.

use crate::packet::*;
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct FakeMessage {
    pub from: String,
    pub to: String,
    pub text: String,
    /// `yyyymmddThhmmssZ`
    pub datetime: String,
    pub read: bool,
}

#[derive(Debug, Default)]
pub struct FakeState {
    pub inbox: Vec<FakeMessage>,
    pub sent: Vec<FakeMessage>,
    pub contacts_vcf: String,
    /// When set, new sessions are refused with this OBEX code (0xC3 = forbidden).
    pub refuse_with: Option<u8>,
    pub sessions_opened: usize,
    pub clock: String,
}

/// Cloneable handle to the phone's state; each `connection()` is one session.
#[derive(Debug, Clone, Default)]
pub struct FakePhoneServer {
    pub state: Arc<Mutex<FakeState>>,
}

impl FakePhoneServer {
    pub fn new() -> Self {
        let s = Self::default();
        s.state.lock().unwrap().clock = "20260101T120000Z".into();
        s
    }

    pub fn connection(&self) -> FakeConnection {
        FakeConnection {
            state: self.state.clone(),
            out: VecDeque::new(),
            folder: vec![],
            connected: false,
            put_buf: Vec::new(),
        }
    }
}

pub struct FakeConnection {
    state: Arc<Mutex<FakeState>>,
    out: VecDeque<u8>,
    folder: Vec<String>,
    connected: bool,
    put_buf: Vec<u8>,
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn find(headers: &[Header], f: impl Fn(&Header) -> bool) -> Option<&Header> {
    headers.iter().find(|h| f(h))
}

impl FakeConnection {
    fn reply(&mut self, code: u8, fixed: &[u8], headers: &[Header]) {
        let p = encode_request(code, fixed, headers);
        self.out.extend(p);
    }

    fn listing(&self, folder: &str) -> String {
        let st = self.state.lock().unwrap();
        let (msgs, sent_folder) = if folder == "sent" {
            (&st.sent, true)
        } else {
            (&st.inbox, false)
        };
        let mut xml = String::from("<?xml version=\"1.0\"?>\n<MAP-msg-listing version=\"1.0\">\n");
        for (i, m) in msgs.iter().enumerate() {
            xml.push_str(&format!(
                " <msg handle=\"{i:04}\" subject=\"{}\" datetime=\"{}\" sender_addressing=\"{}\" recipient_addressing=\"{}\" type=\"SMS_GSM\" read=\"{}\" sent=\"{}\"/>\n",
                xml_escape(&m.text), m.datetime, xml_escape(&m.from), xml_escape(&m.to),
                if m.read { "yes" } else { "no" }, if sent_folder { "yes" } else { "no" }
            ));
        }
        xml.push_str("</MAP-msg-listing>\n");
        xml
    }

    fn handle(&mut self, packet: &[u8]) {
        let op = packet[0];
        let body = &packet[3..];
        match op {
            OP_CONNECT => {
                let refuse = self.state.lock().unwrap().refuse_with;
                if let Some(code) = refuse {
                    self.reply(code, &[], &[]);
                    return;
                }
                self.state.lock().unwrap().sessions_opened += 1;
                self.connected = true;
                self.reply(
                    RSP_SUCCESS,
                    &[0x10, 0x00, 0x20, 0x00],
                    &[Header::ConnectionId(1)],
                );
            }
            OP_DISCONNECT => {
                self.connected = false;
                self.reply(RSP_SUCCESS, &[], &[]);
            }
            OP_SETPATH => {
                let flags = body.first().copied().unwrap_or(0);
                let headers = decode_headers(body.get(2..).unwrap_or(&[])).unwrap_or_default();
                if flags & 1 == 1 {
                    self.folder.pop();
                } else if let Some(Header::Name(n)) =
                    find(&headers, |h| matches!(h, Header::Name(_)))
                {
                    if n.is_empty() {
                        self.folder.clear();
                    } else {
                        self.folder.push(n.clone());
                    }
                }
                self.reply(RSP_SUCCESS, &[], &[]);
            }
            OP_GET_FINAL => {
                let headers = decode_headers(body).unwrap_or_default();
                let ty = match find(&headers, |h| matches!(h, Header::Type(_))) {
                    Some(Header::Type(t)) => t.clone(),
                    _ => String::new(),
                };
                let name = match find(&headers, |h| matches!(h, Header::Name(_))) {
                    Some(Header::Name(n)) => n.clone(),
                    _ => String::new(),
                };
                let data = match ty.as_str() {
                    "x-bt/MAP-msg-listing" => self.listing(self.folder.last().map_or("inbox", String::as_str)),
                    "x-obex/folder-listing" => "<folder-listing><folder name=\"inbox\"/><folder name=\"sent\"/></folder-listing>".to_string(),
                    "x-bt/phonebook" if name.ends_with("pb.vcf") => self.state.lock().unwrap().contacts_vcf.clone(),
                    "x-bt/phonebook" => String::new(),
                    _ => {
                        self.reply(0xC4, &[], &[]);
                        return;
                    }
                };
                self.reply(RSP_SUCCESS, &[], &[Header::EndOfBody(data.into_bytes())]);
            }
            OP_PUT | OP_PUT_FINAL => {
                let headers = decode_headers(body).unwrap_or_default();
                let chunk = headers.iter().find_map(|h| match h {
                    Header::Body(b) | Header::EndOfBody(b) => Some(b.clone()),
                    _ => None,
                });
                if op == OP_PUT {
                    self.put_buf.extend(chunk.unwrap_or_default());
                    self.reply(RSP_CONTINUE, &[], &[]);
                    return;
                }
                let mut whole = std::mem::take(&mut self.put_buf);
                whole.extend(chunk.unwrap_or_default());
                let text = String::from_utf8_lossy(&whole).to_string();
                let tel = text
                    .lines()
                    .find_map(|l| l.trim().strip_prefix("TEL:"))
                    .unwrap_or("")
                    .to_string();
                let msg = crate::bmessage::parse_text(&whole).unwrap_or_default();
                let mut st = self.state.lock().unwrap();
                let now = st.clock.clone();
                st.sent.push(FakeMessage {
                    from: String::new(),
                    to: tel,
                    text: msg,
                    datetime: now,
                    read: true,
                });
                drop(st);
                self.reply(RSP_SUCCESS, &[], &[]);
            }
            _ => self.reply(0xD1, &[], &[]),
        }
    }
}

impl Read for FakeConnection {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.out.is_empty() {
            // A real socket would block; a closed fake ends the stream.
            return Ok(0);
        }
        let n = buf.len().min(self.out.len());
        for slot in buf.iter_mut().take(n) {
            *slot = self.out.pop_front().unwrap_or(0);
        }
        Ok(n)
    }
}

impl Write for FakeConnection {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.len() >= 3 {
            self.handle(buf);
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
