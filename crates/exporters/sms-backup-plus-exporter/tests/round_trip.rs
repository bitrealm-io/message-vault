//! SMS Backup+ mail imported, exported as `EML (SMS Backup+)`, and imported
//! again gives the same messages (#543, ADR 0021).

use crate::SmsBackupPlusArchive;
use crate::emit::{ConvertExportArgs, convert_export};
use chrono::{TimeZone, Utc};
use message_crate_core::{ExportReport, ExportTransforms, OutputFormat};
use message_ir::{ConversationDocument, IrDirection};
use message_ir_format::{FormatSink, read_conversation_jsonl};
use std::fs;
use std::path::{Path, PathBuf};

/// SMS Backup+ mail with an SMS each way, an MMS with a photo, and a group
/// message each way, one with a picture. It sits outside `tests/fixtures`
/// because other tests import that whole folder.
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/round_trip_fixture")
}

/// Import SMS Backup+ mail under `input` as JSON Lines into `output`, and
/// read the conversations back.
fn import(input: &Path, output: &Path) -> Vec<ConversationDocument> {
    convert_export(ConvertExportArgs {
        inputs: &[input.to_path_buf()],
        output_dir: output,
        owner_phones: &["+15555550100".into()],
        owner_emails: &["owner@example.com".into()],
        verbose: false,
        transforms: ExportTransforms::default(),
        output_format: OutputFormat::Jsonl,
        cancel: None,
        log: None,
        resume: false,
    })
    .unwrap();
    let mut documents: Vec<ConversationDocument> = fs::read_dir(output)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .map(|path| read_conversation_jsonl(&path).unwrap())
        .collect();
    documents.sort_by(|a, b| {
        a.conversation
            .chat_identifier
            .cmp(&b.conversation.chat_identifier)
    });
    documents
}

/// Write `documents` as SMS Backup+ mail into `output`, with the
/// attachments the import staged under `staged` copied in first, as Convert
/// does.
fn export(documents: Vec<ConversationDocument>, staged: &Path, output: &Path) -> ExportReport {
    let attachments = output.join("attachments");
    fs::create_dir_all(&attachments).unwrap();
    for entry in fs::read_dir(staged.join("attachments")).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), attachments.join(entry.file_name())).unwrap();
    }
    let backup_time = Utc.with_ymd_and_hms(2026, 10, 2, 12, 0, 0).unwrap();
    let mut sink = FormatSink::open(
        output,
        OutputFormat::SmsBackupPlus,
        ExportTransforms::default(),
    )
    .unwrap()
    .with_archive(Box::new(SmsBackupPlusArchive::new(backup_time)));
    for document in documents {
        sink.write_document(document).unwrap();
    }
    let mut report = ExportReport::default();
    sink.finish(&mut report).unwrap();
    report
}

/// What makes two imports the same messages: the conversation, the
/// message's id (made from its identity), time to the millisecond,
/// direction, sender, kind, text, and each attachment's bytes, type and name.
type MessageShape = (
    String,
    String,
    i64,
    IrDirection,
    Option<String>,
    String,
    String,
    Vec<(Option<String>, Option<String>, Option<String>)>,
);

fn shape(documents: &[ConversationDocument]) -> Vec<MessageShape> {
    let mut messages: Vec<MessageShape> = documents
        .iter()
        .flat_map(|document| {
            document.messages.iter().map(|message| {
                (
                    document.conversation.chat_identifier.clone(),
                    message.guid.clone(),
                    message.timestamp_unix_ms,
                    message.direction,
                    message.sender_handle.clone(),
                    message.message_kind.as_str().to_string(),
                    message.text.clone(),
                    message
                        .attachments
                        .iter()
                        .map(|attachment| {
                            (
                                attachment.digest_sha256.clone(),
                                attachment.mime_type.clone(),
                                attachment.original_name.clone(),
                            )
                        })
                        .collect(),
                )
            })
        })
        .collect();
    messages.sort_by(|a, b| a.1.cmp(&b.1));
    messages
}

#[test]
fn sms_backup_plus_mail_survives_an_export_and_a_second_import() {
    let first = tempfile::tempdir().unwrap();
    let exported = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();

    let before = import(&fixture(), first.path());
    assert_eq!(shape(&before).len(), 5, "the fixture holds five messages");
    assert!(
        shape(&before)
            .iter()
            .filter(|message| !message.7.is_empty())
            .count()
            == 2,
        "two of them carry an attachment"
    );

    let report = export(before.clone(), first.path(), exported.path());
    assert_eq!(report.extra(crate::write::LEFT_OUT), 0);
    let after = import(exported.path(), second.path());

    assert_eq!(shape(&after), shape(&before));
}

#[test]
fn each_conversation_is_one_folder_of_one_eml_per_message() {
    let first = tempfile::tempdir().unwrap();
    let exported = tempfile::tempdir().unwrap();
    let before = import(&fixture(), first.path());
    let stems: Vec<String> = before
        .iter()
        .map(ConversationDocument::filename_stem)
        .collect();

    export(before, first.path(), exported.path());

    for stem in stems {
        let folder = exported.path().join(&stem);
        let names: Vec<String> = fs::read_dir(&folder)
            .unwrap_or_else(|err| panic!("no folder {stem}: {err}"))
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(!names.is_empty(), "{stem}");
        assert!(
            names
                .iter()
                .all(|name| name.starts_with("00000") && name.ends_with(".eml")),
            "{names:?}"
        );
    }
    assert!(
        !exported.path().join("attachments").exists(),
        "the mail holds the attachment bytes"
    );
}
