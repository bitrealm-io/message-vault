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

fn convert(
    input: &std::path::Path,
    output: &std::path::Path,
) -> Result<(ExportReport, FormatSinkResult)> {
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
    let (report, _) = convert(dir.path(), &out).unwrap();
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
    let (report, _) = convert(dir.path(), &out).unwrap();
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
    let (report, _) = convert(dir.path(), &out).unwrap();
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
    let (report, _) = convert(dir.path(), &out).unwrap();
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
    let (report, _) = convert(dir.path(), &out).unwrap();
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
    let (report, _) = convert(dir.path(), &out).unwrap();
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
    let (report, _) = convert(dir.path(), &out).unwrap();
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
    let err = resolve_tz(Some("America/New_York")).unwrap_err();
    assert!(err.to_string().contains("UTC"));
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
    let (report, _) = convert(&chat, &out).unwrap();
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
    let (report, _) = convert(dir.path(), &out).unwrap();
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
    let (report, _) = convert(dir.path(), &out).unwrap();
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
