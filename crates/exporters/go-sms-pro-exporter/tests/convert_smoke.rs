use crate::emit::{ConvertExportArgs, convert_export};
use anyhow::Result;
use message_vault_io_core::testutil::{
    assert_csv_export, assert_csv_row, assert_jsonl_resumes, csv_files,
};
use message_vault_io_core::{ExportReport, ExportTransforms, OutputFormat};
use std::path::{Path, PathBuf};

fn convert(input_dir: &Path, output_dir: &Path) -> Result<ExportReport> {
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
    let report = convert(input.as_path(), tmp.path()).expect("convert_export should succeed");
    assert!(report.conversations >= 1);
    assert!(report.extra.get("xml_messages_seen").copied().unwrap_or(0) >= 2);

    // Every message the fixture carries, read back out of the export. The
    // previous version of this passed `""` as the body needle, which every
    // file contains, so it asserted the header columns and nothing else — an
    // exporter that wrote a correct header and no messages passed it.
    assert_csv_export(
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
    assert_jsonl_resumes(tmp.path(), |resume| {
        convert_export(ConvertExportArgs {
            input_dir: input.as_path(),
            output_dir: tmp.path(),
            owner_phones: &["+15555550100".into()],
            transforms: ExportTransforms::none(),
            output_format: OutputFormat::Jsonl,
            cancel: None,
            resume,
        })
    });
}
