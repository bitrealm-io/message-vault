use super::*;
use crate::decoders::{decode_uint_var, decode_value_length};
use std::path::PathBuf;

#[test]
fn uint_var_single_and_multi() {
    let mut c = Cursor::new(&[0x05]);
    assert_eq!(decode_uint_var(&mut c).unwrap(), 5);
    // 0x81 0x02 => (1<<7)|2 = 130
    let mut c = Cursor::new(&[0x81, 0x02]);
    assert_eq!(decode_uint_var(&mut c).unwrap(), 130);
}

#[test]
fn value_length_short_and_quoted() {
    let mut c = Cursor::new(&[0x1a]);
    assert_eq!(decode_value_length(&mut c).unwrap(), 26);
    let mut c = Cursor::new(&[0x1f, 0x20]);
    assert_eq!(decode_value_length(&mut c).unwrap(), 32);
}

#[test]
fn scan_from_to_on_recv_fixture_shape() {
    // From + To fragment matching I_1609459200_recv.pdu: Value-lengths overshoot
    // into the next short-integer header (no NULs after PLMN).
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&[0x89, 0x1a, 0x80, 0x18, 0xea]);
    bytes.extend_from_slice(b"+4075551234/TYPE=PLMN");
    bytes.extend_from_slice(&[0x97, 0x18, 0xea]);
    bytes.extend_from_slice(b"+15555550100/TYPE=PLMN");
    bytes.extend_from_slice(&[0x8e]); // next header byte overlapped by To length
    let msg = scan_mms_addresses(&bytes);
    assert_eq!(msg.from.as_deref(), Some("+4075551234/TYPE=PLMN"));
    assert_eq!(msg.to, vec!["+15555550100/TYPE=PLMN".to_string()]);
}

#[test]
fn scan_real_recv_fixture() {
    let data = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pdu/I_1609459200_recv.pdu"),
    )
    .unwrap();
    let msg = decode_mms_best_effort(&data);
    assert_eq!(msg.from.as_deref(), Some("+4075551234/TYPE=PLMN"));
    assert!(msg.to.iter().any(|t| t.contains("15555550100")));
    assert!(
        msg.named_parts
            .iter()
            .any(|p| p.name == "text.txt" && p.data.starts_with(b"Hello one to one"))
    );
}

#[test]
fn scan_date_header_fragment() {
    // Date = 0x85, long-integer length 4, value 0x5fee6600 = 1609459200
    let mut bytes = vec![0x85, 0x04, 0x5f, 0xee, 0x66, 0x00];
    bytes.extend_from_slice(&[0x8e]);
    bytes.extend_from_slice(b"text.txt\0hi");
    let msg = decode_mms_best_effort(&bytes);
    assert_eq!(msg.date_unix, Some(1609459200));
    assert_eq!(msg.named_parts[0].name, "text.txt");
    assert_eq!(msg.named_parts[0].data, b"hi");
}

#[test]
fn midfile_multipart_content_type() {
    let related_idx = WELL_KNOWN_CONTENT_TYPES
        .iter()
        .position(|s| *s == "application/vnd.wap.multipart.related")
        .expect("related ct");
    let related_si = (related_idx as u8) | 0x80;

    // Junk prefix, then Content-Type multipart.related with text + jpeg parts.
    let text = b"Hello multipart";
    let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe0];
    jpeg.extend(std::iter::repeat_n(0x11, 80));

    let mut body = vec![
        0x02,             // nEntries
        0x01,             // headersLen
        text.len() as u8, // dataLen
        0x83,             // text/plain
    ];
    body.extend_from_slice(text);
    body.push(0x01);
    body.push(jpeg.len() as u8);
    body.push(0x97); // image/jpeg
    body.extend_from_slice(&jpeg);

    let mut bytes = vec![0x00, 0x01, 0x02, 0x03, 0x04];
    bytes.push(0x84);
    bytes.push(related_si);
    bytes.extend_from_slice(&body);

    let parts = scan_multipart_bodies(&bytes);
    assert_eq!(parts.len(), 2);
    assert!(parts[0].content_type.contains("text/plain"));
    assert_eq!(parts[0].data, text);
    assert!(parts[1].content_type.contains("image/jpeg"));
    assert!(parts[1].data.starts_with(b"\xff\xd8\xff"));

    let msg = decode_mms_best_effort(&bytes);
    assert!(msg.parts.iter().any(|p| p.data == text));
}

#[test]
fn scan_subject_header() {
    // Subject 0x96 (field 0x16), encoded-string: value-length 4, charset UTF-8, "Hi\0"
    let bytes = [0x96u8, 0x04, 0xea, b'H', b'i', 0x00];
    let msg = scan_mms_addresses(&bytes);
    assert_eq!(msg.subject.as_deref(), Some("Hi"));
}

#[test]
fn scan_status_header() {
    // Status 0x95 (field 0x15), Retrieved = short-int 0x81
    let bytes = [0x95u8, 0x81];
    let msg = scan_mms_addresses(&bytes);
    assert_eq!(msg.status.as_deref(), Some("Retrieved"));
}

#[test]
fn scan_response_status_and_text() {
    // Response-Status Ok (0x92, short-int 0x80) + Response-Text "Error"
    let mut bytes = vec![0x92u8, 0x80];
    bytes.push(0x93);
    bytes.extend_from_slice(b"Error\0");
    let msg = scan_mms_addresses(&bytes);
    assert_eq!(msg.response_status.as_deref(), Some("Ok"));
    assert_eq!(msg.response_text.as_deref(), Some("Error"));
}

#[test]
fn scan_message_size_header() {
    // Message-Size 0x8e + long-int length 2, value 0x1388 = 5000
    let bytes = [0x8eu8, 0x02, 0x13, 0x88];
    let msg = scan_mms_addresses(&bytes);
    assert_eq!(msg.message_size, Some(5000));
}

#[test]
fn go_0x8e_named_part_not_message_size() {
    let mut bytes = Vec::new();
    bytes.push(0x8e);
    bytes.extend_from_slice(b"text.txt\0Hello body");
    let msg = decode_mms_best_effort(&bytes);
    assert!(msg.message_size.is_none());
    assert_eq!(msg.named_parts.len(), 1);
    assert_eq!(msg.named_parts[0].name, "text.txt");
    assert_eq!(msg.named_parts[0].data, b"Hello body");
}

#[test]
fn named_text_part_keeps_non_ascii_utf8() {
    // Continuation bytes (>= 0x80) are part of the text, not part terminators.
    let mut bytes = Vec::new();
    bytes.push(0x8e);
    bytes.extend_from_slice(b"text.txt\0");
    bytes.extend_from_slice("Héllo — привет, 世界 🌍".as_bytes());
    bytes.push(0x8c); // trailing header byte, as in real GO dumps
    let named = scan_named_parts(&bytes);
    assert_eq!(named.len(), 1);
    assert_eq!(named[0].name, "text.txt");
    assert_eq!(named[0].data, "Héllo — привет, 世界 🌍".as_bytes());
}

#[test]
fn named_text_part_stops_at_header_byte() {
    // ASCII payload followed by a high-bit header byte: payload ends at the
    // header byte, which is not valid UTF-8 on its own.
    let mut bytes = Vec::new();
    bytes.push(0x8e);
    bytes.extend_from_slice(b"text.txt\0Hello");
    bytes.push(0x8c);
    bytes.extend_from_slice(&[0xff, 0xd8]); // JPEG SOI after the header byte
    let named = scan_named_parts(&bytes);
    assert_eq!(named.len(), 1);
    assert_eq!(named[0].name, "text.txt");
    assert_eq!(named[0].data, b"Hello");
}

#[test]
fn scan_subject_ucs2() {
    // Subject + UCS-2 charset (MIBEnum 1000 as long-int) + "Hi" UTF-16BE
    let bytes = [0x96u8, 0x07, 0x02, 0x03, 0xe8, 0x00, 0x48, 0x00, 0x69];
    let msg = scan_mms_addresses(&bytes);
    assert_eq!(msg.subject.as_deref(), Some("Hi"));
}

#[test]
fn scan_bcc_transaction_class_version() {
    let mut bytes = Vec::new();
    // Bcc +15551234567/TYPE=PLMN (UTF-8 encoded-string)
    bytes.push(0x81); // Bcc
    bytes.push(0x18);
    bytes.push(0xea);
    bytes.extend_from_slice(b"+15551234567/TYPE=PLMN");
    // Transaction-Id
    bytes.push(0x98);
    bytes.extend_from_slice(b"tx-abc\0");
    // Message-Class Personal
    bytes.push(0x8a);
    bytes.push(0x80);
    // MMS-Version 1.2
    bytes.push(0x8d);
    bytes.push(0x92); // 0x12 | 0x80
    let msg = scan_mms_addresses(&bytes);
    assert!(msg.bcc.iter().any(|a| a.contains("15551234567")));
    assert_eq!(msg.transaction_id.as_deref(), Some("tx-abc"));
    assert_eq!(msg.message_class.as_deref(), Some("Personal"));
    assert_eq!(msg.mms_version.as_deref(), Some("1.2"));
}

#[test]
fn application_header_captured() {
    // Full-ish header: Message-Type m-retrieve-conf, app header, Content-Type text/plain
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&[0x8c, 0x84]); // Message-Type retrieve-conf
    bytes.extend_from_slice(b"X-Custom\0hello-app\0");
    bytes.extend_from_slice(&[0x84, 0x83]); // Content-Type text/plain
    let msg = decode_mms(&bytes).expect("decode");
    assert_eq!(
        msg.application_headers.get("X-Custom").map(String::as_str),
        Some("hello-app")
    );
}

#[test]
fn part_filename_from_content_type_param() {
    let related_idx = WELL_KNOWN_CONTENT_TYPES
        .iter()
        .position(|s| *s == "application/vnd.wap.multipart.related")
        .expect("related ct");
    let related_si = (related_idx as u8) | 0x80;
    let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe0];
    jpeg.extend(std::iter::repeat_n(0x11u8, 80));

    // Content-Type general form: value-length, image/jpeg, Filename param
    // length = 1 (media si) + 1 (Filename si) + 9 ("photo.jpg\0") = 11 = 0x0b
    let mut headers = vec![
        0x0b, // value-length
        0x97, // image/jpeg
        0x86, // Filename (0x06|0x80)
    ];
    headers.extend_from_slice(b"photo.jpg\0");

    let mut body = vec![0x01, headers.len() as u8, jpeg.len() as u8];
    body.extend_from_slice(&headers);
    body.extend_from_slice(&jpeg);

    let mut bytes = vec![0x84, related_si];
    bytes.extend_from_slice(&body);
    let parts = scan_multipart_bodies(&bytes);
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].filename.as_deref(), Some("photo.jpg"));
    assert_eq!(parts[0].content_location.as_deref(), Some("photo.jpg"));
}

#[test]
fn part_filename_from_content_disposition() {
    let related_idx = WELL_KNOWN_CONTENT_TYPES
        .iter()
        .position(|s| *s == "application/vnd.wap.multipart.related")
        .expect("related ct");
    let related_si = (related_idx as u8) | 0x80;
    let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe0];
    jpeg.extend(std::iter::repeat_n(0x22u8, 80));

    // CT short jpeg + Content-Disposition attachment with Filename
    // CD: value-length = 1 (token) + 1 (Filename) + 8 ("pic.png\0") = 10
    let mut headers = vec![
        0x97, // image/jpeg
        0xae, // Content-Disposition
        0x0a, // value-length
        0x81, // attachment
        0x86, // Filename
    ];
    headers.extend_from_slice(b"pic.png\0");

    let mut body = vec![0x01, headers.len() as u8, jpeg.len() as u8];
    body.extend_from_slice(&headers);
    body.extend_from_slice(&jpeg);

    let mut bytes = vec![0x84, related_si];
    bytes.extend_from_slice(&body);
    let parts = scan_multipart_bodies(&bytes);
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].filename.as_deref(), Some("pic.png"));
}

#[test]
fn content_type_charset_and_filename_params() {
    // value-length: jpeg(1) + Charset si(1)+UTF-8(1) + Filename(1)+name(9) = 13
    let mut headers = vec![
        0x0d, 0x97, // image/jpeg
        0x88, // Charset
        0xea, // UTF-8
        0x86, // Filename
    ];
    headers.extend_from_slice(b"photo.jpg\0");
    let mut cur = Cursor::new(&headers);
    let (ct, params) = decode_content_type_value(&mut cur).expect("ct");
    assert!(ct.contains("jpeg"));
    assert_eq!(params.get("Charset").map(String::as_str), Some("106"));
    assert_eq!(
        params.get("Filename").map(String::as_str),
        Some("photo.jpg")
    );
}

#[test]
fn multipart_0x83_is_not_content_location() {
    let related_idx = WELL_KNOWN_CONTENT_TYPES
        .iter()
        .position(|s| *s == "application/vnd.wap.multipart.related")
        .expect("related");
    let related_si = (related_idx as u8) | 0x80;
    let text = b"hi";
    // CT text/plain + spurious 0x83 (WSP Accept-Language id) + short-int value
    let headers = vec![0x83, 0x83, 0x80];
    let mut body = vec![0x01, headers.len() as u8, text.len() as u8];
    body.extend_from_slice(&headers);
    body.extend_from_slice(text);
    let mut bytes = vec![0x84, related_si];
    bytes.extend_from_slice(&body);
    let parts = scan_multipart_bodies(&bytes);
    assert_eq!(parts.len(), 1);
    assert!(parts[0].content_location.is_none());
}

#[test]
fn go_0x8e_after_from_soft_stops_header_decode() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&[0x8c, 0x84]); // m-retrieve-conf
    bytes.extend_from_slice(&[0x89, 0x1a, 0x80, 0x18, 0xea]);
    bytes.extend_from_slice(b"+4075551234/TYPE=PLMN");
    bytes.push(0x8e);
    bytes.extend_from_slice(b"text.txt\0Hello soft");
    let msg = decode_mms_best_effort(&bytes);
    assert!(msg.from.is_some());
    assert!(msg.message_size.is_none());
    assert_eq!(msg.named_parts.len(), 1);
    assert_eq!(msg.named_parts[0].data, b"Hello soft");
}

#[test]
fn empty_subject_does_not_drop_following_to() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&[0x89, 0x1a, 0x80, 0x18, 0xea]);
    bytes.extend_from_slice(b"+4075551234/TYPE=PLMN");
    // Empty Subject text-string
    bytes.extend_from_slice(&[0x96, 0x00]);
    bytes.extend_from_slice(&[0x97, 0x18, 0xea]);
    bytes.extend_from_slice(b"+15555550100/TYPE=PLMN");
    bytes.push(0x8c); // pad
    let msg = scan_mms_addresses(&bytes);
    assert!(msg.from.is_some());
    assert!(msg.to.iter().any(|t| t.contains("5555550100")));
    assert!(msg.subject.is_none());
}
