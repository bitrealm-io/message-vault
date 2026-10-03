use super::*;
use chrono::TimeZone;
use mailparse::{MailHeaderMap, parse_mail};
use message_ir::testutil::{sample_document, sample_imessage_document};
use message_ir::{IrAttachment, IrConversationType, IrDirection, IrMessageKind, IrParticipant};
use std::fs;

const BACKUP_TIME_MS: i64 = 1_791_000_000_000;

fn archive() -> SmsBackupPlusArchive {
    SmsBackupPlusArchive::new(Utc.timestamp_millis_opt(BACKUP_TIME_MS).unwrap())
}

fn photo(bytes: &[u8]) -> IrAttachment {
    IrAttachment {
        path: None,
        original_name: Some("photo.jpg".into()),
        mime_type: Some("image/jpeg".into()),
        digest_sha256: None,
        is_sticker: false,
        transcription: None,
        sticker_effect: None,
        size_bytes: None,
        missing_reason: None,
        bytes: Some(bytes.to_vec()),
    }
}

/// Every `.eml` under `dir/folder`, in file name order, as raw bytes.
fn mails(dir: &Path, folder: &str) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = fs::read_dir(dir.join(folder))
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_string_lossy().into_owned(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect();
    out.sort();
    out
}

fn header(raw: &[u8], name: &str) -> Option<String> {
    parse_mail(raw).unwrap().headers.get_first_value(name)
}

/// A 1:1 SMS conversation with Sam: an incoming SMS, an outgoing SMS, an
/// incoming MMS with a photo and an outgoing MMS with a photo, a second apart.
fn sms_and_mms_document() -> ConversationDocument {
    let mut doc = sample_document("incoming sms");
    let base = doc.messages[0].clone();
    let message = |i: i64, direction, kind, text: &str, attachments| {
        let mut m = base.clone();
        m.guid = format!("{i:032x}");
        m.timestamp_unix_ms = base.timestamp_unix_ms + i * 1000 + 123;
        m.direction = direction;
        m.message_kind = kind;
        m.text = text.into();
        m.attachments = attachments;
        if direction == IrDirection::Outgoing {
            m.sender_handle = Some("+15555550100".into());
            m.sender_display_name = Some("Me".into());
        }
        m
    };
    doc.messages = vec![
        message(
            1,
            IrDirection::Incoming,
            IrMessageKind::Sms,
            "in sms",
            vec![],
        ),
        message(
            2,
            IrDirection::Outgoing,
            IrMessageKind::Sms,
            "out sms",
            vec![],
        ),
        message(
            3,
            IrDirection::Incoming,
            IrMessageKind::Mms,
            "in mms",
            vec![photo(b"\xff\xd8\xffin")],
        ),
        message(
            4,
            IrDirection::Outgoing,
            IrMessageKind::Mms,
            "out mms",
            vec![photo(b"\xff\xd8\xffout")],
        ),
    ];
    doc
}

#[test]
fn each_message_carries_the_headers_its_importer_reads() {
    let tmp = tempfile::tempdir().unwrap();
    let mut report = ExportReport::default();
    archive()
        .write(tmp.path(), &[sms_and_mms_document()], &mut report)
        .unwrap();

    let written = mails(tmp.path(), "+15555550101");
    assert_eq!(written.len(), 4);
    let expected = [
        ("SMS", "1", "in sms"),
        ("SMS", "2", "out sms"),
        ("MMS", "132", "in mms"),
        ("MMS", "128", "out mms"),
    ];
    for ((name, raw), (datatype, kind, text)) in written.iter().zip(expected) {
        assert!(name.ends_with(".eml"), "{name}");
        let get = |h: &str| header(raw, h).unwrap_or_else(|| panic!("{name}: no {h}"));
        assert_eq!(get("X-smssync-datatype"), datatype, "{name}");
        assert_eq!(get("X-smssync-type"), kind, "{name}");
        assert_eq!(get("X-smssync-address"), "+15555550101", "{name}");
        assert_eq!(get("X-smssync-backup-time"), BACKUP_TIME_MS.to_string());
        assert_eq!(get("Subject"), "SMS with Sam", "{name}");
        assert!(
            get("Message-ID").ends_with("@sms-backup-plus.local>"),
            "{name}"
        );
        assert!(
            get("References").ends_with("@sms-backup-plus.local>"),
            "{name}"
        );
        assert_eq!(get("MIME-Version"), "1.0", "{name}");
        get("From");
        get("To");
        get("Date");
        get("Content-Type");
        get("Content-Transfer-Encoding");
        let parsed = parse_mail(raw).unwrap();
        let body = parsed
            .parts()
            .find(|part| part.ctype.mimetype == "text/plain")
            .unwrap()
            .get_body()
            .unwrap();
        assert_eq!(body, text, "{name}");
        if datatype == "SMS" {
            assert_eq!(parsed.ctype.mimetype, "text/plain", "{name}");
        } else {
            assert_eq!(parsed.ctype.mimetype, "multipart/mixed", "{name}");
            let photo = &parsed.subparts[1];
            assert_eq!(photo.ctype.mimetype, "image/jpeg", "{name}");
            assert_eq!(
                photo.get_content_disposition().params.get("filename"),
                Some(&"photo.jpg".to_string())
            );
        }
    }
    let dates: Vec<String> = written
        .iter()
        .map(|(_, raw)| header(raw, "X-smssync-date").unwrap())
        .collect();
    assert_eq!(
        dates,
        [
            "1400773262123",
            "1400773263123",
            "1400773264123",
            "1400773265123"
        ],
        "epoch milliseconds"
    );
}

#[test]
fn no_mail_claims_a_value_the_database_does_not_keep() {
    let tmp = tempfile::tempdir().unwrap();
    archive()
        .write(
            tmp.path(),
            &[sms_and_mms_document()],
            &mut ExportReport::default(),
        )
        .unwrap();
    for (name, raw) in mails(tmp.path(), "+15555550101") {
        let parsed = parse_mail(&raw).unwrap();
        for absent in [
            "X-smssync-id",
            "X-smssync-thread",
            "X-smssync-read",
            "X-smssync-status",
            "X-smssync-protocol",
            "X-smssync-version",
            "X-GM-THRID",
            "X-Gmail-Labels",
        ] {
            assert!(
                parsed.headers.get_first_value(absent).is_none(),
                "{name} has {absent}"
            );
        }
        assert!(
            !parsed
                .headers
                .iter()
                .any(|h| h.get_key().to_ascii_lowercase().starts_with("x-me-")),
            "{name} carries Message Crate's own mail headers"
        );
    }
}

#[test]
fn a_group_lists_every_peer_and_names_the_sender() {
    let mut doc = sample_document("hello group");
    doc.conversation.chat_identifier = "chat-group-1".into();
    doc.conversation.conversation_type = IrConversationType::Group;
    doc.conversation.participants.push(IrParticipant {
        handle: Some("+15555550102".into()),
        display_name: Some("Bo".into()),
        handle_type: None,
    });
    doc.messages[0].sender_handle = Some("+15555550102".into());
    doc.messages[0].sender_display_name = Some("Bo".into());
    let tmp = tempfile::tempdir().unwrap();
    archive()
        .write(tmp.path(), &[doc.clone()], &mut ExportReport::default())
        .unwrap();

    let written = mails(tmp.path(), &doc.filename_stem());
    let raw = &written[0].1;
    assert_eq!(
        header(raw, "X-smssync-address").unwrap(),
        "+15555550101~+15555550102"
    );
    assert_eq!(header(raw, "Subject").unwrap(), "SMS with Sam, Bo");
    assert!(
        header(raw, "From").unwrap().contains("+15555550102@"),
        "{:?}",
        header(raw, "From")
    );
}

#[test]
fn only_sms_and_mms_are_written_and_the_rest_are_counted() {
    let sms = sample_document("an sms");
    let mut imessage = sample_imessage_document();
    imessage.conversation.chat_identifier = "+15555550102".into();
    let mut whatsapp = sample_document("a whatsapp message");
    whatsapp.conversation.chat_identifier = "+15555550103".into();
    whatsapp.messages[0].service = message_ir::IrService::Whatsapp;
    let tmp = tempfile::tempdir().unwrap();
    let mut report = ExportReport::default();

    archive()
        .write(tmp.path(), &[imessage, sms, whatsapp], &mut report)
        .unwrap();

    let folders: Vec<String> = fs::read_dir(tmp.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(folders, ["+15555550101"]);
    assert_eq!(report.extra(LEFT_OUT), 3);
    assert_eq!(
        left_out_line(&report).as_deref(),
        Some(
            "Left out 3 message(s) that are not SMS or MMS, because SMS Backup+ \
             holds only SMS and MMS"
        )
    );
    assert_eq!(left_out_line(&ExportReport::default()), None);
}
