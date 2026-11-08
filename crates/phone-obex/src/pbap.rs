// SPDX-License-Identifier: GPL-3.0-or-later
//! Bluetooth Phone Book Access Profile (PBAP) client: pull the phonebook and
//! the call history as vCard text.

use crate::client::{ObexClient, ObexError};
use std::io::{Read, Write};

/// The PBAP Phonebook Server Equipment target UUID.
pub const PSE_TARGET: [u8; 16] = [
    0x79, 0x61, 0x35, 0xF0, 0xF0, 0xC5, 0x11, 0xD8, 0x09, 0x66, 0x08, 0x00, 0x20, 0x0C, 0x9A, 0x66,
];

const TYPE_PHONEBOOK: &str = "x-bt/phonebook";

const TAG_FORMAT: u8 = 0x07;
const TAG_MAX_LIST_COUNT: u8 = 0x04;
const TAG_START_OFFSET: u8 = 0x05;
const FORMAT_VCARD_30: u8 = 0x01;

/// The objects a phone offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Book {
    Contacts,
    Received,
    Dialed,
    Missed,
}

impl Book {
    fn object(self) -> &'static str {
        match self {
            Book::Contacts => "telecom/pb.vcf",
            Book::Received => "telecom/ich.vcf",
            Book::Dialed => "telecom/och.vcf",
            Book::Missed => "telecom/mch.vcf",
        }
    }
}

fn param(out: &mut Vec<u8>, tag: u8, value: &[u8]) {
    out.push(tag);
    out.push(value.len() as u8);
    out.extend_from_slice(value);
}

pub struct PbapClient<S: Read + Write> {
    obex: ObexClient<S>,
}

impl<S: Read + Write> PbapClient<S> {
    pub fn connect(stream: S) -> Result<Self, ObexError> {
        let mut obex = ObexClient::new(stream);
        obex.connect(&PSE_TARGET)?;
        Ok(Self { obex })
    }

    /// Pulls a whole book as vCard 3.0 text.
    pub fn pull(&mut self, book: Book, max: u16) -> Result<String, ObexError> {
        let mut params = Vec::new();
        param(&mut params, TAG_FORMAT, &[FORMAT_VCARD_30]);
        param(&mut params, TAG_MAX_LIST_COUNT, &max.to_be_bytes());
        param(&mut params, TAG_START_OFFSET, &0u16.to_be_bytes());
        let data = self.obex.get(TYPE_PHONEBOOK, Some(book.object()), Some(params))?;
        Ok(String::from_utf8_lossy(&data).into_owned())
    }

    pub fn close(mut self) -> S {
        let _ = self.obex.disconnect();
        self.obex.into_inner()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::testing::FakePhone;
    use crate::packet::*;

    #[test]
    fn pulls_the_contact_book() {
        let mut ok_connect = vec![0x10, 0x00, 0x20, 0x00, 0xCB, 0, 0, 0, 3];
        let mut c = vec![RSP_SUCCESS];
        c.extend_from_slice(&((ok_connect.len() + 3) as u16).to_be_bytes());
        c.append(&mut ok_connect);
        let vcf = b"BEGIN:VCARD\r\nFN:Ali\r\nTEL:+1\r\nEND:VCARD\r\n".to_vec();
        let phone = FakePhone::new(vec![c, encode_request(RSP_SUCCESS, &[], &[Header::EndOfBody(vcf)])]);
        let mut pbap = PbapClient::connect(phone).unwrap();
        let text = pbap.pull(Book::Contacts, 500).unwrap();
        assert!(text.contains("FN:Ali"));
        let sent = pbap.close().sent;
        let hdrs = decode_headers(&sent[1][3..]).unwrap();
        assert!(hdrs.contains(&Header::Name("telecom/pb.vcf".into())));
        assert!(hdrs.contains(&Header::Type("x-bt/phonebook".into())));
    }
}
