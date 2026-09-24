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

/// Parse `bytes` as a PDU file named with a valid timestamp.
fn parse_bytes(bytes: &[u8]) -> ParsedPdu {
    let (owners, primary) = test_owners();
    parse_pdu_bytes(Path::new("I_1609459200_t.pdu"), bytes, &owners, &primary).expect("parsed")
}

/// A From header naming `number`, or the insert-address token when `None`.
fn from_header(number: Option<&str>) -> Vec<u8> {
    let Some(number) = number else {
        return vec![0x89, 0x01, 0x81];
    };
    let address = format!("{number}/TYPE=PLMN\0");
    let mut h = vec![0x89, (address.len() + 1) as u8, 0x80];
    h.extend_from_slice(address.as_bytes());
    h
}

/// A To header naming `number`.
fn to_header(number: &str) -> Vec<u8> {
    let mut h = vec![0x97];
    h.extend_from_slice(format!("{number}/TYPE=PLMN\0").as_bytes());
    h
}

/// A PDU with the given headers and a `text.txt` body.
fn pdu_with(headers: &[Vec<u8>], body: &[u8]) -> Vec<u8> {
    let mut bytes = headers.concat();
    bytes.push(0x8e);
    bytes.extend_from_slice(b"text.txt\0");
    bytes.extend_from_slice(body);
    bytes
}

#[test]
fn multi_line_body_survives_and_a_control_byte_ends_it() {
    let headers = [to_header("+15555550100")];
    let parsed = parse_bytes(&pdu_with(&headers, b"a\nb\r\nc\td"));
    assert_eq!(parsed.body, "a\nb\r\nc\td");
    let parsed = parse_bytes(&pdu_with(&headers, b"kept\x01lost"));
    assert_eq!(parsed.body, "kept");
}

#[test]
fn from_the_owner_is_sent() {
    let parsed = parse_bytes(&pdu_with(
        &[from_header(Some("+15555550100")), to_header("+14075551234")],
        b"sent",
    ));
    assert!(parsed.has_from);
    assert!(parsed.is_sent);
    assert_eq!(parsed.sender_number, "5555550100");
}

#[test]
fn from_another_number_is_received_from_it() {
    let parsed = parse_bytes(&pdu_with(
        &[from_header(Some("+14075551234")), to_header("+15555550100")],
        b"received",
    ));
    assert!(parsed.has_from);
    assert!(!parsed.is_sent);
    assert_eq!(parsed.sender_number, "4075551234");
}

#[test]
fn from_the_insert_address_token_to_the_owner_is_received_from_the_other_party() {
    // A group MMS whose From the carrier left for the phone to fill in.
    let mut cc = to_header("+14075551234");
    cc[0] = 0x82; // Cc
    let parsed = parse_bytes(&pdu_with(
        &[from_header(None), to_header("+15555550100"), cc],
        b"received",
    ));
    assert!(!parsed.is_sent);
    assert_eq!(parsed.sender_number, "4075551234");
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

/// A GO named part: `0x8e`, the NUL-terminated name, then the payload.
fn push_named_part(data: &mut Vec<u8>, name: &str, payload: &[u8]) {
    data.push(0x8e);
    data.extend_from_slice(name.as_bytes());
    data.push(0);
    data.extend_from_slice(payload);
}

/// A WSP uintvar: seven bits per byte, high bit set on all but the last.
fn uintvar(mut value: usize) -> Vec<u8> {
    let mut out = vec![(value & 0x7f) as u8];
    value >>= 7;
    while value > 0 {
        out.insert(0, 0x80 | (value & 0x7f) as u8);
        value >>= 7;
    }
    out
}

/// A multipart.mixed Content-Type header and a WSP multipart body (WAP-230
/// 8.5): the part count, then each part's header length, data length,
/// headers, and data.
fn wsp_multipart(parts: &[(Vec<u8>, &[u8])]) -> Vec<u8> {
    let mut data = vec![0x84]; // Content-Type
    data.extend_from_slice(b"application/vnd.wap.multipart.mixed\0");
    data.extend(uintvar(parts.len()));
    for (headers, payload) in parts {
        data.extend(uintvar(headers.len()));
        data.extend(uintvar(payload.len()));
        data.extend_from_slice(headers);
        data.extend_from_slice(payload);
    }
    data
}

/// A multipart whose parts carry only a text content type (no name).
/// Part `i` holds `len` bytes of value `i`.
fn multipart_of(content_types: &[&str], len: usize) -> Vec<u8> {
    let payloads: Vec<Vec<u8>> = (0..content_types.len())
        .map(|i| vec![i as u8 + 0x20; len])
        .collect();
    let parts: Vec<(Vec<u8>, &[u8])> = content_types
        .iter()
        .zip(&payloads)
        .map(|(ct, payload)| {
            let mut headers = ct.as_bytes().to_vec();
            headers.push(0);
            (headers, payload.as_slice())
        })
        .collect();
    wsp_multipart(&parts)
}

/// Part headers in the Content-Type general form: a well-known media id and
/// a `Name` parameter (WAP-230 table 38), plus a UTF-8 `Charset` when asked.
fn part_headers(media: u8, name: &str, utf8: bool) -> Vec<u8> {
    let mut value = vec![media | 0x80];
    if utf8 {
        value.extend_from_slice(&[0x88, 0xea]); // Charset = UTF-8
    }
    value.push(0x85); // Name
    value.extend_from_slice(name.as_bytes());
    value.push(0);
    let mut headers = vec![value.len() as u8];
    headers.extend(value);
    headers
}

/// A JPEG: the SOI marker and `len` bytes of `fill`, big enough to pass the stub guard.
fn jpeg_of(fill: u8, len: usize) -> Vec<u8> {
    let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe0];
    jpeg.extend(std::iter::repeat_n(fill, len));
    jpeg
}

/// WSP well-known media ids (WAP-230 table 40).
const TEXT_PLAIN: u8 = 0x03;
const IMAGE_JPEG: u8 = 0x17;

#[test]
fn multipart_without_smil_gives_the_text_body_and_the_named_jpeg() {
    let jpeg = jpeg_of(0x11, 200);
    let body = wsp_multipart(&[
        (
            part_headers(TEXT_PLAIN, "text_0.txt", true),
            b"Photo attached",
        ),
        (part_headers(IMAGE_JPEG, "photo.jpg", false), &jpeg),
    ]);
    let mut bytes = vec![0x8c, 0x84]; // m-retrieve-conf
    bytes.extend(from_header(Some("+4075551234")));
    bytes.extend(to_header("+15555550100"));
    bytes.extend(body);

    let parsed = parse_bytes(&bytes);
    assert_eq!(parsed.body, "Photo attached");
    assert_eq!(parsed.attachments.len(), 1);
    assert_eq!(parsed.attachments[0].ext, ".jpg");
    assert_eq!(parsed.attachments[0].data, jpeg);
    assert_eq!(
        parsed.attachments[0].smil_name.as_deref(),
        Some("photo.jpg")
    );
    assert_eq!(parsed.decode_quality, "structured");
}

#[test]
fn two_jpeg_parts_give_two_attachments_with_their_own_bytes() {
    let first = jpeg_of(0x11, 200);
    let second = jpeg_of(0x22, 300);
    let body = wsp_multipart(&[
        (part_headers(IMAGE_JPEG, "one.jpg", false), &first),
        (part_headers(IMAGE_JPEG, "two.jpg", false), &second),
    ]);
    let mut bytes = vec![0x8c, 0x84]; // m-retrieve-conf
    bytes.extend(from_header(Some("+4075551234")));
    bytes.extend(to_header("+15555550100"));
    bytes.extend(body);

    let parsed = parse_bytes(&bytes);
    assert_eq!(parsed.body, "");
    let names: Vec<Option<&str>> = parsed
        .attachments
        .iter()
        .map(|a| a.smil_name.as_deref())
        .collect();
    assert_eq!(names, [Some("one.jpg"), Some("two.jpg")]);
    assert_eq!(parsed.attachments[0].data, first);
    assert_eq!(parsed.attachments[1].data, second);
}

#[test]
fn named_media_parts_keep_their_type() {
    let media = vec![0x11u8; 80];
    let wav = vec![0x11u8; 10_000]; // shorter WAVs are dropped as stubs
    let mut data = Vec::new();
    push_named_part(&mut data, "pic.png", &media);
    push_named_part(&mut data, "anim.gif", &media);
    push_named_part(&mut data, "voice.amr", &media);
    push_named_part(&mut data, "song.mp3", &media);
    push_named_part(&mut data, "sound.wav", &wav);
    push_named_part(&mut data, "clip.mp4", &media);
    push_named_part(&mut data, "movie.3gp", &media);
    push_named_part(&mut data, "note.txt", b"Hello note");
    push_named_part(&mut data, "page.html", b"Hello page");

    let structured = decode_mms_best_effort(&data);
    let smil = SmilRefs::default();
    let atts = attachments_from_named_parts(&structured.named_parts, &smil);
    let exts: Vec<&str> = atts.iter().map(|a| a.ext.as_str()).collect();
    assert_eq!(
        exts,
        [".png", ".gif", ".amr", ".mp3", ".wav", ".mp4", ".3gp"],
        "text and HTML parts are not attachments"
    );
    assert_eq!(atts[0].smil_name.as_deref(), Some("pic.png"));
    assert_eq!(atts[0].data, media);
    let body = body_from_named_parts(&structured.named_parts, &smil).unwrap();
    assert_eq!(body, "Hello note\nHello page");
}

#[test]
fn unnamed_parts_take_their_extension_from_the_content_type() {
    let cases = [
        ("image/jpg", Some(".jpg")),
        ("image/png", Some(".png")),
        ("image/gif", Some(".gif")),
        ("image/tiff", Some(".tiff")),
        ("image/vnd.wap.wbmp", Some(".wbmp")),
        ("audio/amr", Some(".amr")),
        ("audio/3gpp", Some(".amr")),
        ("audio/mpeg", Some(".mp3")),
        ("audio/mp3", Some(".mp3")),
        ("audio/wav", Some(".wav")),
        ("audio/x-wav", Some(".wav")),
        ("video/3gpp", Some(".3gp")),
        ("video/mp4", Some(".mp4")),
        // Types with no extension of their own are kept as .bin.
        ("image/heic", Some(".bin")),
        ("audio/aac", Some(".bin")),
        ("video/quicktime", Some(".bin")),
        // Text is the body, never an attachment.
        ("text/plain", None),
        ("text/html", None),
    ];
    let types: Vec<&str> = cases.iter().map(|(ct, _)| *ct).collect();
    // 10,000 bytes, since a shorter WAV is dropped as a stub.
    let structured = decode_mms_best_effort(&multipart_of(&types, 10_000));
    assert_eq!(structured.parts.len(), cases.len());
    let smil = SmilRefs::default();
    for (part, (ct, ext)) in structured.parts.iter().zip(cases) {
        let single = StructuredMms {
            parts: vec![part.clone()],
            ..StructuredMms::default()
        };
        let atts = attachments_from_structured(&single, &smil);
        let got = atts.first().map(|a| a.ext.as_str());
        assert_eq!(got, ext, "{ct}");
    }
}

mod robustness;

/// Part headers: a well-known media id, then a WSP Content-Location header
/// (`0x8e` + text-string), the wire shape GO SMS Pro's named parts share.
fn part_headers_with_location(media: u8, name: &str) -> Vec<u8> {
    let mut headers = vec![media | 0x80, 0x8e];
    headers.extend_from_slice(name.as_bytes());
    headers.push(0);
    headers
}

#[test]
fn two_jpeg_parts_with_content_locations_keep_their_own_bytes() {
    let first = jpeg_of(0x11, 200);
    let second = jpeg_of(0x22, 300);
    let body = wsp_multipart(&[
        (part_headers_with_location(IMAGE_JPEG, "one.jpg"), &first),
        (part_headers_with_location(IMAGE_JPEG, "two.jpg"), &second),
    ]);
    let mut bytes = vec![0x8c, 0x84]; // m-retrieve-conf
    bytes.extend(from_header(Some("+4075551234")));
    bytes.extend(to_header("+15555550100"));
    bytes.extend(body);

    let parsed = parse_bytes(&bytes);
    let names: Vec<Option<&str>> = parsed
        .attachments
        .iter()
        .map(|a| a.smil_name.as_deref())
        .collect();
    assert_eq!(names, [Some("one.jpg"), Some("two.jpg")]);
    assert_eq!(parsed.attachments[0].data, first);
    assert_eq!(parsed.attachments[1].data, second);
}

/// A structured part with a content id, a content type, and UTF-8 data.
fn part(cid: &str, content_type: &str, data: &str) -> MmsPart {
    MmsPart {
        content_type: content_type.to_string(),
        content_location: None,
        content_id: Some(cid.to_string()),
        filename: None,
        charset: Some(crate::mms_enc::CHARSET_UTF8),
        data: data.as_bytes().to_vec(),
    }
}

#[test]
fn start_names_the_text_part_that_is_the_body() {
    let msg = StructuredMms {
        content_start: Some("<second>".to_string()),
        parts: vec![
            part("first", "text/plain", "First"),
            part("second", "text/plain", "Second"),
        ],
        ..StructuredMms::default()
    };
    let body = body_from_structured(&msg, &SmilRefs::default());
    assert_eq!(body.as_deref(), Some("Second"));
}

#[test]
fn start_naming_smil_under_a_text_type_is_not_the_body() {
    let msg = StructuredMms {
        content_start: Some("<layout>".to_string()),
        parts: vec![
            part("layout", "text/xml", "<smil><body/></smil>"),
            part("words", "text/plain", "Hello"),
        ],
        ..StructuredMms::default()
    };
    let body = body_from_structured(&msg, &SmilRefs::default());
    assert_eq!(body.as_deref(), Some("Hello"));
}

#[test]
fn body_joins_plain_and_html_text_parts_and_skips_media() {
    let msg = StructuredMms {
        parts: vec![
            part("a", "text/plain; charset=utf-8", "Plain"),
            part("b", "text/html", "Marked up"),
            part("c", "image/jpeg", "not text"),
        ],
        ..StructuredMms::default()
    };
    let body = body_from_structured(&msg, &SmilRefs::default());
    assert_eq!(body.as_deref(), Some("Plain\nMarked up"));
}
