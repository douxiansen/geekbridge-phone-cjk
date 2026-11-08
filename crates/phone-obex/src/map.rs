// SPDX-License-Identifier: GPL-3.0-or-later
//! Bluetooth Message Access Profile (MAP) client: list and send text messages.

use crate::bmessage;
use crate::client::{ObexClient, ObexError};
use std::io::{Read, Write};

/// The MAP Message Access Server target UUID.
pub const MAS_TARGET: [u8; 16] = [
    0xBB, 0x58, 0x2B, 0x40, 0x42, 0x0C, 0x11, 0xDB, 0xB0, 0xDE, 0x08, 0x00, 0x20, 0x0C, 0x9A, 0x66,
];

const TYPE_FOLDER_LISTING: &str = "x-obex/folder-listing";
const TYPE_MSG_LISTING: &str = "x-bt/MAP-msg-listing";
const TYPE_MESSAGE: &str = "x-bt/message";

const TAG_MAX_LIST_COUNT: u8 = 0x01;
const TAG_START_OFFSET: u8 = 0x02;
const TAG_SUBJECT_LENGTH: u8 = 0x13;
const TAG_CHARSET: u8 = 0x14;
const CHARSET_UTF8: u8 = 0x01;

/// One row of a message listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageEntry {
    pub handle: String,
    /// The start of the text. For SMS, phones put the body here.
    pub subject: String,
    /// `yyyymmddThhmmss`, optionally with a zone suffix.
    pub datetime: String,
    pub sender_name: String,
    pub sender_address: String,
    pub recipient_name: String,
    pub recipient_address: String,
    pub kind: String,
    pub read: bool,
    pub sent: bool,
}

fn app_param(out: &mut Vec<u8>, tag: u8, value: &[u8]) {
    out.push(tag);
    out.push(value.len() as u8);
    out.extend_from_slice(value);
}

/// Parses a MAP message listing document.
pub fn parse_message_listing(xml: &[u8]) -> Result<Vec<MessageEntry>, ObexError> {
    let text = std::str::from_utf8(xml).map_err(|_| ObexError::Malformed("listing is not UTF-8"))?;
    let doc = roxmltree::Document::parse(text).map_err(|_| ObexError::Malformed("listing is not XML"))?;
    let attr = |n: &roxmltree::Node, k: &str| n.attribute(k).unwrap_or("").to_string();
    Ok(doc
        .descendants()
        .filter(|n| n.has_tag_name("msg"))
        .map(|n| MessageEntry {
            handle: attr(&n, "handle"),
            subject: attr(&n, "subject"),
            datetime: attr(&n, "datetime"),
            sender_name: attr(&n, "sender_name"),
            sender_address: attr(&n, "sender_addressing"),
            recipient_name: attr(&n, "recipient_name"),
            recipient_address: attr(&n, "recipient_addressing"),
            kind: attr(&n, "type"),
            read: n.attribute("read") == Some("yes"),
            sent: n.attribute("sent") == Some("yes"),
        })
        .collect())
}

pub struct MapClient<S: Read + Write> {
    obex: ObexClient<S>,
}

impl<S: Read + Write> MapClient<S> {
    /// Opens a MAP session over an RFCOMM stream to the phone.
    pub fn connect(stream: S) -> Result<Self, ObexError> {
        let mut obex = ObexClient::new(stream);
        obex.connect(&MAS_TARGET)?;
        Ok(Self { obex })
    }

    /// Goes to an absolute folder such as `/telecom/msg/inbox`.
    pub fn set_folder(&mut self, path: &str) -> Result<(), ObexError> {
        self.obex.set_path("", false)?; // root
        for part in path.split('/').filter(|p| !p.is_empty()) {
            self.obex.set_path(part, false)?;
        }
        Ok(())
    }

    pub fn list_folders(&mut self) -> Result<Vec<String>, ObexError> {
        let data = self.obex.get(TYPE_FOLDER_LISTING, None, None)?;
        let text = String::from_utf8_lossy(&data).into_owned();
        let doc = roxmltree::Document::parse(&text).map_err(|_| ObexError::Malformed("folder listing"))?;
        Ok(doc
            .descendants()
            .filter(|n| n.has_tag_name("folder"))
            .filter_map(|n| n.attribute("name").map(str::to_string))
            .collect())
    }

    /// Lists messages in the current folder.
    pub fn list_messages(&mut self, max: u16, subject_len: u8) -> Result<Vec<MessageEntry>, ObexError> {
        let mut params = Vec::new();
        app_param(&mut params, TAG_MAX_LIST_COUNT, &max.to_be_bytes());
        app_param(&mut params, TAG_START_OFFSET, &0u16.to_be_bytes());
        app_param(&mut params, TAG_SUBJECT_LENGTH, &[subject_len]);
        let data = self.obex.get(TYPE_MSG_LISTING, Some(""), Some(params))?;
        parse_message_listing(&data)
    }

    /// Fetches one message's full text by handle.
    pub fn get_text(&mut self, handle: &str) -> Result<Option<String>, ObexError> {
        let mut params = Vec::new();
        app_param(&mut params, 0x0A, &[0x00]); // Attachment: off
        app_param(&mut params, TAG_CHARSET, &[CHARSET_UTF8]);
        let data = self.obex.get(TYPE_MESSAGE, Some(handle), Some(params))?;
        Ok(bmessage::parse_text(&data))
    }

    /// Sends a text through the phone's outbox.
    pub fn send_sms(&mut self, number: &str, text: &str) -> Result<(), ObexError> {
        let number = bmessage::clean_number(number).ok_or(ObexError::Protocol("not a phone number"))?;
        self.set_folder("/telecom/msg/outbox")?;
        let mut params = Vec::new();
        app_param(&mut params, TAG_CHARSET, &[CHARSET_UTF8]);
        self.obex.put(TYPE_MESSAGE, "", Some(params), &bmessage::build_sms(&number, text))
    }

    pub fn close(mut self) -> Result<S, ObexError> {
        let _ = self.obex.disconnect();
        Ok(self.obex.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::testing::FakePhone;
    use crate::packet::*;

    fn ok(headers: &[Header]) -> Vec<u8> {
        encode_request(RSP_SUCCESS, &[], headers)
    }
    fn connect_ok() -> Vec<u8> {
        let mut body = vec![0x10, 0x00, 0x20, 0x00, 0xCB, 0, 0, 0, 9];
        let mut p = vec![RSP_SUCCESS];
        p.extend_from_slice(&((body.len() + 3) as u16).to_be_bytes());
        p.append(&mut body);
        p
    }

    const LISTING: &str = r#"<?xml version="1.0"?>
<MAP-msg-listing version="1.0">
 <msg handle="0001" subject="See you at 8" datetime="20260417T123456" sender_name="Ali"
      sender_addressing="+213550000001" recipient_addressing="+213550000009"
      type="SMS_GSM" size="12" read="no" sent="no"/>
 <msg handle="0002" subject="ok" datetime="20260417T123556" sender_name=""
      sender_addressing="+213550000009" recipient_name="Ali" recipient_addressing="+213550000001"
      type="SMS_GSM" size="2" read="yes" sent="yes"/>
</MAP-msg-listing>"#;

    #[test]
    fn parses_a_listing() {
        let list = parse_message_listing(LISTING.as_bytes()).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].sender_name, "Ali");
        assert_eq!(list[0].subject, "See you at 8");
        assert!(!list[0].read && !list[0].sent);
        assert!(list[1].read && list[1].sent);
        assert_eq!(list[1].recipient_address, "+213550000001");
    }

    #[test]
    fn rejects_garbage_listing() {
        assert!(parse_message_listing(b"not xml").is_err());
    }

    #[test]
    fn lists_messages_over_the_wire() {
        let phone = FakePhone::new(vec![connect_ok(), ok(&[Header::EndOfBody(LISTING.as_bytes().to_vec())])]);
        let mut map = MapClient::connect(phone).unwrap();
        let msgs = map.list_messages(100, 255).unwrap();
        assert_eq!(msgs.len(), 2);
        let sent = map.close().unwrap().sent;
        // The listing request names the right type and carries our parameters.
        let get = &sent[1];
        assert_eq!(get[0], OP_GET_FINAL);
        let hdrs = decode_headers(&get[3..]).unwrap();
        assert!(hdrs.contains(&Header::Type("x-bt/MAP-msg-listing".into())));
        let params = hdrs.iter().find_map(|h| if let Header::AppParams(p) = h { Some(p.clone()) } else { None }).unwrap();
        assert_eq!(&params[..4], &[0x01, 0x02, 0x00, 100]);
        assert!(params.windows(3).any(|w| w == [0x13, 0x01, 255]));
    }

    #[test]
    fn sends_sms_to_the_outbox() {
        // connect, root, telecom, msg, outbox, put
        let mut replies = vec![connect_ok()];
        for _ in 0..4 {
            replies.push(ok(&[]));
        }
        replies.push(ok(&[]));
        let mut map = MapClient::connect(FakePhone::new(replies)).unwrap();
        map.send_sms("+213 550 00 00 01", "salut").unwrap();
        let sent = map.close().unwrap().sent;
        let put = sent.iter().find(|p| p[0] == OP_PUT_FINAL).expect("a final put");
        let hdrs = decode_headers(&put[3..]).unwrap();
        assert!(hdrs.contains(&Header::Type("x-bt/message".into())));
        let body = hdrs.iter().find_map(|h| if let Header::EndOfBody(b) = h { Some(b.clone()) } else { None }).unwrap();
        assert!(String::from_utf8(body).unwrap().contains("TEL:+213550000001"));
    }

    #[test]
    fn refuses_a_non_number_before_touching_the_phone() {
        let mut map = MapClient::connect(FakePhone::new(vec![connect_ok()])).unwrap();
        assert!(map.send_sms("abc", "x").is_err());
        assert_eq!(map.close().unwrap().sent.len(), 2); // connect + disconnect only
    }
}
