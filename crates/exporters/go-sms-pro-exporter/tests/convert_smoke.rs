use crate::emit::{ConvertExportArgs, convert_export};
use anyhow::Result;
use go_sms_mms::testutil::PduBuilder;
use message_vault_io_core::testutil::{
    assert_csv_export, assert_csv_row, assert_jsonl_resumes, csv_files,
};
use message_vault_io_core::{ExportReport, ExportTransforms, OutputFormat};
use std::fs;
use std::path::{Path, PathBuf};

/// The smoke backup: the two-message XML fixture and one received MMS.
fn write_backup(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    let xml = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/sample_export/gosms_sys_smoke.xml");
    fs::copy(xml, dir.join("gosms_sys_smoke.xml")).unwrap();
    let pdu = PduBuilder::received("+14075551234")
        .to("+15555550100")
        .text("Hello one to one")
        .build();
    fs::write(dir.join("I_1609459200_1_0.pdu"), pdu).unwrap();
}

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
    let tmp = tempfile::tempdir().expect("tempdir");
    let input = tmp.path().join("backup");
    write_backup(&input);
    let output = tmp.path().join("out");
    let report = convert(&input, &output).expect("convert_export should succeed");
    assert_eq!(report.conversations, 1);
    assert_eq!(report.extra("xml_messages_seen"), 2);
    assert_eq!(report.extra("pdu_messages"), 1);

    // Every message the fixture carries, read back out of the export. An
    // exporter that wrote a correct header and no messages must fail here.
    assert_csv_export(
        &output,
        &["chat_identifier", "direction", "attachments_json"],
        &["export_schema"],
        &[
            // `+g1f44b` in the fixture is GO SMS Pro's escape for an emoji;
            // the export must carry the decoded character, not the escape.
            ("text", "smoke hello \u{1f44b}"),
            ("direction", "incoming"),
            ("sender_handle", "+14075551234"),
            ("timestamp_unix_ms", "1609459200000"),
            ("chat_identifier", "+14075551234"),
        ],
    );

    let csv = &csv_files(&output)[0];
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
    // The PDU beside the XML lands in the same conversation.
    assert_csv_row(
        csv,
        &[
            ("text", "Hello one to one"),
            ("direction", "incoming"),
            ("sender_handle", "+14075551234"),
        ],
    );
}

#[test]
fn output_equals_input_bails_before_cleaning() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let input = tmp.path().join("backup");
    write_backup(&input);
    let err = convert(&input, &input).expect_err("output == input must fail");
    assert!(
        err.to_string()
            .contains("must not be the same as, or contain"),
        "unexpected error: {err}"
    );
    // The backup directory must not have been cleaned by the failed run.
    assert!(input.join("gosms_sys_smoke.xml").is_file());
    assert!(input.join("I_1609459200_1_0.pdu").is_file());
}

#[test]
fn jsonl_drains_the_write_queue_and_a_second_run_resumes_it() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let input = tmp.path().join("backup");
    write_backup(&input);
    let output = tmp.path().join("out");
    assert_jsonl_resumes(&output, |resume| {
        convert_export(ConvertExportArgs {
            input_dir: &input,
            output_dir: &output,
            owner_phones: &["+15555550100".into()],
            transforms: ExportTransforms::none(),
            output_format: OutputFormat::Jsonl,
            cancel: None,
            resume,
        })
    });
}
