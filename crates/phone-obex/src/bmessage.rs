// SPDX-License-Identifier: GPL-3.0-or-later
//! The bMessage format MAP uses to carry one text message.

/// Builds the bMessage for sending an SMS to `number`.
///
/// CRLF line ends throughout, and LENGTH counts the bytes from `BEGIN:MSG` to
/// `END:MSG` inclusive, line ends included.
pub fn build_sms(number: &str, text: &str) -> Vec<u8> {
    let body = format!("BEGIN:MSG\r\n{text}\r\nEND:MSG\r\n");
    format!(
        "BEGIN:BMSG\r\nVERSION:1.0\r\nSTATUS:UNREAD\r\nTYPE:SMS_GSM\r\nFOLDER:\r\n\
         BEGIN:BENV\r\nBEGIN:VCARD\r\nVERSION:3.0\r\nN:\r\nTEL:{number}\r\nEND:VCARD\r\n\
         BEGIN:BBODY\r\nCHARSET:UTF-8\r\nLENGTH:{}\r\n{body}END:BBODY\r\nEND:BENV\r\nEND:BMSG\r\n",
        body.len()
    )
    .into_bytes()
}

/// Pulls the message text out of a received bMessage.
pub fn parse_text(bmessage: &[u8]) -> Option<String> {
    let s = String::from_utf8_lossy(bmessage);
    let start = s.find("BEGIN:MSG")? + "BEGIN:MSG".len();
    let end = s[start..].rfind("END:MSG")? + start;
    Some(s[start..end].trim_matches(|c| c == '\r' || c == '\n').to_string())
}

/// Keeps digits and a leading plus; `None` when nothing dialable remains.
pub fn clean_number(input: &str) -> Option<String> {
    let mut out = String::new();
    for c in input.trim().chars() {
        if c.is_ascii_digit() || (c == '+' && out.is_empty()) {
            out.push(c);
        }
    }
    (out != "+" && !out.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_counts_the_msg_block() {
        let b = build_sms("+4912345", "héllo\nworld");
        let s = String::from_utf8(b.clone()).unwrap();
        let declared: usize = s.split("LENGTH:").nth(1).unwrap().split("\r\n").next().unwrap().parse().unwrap();
        let start = s.find("BEGIN:MSG").unwrap();
        let end = s.find("END:MSG\r\n").unwrap() + "END:MSG\r\n".len();
        assert_eq!(declared, end - start);
        assert!(s.contains("TEL:+4912345\r\n"));
        assert_eq!(parse_text(&b).unwrap(), "héllo\nworld");
    }

    #[test]
    fn numbers_are_cleaned() {
        assert_eq!(clean_number(" +213 (550) 00-00-01 ").as_deref(), Some("+213550000001"));
        assert_eq!(clean_number("12+34").as_deref(), Some("1234"));
        assert_eq!(clean_number("+"), None);
        assert_eq!(clean_number("abc"), None);
    }
}
