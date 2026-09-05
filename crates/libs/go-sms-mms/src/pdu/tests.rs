use super::*;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/pdu")
        .join(name)
}

fn test_owners() -> (HashSet<String>, String) {
    let primary = "5555550100".to_string();
    let mut owners = HashSet::new();
    owners.insert(primary.clone());
    (owners, primary)
}

#[test]
fn invalid_filename_returns_none() {
    let (owners, primary) = test_owners();
    let r = parse_pdu_file(&fixture("bad_name.pdu"), &owners, &primary).unwrap();
    assert!(r.is_none());
}

#[test]
fn received_one_to_one() {
    let (owners, primary) = test_owners();
    let parsed = parse_pdu_file(&fixture("I_1609459200_recv.pdu"), &owners, &primary)
        .unwrap()
        .expect("parsed");
    assert_eq!(parsed.body, "Hello one to one");
    assert_eq!(
        parsed.participants,
        vec!["4075551234".to_string(), "5555550100".to_string()]
    );
    assert!(!parsed.is_sent);
    assert!(!parsed.is_group);
    assert_eq!(parsed.sender_number, "4075551234");
    assert_eq!(parsed.timestamp, 1609459200);
    assert_eq!(parsed.decode_quality, "structured");
}

#[test]
fn sent_one_to_one() {
    let (owners, primary) = test_owners();
    let parsed = parse_pdu_file(&fixture("I_1609459200_sent.pdu"), &owners, &primary)
        .unwrap()
        .expect("parsed");
    assert_eq!(parsed.body, "Sent MMS");
    assert!(parsed.is_sent);
    assert!(!parsed.is_group);
    assert_eq!(parsed.sender_number, "5555550100");
    // No From/To headers → direction falls back to owner rules.
    assert_eq!(parsed.decode_quality, "mixed");
}

#[test]
fn group_pdu() {
    let (owners, primary) = test_owners();
    let parsed = parse_pdu_file(&fixture("I_1609459200_group.pdu"), &owners, &primary)
        .unwrap()
        .expect("parsed");
    assert_eq!(parsed.body, "Group MMS body");
    assert!(parsed.is_group);
    assert_eq!(
        parsed.participants,
        vec![
            "5551112222".to_string(),
            "5552223333".to_string(),
            "5553334444".to_string(),
            "5555550100".to_string()
        ]
    );
    assert!(!parsed.is_sent);
    assert_eq!(parsed.sender_number, "5551112222");
}

#[test]
fn sent_group_without_headers_is_sent() {
    // Owner + 2 recipients, no From/To/Cc headers. The long participant
    // list alone must not flip the direction to "received".
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("I_1609459200_sentgrp.pdu");
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"+15555550100/TYPE=PLMN"); // owner
    bytes.extend_from_slice(b"+15551112222/TYPE=PLMN"); // recipient 1
    bytes.extend_from_slice(b"+15552223333/TYPE=PLMN"); // recipient 2
    bytes.extend_from_slice(&[0x8e]);
    bytes.extend_from_slice(b"text.txt\0Group sent MMS");
    std::fs::write(&path, &bytes).unwrap();

    let (owners, primary) = test_owners();
    let parsed = parse_pdu_file(&path, &owners, &primary)
        .unwrap()
        .expect("parsed");
    assert!(parsed.is_group);
    assert!(parsed.is_sent);
    assert_eq!(
        parsed.participants,
        vec![
            "5555550100".to_string(),
            "5551112222".to_string(),
            "5552223333".to_string(),
        ]
    );
}

#[test]
fn received_group_without_headers_stays_received() {
    // No owner among the participants: still a received group MMS from the
    // first listed number.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("I_1609459200_recvgrp.pdu");
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"+15551112222/TYPE=PLMN");
    bytes.extend_from_slice(b"+15552223333/TYPE=PLMN");
    bytes.extend_from_slice(b"+15553334444/TYPE=PLMN");
    bytes.extend_from_slice(&[0x8e]);
    bytes.extend_from_slice(b"text.txt\0Group recv MMS");
    std::fs::write(&path, &bytes).unwrap();

    let (owners, primary) = test_owners();
    let parsed = parse_pdu_file(&path, &owners, &primary)
        .unwrap()
        .expect("parsed");
    assert!(parsed.is_group);
    assert!(!parsed.is_sent);
    assert_eq!(parsed.sender_number, "5551112222");
}

#[test]
fn jpeg_attachment() {
    let (owners, primary) = test_owners();
    let parsed = parse_pdu_file(&fixture("I_1609459200_att.pdu"), &owners, &primary)
        .unwrap()
        .expect("parsed");
    assert_eq!(parsed.attachments.len(), 1);
    assert_eq!(parsed.attachments[0].ext, ".jpg");
    assert!(parsed.attachments[0].data.len() >= 256);
    // Named text body + magic-byte JPEG.
    assert_eq!(parsed.decode_quality, "mixed");
}

#[test]
fn message_size_in_pdu_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("I_1609459200_msize.pdu");
    let mut bytes = Vec::new();
    // Message-Size 5000
    bytes.extend_from_slice(&[0x8e, 0x02, 0x13, 0x88]);
    bytes.extend_from_slice(&[0x89, 0x1a, 0x80, 0x18, 0xea]);
    bytes.extend_from_slice(b"+4075551234/TYPE=PLMN");
    bytes.extend_from_slice(&[0x97, 0x18, 0xea]);
    bytes.extend_from_slice(b"+15555550100/TYPE=PLMN");
    bytes.push(0x8c); // pad
    std::fs::write(&path, &bytes).unwrap();

    let (owners, primary) = test_owners();
    let parsed = parse_pdu_file(&path, &owners, &primary)
        .unwrap()
        .expect("parsed");
    assert_eq!(
        parsed.pdu_fields.get("message_size").map(String::as_str),
        Some("5000")
    );
}

#[test]
fn recv_fixture_0x8e_is_named_part_not_message_size() {
    let (owners, primary) = test_owners();
    let parsed = parse_pdu_file(&fixture("I_1609459200_recv.pdu"), &owners, &primary)
        .unwrap()
        .expect("parsed");
    assert!(!parsed.pdu_fields.contains_key("message_size"));
    assert_eq!(parsed.body, "Hello one to one");
}

#[test]
fn subject_used_as_body_when_no_text_part() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("I_1609459200_subject.pdu");
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&[0x89, 0x1a, 0x80, 0x18, 0xea]);
    bytes.extend_from_slice(b"+4075551234/TYPE=PLMN");
    bytes.extend_from_slice(&[0x97, 0x18, 0xea]);
    bytes.extend_from_slice(b"+15555550100/TYPE=PLMN");
    bytes.extend_from_slice(&[0x8e]); // overshoot pad for To length
    // Subject "SubjOnly" as text-string (0x96 = Subject 0x16|0x80)
    bytes.push(0x96);
    bytes.extend_from_slice(b"SubjOnly\0");
    std::fs::write(&path, &bytes).unwrap();

    let (owners, primary) = test_owners();
    let parsed = parse_pdu_file(&path, &owners, &primary)
        .unwrap()
        .expect("parsed");
    assert_eq!(parsed.body, "SubjOnly");
    assert_eq!(
        parsed.pdu_fields.get("subject").map(String::as_str),
        Some("SubjOnly")
    );
}

#[test]
fn body_from_content_location_without_marker_regex() {
    let data = std::fs::read(fixture("I_1609459200_recv.pdu")).unwrap();
    let structured = decode_mms_best_effort(&data);
    let smil = SmilRefs::default();
    let body = body_from_named_parts(&structured.named_parts, &smil).expect("named body");
    assert_eq!(body, "Hello one to one");
}

#[test]
fn smil_binds_text_and_image_parts() {
    let mut data = Vec::new();
    data.extend_from_slice(
        b"<smil><body><text src=\"text.txt\"/><img src=\"IMG_1.jpg\"/></body></smil>",
    );
    data.extend_from_slice(&[0x8e]);
    data.extend_from_slice(b"text.txt\0Hello from SMIL");
    data.extend_from_slice(&[0x8e]);
    data.extend_from_slice(b"IMG_1.jpg\0");
    // Minimal JPEG large enough to pass size guard
    let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe0];
    jpeg.extend(std::iter::repeat_n(0x00, 80));
    data.extend_from_slice(&jpeg);

    let structured = decode_mms_best_effort(&data);
    let smil = parse_smil_refs(&data);
    assert_eq!(smil.text_srcs, vec!["text.txt".to_string()]);
    assert_eq!(smil.media_srcs, vec!["IMG_1.jpg".to_string()]);
    let body = body_from_named_parts(&structured.named_parts, &smil).unwrap();
    assert_eq!(body, "Hello from SMIL");
    let atts = attachments_from_named_parts(&structured.named_parts, &smil);
    assert_eq!(atts.len(), 1);
    assert_eq!(atts[0].ext, ".jpg");
    assert_eq!(atts[0].smil_name.as_deref(), Some("IMG_1.jpg"));
}

#[test]
fn smil_src_matches_filename_case_insensitively() {
    let mut data = Vec::new();
    data.extend_from_slice(b"<smil><body><img src=\"img_1.jpg\"/></body></smil>");
    data.extend_from_slice(&[0x8e]);
    data.extend_from_slice(b"IMG_1.jpg\0");
    let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe0];
    jpeg.extend(std::iter::repeat_n(0x00, 80));
    data.extend_from_slice(&jpeg);

    let structured = decode_mms_best_effort(&data);
    let smil = parse_smil_refs(&data);
    let atts = attachments_from_named_parts(&structured.named_parts, &smil);
    assert_eq!(atts.len(), 1);
    assert_eq!(atts[0].smil_name.as_deref(), Some("img_1.jpg"));
}

#[test]
fn smil_cid_binds_to_part_content_id() {
    let text = b"Body via cid";
    let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe0];
    jpeg.extend(std::iter::repeat_n(0x11u8, 80));

    // Multipart related: text + jpeg with Content-ID headers
    let mut body = Vec::new();
    body.push(0x02); // nEntries
    // text/plain + Content-ID <text1>
    let text_headers = {
        let mut h = vec![0x83]; // text/plain
        h.push(0xc0); // Content-ID
        h.extend_from_slice(b"<text1>\0");
        h
    };
    body.push(text_headers.len() as u8);
    body.push(text.len() as u8);
    body.extend_from_slice(&text_headers);
    body.extend_from_slice(text);
    // image/jpeg + Content-ID <img1>
    let img_headers = {
        let mut h = vec![0x97]; // image/jpeg
        h.push(0xc0);
        h.extend_from_slice(b"<img1>\0");
        h
    };
    body.push(img_headers.len() as u8);
    body.push(jpeg.len() as u8);
    body.extend_from_slice(&img_headers);
    body.extend_from_slice(&jpeg);

    let mut data = Vec::new();
    data.extend_from_slice(
        b"<smil><body><text src=\"cid:text1\"/><img src=\"cid:img1\"/></body></smil>",
    );
    data.push(0x84);
    // multipart.related short-int (well-known index 0x2c)
    data.push(0xac);
    data.extend_from_slice(&body);

    let structured = decode_mms_best_effort(&data);
    let smil = parse_smil_refs(&data);
    assert!(smil.media_srcs.iter().any(|s| s.contains("img1")));
    let body_text = body_from_structured(&structured, &smil).expect("cid body");
    assert_eq!(body_text, "Body via cid");
    let atts = attachments_from_structured(&structured, &smil);
    assert_eq!(atts.len(), 1);
    assert_eq!(atts[0].ext, ".jpg");
    assert_eq!(atts[0].smil_name.as_deref(), Some("cid:img1"));
}

#[test]
fn application_header_in_pdu_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("I_1609459200_app.pdu");
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&[0x8c, 0x84]); // m-retrieve-conf
    bytes.extend_from_slice(b"X-Go-Extra\0abc\0");
    bytes.extend_from_slice(&[0x84, 0x83]); // text/plain CT ends headers
    bytes.extend_from_slice(&[0x89, 0x1a, 0x80, 0x18, 0xea]);
    bytes.extend_from_slice(b"+4075551234/TYPE=PLMN");
    std::fs::write(&path, &bytes).unwrap();

    let (owners, primary) = test_owners();
    let parsed = parse_pdu_file(&path, &owners, &primary)
        .unwrap()
        .expect("parsed");
    assert_eq!(
        parsed.pdu_fields.get("app:X-Go-Extra").map(String::as_str),
        Some("abc")
    );
}

#[test]
fn bcc_and_extra_headers_in_pdu_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("I_1609459200_headers.pdu");
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&[0x89, 0x1a, 0x80, 0x18, 0xea]);
    bytes.extend_from_slice(b"+4075551234/TYPE=PLMN");
    bytes.extend_from_slice(&[0x97, 0x18, 0xea]);
    bytes.extend_from_slice(b"+15555550100/TYPE=PLMN");
    // Bcc
    bytes.push(0x81);
    bytes.push(0x18);
    bytes.push(0xea);
    bytes.extend_from_slice(b"+15559876543/TYPE=PLMN");
    // Transaction-Id / Message-Class / Version
    bytes.push(0x98);
    bytes.extend_from_slice(b"txn-1\0");
    bytes.push(0x8a);
    bytes.push(0x80); // Personal
    bytes.push(0x8d);
    bytes.push(0x92); // 1.2
    bytes.push(0x8e); // pad for overshoot
    bytes.extend_from_slice(b"text.txt\0hello");
    std::fs::write(&path, &bytes).unwrap();

    let (owners, primary) = test_owners();
    let parsed = parse_pdu_file(&path, &owners, &primary)
        .unwrap()
        .expect("parsed");
    assert!(parsed.participants.iter().any(|p| p.contains("5559876543")));
    assert_eq!(
        parsed.pdu_fields.get("transaction_id").map(String::as_str),
        Some("txn-1")
    );
    assert_eq!(
        parsed.pdu_fields.get("message_class").map(String::as_str),
        Some("Personal")
    );
    assert_eq!(
        parsed.pdu_fields.get("mms_version").map(String::as_str),
        Some("1.2")
    );
    assert!(
        parsed
            .pdu_fields
            .get("bcc")
            .is_some_and(|b| b.contains("15559876543"))
    );
}

#[test]
fn mms_date_overrides_filename_timestamp() {
    let dir = tempfile::tempdir().unwrap();
    // Filename says 1609459200; Date header says 1700000000
    let path = dir.path().join("I_1609459200_dated.pdu");
    let mut bytes = vec![0x85, 0x04, 0x65, 0x53, 0xf1, 0x00]; // 1700000000
    bytes.extend_from_slice(&[0x89, 0x1a, 0x80, 0x18, 0xea]);
    bytes.extend_from_slice(b"+4075551234/TYPE=PLMN");
    bytes.extend_from_slice(&[0x97, 0x18, 0xea]);
    bytes.extend_from_slice(b"+15555550100/TYPE=PLMN");
    bytes.extend_from_slice(&[0x8e]);
    bytes.extend_from_slice(b"text.txt\0Dated body");
    // Pad so To value-length overshoot has a following header byte
    bytes.push(0x8c);
    std::fs::write(&path, &bytes).unwrap();

    let (owners, primary) = test_owners();
    let parsed = parse_pdu_file(&path, &owners, &primary)
        .unwrap()
        .expect("parsed");
    assert_eq!(parsed.timestamp, 1700000000);
    assert_eq!(parsed.body, "Dated body");
}
