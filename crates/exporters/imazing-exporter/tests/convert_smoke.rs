use crate::emit::{ConvertExportArgs, convert_export};
use anyhow::Result;
use message_ir_format::{ExportTransforms, FormatSinkResult};
use message_vault_io_core::testutil::{assert_csv_row, csv_rows};
use message_vault_io_core::{ExportReport, OutputFormat};
use std::fs;
use std::path::{Path, PathBuf};

fn convert(input: &Path, output: &Path) -> Result<(ExportReport, FormatSinkResult)> {
    convert_export(ConvertExportArgs {
        input,
        output,
        time_zone: Some(chrono_tz::UTC),
        transforms: ExportTransforms::none(),
        output_format: OutputFormat::Csv,
        cancel: None,
        resume: false,
    })
}

#[test]
fn convert_messages_keys_the_chat_by_its_number() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let messages = fixture.join("messages.csv");
    assert!(messages.is_file(), "missing {}", messages.display());

    let tmp = tempfile::tempdir().expect("tempdir");
    let (report, _) = convert(&messages, tmp.path()).expect("convert");

    assert_eq!(report.conversations, 1);
    assert_eq!(report.messages, 3);
    assert_eq!(report.extra.get("messages_files").copied().unwrap_or(0), 1);
    assert_eq!(report.extra.get("whatsapp_files").copied().unwrap_or(0), 0);
    assert_eq!(report.extra.get("name_only_chat").copied().unwrap_or(0), 0);

    let out = tmp.path().join("+13212462167.csv");
    let body = fs::read_to_string(&out).expect("read csv");
    assert!(body.contains("imazing"));
    assert!(body.contains("iMazing"));
    assert!(body.contains("3.5.5"));

    // The three messages, read back by column. The substring assertions above
    // are satisfied by the header line and the export metadata, so they hold
    // even if every row was dropped — `chat_identifier` and `imazing_type` are
    // column names, not values.
    assert_csv_row(
        &out,
        &[
            ("text", "Hello from Bob"),
            ("direction", "incoming"),
            ("service", "sms"),
            ("sender_display_name", "Bob McRoy"),
        ],
    );
    assert_csv_row(&out, &[("text", "Hi Bob"), ("direction", "outgoing")]);
    // The third row is an iMessage carrying an attachment, so it proves both
    // that the service column follows the source and that the attachment file
    // name reached the row rather than only the folder.
    let rows = csv_rows(&out);
    let photo = rows
        .iter()
        .find(|r| r.get("text").is_some_and(|t| t == "Photo"))
        .expect("the attachment message must be in the export");
    assert_eq!(photo.get("service").map(String::as_str), Some("imessage"));
    assert!(
        photo
            .get("attachments_json")
            .is_some_and(|a| a.contains("image000000.jpg")),
        "the attachment must be recorded on its own message, not merely \
         somewhere in the file: {photo:#?}"
    );
}

#[test]
fn convert_whatsapp_csv_direct() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let whatsapp = fixture.join("whatsapp.csv");
    let tmp = tempfile::tempdir().expect("tempdir");
    let (report, _) = convert(&whatsapp, tmp.path()).expect("convert");

    assert_eq!(report.conversations, 1);
    assert_eq!(report.messages, 3);
    assert_eq!(report.extra.get("whatsapp_files").copied().unwrap_or(0), 1);
    let out = tmp.path().join("+13212462167__whatsapp.csv");
    let body = fs::read_to_string(&out).expect("read csv");
    assert!(body.contains("WhatsApp"));

    // `contains("forwarded")` matched the `forwarded` key that every row's
    // `source_fields_json` carries whatever its value, so it passed even when
    // no message was actually marked forwarded. The flag is read off the row
    // that should carry it instead, together with the other two messages, so a
    // parse that lost the column or put the flag on the wrong message fails.
    assert_csv_row(
        &out,
        &[("text", "Hello on WhatsApp"), ("direction", "incoming")],
    );
    assert_csv_row(
        &out,
        &[("text", "Reply on WhatsApp"), ("direction", "outgoing")],
    );
    let rows = csv_rows(&out);
    let forwarded = rows
        .iter()
        .find(|r| r.get("text").is_some_and(|t| t == "Forwarded photo"))
        .expect("the forwarded message must be in the export");
    let source: serde_json::Value =
        serde_json::from_str(forwarded.get("source_fields_json").expect("source fields"))
            .expect("source_fields_json is JSON");
    assert_eq!(
        source.get("forwarded").and_then(|v| v.as_str()),
        Some("Yes"),
        "the forwarded flag belongs to this message: {forwarded:#?}"
    );
    assert_eq!(
        source.get("attachment_info").and_then(|v| v.as_str()),
        Some("12.34 KB")
    );
}

#[test]
fn convert_export_root_recursively_keeps_services_separate() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/export_root");
    let tmp = tempfile::tempdir().expect("tempdir");
    let (report, _) = convert(&root, tmp.path()).expect("convert");

    assert_eq!(report.extra.get("messages_files").copied().unwrap_or(0), 2);
    assert_eq!(report.extra.get("whatsapp_files").copied().unwrap_or(0), 1);
    assert!(report.conversations >= 3);
    assert!(tmp.path().join("+13212462167.csv").is_file());
    assert!(tmp.path().join("+13212462167__whatsapp.csv").is_file());
    // Silent Carol never sent a message, so the source records no address for
    // her. She is reported rather than given an invented number.
    let group = fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .find(|n| n.ends_with(".csv") && n.starts_with("group") && !n.contains("whatsapp"))
        .expect("group csv");
    let body = fs::read_to_string(tmp.path().join(group)).unwrap();
    assert!(body.contains("group"));
    assert!(body.contains("Notification") || body.contains("notification"));
    assert!(
        report
            .extra
            .get("unresolved_group_participants")
            .copied()
            .unwrap_or(0)
            >= 1
    );
}

#[test]
fn jsonl_drains_the_write_queue_and_a_second_run_resumes_it() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let messages = fixture.join("messages.csv");
    let tmp = tempfile::tempdir().expect("tempdir");

    let convert_jsonl = |resume: bool| {
        convert_export(ConvertExportArgs {
            input: &messages,
            output: tmp.path(),
            time_zone: Some(chrono_tz::UTC),
            transforms: ExportTransforms::none(),
            output_format: OutputFormat::Jsonl,
            cancel: None,
            resume,
        })
    };

    let (report, _) = convert_jsonl(false).expect("convert");
    assert_eq!(report.conversations, 1);

    let jsonl_files = |dir: &Path| -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .expect("read output")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".jsonl"))
            .collect();
        names.sort();
        names
    };
    let first = jsonl_files(tmp.path());
    assert_eq!(first.len(), 1, "the queue wrote a file per conversation");
    let before = fs::read_to_string(tmp.path().join(&first[0])).expect("read jsonl");

    // The file bytes alone prove nothing here: the writer is deterministic, so
    // a resumed run that quietly rewrote every conversation would produce the
    // same bytes and this test would still pass. `conversations_skipped` is
    // the only observable difference between resuming and starting over.
    let (resumed, _) = convert_jsonl(true).expect("resume convert");
    assert_eq!(resumed.conversations, 1, "resume still accounts for it");
    assert_eq!(
        resumed.conversations_skipped, 1,
        "the one conversation was already written, so the resume skipped it"
    );
    assert_eq!(jsonl_files(tmp.path()), first, "same file set");
    assert_eq!(
        fs::read_to_string(tmp.path().join(&first[0])).expect("reread"),
        before,
        "a resumed run must not rewrite a conversation it already has"
    );
}
