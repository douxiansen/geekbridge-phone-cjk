// SPDX-License-Identifier: GPL-3.0-or-later
//! OBEX 1.0 packets: opcodes, headers, and their wire encoding.

use crate::client::ObexError;

pub const OP_CONNECT: u8 = 0x80;
pub const OP_DISCONNECT: u8 = 0x81;
pub const OP_PUT: u8 = 0x02;
pub const OP_PUT_FINAL: u8 = 0x82;
pub const OP_GET_FINAL: u8 = 0x83;
pub const OP_SETPATH: u8 = 0x85;

pub const RSP_CONTINUE: u8 = 0x90;
pub const RSP_SUCCESS: u8 = 0xA0;

const HDR_NAME: u8 = 0x01;
const HDR_TYPE: u8 = 0x42;
const HDR_TARGET: u8 = 0x46;
const HDR_BODY: u8 = 0x48;
const HDR_END_OF_BODY: u8 = 0x49;
const HDR_APP_PARAMS: u8 = 0x4C;
const HDR_LENGTH: u8 = 0xC3;
const HDR_CONNECTION_ID: u8 = 0xCB;

/// One OBEX header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Header {
    Name(String),
    Type(String),
    Target(Vec<u8>),
    ConnectionId(u32),
    Length(u32),
    Body(Vec<u8>),
    EndOfBody(Vec<u8>),
    AppParams(Vec<u8>),
    /// A header this client does not interpret; kept so nothing is lost.
    Other(u8, Vec<u8>),
}

impl Header {
    pub fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Header::Name(s) => {
                // Unicode, big-endian, null terminated. An empty name is just
                // the length prefix: that is how "the root folder" is spelled.
                let mut bytes = Vec::new();
                if !s.is_empty() {
                    for unit in s.encode_utf16() {
                        bytes.extend_from_slice(&unit.to_be_bytes());
                    }
                    bytes.extend_from_slice(&[0, 0]);
                }
                out.push(HDR_NAME);
                out.extend_from_slice(&((bytes.len() + 3) as u16).to_be_bytes());
                out.extend_from_slice(&bytes);
            }
            Header::Type(s) => {
                // ASCII, null terminated.
                let mut bytes = s.as_bytes().to_vec();
                bytes.push(0);
                seq(out, HDR_TYPE, &bytes);
            }
            Header::Target(b) => seq(out, HDR_TARGET, b),
            Header::Body(b) => seq(out, HDR_BODY, b),
            Header::EndOfBody(b) => seq(out, HDR_END_OF_BODY, b),
            Header::AppParams(b) => seq(out, HDR_APP_PARAMS, b),
            Header::ConnectionId(v) => word(out, HDR_CONNECTION_ID, *v),
            Header::Length(v) => word(out, HDR_LENGTH, *v),
            Header::Other(id, b) => match id >> 6 {
                0b00 | 0b01 => seq(out, *id, b),
                0b10 => {
                    out.push(*id);
                    out.push(b.first().copied().unwrap_or(0));
                }
                _ => {
                    out.push(*id);
                    let mut w = [0u8; 4];
                    for (i, x) in b.iter().take(4).enumerate() {
                        w[i] = *x;
                    }
                    out.extend_from_slice(&w);
                }
            },
        }
    }
}

fn seq(out: &mut Vec<u8>, id: u8, data: &[u8]) {
    out.push(id);
    out.extend_from_slice(&((data.len() + 3) as u16).to_be_bytes());
    out.extend_from_slice(data);
}

fn word(out: &mut Vec<u8>, id: u8, v: u32) {
    out.push(id);
    out.extend_from_slice(&v.to_be_bytes());
}

/// Parses a run of headers.
pub fn decode_headers(mut data: &[u8]) -> Result<Vec<Header>, ObexError> {
    let mut headers = Vec::new();
    while !data.is_empty() {
        let id = data[0];
        let (value, used): (&[u8], usize) = match id >> 6 {
            0b00 | 0b01 => {
                if data.len() < 3 {
                    return Err(ObexError::Malformed("truncated header length"));
                }
                let len = u16::from_be_bytes([data[1], data[2]]) as usize;
                if len < 3 || len > data.len() {
                    return Err(ObexError::Malformed("bad header length"));
                }
                (&data[3..len], len)
            }
            0b10 => {
                if data.len() < 2 {
                    return Err(ObexError::Malformed("truncated 1-byte header"));
                }
                (&data[1..2], 2)
            }
            _ => {
                if data.len() < 5 {
                    return Err(ObexError::Malformed("truncated 4-byte header"));
                }
                (&data[1..5], 5)
            }
        };
        let header = match id {
            HDR_NAME => Header::Name(decode_unicode(value)),
            HDR_TYPE => Header::Type(
                String::from_utf8_lossy(value.strip_suffix(&[0]).unwrap_or(value)).into_owned(),
            ),
            HDR_TARGET => Header::Target(value.to_vec()),
            HDR_BODY => Header::Body(value.to_vec()),
            HDR_END_OF_BODY => Header::EndOfBody(value.to_vec()),
            HDR_APP_PARAMS => Header::AppParams(value.to_vec()),
            HDR_CONNECTION_ID => Header::ConnectionId(u32::from_be_bytes(value.try_into().unwrap())),
            HDR_LENGTH => Header::Length(u32::from_be_bytes(value.try_into().unwrap())),
            _ => Header::Other(id, value.to_vec()),
        };
        headers.push(header);
        data = &data[used..];
    }
    Ok(headers)
}

fn decode_unicode(value: &[u8]) -> String {
    let units: Vec<u16> = value
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .take_while(|u| *u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

/// Builds a request packet: opcode, length, fixed fields, headers.
pub fn encode_request(opcode: u8, fixed: &[u8], headers: &[Header]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(fixed);
    for h in headers {
        h.encode(&mut body);
    }
    let mut out = Vec::with_capacity(body.len() + 3);
    out.push(opcode);
    out.extend_from_slice(&((body.len() + 3) as u16).to_be_bytes());
    out.extend_from_slice(&body);
    out
}

/// A parsed response.
#[derive(Debug, Clone)]
pub struct Response {
    /// The response code with the final bit as received (0xA0 success).
    pub code: u8,
    /// Fixed fields after the length (only a Connect response has any).
    pub fixed: Vec<u8>,
    pub headers: Vec<Header>,
}

/// Parses one whole response packet. `fixed_len` is the size of the fixed
/// fields for the request it answers (4 for Connect, otherwise 0).
pub fn decode_response(packet: &[u8], fixed_len: usize) -> Result<Response, ObexError> {
    if packet.len() < 3 {
        return Err(ObexError::Malformed("short response"));
    }
    let len = u16::from_be_bytes([packet[1], packet[2]]) as usize;
    if len != packet.len() {
        return Err(ObexError::Malformed("response length mismatch"));
    }
    let body = &packet[3..];
    // Only a successful Connect carries the fixed fields.
    let take = if packet[0] == RSP_SUCCESS { fixed_len.min(body.len()) } else { 0 };
    Ok(Response {
        code: packet[0],
        fixed: body[..take].to_vec(),
        headers: decode_headers(&body[take..])?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_round_trip() {
        let headers = vec![
            Header::ConnectionId(7),
            Header::Name("inbox".into()),
            Header::Type("x-bt/MAP-msg-listing".into()),
            Header::AppParams(vec![0x01, 0x02, 0x00, 0x0A]),
            Header::Body(vec![1, 2, 3]),
        ];
        let mut bytes = Vec::new();
        for h in &headers {
            h.encode(&mut bytes);
        }
        assert_eq!(decode_headers(&bytes).unwrap(), headers);
    }

    #[test]
    fn empty_name_is_three_bytes() {
        let mut b = Vec::new();
        Header::Name(String::new()).encode(&mut b);
        assert_eq!(b, vec![0x01, 0x00, 0x03]);
    }

    #[test]
    fn name_is_utf16_be_null_terminated() {
        let mut b = Vec::new();
        Header::Name("é".into()).encode(&mut b);
        assert_eq!(b, vec![0x01, 0x00, 0x07, 0x00, 0xE9, 0x00, 0x00]);
    }

    #[test]
    fn request_length_counts_whole_packet() {
        let p = encode_request(OP_GET_FINAL, &[], &[Header::Type("t".into())]);
        assert_eq!(u16::from_be_bytes([p[1], p[2]]) as usize, p.len());
    }

    #[test]
    fn truncated_header_is_an_error_not_a_panic() {
        assert!(decode_headers(&[0x48, 0x00]).is_err());
        assert!(decode_headers(&[0x48, 0x00, 0x20, 0x01]).is_err());
        assert!(decode_headers(&[0xCB, 0x00]).is_err());
    }
}
