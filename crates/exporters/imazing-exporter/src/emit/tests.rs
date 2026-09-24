use super::*;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

fn write(dir: &tempfile::TempDir, name: &str, body: &str) -> PathBuf {
    let path = dir.path().join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let mut f = File::create(&path).unwrap();
    write!(f, "{body}").unwrap();
    path
}

fn convert(input: &std::path::Path, output: &std::path::Path) -> Result<ExportReport> {
    convert_export(ConvertExportArgs {
        input,
        output,
        timezone: Some("UTC"),
        transforms: ExportTransforms::none(),
        output_format: OutputFormat::Csv,
        cancel: None,
        resume: false,
    })
}

fn pending_att(rel_path: &str, digest: Option<&str>) -> PendingAttachment {
    PendingAttachment {
        rel_path: rel_path.into(),
        content_type: String::new(),
        extension: "jpg".into(),
        digest_sha256: digest.map(str::to_string),
        name_hint: None,
    }
}

#[test]
fn message_guid_prefers_digest_over_rel_path() {
    // Same digest, different relative paths → same GUID material.
    let a = pending_att("attachments/old_name.jpg", Some("abc123"));
    let b = pending_att("attachments/new_name.jpg", Some("abc123"));
    assert_eq!(
        attachment_guid_materials(&[a]),
        attachment_guid_materials(&[b])
    );

    // Digest present wins over path; path alone differs from digest.
    let with_digest = pending_att("attachments/x.jpg", Some("deadbeef"));
    let path_only = pending_att("attachments/x.jpg", None);
    assert_ne!(
        attachment_guid_materials(&[with_digest]),
        attachment_guid_materials(&[path_only])
    );

    // Order of attachments must not change the sorted material list.
    let mixed = [
        pending_att("a.jpg", Some("bb")),
        pending_att("b.jpg", Some("aa")),
    ];
    assert_eq!(
        attachment_guid_materials(&mixed),
        vec!["aa".to_string(), "bb".to_string()]
    );
}

#[test]
fn name_session_uses_the_number_the_rows_carry() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir,
        "Messages - Bob.csv",
        "Chat Session,Message Date,Service,Type,Sender ID,Sender Name,Status,Replying to,Subject,Text,Reactions,Attachment,Attachment type\n\
Bob McRoy,2020-01-01 12:00:00,SMS,Incoming,+13212462167,Bob McRoy,Read,,,Hello,,,\n\
Bob McRoy,2020-01-01 12:01:00,SMS,Outgoing,,,Read,,,Hi,,,\n",
    );
    let out = dir.path().join("out");
    let report = convert(dir.path(), &out).unwrap();
    assert_eq!(report.conversations, 1);
    assert_eq!(report.extra("name_only_chat"), 0);
    assert_eq!(report.messages, 2);
    let csv_path = out.join("+13212462167.csv");
    let body = fs::read_to_string(&csv_path).unwrap();
    assert!(body.contains("Bob McRoy"));
    assert!(body.contains("imazing"));
    assert!(body.contains("iMazing"));
    assert!(body.contains("imazing_type"));
}

#[test]
fn name_without_any_address_becomes_a_name_only_chat() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir,
        "Messages - Mystery.csv",
        "Chat Session,Message Date,Service,Type,Sender ID,Sender Name,Status,Replying to,Subject,Text,Reactions,Attachment,Attachment type\n\
Mystery Person,2020-01-01 12:00:00,SMS,Incoming,,,Read,,,Hello,,,\n\
Mystery Person,2020-01-01 12:01:00,SMS,Outgoing,,,Read,,,Hi,,,\n",
    );
    let out = dir.path().join("out");
    let report = convert(dir.path(), &out).unwrap();
    assert!(report.extra("name_only_chat") >= 1);
    assert_eq!(report.conversations, 1);
    assert!(out.join("Mystery_Person.csv").is_file());
    let body = fs::read_to_string(out.join("Mystery_Person.csv")).unwrap();
    assert!(
        body.contains("Mystery Person"),
        "the name the source gave must survive: {body}"
    );
}

#[test]
fn drops_exact_duplicate_rows() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir,
        "Messages.csv",
        "Chat Session,Message Date,Service,Type,Sender ID,Sender Name,Status,Replying to,Subject,Text,Reactions,Attachment,Attachment type\n\
Bob,2020-01-01 12:00:00,SMS,Outgoing,,,Read,,,Same,,,\n\
Bob,2020-01-01 12:00:00,SMS,Outgoing,,,Read,,,Same,,,\n",
    );
    let out = dir.path().join("out");
    let report = convert(dir.path(), &out).unwrap();
    assert_eq!(report.messages, 1);
    assert_eq!(report.duplicates_dropped, 1);
}

#[test]
fn keeps_same_text_different_attachment() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir,
        "Messages.csv",
        "Chat Session,Message Date,Service,Type,Sender ID,Sender Name,Status,Replying to,Subject,Text,Reactions,Attachment,Attachment type\n\
Bob,2020-01-01 12:00:00,SMS,Incoming,+15555550100,Bob,Read,,,Photo,,a.jpg,Image\n\
Bob,2020-01-01 12:00:00,SMS,Incoming,+15555550100,Bob,Read,,,Photo,,b.jpg,Image\n",
    );
    let out = dir.path().join("out");
    let report = convert(dir.path(), &out).unwrap();
    assert_eq!(report.messages, 2);
    assert_eq!(report.duplicates_dropped, 0);
}

#[test]
fn silent_group_member_named_without_an_address_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir,
        "Messages.csv",
        "Chat Session,Message Date,Service,Type,Sender ID,Sender Name,Status,Replying to,Subject,Text,Reactions,Attachment,Attachment type\n\
Alice Example & Bob Example & Carol Silent,2020-01-01 12:00:00,iMessage,Incoming,+15555550111,Alice Example,Read,,,Hi,,,\n\
Alice Example & Bob Example & Carol Silent,2020-01-01 12:01:00,iMessage,Incoming,+15555550122,Bob Example,Read,,,Hey,,,\n",
    );
    let out = dir.path().join("out");
    let report = convert(dir.path(), &out).unwrap();
    assert_eq!(report.conversations, 1);
    // Carol Silent sent nothing, so the source recorded no address for
    // her. The exporter reports her rather than inventing one.
    assert_eq!(report.extra("unresolved_group_participants"), 1);
    let body = fs::read_to_string(out.join("group_+15555550111_+15555550122.csv")).unwrap();
    assert!(body.contains("group"));
}

#[test]
fn silent_group_member_without_contacts_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir,
        "Messages.csv",
        "Chat Session,Message Date,Service,Type,Sender ID,Sender Name,Status,Replying to,Subject,Text,Reactions,Attachment,Attachment type\n\
Alice Example & Bob Example & Carol Silent,2020-01-01 12:00:00,iMessage,Incoming,+15555550111,Alice Example,Read,,,Hi,,,\n\
Alice Example & Bob Example & Carol Silent,2020-01-01 12:01:00,iMessage,Incoming,+15555550122,Bob Example,Read,,,Hey,,,\n",
    );
    let out = dir.path().join("out");
    let report = convert(dir.path(), &out).unwrap();
    assert_eq!(report.conversations, 1);
    assert_eq!(report.extra("unresolved_group_participants"), 1);
}

#[test]
fn whatsapp_and_messages_same_peer_stay_separate() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir,
        "Messages/chat/Messages - Bob.csv",
        "Chat Session,Message Date,Delivered Date,Read Date,Edited Date,Deleted Date,Service,Type,Sender ID,Sender Name,Status,Replying to,Subject,Text,Reactions,Attachment,Attachment type\n\
Bob,2020-01-01 12:00:00,,,,,SMS,Incoming,+15555550100,Bob,Read,,,SMS hi,,,\n",
    );
    write(
        &dir,
        "WhatsApp/chat/WhatsApp - Bob.csv",
        "Chat Session,Message Date,Sent Date,Type,Sender ID,Sender Name,Status,Forwarded,Replying to,Text,Reactions,Attachment,Attachment type,Attachment info\n\
Bob,2020-01-01 12:05:00,,Incoming,+15555550100,Bob,Read,,,WA hi,,,,\n",
    );
    let out = dir.path().join("out");
    let report = convert(dir.path(), &out).unwrap();
    assert_eq!(report.conversations, 2);
    assert_eq!(report.extra("messages_files"), 1);
    assert_eq!(report.extra("whatsapp_files"), 1);
    assert!(out.join("+15555550100.csv").is_file());
    assert!(out.join("+15555550100__whatsapp.csv").is_file());
    let wa = fs::read_to_string(out.join("+15555550100__whatsapp.csv")).unwrap();
    assert!(wa.contains("whatsapp"));
}

#[test]
fn rejects_unknown_timezone() {
    let err = Zone::parse(Some("Not/AZone")).unwrap_err();
    assert!(err.to_string().contains("Not/AZone"), "{err}");
}

/// The Unix seconds `parse_message_date` gives `raw` in the zone named `tz`.
fn secs_in(raw: &str, tz: &str) -> i64 {
    let zone = Zone::parse(Some(tz)).unwrap();
    let (secs, date_ms) = parse_message_date(raw, zone).expect("date parses");
    assert_eq!(date_ms, (secs * 1000).to_string());
    secs
}

#[test]
fn fixed_offset_gives_the_exact_instant() {
    // 12:00 at UTC-05:00 is 17:00 UTC, whatever zone the machine is in.
    assert_eq!(secs_in("2020-01-01 12:00:00", "UTC-05:00"), 1_577_898_000);
    assert_eq!(secs_in("2020-01-01 12:00", "UTC+05:30"), 1_577_860_200);
    assert_eq!(secs_in("2020-01-01 12:00:00", "UTC"), 1_577_880_000);
}

#[test]
fn spring_forward_gap_keeps_the_message() {
    // New York skipped 02:00-03:00 on 2026-03-08. A wall clock inside the gap
    // is read with the offset in force before the change (-05:00), so 02:30
    // becomes 07:30 UTC, the same instant as 03:30 EDT.
    assert_eq!(
        secs_in("2026-03-08 02:30:00", "America/New_York"),
        1_772_955_000
    );
}

#[test]
fn fall_back_ambiguity_takes_the_earlier_instant() {
    // 01:30 happened twice in New York on 2025-11-02: first at 05:30 UTC
    // (EDT), then at 06:30 UTC (EST). The earlier one is chosen.
    assert_eq!(
        secs_in("2025-11-02 01:30:00", "America/New_York"),
        1_762_061_400
    );
}

#[test]
fn named_zone_outside_a_transition_is_plain_local_time() {
    // 12:00 EDT on 2020-07-01 is 16:00 UTC.
    assert_eq!(
        secs_in("2020-07-01 12:00:00", "America/New_York"),
        1_593_619_200
    );
}

#[test]
fn gap_row_is_exported_with_its_instant() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir,
        "Messages - Bob.csv",
        "Chat Session,Message Date,Service,Type,Sender ID,Sender Name,Status,Replying to,Subject,Text,Reactions,Attachment,Attachment type\n\
Bob,2026-03-08 02:30:00,SMS,Incoming,+15555550100,Bob,Read,,,Gap,,,\n",
    );
    let out = dir.path().join("out");
    let report = convert_export(ConvertExportArgs {
        input: dir.path(),
        output: &out,
        timezone: Some("America/New_York"),
        transforms: ExportTransforms::none(),
        output_format: OutputFormat::Jsonl,
        cancel: None,
        resume: false,
    })
    .unwrap();
    assert_eq!(report.messages, 1);
    assert_eq!(report.skipped_invalid_date, 0);
    let body = fs::read_to_string(out.join("+15555550100.jsonl")).unwrap();
    assert!(
        body.contains("\"timestamp_unix_ms\":1772955000000"),
        "{body}"
    );
}

#[test]
fn copies_attachment_by_suffix_match() {
    let dir = tempfile::tempdir().unwrap();
    let chat = dir.path().join("chat");
    fs::create_dir_all(&chat).unwrap();
    let csv = chat.join("Messages - Bob.csv");
    fs::write(
        &csv,
        "Chat Session,Message Date,Delivered Date,Read Date,Edited Date,Deleted Date,Service,Type,Sender ID,Sender Name,Status,Replying to,Subject,Text,Reactions,Attachment,Attachment type\n\
Bob McRoy,2020-01-01 12:00:00,,,,,SMS,Incoming,+15555550100,Bob,Read,,,Hi,,image000000.jpg,Image\n",
    )
    .unwrap();
    fs::write(chat.join("ABC123_image000000.jpg"), b"fake-jpeg-bytes").unwrap();
    let out = dir.path().join("out");
    let report = convert(&chat, &out).unwrap();
    assert_eq!(report.attachments_saved, 1);
    assert_eq!(report.messages, 1);
    let att_dir = out.join("attachments");
    assert!(att_dir.is_dir());
    let count = fs::read_dir(&att_dir).unwrap().count();
    assert_eq!(count, 1);
    let body = fs::read_to_string(out.join("+15555550100.csv")).unwrap();
    assert!(body.contains("attachments/"));
}

#[test]
fn email_sender_with_digits_stays_email() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir,
        "Messages - Bob.csv",
        "Chat Session,Message Date,Service,Type,Sender ID,Sender Name,Status,Replying to,Subject,Text,Reactions,Attachment,Attachment type\n\
Bob McRoy,2020-01-01 12:00:00,iMessage,Incoming,bob2024@gmail.com,Bob McRoy,Read,,,Hello,,,\n\
Bob McRoy,2020-01-01 12:01:00,iMessage,Outgoing,,,Read,,,Hi,,,\n",
    );
    let out = dir.path().join("out");
    let report = convert(dir.path(), &out).unwrap();
    assert_eq!(report.conversations, 1);
    assert_eq!(report.messages, 2);
    // Chat id stays the full email; the CSV filename stems `@` to `_`.
    let csv_path = out.join("bob2024_gmail_com.csv");
    assert!(
        csv_path.is_file(),
        "expected email chat file; got {}",
        out.read_dir()
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(", ")
    );
    let body = fs::read_to_string(csv_path).unwrap();
    assert!(body.contains("bob2024@gmail.com"));
    assert!(!body.contains("12024"));
}

#[test]
fn same_text_same_second_different_senders_kept() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir,
        "Messages.csv",
        "Chat Session,Message Date,Service,Type,Sender ID,Sender Name,Status,Replying to,Subject,Text,Reactions,Attachment,Attachment type\n\
Group Chat,2020-01-01 12:00:00,iMessage,Incoming,+15555550111,Alice,Read,,,Same,,,\n\
Group Chat,2020-01-01 12:00:00,iMessage,Incoming,+15555550122,Bob,Read,,,Same,,,\n",
    );
    let out = dir.path().join("out");
    let report = convert(dir.path(), &out).unwrap();
    assert_eq!(report.messages, 2);
    assert_eq!(report.duplicates_dropped, 0);
}

#[test]
fn output_equals_input_bails_before_cleaning() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir,
        "Messages - Bob.csv",
        "Chat Session,Message Date,Service,Type,Sender ID,Sender Name,Status,Replying to,Subject,Text,Reactions,Attachment,Attachment type\n\
Bob,2020-01-01 12:00:00,SMS,Incoming,+13212462167,Bob,Read,,,Hello,,,\n",
    );
    let err = convert(dir.path(), dir.path()).unwrap_err();
    assert!(err.to_string().contains("must not be the same as"), "{err}");
    // Source CSV must survive the refused run.
    assert!(dir.path().join("Messages - Bob.csv").is_file());
}

/// The Messages CSV header every test below writes rows under.
const MESSAGES_HEADER: &str = "Chat Session,Message Date,Service,Type,Sender ID,Sender Name,Status,Replying to,Subject,Text,Reactions,Attachment,Attachment type\n";

/// Convert one Messages CSV holding `rows` to JSON and read each
/// conversation back.
fn convert_rows(rows: &str) -> Vec<message_ir::ConversationDocument> {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in");
    fs::create_dir(&input).unwrap();
    fs::write(
        input.join("Messages.csv"),
        format!("{MESSAGES_HEADER}{rows}"),
    )
    .unwrap();
    let out = dir.path().join("out");
    convert_export(ConvertExportArgs {
        input: &input,
        output: &out,
        timezone: Some("UTC"),
        transforms: ExportTransforms::none(),
        output_format: OutputFormat::Json,
        cancel: None,
        resume: false,
    })
    .unwrap();
    let mut documents: Vec<_> = fs::read_dir(&out)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .map(|path| message_ir_format::read_conversation_json(&path).unwrap())
        .collect();
    documents.sort_by(|a, b| {
        a.conversation
            .chat_identifier
            .cmp(&b.conversation.chat_identifier)
    });
    documents
}

/// iMazing names a chat "Bob (+13212462167)" when it knows the number. A
/// chat whose rows are all outgoing carries no sender id, so the number in
/// the name is the only address the source gives, and it is the roster.
/// The number is read as written, so a digit elsewhere in the name
/// ("Bob 2") doesn't join it.
#[test]
fn a_number_in_the_chat_name_is_the_roster() {
    for session in ["Bob (+13212462167)", "Bob 2 (+13212462167)"] {
        let documents = convert_rows(&format!(
            "{session},2020-01-01 12:00:00,SMS,Outgoing,,,Sent,,,Hi Bob,,,\n"
        ));
        assert_eq!(documents.len(), 1, "{session}");
        let doc = &documents[0];
        assert_eq!(
            doc.conversation.chat_identifier, "+13212462167",
            "{session}"
        );
        let roster: Vec<_> = doc
            .conversation
            .participants
            .iter()
            .filter_map(|p| p.handle.as_deref())
            .collect();
        assert_eq!(roster, vec!["+13212462167"], "{session}");
    }
}

/// A Subject column value is the message's subject; an empty one is none.
#[test]
fn a_subject_reaches_the_message() {
    let documents = convert_rows(
        "Bob,2020-01-01 12:00:00,SMS,Incoming,+13212462167,Bob,Read,,Dinner,See you at 7,,,\n\
Bob,2020-01-01 12:01:00,SMS,Incoming,+13212462167,Bob,Read,,,No subject,,,\n",
    );
    let subjects: Vec<_> = documents[0]
        .messages
        .iter()
        .map(|m| m.subject.as_deref())
        .collect();
    assert_eq!(subjects, vec![Some("Dinner"), None]);
}

/// Two photos sent in the same second with no text are two messages, told
/// apart by their attachment, so each keeps its own GUID.
#[test]
fn two_same_second_photos_are_two_messages() {
    let documents = convert_rows(
        "Bob,2020-01-01 12:00:00,iMessage,Incoming,+13212462167,Bob,Read,,,,,IMG_0001.jpg,Image\n\
Bob,2020-01-01 12:00:00,iMessage,Incoming,+13212462167,Bob,Read,,,,,IMG_0002.jpg,Image\n",
    );
    let messages = &documents[0].messages;
    assert_eq!(messages.len(), 2);
    assert_ne!(messages[0].guid, messages[1].guid);
}

/// A notification row ("Bob left the conversation") is not a message from
/// the chat's peer, so it takes no sender from the chat.
#[test]
fn a_notification_row_has_no_sender() {
    let documents = convert_rows(
        "Bob,2020-01-01 12:00:00,SMS,Incoming,+13212462167,Bob,Read,,,Hello,,,\n\
Bob,2020-01-01 12:01:00,SMS,Notification,,,,,,Bob left the conversation,,,\n",
    );
    let notification = documents[0]
        .messages
        .iter()
        .find(|m| m.text == "Bob left the conversation")
        .unwrap();
    assert_eq!(notification.sender_handle, None);
    assert_eq!(notification.sender_display_name, None);
}

/// An incoming message whose row names no sender is from the chat's peer:
/// the chat's number, and the chat's name when the row gives none. An
/// email chat's address is never read as a number.
#[test]
fn an_incoming_row_without_a_sender_is_from_the_chats_peer() {
    let documents = convert_rows(
        "Bob McRoy,2020-01-01 12:00:00,SMS,Incoming,+13212462167,Robert,Read,,,Hello,,,\n\
Bob McRoy,2020-01-01 12:01:00,SMS,Incoming,,,Read,,,Anyone there,,,\n\
Bob Mail,2020-01-01 12:00:00,iMessage,Incoming,bob2024@gmail.com,Bob,Read,,,Hi,,,\n\
Bob Mail,2020-01-01 12:01:00,iMessage,Incoming,,,Read,,,Still me,,,\n",
    );
    let senders: Vec<(&str, Option<&str>, Option<&str>)> = documents
        .iter()
        .flat_map(|doc| &doc.messages)
        .map(|m| {
            (
                m.text.as_str(),
                m.sender_handle.as_deref(),
                m.sender_display_name.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        senders,
        vec![
            ("Hello", Some("+13212462167"), Some("Robert")),
            ("Anyone there", Some("+13212462167"), Some("Bob McRoy")),
            ("Hi", Some("bob2024@gmail.com"), Some("Bob")),
            ("Still me", None, Some("Bob Mail")),
        ]
    );
}
