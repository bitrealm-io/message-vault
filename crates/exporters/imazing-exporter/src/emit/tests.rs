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
        digest_sha256: digest.map(str::to_string),
        name_hint: None,
    }
}

#[test]
fn message_guid_prefers_digest_over_rel_path() {
    // Same digest, different relative paths → same GUID material.
    let a = pending_att("attachments/old_name.jpg", Some("abc123"));
    let b = pending_att("attachments/new_name.jpg", Some("abc123"));
    assert_eq!(attachment_digests(&[a]), attachment_digests(&[b]));

    // Digest present wins over path; path alone differs from digest.
    let with_digest = pending_att("attachments/x.jpg", Some("deadbeef"));
    let path_only = pending_att("attachments/x.jpg", None);
    assert_ne!(
        attachment_digests(&[with_digest]),
        attachment_digests(&[path_only])
    );

    // Order of attachments must not change the sorted material list.
    let mixed = [
        pending_att("a.jpg", Some("bb")),
        pending_att("b.jpg", Some("aa")),
    ];
    assert_eq!(
        attachment_digests(&mixed),
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
    parse_message_date(raw, zone).expect("date parses")
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

/// The handles on a document's roster, in the order the document lists them.
fn roster(doc: &message_ir::ConversationDocument) -> Vec<Option<&str>> {
    doc.conversation
        .participants
        .iter()
        .map(|p| p.handle.as_deref())
        .collect()
}

/// A group's members are data on the conversation and its chat id is its own
/// key, so the chat id never joins the roster as a handle of its own.
#[test]
fn a_groups_participants_are_its_members_and_not_its_chat_id() {
    let documents = convert_rows(
        "Group Chat,2020-01-01 12:00:00,iMessage,Incoming,+15555550111,Alice,Read,,,Hi,,,\n\
Group Chat,2020-01-01 12:01:00,iMessage,Incoming,+15555550122,Bob,Read,,,Hey,,,\n",
    );
    assert_eq!(documents.len(), 1);
    assert_eq!(
        roster(&documents[0]),
        vec![Some("+15555550111"), Some("+15555550122")]
    );
}

/// iMazing names a chat with an unsaved contact by the number alone. The
/// number is the chat, whether it is the whole name or the end of it.
#[test]
fn a_chat_named_by_a_bare_number_is_that_numbers_chat() {
    for session in ["+13212462167", "Bob +13212462167"] {
        let documents = convert_rows(&format!(
            "{session},2020-01-01 12:00:00,SMS,Outgoing,,,Sent,,,Hi,,,\n"
        ));
        assert_eq!(documents.len(), 1, "{session}");
        assert_eq!(
            documents[0].conversation.chat_identifier, "+13212462167",
            "{session}"
        );
        assert_eq!(
            roster(&documents[0]),
            vec![Some("+13212462167")],
            "{session}"
        );
    }
}

/// A Messages chat named "A & B" is a group even when only one member ever
/// sent a message, and so is a chat in which two people with their own names
/// and addresses wrote under a name with no " & " in it.
#[test]
fn a_roster_name_or_two_people_make_a_group() {
    for rows in [
        "Alice Example & Bob Example,2020-01-01 12:00:00,iMessage,Incoming,+15555550111,Alice Example,Read,,,Hi,,,\n",
        "Book Club,2020-01-01 12:00:00,iMessage,Incoming,+15555550111,Alice,Read,,,Hi,,,\n\
Book Club,2020-01-01 12:01:00,iMessage,Incoming,+15555550122,Bob,Read,,,Hey,,,\n",
    ] {
        let documents = convert_rows(rows);
        assert_eq!(documents.len(), 1, "{rows}");
        assert_eq!(
            documents[0].conversation.conversation_type,
            message_ir::IrConversationType::Group,
            "{rows}"
        );
    }
}

/// A short code has too few digits for a "+" form, and it is still the
/// chat's address: a received row that names no sender is from it.
#[test]
fn an_incoming_row_without_a_sender_in_a_short_code_chat_is_from_the_short_code() {
    let documents = convert_rows("262966,2020-01-01 12:00:00,SMS,Incoming,,,Read,,,Your code,,,\n");
    assert_eq!(documents[0].conversation.chat_identifier, "262966");
    assert_eq!(
        documents[0].messages[0].sender_handle.as_deref(),
        Some("262966")
    );
}

/// The "Attachment type" column says what a file is when its name does not:
/// a sticker is flagged as one, and an image with no extension is an image.
#[test]
fn the_attachment_type_column_reaches_the_attachment() {
    let documents = convert_rows(
        "Bob,2020-01-01 12:00:00,iMessage,Incoming,+13212462167,Bob,Read,,,,,sticker_0001,Sticker\n\
Bob,2020-01-01 12:01:00,iMessage,Incoming,+13212462167,Bob,Read,,,,,IMG_0001,Image\n",
    );
    let attachments: Vec<_> = documents[0]
        .messages
        .iter()
        .map(|m| {
            let attachment = &m.attachments[0];
            (
                attachment.original_name.as_deref(),
                attachment.is_sticker,
                attachment.mime_type.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        attachments,
        vec![
            (Some("sticker_0001"), true, Some("image/webp")),
            (Some("IMG_0001"), false, Some("image/jpeg")),
        ]
    );
}

/// Two chat folders can each hold a file of the same name. A chat's
/// attachment is the file beside its own CSV, not the first one of that
/// name anywhere in the export.
#[test]
fn a_same_named_file_in_two_chat_folders_goes_to_its_own_chat() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in");
    for (folder, number, bytes) in [
        ("alice", "+15555550111", "alice-photo"),
        ("bob", "+15555550122", "bob-photo"),
    ] {
        let chat = input.join(folder);
        fs::create_dir_all(&chat).unwrap();
        fs::write(
            chat.join("Messages.csv"),
            format!(
                "{MESSAGES_HEADER}{folder},2020-01-01 12:00:00,iMessage,Incoming,{number},{folder},Read,,,,,IMG_0001.jpg,Image\n"
            ),
        )
        .unwrap();
        fs::write(chat.join("IMG_0001.jpg"), bytes).unwrap();
    }
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
    for (number, bytes) in [
        ("+15555550111", "alice-photo"),
        ("+15555550122", "bob-photo"),
    ] {
        let doc =
            message_ir_format::read_conversation_json(&out.join(format!("{number}.json"))).unwrap();
        let path = doc.messages[0].attachments[0]
            .path
            .as_deref()
            .expect("the attachment was copied");
        assert_eq!(
            fs::read_to_string(out.join(path)).unwrap(),
            bytes,
            "{number}"
        );
    }
}

/// A Messages chat named "A & B" whose rows carry no address is a group of
/// people the source named and recorded no address for. Each is a
/// participant with the name and no handle, which the server gives an
/// identity of type `other`. The chat id is the group's own key, never made
/// from the names.
#[test]
fn a_group_known_only_by_names_lists_each_named_person_without_a_handle() {
    let documents = convert_rows(
        "Alice Example & Bob Example,2020-01-01 12:00:00,iMessage,Incoming,,Alice Example,Read,,,Hi,,,\n\
Alice Example & Bob Example,2020-01-01 12:01:00,iMessage,Outgoing,,,Sent,,,Hey,,,\n",
    );
    assert_eq!(documents.len(), 1);
    let conversation = &documents[0].conversation;
    assert_eq!(
        conversation.conversation_type,
        message_ir::IrConversationType::Group
    );
    assert!(
        conversation
            .chat_identifier
            .starts_with(message_ir::GROUP_CHAT_ID_PREFIX),
        "{}",
        conversation.chat_identifier
    );
    let participants: Vec<_> = conversation
        .participants
        .iter()
        .map(|p| {
            (
                p.handle.as_deref(),
                p.display_name.as_deref(),
                p.handle_type,
            )
        })
        .collect();
    assert_eq!(
        participants,
        vec![
            (None, Some("Alice Example"), None),
            (None, Some("Bob Example"), None),
        ]
    );
}

/// What one converted chat folder gave: the report, its one conversation,
/// and the output folder (kept alive by the temporary directory).
struct ChatFolderExport {
    report: ExportReport,
    doc: message_ir::ConversationDocument,
    out: PathBuf,
    _dir: tempfile::TempDir,
}

impl ChatFolderExport {
    /// The contents of each copied attachment of the message with `text`, in order.
    fn attachment_bodies(&self, text: &str) -> Vec<String> {
        let message = self
            .doc
            .messages
            .iter()
            .find(|m| m.text == text)
            .unwrap_or_else(|| panic!("no message {text:?}"));
        message
            .attachments
            .iter()
            .map(|a| {
                let path = a.path.as_deref().expect("the attachment was copied");
                fs::read_to_string(self.out.join(path)).unwrap()
            })
            .collect()
    }
}

/// Convert one chat folder holding `Messages.csv` with `rows` and the named
/// `files` to JSON.
fn convert_chat_folder(rows: &str, files: &[(&str, &str)]) -> ChatFolderExport {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in");
    let chat = input.join("2020-01-01 12 00 00 - Bob");
    fs::create_dir_all(&chat).unwrap();
    fs::write(
        chat.join("Messages.csv"),
        format!("{MESSAGES_HEADER}{rows}"),
    )
    .unwrap();
    for (name, body) in files {
        fs::write(chat.join(name), body).unwrap();
    }
    let out = dir.path().join("out");
    let report = convert_export(ConvertExportArgs {
        input: &input,
        output: &out,
        timezone: Some("UTC"),
        transforms: ExportTransforms::none(),
        output_format: OutputFormat::Json,
        cancel: None,
        resume: false,
    })
    .unwrap();
    let doc = message_ir_format::read_conversation_json(&out.join("+15555550100.json")).unwrap();
    ChatFolderExport {
        report,
        doc,
        out,
        _dir: dir,
    }
}

/// iMazing writes a Live Photo's video beside its picture, under the
/// picture's name with `.mov`, and no row names it. It is the second
/// attachment of the picture's message. A link preview (`.url`) holds only
/// an address its message already shows, so it is counted and not imported.
/// Any other file no row names is counted.
#[test]
fn a_live_photo_video_joins_its_picture_and_a_link_preview_is_counted() {
    let export = convert_chat_folder(
        "Bob,2020-01-01 12:00:00,iMessage,Incoming,+15555550100,Bob,Read,,,photo,,IMG_0001.jpg,Image\n\
Bob,2020-01-01 12:01:00,iMessage,Incoming,+15555550100,Bob,Read,,,See https://example.com/page,,,\n",
        &[
            ("2020-01-01 12 00 00 - Bob - IMG_0001.jpg", "picture"),
            ("2020-01-01 12 00 00 - Bob - IMG_0001.mov", "video"),
            (
                "2020-01-01 12 01 00 - Bob - Web link.url",
                "[InternetShortcut]\r\nURL=https://example.com/page\r\n",
            ),
            ("2020-01-01 12 02 00 - Bob - stray.bin", "stray"),
        ],
    );
    assert_eq!(export.doc.messages.len(), 2);
    assert_eq!(export.attachment_bodies("photo"), vec!["picture", "video"]);
    assert_eq!(
        export.doc.messages[0].attachments[1]
            .original_name
            .as_deref(),
        Some("IMG_0001.mov")
    );
    assert!(
        export
            .attachment_bodies("See https://example.com/page")
            .is_empty()
    );
    let report = &export.report;
    assert_eq!(report.attachments_saved, 2);
    assert_eq!(report.extra("live_photo_videos"), 1);
    assert_eq!(report.extra("link_previews_already_in_message"), 1);
    assert_eq!(report.extra("files_named_by_no_row"), 1);
}

/// A link preview whose address no message of its second shows holds
/// something the messages do not, so it is counted with the other files no
/// row names rather than as a link preview.
#[test]
fn a_link_preview_whose_address_no_message_shows_is_counted_as_unnamed() {
    let export = convert_chat_folder(
        "Bob,2020-01-01 12:01:00,iMessage,Incoming,+15555550100,Bob,Read,,,See https://example.com/other,,,\n\
Bob,2020-01-01 12:02:00,iMessage,Incoming,+15555550100,Bob,Read,,,See https://example.com/page,,,\n",
        &[(
            "2020-01-01 12 01 00 - Bob - Web link.url",
            "[InternetShortcut]\nURL=https://example.com/page\n",
        )],
    );
    assert_eq!(export.report.extra("link_previews_already_in_message"), 0);
    assert_eq!(export.report.extra("files_named_by_no_row"), 1);
}

/// When two rows name one picture, its Live Photo video goes to the first of
/// them in CSV order, and the report names the picture.
#[test]
fn a_live_photo_video_of_a_picture_two_rows_name_goes_to_the_first_row() {
    let export = convert_chat_folder(
        "Bob,2020-01-01 12:05:00,iMessage,Incoming,+15555550100,Bob,Read,,,first,,IMG_0002.jpg,Image\n\
Bob,2020-01-01 12:00:00,iMessage,Incoming,+15555550100,Bob,Read,,,second,,IMG_0002.jpg,Image\n",
        &[
            ("2020-01-01 12 05 00 - Bob - IMG_0002.jpg", "picture"),
            ("2020-01-01 12 05 00 - Bob - IMG_0002.MOV", "video"),
        ],
    );
    assert_eq!(export.attachment_bodies("first"), vec!["picture", "video"]);
    assert_eq!(export.attachment_bodies("second"), vec!["picture"]);
    assert_eq!(export.report.extra("live_photo_videos"), 1);
    assert!(
        export
            .report
            .errors
            .iter()
            .any(|e| e.contains("2020-01-01 12 05 00 - Bob - IMG_0002.jpg")),
        "{:?}",
        export.report.errors
    );
}

/// #1080: a group's key is not built from who wrote, so a group in which one
/// other person wrote is not that person's one-to-one conversation.
#[test]
fn a_group_where_one_person_wrote_is_not_their_direct_chat() {
    let documents = convert_rows(
        "Alice Example & Bob Example,2020-01-01 12:00:00,iMessage,Incoming,+15555550111,Alice Example,Read,,,In the group,,,\n\
Alice Example,2020-01-01 12:01:00,iMessage,Incoming,+15555550111,Alice Example,Read,,,Just us,,,\n",
    );
    assert_eq!(
        documents.len(),
        2,
        "the group merged into Alice's direct chat"
    );
}

/// #1080: a group keeps its chat id, and so every message keeps its `guid`,
/// when someone new writes in a later export.
#[test]
fn a_group_keeps_its_chat_id_when_someone_new_writes() {
    let before = convert_rows(
        "Book Club,2020-01-01 12:00:00,iMessage,Incoming,+15555550111,Alice,Read,,,Hi,,,\n\
Book Club,2020-01-01 12:01:00,iMessage,Incoming,+15555550122,Bob,Read,,,Hey,,,\n",
    );
    let after = convert_rows(
        "Book Club,2020-01-01 12:00:00,iMessage,Incoming,+15555550111,Alice,Read,,,Hi,,,\n\
Book Club,2020-01-01 12:01:00,iMessage,Incoming,+15555550122,Bob,Read,,,Hey,,,\n\
Book Club,2020-02-01 12:00:00,iMessage,Incoming,+15555550133,Carol,Read,,,Hello,,,\n",
    );
    assert_eq!(
        before[0].conversation.chat_identifier,
        after[0].conversation.chat_identifier
    );
    assert_eq!(before[0].messages[0].guid, after[0].messages[0].guid);
}

/// #1080: a group message's sender comes only from its row. A received row
/// with no Sender ID has no sender, and none is made up from the chat id.
#[test]
fn an_incoming_group_row_without_a_sender_has_no_made_up_sender() {
    for session in ["Book Club", "Trip 2024"] {
        let documents = convert_rows(&format!(
            "{session},2020-01-01 12:00:00,iMessage,Incoming,+15555550111,Alice,Read,,,Hi,,,\n\
{session},2020-01-01 12:01:00,iMessage,Incoming,+15555550122,Bob,Read,,,Hey,,,\n\
{session},2020-01-01 12:02:00,iMessage,Incoming,,,Read,,,Who,,,\n"
        ));
        let who = documents[0]
            .messages
            .iter()
            .find(|m| m.text == "Who")
            .unwrap();
        assert_eq!(who.sender_handle, None, "{session}");
        assert_eq!(who.sender_display_name, None, "{session}");
    }
}

/// #1080: one person writing from a number and an email address is one
/// person, so the conversation is one-to-one and not a group. The same holds
/// for one contact's two numbers in one chat. The chat's address is the
/// smallest of them, whichever wrote first.
#[test]
fn one_person_from_two_addresses_is_not_a_group() {
    for second in ["bob@icloud.com", "+15555550199"] {
        let documents = convert_rows(&format!(
            "Bob,2020-01-01 12:00:00,iMessage,Incoming,+15555550122,Bob,Read,,,From one,,,\n\
Bob,2020-01-01 12:01:00,iMessage,Incoming,{second},Bob,Read,,,From the other,,,\n"
        ));
        assert_eq!(documents.len(), 1, "{second}");
        let conversation = &documents[0].conversation;
        assert_eq!(
            conversation.conversation_type,
            message_ir::IrConversationType::Individual,
            "{second}"
        );
        assert_eq!(conversation.chat_identifier, "+15555550122", "{second}");
    }
}

/// A Messages group's session name lists every member. A member who never
/// wrote, shown by name, has no address anywhere in the export, so they are
/// a member with the name and no handle; the server gives them an identity
/// of type `other`.
#[test]
fn a_member_who_never_wrote_is_a_member_without_a_handle() {
    let documents = convert_rows(
        "Alice Example & Bob Example & Carol Silent,2020-01-01 12:00:00,iMessage,Incoming,+15555550111,Alice Example,Read,,,Hi,,,\n\
Alice Example & Bob Example & Carol Silent,2020-01-01 12:01:00,iMessage,Incoming,+15555550122,Bob Example,Read,,,Hey,,,\n",
    );
    let members: Vec<_> = documents[0]
        .conversation
        .participants
        .iter()
        .map(|p| {
            (
                p.handle.as_deref(),
                p.display_name.as_deref(),
                p.handle_type,
            )
        })
        .collect();
    assert_eq!(
        members,
        vec![
            (
                Some("+15555550111"),
                Some("Alice Example"),
                Some(message_ir::HandleType::Phone)
            ),
            (
                Some("+15555550122"),
                Some("Bob Example"),
                Some(message_ir::HandleType::Phone)
            ),
            (None, Some("Carol Silent"), None),
        ]
    );
}

/// iMazing gives no group id and a session name is not unique: two groups
/// with the same name, each in its own chat folder, stay two conversations.
#[test]
fn two_groups_with_one_name_in_two_files_stay_two_conversations() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in");
    for (folder, rows) in [
        (
            "2020-01-01 12 00 00 - Book Club",
            "Book Club,2020-01-01 12:00:00,iMessage,Incoming,+15555550111,Alice,Read,,,Hi,,,\n\
Book Club,2020-01-01 12:01:00,iMessage,Incoming,+15555550122,Bob,Read,,,Hey,,,\n",
        ),
        (
            "2021-01-01 12 00 00 - Book Club",
            "Book Club,2021-01-01 12:00:00,iMessage,Incoming,+15555550111,Alice,Read,,,Other club,,,\n\
Book Club,2021-01-01 12:01:00,iMessage,Incoming,+15555550122,Bob,Read,,,Yes,,,\n",
        ),
    ] {
        let chat = input.join(folder);
        fs::create_dir_all(&chat).unwrap();
        fs::write(
            chat.join("Messages.csv"),
            format!("{MESSAGES_HEADER}{rows}"),
        )
        .unwrap();
    }
    let out = dir.path().join("out");
    let report = convert_export(ConvertExportArgs {
        input: &input,
        output: &out,
        timezone: Some("UTC"),
        transforms: ExportTransforms::none(),
        output_format: OutputFormat::Json,
        cancel: None,
        resume: false,
    })
    .unwrap();
    assert_eq!(report.conversations, 2);
    assert_eq!(report.messages, 4);
}

/// A group's key is its earliest row. When rows share the earliest time,
/// the smallest of them is taken, so a second export that writes them in
/// another order gives the group the same key.
#[test]
fn a_groups_chat_id_does_not_depend_on_row_order() {
    let alice = "Book Club,2020-01-01 12:00:00,iMessage,Incoming,+15555550111,Alice,Read,,,Hi,,,\n";
    let bob = "Book Club,2020-01-01 12:00:00,iMessage,Incoming,+15555550122,Bob,Read,,,Hey,,,\n";
    let chat_id = |rows: String| convert_rows(&rows)[0].conversation.chat_identifier.clone();
    assert_eq!(
        chat_id(format!("{alice}{bob}")),
        chat_id(format!("{bob}{alice}"))
    );
}

/// The address of a one-to-one chat in which one person wrote from two
/// addresses does not depend on which one wrote first, so an export that
/// starts later in the chat gives it the same key.
#[test]
fn a_direct_chats_address_does_not_depend_on_which_address_wrote_first() {
    let phone_first = "Alice,2020-01-01 12:00:00,iMessage,Incoming,+15555550111,Alice,Read,,,One,,,\n\
Alice,2020-01-01 12:01:00,iMessage,Incoming,alice@example.com,Alice,Read,,,Two,,,\n";
    let email_first = "Alice,2020-01-01 12:00:00,iMessage,Incoming,alice@example.com,Alice,Read,,,One,,,\n\
Alice,2020-01-01 12:01:00,iMessage,Incoming,+15555550111,Alice,Read,,,Two,,,\n";
    let chat_id = |rows: &str| convert_rows(rows)[0].conversation.chat_identifier.clone();
    assert_eq!(chat_id(phone_first), "+15555550111");
    assert_eq!(chat_id(email_first), "+15555550111");
}

const WHATSAPP_HEADER: &str = "Chat Session,Message Date,Sent Date,Type,Sender ID,Sender Name,Status,Forwarded,Text,Attachment info\n";

/// A WhatsApp account has one number, so two numbers under one Sender Name
/// are two people, and the chat they wrote in is a group.
#[test]
fn two_whatsapp_people_with_one_name_make_a_group() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in");
    fs::create_dir(&input).unwrap();
    fs::write(
        input.join("WhatsApp - Climbing.csv"),
        format!(
            "{WHATSAPP_HEADER}Climbing,2020-01-01 12:00:00,,Incoming,+15555550111,Chris,Read,,Hi,\n\
Climbing,2020-01-01 12:01:00,,Incoming,+15555550122,Chris,Read,,Hey,\n"
        ),
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
    let documents: Vec<_> = fs::read_dir(&out)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .map(|path| message_ir_format::read_conversation_json(&path).unwrap())
        .collect();
    assert_eq!(documents.len(), 1);
    let conversation = &documents[0].conversation;
    assert_eq!(
        conversation.conversation_type,
        message_ir::IrConversationType::Group
    );
    assert_eq!(conversation.participants.len(), 2);
}

/// Two groups can start with the same row: the account holder sends one
/// message to two new groups in the same second. They stay two
/// conversations, and each keeps its key whichever file is read first.
#[test]
fn two_groups_that_start_with_one_row_stay_two_conversations() {
    let first = "Hi all,2020-01-01 12:00:00,iMessage,Outgoing,,,Sent,,,Happy new year,,,\n";
    let book_club = format!(
        "{}Book Club,2020-01-01 12:05:00,iMessage,Incoming,+15555550111,Alice,Read,,,Thanks,,,\n\
Book Club,2020-01-01 12:06:00,iMessage,Incoming,+15555550122,Bob,Read,,,Same,,,\n",
        first.replace("Hi all", "Book Club")
    );
    let climbing = format!(
        "{}Climbing,2020-01-01 12:07:00,iMessage,Incoming,+15555550133,Carol,Read,,,You too,,,\n\
Climbing,2020-01-01 12:08:00,iMessage,Incoming,+15555550144,Dan,Read,,,Cheers,,,\n",
        first.replace("Hi all", "Climbing")
    );
    let ids = |files: [(&str, &str); 2]| {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("in");
        for (folder, rows) in files {
            let chat = input.join(folder);
            fs::create_dir_all(&chat).unwrap();
            fs::write(
                chat.join("Messages.csv"),
                format!("{MESSAGES_HEADER}{rows}"),
            )
            .unwrap();
        }
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
        let mut ids: Vec<(String, String)> = fs::read_dir(&out)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .map(|path| message_ir_format::read_conversation_json(&path).unwrap())
            .map(|doc| {
                (
                    doc.conversation.group_title.clone().unwrap_or_default(),
                    doc.conversation.chat_identifier.clone(),
                )
            })
            .collect();
        ids.sort();
        ids
    };
    let in_order = ids([("a - Book Club", &book_club), ("b - Climbing", &climbing)]);
    let swapped = ids([("a - Climbing", &climbing), ("b - Book Club", &book_club)]);
    assert_eq!(in_order.len(), 2, "{in_order:?}");
    assert_ne!(in_order[0].1, in_order[1].1);
    assert_eq!(in_order, swapped);
}
