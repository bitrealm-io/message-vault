use crate::emit::{ConvertExportArgs, convert_export};
use anyhow::Result;
use message_ir_format::{ExportTransforms, FormatSinkResult};
use message_vault_io_core::testutil::assert_csv_row;
use message_vault_io_core::{ExportReport, OutputFormat};
use std::fs;
use std::path::{Path, PathBuf};

fn convert(input: &Path, output: &Path) -> Result<(ExportReport, FormatSinkResult)> {
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
    let (report, _) = convert(&csv, tmp.path()).expect("convert");

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
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let csv = fixture.join("all_conversations.csv");
    let tmp = tempfile::tempdir().expect("tempdir");
    let out = tmp.path().join("out");
    fs::create_dir_all(&out).expect("out dir");

    let run = |resume: bool| {
        convert_export(ConvertExportArgs {
            input: &csv,
            output: &out,
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
        let mut names: Vec<String> = fs::read_dir(dir)
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
        .map(|n| fs::read_to_string(out.join(n)).expect("read jsonl"))
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
            fs::read_to_string(out.join(name)).expect("reread"),
            before,
            "a resumed run must not rewrite {name}"
        );
    }
}
