//! Each test guards one rule from the module documentation.

use super::*;
use crate::testutil::{BuildPart, PduBuilder};

fn parse(name: &str, bytes: &[u8]) -> Result<ParsedPdu, PduError> {
    parse_pdu_bytes(Path::new(name), bytes)
}

#[test]
fn a_received_message_names_its_sender_recipients_text_and_picture() {
    let bytes = PduBuilder::received("+14075551234")
        .to("+15555550100")
        .to("+14075559876")
        .cc("+15555550100")
        .text("Look at this")
        .part(BuildPart::jpeg("IMG_1.jpg", 100))
        .build();
    let p = parse("I_1609459300_1_0.pdu", &bytes).unwrap();
    assert!(!p.is_sent);
    assert_eq!(p.sender.as_deref(), Some("14075551234"));
    assert_eq!(
        p.recipients,
        ["15555550100", "14075559876"],
        "once each, To before Cc"
    );
    assert_eq!(
        p.timestamp, 1_609_459_200,
        "the Date header, not the file name"
    );
    assert_eq!(p.body, "Look at this");
    assert_eq!(p.attachments.len(), 1);
    let att = &p.attachments[0];
    assert_eq!(att.content_type, "image/jpeg");
    assert_eq!(att.name.as_deref(), Some("IMG_1.jpg"));
    assert_eq!(att.data.len(), 100);
    assert_eq!(p.fields["message-type"], "m-retrieve-conf");
    assert_eq!(
        p.fields["content-type"],
        "application/vnd.wap.multipart.related"
    );
    assert_eq!(p.fields["message-id"], "MSG-1");
    assert!(!p.fields.contains_key("subject"));
}

#[test]
fn a_sent_message_has_no_sender() {
    let bytes = PduBuilder::sent().to("+14075551234").text("hi").build();
    let p = parse("S_1609459300_1_0.pdu", &bytes).unwrap();
    assert!(p.is_sent);
    assert_eq!(p.sender, None);
    assert_eq!(p.recipients, ["14075551234"]);
}

#[test]
fn direction_is_the_message_type_not_the_file_name() {
    let bytes = PduBuilder::received("+14075551234")
        .to("+15555550100")
        .text("x")
        .build();
    let p = parse("S_1609459300_1_0.pdu", &bytes).unwrap();
    assert!(!p.is_sent);
}

#[test]
fn a_transaction_that_is_not_a_message_is_refused_by_name() {
    let err = parse("I_1_1_0.pdu", &[0x8c, 0x86, 0x84, 0x83]).unwrap_err();
    assert_eq!(err.to_string(), "m-delivery-ind is not a message");
}

#[test]
fn a_file_without_a_message_type_is_a_stub() {
    assert!(matches!(
        parse("I_1_1_0.pdu", b"application/smil\0"),
        Err(PduError::Stub)
    ));
    assert!(matches!(parse("I_1_1_0.pdu", b""), Err(PduError::Stub)));
    assert_eq!(PduError::Stub.to_string(), "stub PDU with no message");
}

#[test]
fn a_broken_pdu_reports_the_shape_and_the_byte() {
    let err = parse("I_1_1_0.pdu", &[0x8c, 0x84, 0xff]).unwrap_err();
    assert_eq!(
        err.to_string(),
        "malformed PDU: expected unknown header field code at byte 2"
    );
}

#[test]
fn the_time_falls_back_to_the_file_name_and_then_to_zero() {
    let bytes = PduBuilder::received("+14075551234")
        .date(None)
        .text("x")
        .build();
    assert_eq!(
        parse("I_1609459300_1_0.pdu", &bytes).unwrap().timestamp,
        1_609_459_300
    );
    assert_eq!(
        parse("S_1609459400_1_0.pdu", &bytes).unwrap().timestamp,
        1_609_459_400
    );
    assert_eq!(parse("other.pdu", &bytes).unwrap().timestamp, 0);
    assert_eq!(parse("I_x_1_0.pdu", &bytes).unwrap().timestamp, 0);
}

#[test]
fn the_body_is_every_text_part_and_never_the_smil() {
    let bytes = PduBuilder::received("+14075551234")
        .text("first")
        .part(BuildPart::jpeg("a.jpg", 70))
        .text("second +g1f602")
        .build();
    let p = parse("I_1_1_0.pdu", &bytes).unwrap();
    assert_eq!(p.body, "first\nsecond 😂");
    assert_eq!(
        p.attachments.len(),
        1,
        "the SMIL is not an attachment either"
    );
}

#[test]
fn the_subject_is_the_body_only_when_there_is_no_text_part() {
    let bytes = PduBuilder::received("+14075551234")
        .subject(" Beach day ")
        .part(BuildPart::jpeg("a.jpg", 70))
        .build();
    let p = parse("I_1_1_0.pdu", &bytes).unwrap();
    assert_eq!(p.body, "Beach day");
    assert_eq!(p.fields["subject"], " Beach day ");
    let bytes = PduBuilder::received("+14075551234")
        .subject("Beach day")
        .text("words")
        .build();
    assert_eq!(parse("I_1_1_0.pdu", &bytes).unwrap().body, "words");
}

#[test]
fn an_empty_text_part_gives_an_empty_body() {
    let bytes = PduBuilder::received("+14075551234").text("  \0").build();
    assert_eq!(parse("I_1_1_0.pdu", &bytes).unwrap().body, "");
}

#[test]
fn every_part_that_is_not_text_or_smil_is_an_attachment() {
    let bytes = PduBuilder::received("+14075551234")
        .part(BuildPart {
            content_type: "text/x-vcard",
            name: Some("card.vcf"),
            charset_utf8: false,
            data: b"BEGIN:VCARD".to_vec(),
        })
        .part(BuildPart {
            content_type: "video/3gpp",
            name: None,
            charset_utf8: false,
            data: vec![1, 2, 3],
        })
        .part(BuildPart::jpeg("tiny.jpg", 12))
        .build();
    let p = parse("I_1_1_0.pdu", &bytes).unwrap();
    let got: Vec<(&str, Option<&str>, usize)> = p
        .attachments
        .iter()
        .map(|a| (a.content_type.as_str(), a.name.as_deref(), a.data.len()))
        .collect();
    assert_eq!(
        got,
        [
            ("text/x-vcard", Some("card.vcf"), 11),
            ("video/3gpp", None, 3),
            ("image/jpeg", Some("tiny.jpg"), 12),
        ]
    );
}

#[test]
fn a_ucs2_text_part_is_decoded_by_its_charset() {
    // Content-Type text/plain; Charset UCS-2 (Long-integer 1000).
    let headers = b"\x05\x83\x81\x02\x03\xe8";
    let mut bytes = PduBuilder::received("+14075551234").no_parts().build();
    assert_eq!(bytes.pop(), Some(0), "the part count");
    bytes.push(1);
    bytes.push(headers.len() as u8);
    bytes.push(4);
    bytes.extend_from_slice(headers);
    bytes.extend_from_slice(&[0x00, 0x68, 0x00, 0x69]);
    assert_eq!(parse("I_1_1_0.pdu", &bytes).unwrap().body, "hi");
}

#[test]
fn addresses_keep_their_digits_only() {
    assert_eq!(
        address_digits("+14075551234/TYPE=PLMN").as_deref(),
        Some("14075551234")
    );
    assert_eq!(address_digits("4075551234").as_deref(), Some("4075551234"));
    assert_eq!(
        address_digits("(407) 555-1234").as_deref(),
        Some("4075551234")
    );
    assert_eq!(address_digits("someone@example.com"), None);
    let bytes = PduBuilder::received("someone@example.com")
        .to("+15555550100")
        .to("other@example.com")
        .text("x")
        .build();
    let p = parse("I_1_1_0.pdu", &bytes).unwrap();
    assert_eq!(p.sender, None);
    assert_eq!(p.recipients, ["15555550100"]);
}
