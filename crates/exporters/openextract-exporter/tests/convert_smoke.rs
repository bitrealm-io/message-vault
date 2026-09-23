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
