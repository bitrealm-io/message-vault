use crate::emit::{ConvertExportArgs, convert_export};
use anyhow::Result;
use message_vault_io_core::testutil::{assert_csv_row, assert_jsonl_resumes};
use message_vault_io_core::{ExportReport, ExportTransforms, OutputFormat};
use std::fs;
use std::path::{Path, PathBuf};

fn convert(input: &Path, output: &Path) -> Result<ExportReport> {
    convert_export(ConvertExportArgs {
        input,
        output,
        transforms: ExportTransforms::none(),
        output_format: OutputFormat::Csv,
        cancel: None,
        resume: false,
    })
}

#[test]
fn output_equals_input_dir_bails_before_cleaning() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let csv = fixture.join("all_conversations.csv");
    assert!(csv.is_file(), "missing {}", csv.display());
    // Output = fixture dir that holds the source CSV — open_prepared would
    // delete every *.csv before discovery.
    let err = convert(&fixture, &fixture).expect_err("output == input must fail");
    assert!(
        err.to_string()
            .contains("must not be the same as, or contain"),
        "unexpected error: {err}"
    );
    assert!(
        csv.is_file(),
        "source CSV must survive the refused run: {}",
        csv.display()
    );
}

#[test]
fn convert_all_conversations_keys_the_chat_by_its_number() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let csv = fixture.join("all_conversations.csv");
    assert!(csv.is_file(), "missing {}", csv.display());

    let tmp = tempfile::tempdir().expect("tempdir");
    let report = convert(&csv, tmp.path()).expect("convert");

    assert_eq!(report.conversations, 1);
    assert_eq!(report.messages, 2);
    // The source gave a number, so nothing is name-only.
    assert_eq!(report.extra.get("name_only_chat").copied().unwrap_or(0), 0);

    let out = tmp.path().join("+15555550122.csv");
    let body = fs::read_to_string(&out).expect("read csv");
    assert!(body.contains("openextract"));
    assert!(body.contains("all-conversations"));

    // Both messages the fixture carries, read back by column. The three
    // assertions above are all satisfied by the header line and the export
    // metadata, so before this the crate parsed no content in any test: an
    // exporter that dropped every row still passed.
    assert_csv_row(
        &out,
        &[
            ("text", "Hello from Sam"),
            ("direction", "incoming"),
            ("sender_handle", "+15555550122"),
            // 2020-01-01T17:00:00+00:00 in the source.
            ("timestamp_unix_ms", "1577898000000"),
        ],
    );
    // "Is From Me" is True on this row, and the direction column is where that
    // decision lands.
    assert_csv_row(
        &out,
        &[
            ("text", "Hi Sam"),
            ("direction", "outgoing"),
            ("timestamp_unix_ms", "1577898060000"),
        ],
    );
}

#[test]
fn jsonl_drains_the_write_queue_and_a_second_run_resumes_it() {
    let csv =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/all_conversations.csv");
    let tmp = tempfile::tempdir().expect("tempdir");
    assert_jsonl_resumes(tmp.path(), |resume| {
        convert_export(ConvertExportArgs {
            input: &csv,
            output: tmp.path(),
            transforms: ExportTransforms::none(),
            output_format: OutputFormat::Jsonl,
            cancel: None,
            resume,
        })
    });
}

/// Convert the CSV files `files` (name, body) as one OpenExtract export to
/// JSON, and read each conversation back, keyed by chat identifier.
fn convert_to_documents(
    files: &[(&str, &str)],
) -> (
    ExportReport,
    std::collections::BTreeMap<String, message_ir::ConversationDocument>,
) {
    let dir = tempfile::tempdir().expect("tempdir");
    let input = dir.path().join("in");
    fs::create_dir(&input).unwrap();
    for (name, body) in files {
        fs::write(input.join(name), body).unwrap();
    }
    let out = dir.path().join("out");
    let report = convert_export(ConvertExportArgs {
        input: &input,
        output: &out,
        transforms: ExportTransforms::none(),
        output_format: OutputFormat::Json,
        cancel: None,
        resume: false,
    })
    .expect("convert");
    let documents = fs::read_dir(&out)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .map(|path| message_ir_format::read_conversation_json(&path).expect("read back"))
        .map(|doc| (doc.conversation.chat_identifier.clone(), doc))
        .collect();
    (report, documents)
}

/// The phone handles on a conversation's roster.
fn roster(doc: &message_ir::ConversationDocument) -> Vec<&str> {
    doc.conversation
        .participants
        .iter()
        .filter_map(|p| p.handle.as_deref())
        .collect()
}

/// A chat labelled with a name is keyed by the number its incoming rows
/// carry, and that number is the roster. The owner's own number on a sent
/// row comes first here and must not become the chat's key.
#[test]
fn a_named_chat_is_keyed_by_its_peers_number() {
    let (_, documents) = convert_to_documents(&[(
        "all_conversations.csv",
        "Date,Conversation,Direction,Sender,Text,Is From Me,Has Attachments\n\
2020-01-01T17:00:00+00:00,Sam Example,Sent,+15555550100,Hi Sam,True,False\n\
2020-01-01T17:01:00+00:00,Sam Example,Received,+15555550122,Hello from Sam,False,False\n",
    )]);
    let keys: Vec<_> = documents.keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["+15555550122"]);
    let doc = &documents["+15555550122"];
    assert_eq!(doc.messages.len(), 2);
    assert_eq!(roster(doc), vec!["+15555550122"]);
}

/// A row with no conversation named, or one named only `Me`, belongs to
/// its incoming sender. An outgoing row with nothing to say who it went to
/// lands in the `unknown` chat, which has no roster.
#[test]
fn a_row_without_a_conversation_belongs_to_its_sender_or_to_no_one() {
    let (_, documents) = convert_to_documents(&[(
        "all_conversations.csv",
        "Date,Conversation,Direction,Sender,Text,Is From Me,Has Attachments\n\
2020-01-01T17:00:00+00:00,,Received,+15555550133,Hey,False,False\n\
2020-01-01T17:01:00+00:00,,Sent,me,To whom,True,False\n\
2020-01-01T17:02:00+00:00,Me,Received,Cathy Arp,Still here,False,False\n",
    )]);
    let keys: Vec<_> = documents.keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["+15555550133", "Cathy_Arp", "unknown"]);
    assert_eq!(roster(&documents["+15555550133"]), vec!["+15555550133"]);
    assert!(documents["unknown"].conversation.participants.is_empty());
}

/// In a per-chat file every row is the one chat, whatever its sender says:
/// a sent row carrying the owner's number, and a `Me` row whose
/// "Is From Me" column was left blank.
#[test]
fn every_row_of_a_per_chat_file_is_the_one_chat() {
    let (report, documents) = convert_to_documents(&[
        (
            "conversation_1.csv",
            "Date,Sender,Text,Is From Me,Has Attachments\n\
2020-01-01T12:00:00+00:00,+15555550122,Hello,False,False\n\
2020-01-01T12:01:00+00:00,+15555550100,Hi,True,False\n",
        ),
        (
            "conversation_2.csv",
            "Date,Sender,Text,Is From Me,Has Attachments\n\
2020-01-01T12:00:00+00:00,Me,Are you there,,False\n\
2020-01-01T12:01:00+00:00,Cathy Arp,Yes,False,False\n",
        ),
    ]);
    let keys: Vec<_> = documents.keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["+15555550122", "Cathy_Arp"]);
    assert_eq!(documents["+15555550122"].messages.len(), 2);
    assert_eq!(documents["Cathy_Arp"].messages.len(), 2);
    assert_eq!(report.conversations, 2);
}

/// OpenExtract writes each chat's attachments to a CSV of their own. That
/// file is not a conversation, so it is skipped rather than read and
/// reported as a file that failed to parse.
#[test]
fn an_attachments_csv_is_not_read_as_a_conversation() {
    let (report, documents) = convert_to_documents(&[
        (
            "conversation_1.csv",
            "Date,Sender,Text,Is From Me,Has Attachments\n\
2020-01-01T12:00:00+00:00,+15555550122,Hello,False,True\n",
        ),
        (
            "conversation_1_attachments.csv",
            "Date,Filename,Mime Type\n2020-01-01T12:00:00+00:00,photo.jpg,image/jpeg\n",
        ),
    ]);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(documents.len(), 1);

    let dir = tempfile::tempdir().unwrap();
    let attachments = dir.path().join("conversation_1_attachments.csv");
    fs::write(&attachments, "Date,Filename\n").unwrap();
    let err = convert(&attachments, &dir.path().join("out")).unwrap_err();
    assert!(
        err.to_string()
            .contains("not an OpenExtract conversation CSV"),
        "{err}"
    );
}
