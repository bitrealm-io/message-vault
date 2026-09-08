use crate::emit::{ConvertExportArgs, convert_export};
use anyhow::Result;
use message_ir_format::{ExportTransforms, FormatSinkResult};
use message_vault_io_core::testutil::{assert_csv_header, assert_csv_row, csv_files};
use message_vault_io_core::{ExportReport, OutputFormat};
use std::path::{Path, PathBuf};

fn convert(input_dir: &Path, output_dir: &Path) -> Result<(ExportReport, FormatSinkResult)> {
    convert_export(ConvertExportArgs {
        input_dir,
        output_dir,
        owner_phones: &["+15555550100".into()],
        transforms: ExportTransforms::none(),
        output_format: OutputFormat::Csv,
        cancel: None,
        resume: false,
    })
}

#[test]
fn convert_smoke_writes_csv_not_json() {
    let input = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample_export");
    assert!(input.is_dir(), "missing fixture: {}", input.display());

    let tmp = tempfile::tempdir().expect("tempdir");
    let (report, _) = convert(input.as_path(), tmp.path()).expect("convert_export should succeed");
    assert!(report.conversations >= 1);
    assert!(report.extra.get("xml_messages_seen").copied().unwrap_or(0) >= 2);

    // Every message the fixture carries, read back out of the export. The
    // previous version of this passed `""` as the body needle, which every
    // file contains, so it asserted the header columns and nothing else — an
    // exporter that wrote a correct header and no messages passed it.
    assert_csv_header(
        tmp.path(),
        &["chat_identifier", "direction", "attachments_json"],
        &["export_schema"],
        &[
            // `+g1f44b` in the fixture is GO SMS Pro's escape for an emoji;
            // the export must carry the decoded character, not the escape.
            ("text", "smoke hello \u{1f44b}"),
            ("direction", "incoming"),
            ("timestamp_unix_ms", "1609459200000"),
            ("chat_identifier", "+14075551234"),
        ],
    );

    let csv = &csv_files(tmp.path())[0];
    // `type` 2 in the fixture is an outgoing message, and the direction column
    // is where that decision shows up.
    assert_csv_row(
        csv,
        &[
            ("text", "smoke reply"),
            ("direction", "outgoing"),
            ("timestamp_unix_ms", "1609459260000"),
        ],
    );
    // The `.pdu` fixture beside the XML. Nothing read it before: the export
    // could have dropped every binary PDU and this test still passed, because
    // the XML alone satisfied the conversation count.
    assert_csv_row(
        csv,
        &[
            ("text", "Hello one to one"),
            ("direction", "incoming"),
            ("message_kind", "sms"),
        ],
    );
}

#[test]
fn output_equals_input_bails_before_cleaning() {
    let input = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample_export");
    let err = convert(input.as_path(), input.as_path()).expect_err("output == input must fail");
    assert!(
        err.to_string()
            .contains("must not be the same as, or contain"),
        "unexpected error: {err}"
    );
    // The backup directory must not have been cleaned by the failed run.
    assert!(input.join("gosms_sys_smoke.xml").is_file());
    assert!(input.join("I_1609459200_recv.pdu").is_file());
}

#[test]
fn jsonl_drains_the_write_queue_and_a_second_run_resumes_it() {
    let input = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample_export");
    let tmp = tempfile::tempdir().expect("tempdir");
    let out = tmp.path().join("out");
    std::fs::create_dir_all(&out).expect("out dir");

    let run = |resume: bool| {
        convert_export(ConvertExportArgs {
            input_dir: input.as_path(),
            output_dir: &out,
            owner_phones: &["+15555550100".into()],
            transforms: ExportTransforms::none(),
            output_format: OutputFormat::Jsonl,
            cancel: None,
            resume,
        })
    };

    let (report, _) = run(false).expect("convert");
    assert!(report.conversations >= 1);
    assert_eq!(
        report.conversations_skipped, 0,
        "a first run into an empty folder skips nothing"
    );

    let jsonl_files = |dir: &Path| -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .expect("read output")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".jsonl"))
            .collect();
        names.sort();
        names
    };
    let first = jsonl_files(&out);
    assert!(!first.is_empty(), "the queue wrote conversation files");
    let bodies: Vec<String> = first
        .iter()
        .map(|n| std::fs::read_to_string(out.join(n)).expect("read jsonl"))
        .collect();

    // The file bytes alone prove nothing here: the writer is deterministic, so
    // a resumed run that quietly rewrote every conversation would produce the
    // same bytes and this test would still pass. `conversations_skipped` is
    // the only observable difference between resuming and starting over.
    let (resumed, _) = run(true).expect("resume convert");
    assert_eq!(
        resumed.conversations_skipped, report.conversations,
        "a resumed run must skip every conversation the first run wrote"
    );
    assert!(
        resumed.conversations_skipped > 0,
        "the fixture must produce at least one conversation to skip"
    );

    assert_eq!(jsonl_files(&out), first, "same file set after a resume");
    for (name, before) in first.iter().zip(bodies) {
        assert_eq!(
            std::fs::read_to_string(out.join(name)).expect("reread"),
            before,
            "a resumed run must not rewrite {name}"
        );
    }
}
