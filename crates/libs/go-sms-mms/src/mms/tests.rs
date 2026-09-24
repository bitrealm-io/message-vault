//! Each test guards one rule from the module documentation.

use super::*;
use crate::testutil::{BuildPart, PduBuilder};

#[test]
fn a_received_message_decodes_its_headers_and_parts() {
    let bytes = PduBuilder::received("+14075551234")
        .to("+15555550100")
        .cc("+14075559876")
        .subject("Hi there")
        .text("hello")
        .part(BuildPart::jpeg("pic.jpg", 64))
        .build();
    let msg = decode(&bytes).unwrap();
    assert_eq!(msg.message_type, Some(MessageType::RetrieveConf));
    assert_eq!(msg.from.as_deref(), Some("+14075551234/TYPE=PLMN"));
    assert_eq!(msg.to, ["+15555550100/TYPE=PLMN"]);
    assert_eq!(msg.cc, ["+14075559876/TYPE=PLMN"]);
    assert_eq!(msg.date, Some(1_609_459_200));
    assert_eq!(msg.subject.as_deref(), Some("Hi there"));
    assert_eq!(msg.headers["transaction-id"], "T1");
    assert_eq!(msg.headers["mms-version"], "1.0");
    assert_eq!(msg.headers["message-class"], "Personal");
    assert_eq!(msg.headers["message-id"], "MSG-1");
    assert_eq!(msg.headers["priority"], "Normal");
    assert_eq!(msg.headers["delivery-report"], "no");
    let ct = msg.content_type.unwrap();
    assert_eq!(ct.media, "application/vnd.wap.multipart.related");
    assert_eq!(ct.params["Start"], "<smil.xml>");
    let names: Vec<_> = msg.parts.iter().map(|p| p.name().unwrap()).collect();
    assert_eq!(names, ["smil.xml", "text_0.txt", "pic.jpg"]);
    assert_eq!(msg.parts[1].data, b"hello");
    assert_eq!(msg.parts[2].data.len(), 64);
}

#[test]
fn a_sent_message_has_the_insert_address_token_and_no_from() {
    let bytes = PduBuilder::sent().to("+14075551234").text("x").build();
    let msg = decode(&bytes).unwrap();
    assert_eq!(msg.message_type, Some(MessageType::SendReq));
    assert_eq!(msg.from, None);
    assert_eq!(msg.to, ["+14075551234/TYPE=PLMN"]);
}

#[test]
fn nothing_after_content_type_is_read_as_a_header() {
    // A picture whose bytes are a To header (0x97) and a From header (0x89)
    // with plausible values. Under a byte scan those became addresses.
    let mut picture = b"\x97\x0b\xea+19999999999\0".to_vec();
    picture.extend_from_slice(b"\x89\x0d\x80\x0b\xea+18888888888\0");
    let bytes = PduBuilder::received("+14075551234")
        .to("+15555550100")
        .part(BuildPart {
            content_type: "image/jpeg",
            name: Some("pic.jpg"),
            charset_utf8: false,
            data: picture.clone(),
        })
        .build();
    let msg = decode(&bytes).unwrap();
    assert_eq!(msg.to, ["+15555550100/TYPE=PLMN"]);
    assert_eq!(msg.from.as_deref(), Some("+14075551234/TYPE=PLMN"));
    assert_eq!(msg.parts[1].data, picture);
}

#[test]
fn the_first_header_must_be_the_message_type() {
    assert!(!starts_with_message_type(b"application/smil\0"));
    assert!(!starts_with_message_type(b""));
    assert!(starts_with_message_type(&[0x8c, 0x84]));
    let err = decode(b"application/smil\0").unwrap_err();
    assert_eq!(err.at, 0);
    assert_eq!(err.what, "X-Mms-Message-Type header");
    // Every other transaction type is named, not refused, so the caller can
    // say what the file was.
    let msg = decode(&[0x8c, 0x86, 0x84, 0x83]).unwrap();
    assert_eq!(msg.message_type, Some(MessageType::Other(0x86)));
    assert_eq!(msg.message_type.unwrap().name(), "m-delivery-ind");
    assert_eq!(MessageType::Other(0x90).name(), "0x90");
}

#[test]
fn an_unknown_header_code_is_an_error_at_its_byte() {
    // 0x22 is past the codes WAP-209 1.3 assigns, so the walk cannot know
    // its value's shape.
    let bytes = PduBuilder::received("+14075551234")
        .raw_header(&[0xa2, 0x80])
        .build();
    let err = decode(&bytes).unwrap_err();
    assert_eq!(err.what, "unknown header field code");
    assert_eq!(bytes[err.at], 0xa2);
    // A text byte where a code should be is the same error, at that byte.
    let err = decode(&[0x8c, 0x84, b'x', 0x00]).unwrap_err();
    assert_eq!(
        err,
        crate::wsp::Error {
            at: 2,
            what: "header field code"
        }
    );
}

#[test]
fn from_takes_only_its_two_token_forms_and_its_declared_length() {
    let err = decode(&[0x8c, 0x84, 0x89, 0x01, 0x82]).unwrap_err();
    assert_eq!(
        err,
        crate::wsp::Error {
            at: 4,
            what: "from address token"
        }
    );
    let err = decode(&[0x8c, 0x84, 0x89, 0x02, 0x81, 0x84]).unwrap_err();
    assert_eq!(err.what, "from length does not match its address");
    let err = decode(&[0x8c, 0x84, 0x89, 0x09, 0x81]).unwrap_err();
    assert_eq!(err.what, "from length past the end");
}

#[test]
fn a_non_multipart_body_is_one_part_of_the_declared_type() {
    // Content-Type text/plain (constrained), then the body bytes.
    let bytes = [0x8c, 0x84, 0x84, 0x83, b'h', b'i'];
    let msg = decode(&bytes).unwrap();
    assert_eq!(msg.parts.len(), 1);
    assert_eq!(msg.parts[0].content_type.media, "text/plain");
    assert_eq!(msg.parts[0].data, b"hi");
}

#[test]
fn timed_and_token_headers_decode_to_their_names() {
    let bytes = PduBuilder::sent()
        .to("+14075551234")
        // Expiry: relative 604800 s. Read-Report yes. Sender-Visibility Show.
        .raw_header(&[
            0x88, 0x05, 0x81, 0x03, 0x09, 0x3a, 0x80, 0x90, 0x80, 0x94, 0x81,
        ])
        // Delivery-Time absolute 1609459200. Status Retrieved. Message-Size 1234.
        .raw_header(&[
            0x87, 0x06, 0x80, 0x04, 0x5f, 0xee, 0x66, 0x00, 0x95, 0x81, 0x8e, 0x02, 0x04, 0xd2,
        ])
        // Message-Class as text. Response-Text. Read-Status.
        .raw_header(b"\x8aBulk\0\x93ok\0\x9b\x81")
        .build();
    let msg = decode(&bytes).unwrap();
    assert_eq!(msg.headers["expiry"], "relative:604800");
    assert_eq!(msg.headers["read-report"], "yes");
    assert_eq!(msg.headers["sender-visibility"], "Show");
    assert_eq!(msg.headers["delivery-time"], "absolute:1609459200");
    assert_eq!(msg.headers["status"], "Retrieved");
    assert_eq!(msg.headers["message-size"], "1234");
    assert_eq!(msg.headers["message-class"], "Bulk");
    assert_eq!(msg.headers["response-text"], "ok");
    assert_eq!(msg.headers["read-status"], "Deleted without being read");
    // A token outside the list is kept as its number.
    let bytes = PduBuilder::sent().raw_header(&[0x8f, 0x87]).build();
    assert_eq!(decode(&bytes).unwrap().headers["priority"], "0x07");
    // A time value with a bad token or a short length is refused.
    let bytes = PduBuilder::sent()
        .raw_header(&[0x88, 0x02, 0x82, 0x81])
        .build();
    let err = decode(&bytes).unwrap_err();
    assert_eq!(err.what, "time token");
    assert_eq!(bytes[err.at], 0x82, "the offset is the token's");
    let bytes = PduBuilder::sent()
        .raw_header(&[0x88, 0x03, 0x81, 0x81])
        .build();
    assert_eq!(
        decode(&bytes).unwrap_err().what,
        "time value length does not match"
    );
}

#[test]
fn every_other_header_code_decodes_under_its_name() {
    // One header of each remaining kind, each in the value shape WAP-209
    // table 8 gives it. A code the decoder did not know would end the
    // whole file, so every assigned code has to be here.
    let cases: &[(&[u8], &str, &str)] = &[
        (b"\x83loc\0", "content-location", "loc"),
        (b"\x9atext\0", "retrieve-text", "text"),
        (b"\x9eRC1\0", "reply-charging-id", "RC1"),
        (b"\x9f\x02\x01\x00", "reply-charging-size", "256"),
        (
            b"\x9d\x02\x81\x8a",
            "reply-charging-deadline",
            "relative:10",
        ),
        (b"\x91\x80", "report-allowed", "yes"),
        (b"\x92\x80", "response-status", "0"),
        (b"\x99\x80", "retrieve-status", "0"),
        (b"\x9c\x82", "reply-charging", "Accepted"),
        // Forwarded twice, by a number.
        (
            b"\xa0\x0c\x82\x0a\xea+1555000\0",
            "previously-sent-by",
            "2:+1555000",
        ),
        // Forwarded once, on the 5th of March 2021.
        (
            b"\xa1\x06\x81\x04\x60\x41\xa3\x80",
            "previously-sent-date",
            "1:1614914432",
        ),
    ];
    for (raw, name, value) in cases {
        let bytes = PduBuilder::sent().raw_header(raw).build();
        let msg = decode(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            msg.headers.get(*name).map(String::as_str),
            Some(*value),
            "{name}"
        );
    }
    let bytes = PduBuilder::sent()
        .raw_header(b"\x81\x0a\xea+1555000\0")
        .build();
    assert_eq!(decode(&bytes).unwrap().bcc, ["+1555000"]);
    // A previously-sent value that does not fill its length is refused.
    let bytes = PduBuilder::sent()
        .raw_header(b"\xa1\x07\x81\x04\x60\x41\xa3\x80\x80")
        .build();
    assert_eq!(
        decode(&bytes).unwrap_err().what,
        "previously-sent length does not match"
    );
}
