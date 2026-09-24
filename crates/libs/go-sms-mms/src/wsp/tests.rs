//! Each test guards one shape rule from the module documentation.

use super::*;

fn cur(bytes: &[u8]) -> Cursor<'_> {
    Cursor::new(bytes)
}

#[test]
fn uintvar_takes_seven_bits_per_byte_and_stops_at_the_clear_high_bit() {
    assert_eq!(uintvar(&mut cur(&[0x05])).unwrap(), 5);
    // 0x83 0x5e is (3 << 7) | 0x5e = 478, the data length of a real SMIL part.
    let mut c = cur(&[0x83, 0x5e, 0xaa]);
    assert_eq!(uintvar(&mut c).unwrap(), 478);
    assert_eq!(c.pos, 2);
    assert_eq!(
        uintvar(&mut cur(&[0xff, 0xff, 0xff, 0xff, 0xff, 0x01])).unwrap_err(),
        Error {
            at: 0,
            what: "uintvar longer than five bytes"
        }
    );
}

#[test]
fn value_length_is_one_byte_to_30_and_a_uintvar_after_31() {
    assert_eq!(value_length(&mut cur(&[30])).unwrap(), 30);
    assert_eq!(value_length(&mut cur(&[31, 0x81, 0x00])).unwrap(), 128);
    let mut c = cur(&[0x05, 32]);
    c.pos = 1;
    assert_eq!(
        value_length(&mut c).unwrap_err(),
        Error {
            at: 1,
            what: "value-length above 31"
        }
    );
}

#[test]
fn text_string_drops_the_quote_byte_and_needs_a_nul() {
    assert_eq!(text_string(&mut cur(b"abc\0")).unwrap(), b"abc");
    assert_eq!(text_string(&mut cur(b"\x7f\x80x\0")).unwrap(), b"\x80x");
    assert_eq!(text_string(&mut cur(b"\"<smil>\0")).unwrap(), b"<smil>");
    let mut c = cur(b"a\0b\0");
    text_string(&mut c).unwrap();
    assert_eq!(c.pos, 2, "the NUL is consumed");
    assert_eq!(
        text_string(&mut cur(b"abc")).unwrap_err().what,
        "text-string without a NUL"
    );
}

#[test]
fn encoded_string_reads_a_charset_and_checks_its_length() {
    // Value-length 5: charset UTF-8 (0xea), "hi", NUL — as a phone writes a To.
    let mut c = cur(&[0x04, 0xea, b'h', b'i', 0x00, 0x97]);
    assert_eq!(
        encoded_string(&mut c).unwrap(),
        EncodedString {
            charset: Some(CHARSET_UTF8),
            bytes: b"hi".to_vec()
        }
    );
    assert_eq!(c.pos, 5, "the next header code is left in place");
    // A plain Text-string has no charset.
    assert_eq!(
        encoded_string(&mut cur(b"hi\0")).unwrap(),
        EncodedString {
            charset: None,
            bytes: b"hi".to_vec()
        }
    );
    // A length that does not match the text is refused, not read past.
    assert_eq!(
        encoded_string(&mut cur(&[0x06, 0xea, b'h', b'i', 0x00, 0x97, 0x00])).unwrap_err(),
        Error {
            at: 0,
            what: "encoded-string length does not match its text"
        }
    );
    assert_eq!(
        encoded_string(&mut cur(&[0x09, 0xea, b'h']))
            .unwrap_err()
            .what,
        "encoded-string length past the end"
    );
}

#[test]
fn decode_text_follows_the_charset() {
    assert_eq!(decode_text("café".as_bytes(), Some(CHARSET_UTF8)), "café");
    assert_eq!(decode_text("café".as_bytes(), None), "café");
    assert_eq!(decode_text(b"caf\xe9", Some(CHARSET_ISO_8859_1)), "café");
    assert_eq!(
        decode_text(&[0x00, 0x68, 0x00, 0x69], Some(CHARSET_UCS2)),
        "hi"
    );
    assert_eq!(
        decode_text(b"\xff\xfe", Some(CHARSET_UTF8)),
        "\u{fffd}\u{fffd}"
    );
}

#[test]
fn well_known_content_types_are_at_their_assigned_numbers() {
    // The table has 0x00 through 0x4b. A missing row shifts every id after
    // it: with 0x04 to 0x0a absent, 0x33 read as cert-response and no
    // multipart was ever recognised.
    assert_eq!(WELL_KNOWN_CONTENT_TYPES.len(), 0x4c);
    assert_eq!(WELL_KNOWN_CONTENT_TYPES[0x03], "text/plain");
    assert_eq!(WELL_KNOWN_CONTENT_TYPES[0x0a], "text/vnd.wap.wta-event");
    assert_eq!(WELL_KNOWN_CONTENT_TYPES[0x0b], "multipart/*");
    assert_eq!(WELL_KNOWN_CONTENT_TYPES[0x1e], "image/jpeg");
    assert_eq!(
        WELL_KNOWN_CONTENT_TYPES[0x23],
        "application/vnd.wap.multipart.mixed"
    );
    assert_eq!(
        WELL_KNOWN_CONTENT_TYPES[0x33],
        "application/vnd.wap.multipart.related"
    );
    assert_eq!(
        WELL_KNOWN_CONTENT_TYPES[0x4b],
        "application/vnd.oma.drm.rights+wbxml"
    );
}

#[test]
fn constrained_content_type_is_one_id_or_a_text_string() {
    assert_eq!(content_type(&mut cur(&[0x9e])).unwrap().media, "image/jpeg");
    assert_eq!(
        content_type(&mut cur(b"Image/JPEG\0")).unwrap().media,
        "image/jpeg"
    );
    assert_eq!(
        content_type(&mut cur(&[0xff])).unwrap_err(),
        Error {
            at: 0,
            what: "unassigned well-known content type"
        }
    );
}

#[test]
fn general_content_type_reads_its_parameters_by_table_38_code() {
    // Value-length 16: text/plain, Name "text_01.txt", Charset UTF-8 — a
    // real text part's header.
    let bytes = b"\x10\x83\x85text_01.txt\0\x81\xea\x97";
    let mut c = cur(bytes);
    let ct = content_type(&mut c).unwrap();
    assert_eq!(ct.media, "text/plain");
    assert_eq!(ct.params["Name"], "text_01.txt");
    assert_eq!(ct.charset(), Some(CHARSET_UTF8));
    assert_eq!(c.pos, 17, "the cursor lands on the byte after the value");

    // multipart.related with Start and Type, as the message Content-Type.
    let bytes = b"\x1d\xb3\x8a<0.smil>\0\x89application/smil\0";
    let ct = content_type(&mut cur(bytes)).unwrap();
    assert!(ct.is_multipart());
    assert_eq!(ct.params["Start"], "<0.smil>");
    assert_eq!(ct.params["Type"], "application/smil");
}

#[test]
fn every_parameter_code_a_part_uses_is_read() {
    // Type as an integer (0x03), Filename in both its codes, Start-info,
    // and the 1.4 text forms of Name and Start.
    let bytes = b"\x1b\x83\x83\x9e\x86a.txt\0\x98b.txt\0\x8bsi\0\x97n\0\x99s\0";
    let ct = content_type(&mut cur(bytes)).unwrap();
    assert_eq!(ct.params["Type"], "30");
    assert_eq!(ct.params["Filename"], "b.txt", "the later code wins");
    assert_eq!(ct.params["Start-info"], "si");
    assert_eq!(ct.params["Name"], "n");
    assert_eq!(ct.params["Start"], "s");
}

#[test]
fn an_unknown_parameter_ends_the_parameters_but_not_the_value() {
    // Padding (0x08) is not read; the cursor still lands at the declared end.
    let bytes = b"\x06\x83\x88\x80\x85x\0\x97";
    let mut c = cur(bytes);
    let ct = content_type(&mut c).unwrap();
    assert_eq!(ct.media, "text/plain");
    assert!(ct.params.is_empty());
    assert_eq!(c.pos, 7);
    // A length past the end is refused.
    assert_eq!(
        content_type(&mut cur(&[0x09, 0x83])).unwrap_err().what,
        "content-type length past the end"
    );
}

#[test]
fn general_content_type_accepts_a_long_integer_media_id_and_an_untyped_parameter() {
    // Long-integer 0x1e (one byte), then an untyped "foo" = "bar" and an
    // untyped "n" = 3 as a Short-integer.
    let bytes = b"\x0d\x01\x1efoo\0bar\0n\0\x83";
    let ct = content_type(&mut cur(bytes)).unwrap();
    assert_eq!(ct.media, "image/jpeg");
    assert_eq!(ct.params["foo"], "bar");
    assert_eq!(ct.params["n"], "3");
}

/// A multipart body of `parts`, each `(header bytes, data)`.
fn body(parts: &[(&[u8], &[u8])]) -> Vec<u8> {
    let mut out = vec![parts.len() as u8];
    for (h, d) in parts {
        out.push(h.len() as u8);
        out.push(d.len() as u8);
        out.extend_from_slice(h);
        out.extend_from_slice(d);
    }
    out
}

#[test]
fn multipart_finds_each_part_by_its_two_lengths_alone() {
    // The picture's bytes are header codes and a NUL; the lengths, not the
    // bytes, say where the part ends.
    let picture = b"\x97\x89\x00\x84\x8c";
    let bytes = body(&[(b"\x83", b"hello"), (b"\x9e", picture)]);
    let mut c = cur(&bytes);
    let parts = multipart(&mut c).unwrap();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].content_type.media, "text/plain");
    assert_eq!(parts[0].data, b"hello");
    assert_eq!(parts[1].content_type.media, "image/jpeg");
    assert_eq!(parts[1].data, picture);
    assert_eq!(c.remaining(), 0);
}

#[test]
fn multipart_part_headers_give_a_content_id_and_a_location() {
    // Content-ID as a Quoted-string, Content-Location as text; the
    // Content-Type Name wins as the part's name.
    let headers = b"\x0f\x83\x85text_2.txt\0\x81\xea\xc0\"<text_2.txt>\0\x8eloc.txt\0";
    let bytes = body(&[(headers, b"x")]);
    let parts = multipart(&mut cur(&bytes)).unwrap();
    let p = &parts[0];
    assert_eq!(p.content_id.as_deref(), Some("text_2.txt"));
    assert_eq!(p.content_location.as_deref(), Some("loc.txt"));
    assert_eq!(p.name(), Some("text_2.txt"));

    // Token-text headers name the same things; Content-Disposition is skipped
    // by its length; another well-known header ends the header walk.
    let headers = b"\x9eContent-ID\0<img1>\0\xae\x03\x81\x85x\x8e_\0Content-Location\0pic.jpg\0";
    let bytes = body(&[(headers, b"y")]);
    let parts = multipart(&mut cur(&bytes)).unwrap();
    let p = &parts[0];
    assert_eq!(p.content_id.as_deref(), Some("img1"));
    assert_eq!(p.content_location.as_deref(), Some("pic.jpg"));
    assert_eq!(p.name(), Some("pic.jpg"), "location before id");
    let headers = b"\x9e\x93x\0\x8eignored.jpg\0";
    let bytes = body(&[(headers, b"z")]);
    let parts = multipart(&mut cur(&bytes)).unwrap();
    assert_eq!(
        parts[0].content_location, None,
        "headers after an unknown code are not read"
    );
    assert_eq!(parts[0].name(), None);
}

#[test]
fn multipart_refuses_a_part_that_runs_past_the_end_or_an_absurd_count() {
    let mut short = body(&[(b"\x83", b"hello")]);
    short.pop();
    assert_eq!(multipart(&mut cur(&short)).unwrap_err().what, "part data");
    assert_eq!(
        multipart(&mut cur(&[0x82, 0x01])).unwrap_err(),
        Error {
            at: 0,
            what: "multipart with more than 256 parts"
        }
    );
    // 256 empty parts are the most a body may hold, and they are read.
    let mut many = vec![0x82, 0x00];
    many.extend(std::iter::repeat_n([1u8, 0u8, 0x83], 256).flatten());
    assert_eq!(multipart(&mut cur(&many)).unwrap().len(), 256);
    assert_eq!(
        multipart(&mut cur(&[0x01, 0x05])).unwrap_err().what,
        "uintvar"
    );
}

#[test]
fn strip_angle_brackets_leaves_the_bare_id() {
    assert_eq!(strip_angle_brackets("<img1>"), "img1");
    assert_eq!(strip_angle_brackets("cid:img1"), "img1");
    assert_eq!(strip_angle_brackets(" <a> "), "a");
    assert_eq!(strip_angle_brackets("plain"), "plain");
}

#[test]
fn integers_have_their_two_shapes() {
    assert_eq!(short_integer(&mut cur(&[0x90])).unwrap(), 0x10);
    assert_eq!(
        short_integer(&mut cur(&[0x10])).unwrap_err().what,
        "short-integer"
    );
    assert_eq!(long_integer(&mut cur(&[0x02, 0x03, 0xe8])).unwrap(), 1000);
    let mut c = cur(&[0xff, 31]);
    c.pos = 1;
    assert_eq!(
        long_integer(&mut c).unwrap_err(),
        Error {
            at: 1,
            what: "long-integer length outside 1 to 30"
        }
    );
    assert_eq!(long_integer(&mut cur(&[0x00])).unwrap_err().at, 0);
    let thirty = [&[30u8][..], &[0u8; 29], &[7u8]].concat();
    assert_eq!(
        long_integer(&mut cur(&thirty)).unwrap(),
        7,
        "30 bytes is the most a Long-integer holds"
    );
    assert_eq!(
        long_integer(&mut cur(&[0x02, 0x03])).unwrap_err().what,
        "long-integer"
    );
    assert_eq!(integer_value(&mut cur(&[0xea])).unwrap(), 106);
    assert_eq!(integer_value(&mut cur(&[0x02, 0x03, 0xe8])).unwrap(), 1000);
}
