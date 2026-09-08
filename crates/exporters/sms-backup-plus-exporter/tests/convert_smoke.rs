use crate::emit::{ConvertExportArgs, convert_export};
use anyhow::Result;
use message_ir_format::{ExportTransforms, FormatSinkResult};
use message_vault_io_core::testutil::{assert_csv_header, assert_csv_row, csv_files, csv_rows};
use message_vault_io_core::{ExportReport, OutputFormat};
use std::fs;
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn convert(inputs: &[&Path], output_dir: &Path) -> Result<(ExportReport, FormatSinkResult)> {
    convert_export(ConvertExportArgs {
        inputs,
        output_dir,
        owner_phones: &["+15555550100".into()],
        owner_emails: &["owner@example.com".into()],
        verbose: false,
        transforms: ExportTransforms::none(),
        output_format: OutputFormat::Csv,
        cancel: None,
        log: None,
        resume: false,
    })
}

#[test]
fn output_equals_input_bails_before_cleaning() {
    let input = fixtures();
    let sample = input.join("flat_smssync_276_sam.eml");
    assert!(
        sample.is_file() || input.is_dir(),
        "missing fixtures under {}",
        input.display()
    );
    let err = convert(&[input.as_path()], input.as_path()).expect_err("output == input must fail");
    assert!(
        err.to_string()
            .contains("must not be the same as, or contain"),
        "unexpected error: {err}"
    );
    // Fixture EMLs must still be present after the refused run.
    let eml_still_there = fs::read_dir(&input)
        .unwrap()
        .filter_map(|e| e.ok())
        .any(|e| e.path().extension().and_then(|x| x.to_str()) == Some("eml"));
    assert!(eml_still_there, "fixture .eml files must survive");
}

#[test]
fn convert_smoke_writes_csv_not_json() {
    let input = fixtures();
    let tmp = tempfile::tempdir().unwrap();
    let (report, _) = convert(&[input.as_path()], tmp.path()).unwrap();

    assert!(report.conversations >= 1);
    let flat = report.extra("flat_eml");
    let archive = report.extra("archive_eml");
    assert!(flat >= 1 || archive >= 1);

    assert_csv_header(
        tmp.path(),
        &[
            "chat_identifier",
            "attachments_json",
            "export_source",
            "export_tool",
            "export_tool_version",
            "timestamp_unix_ms",
            "android_type",
            "source_fields_json",
            "owner_handle",
            "participants_json",
            "read_receipt", // unified header; empty for SMS
            "tapbacks_json",
        ],
        &["date_ms", "contact_name", "xml_fields_json"],
        // The flat SMSSync message, read back out of the export. The previous
        // needle here was "sms-backup-plus", which is the value of the
        // `export_source` column and is written whether or not a single
        // message survived the parse.
        &[
            ("text", "Hello from Alice"),
            ("direction", "incoming"),
            ("timestamp_unix_ms", "1609459200000"),
            ("chat_identifier", "+14075551234"),
        ],
    );

    // The archive `.eml` is a different parser: one mail carrying a
    // transcript, with the sender named per line. Both of its messages, and
    // the direction each line's name decides, must come through.
    let csv = &csv_files(tmp.path())[0];
    assert_csv_row(
        csv,
        &[
            ("text", "Check this"),
            ("direction", "outgoing"),
            ("timestamp_unix_ms", "1577898000000"),
        ],
    );
    assert_csv_row(
        csv,
        &[
            ("text", "Thanks"),
            ("direction", "incoming"),
            ("timestamp_unix_ms", "1577898060000"),
        ],
    );
    // Vendor fields (source_kind, smssync_id, eml_path) live inside source_fields_json.
    let contents = fs::read_to_string(&csv_files(tmp.path())[0]).unwrap();
    assert!(contents.contains("source_kind"));
}

#[test]
fn end_dedupe_collapses_duplicate_flats() {
    let tmp = tempfile::tempdir().unwrap();
    let input_dir = tmp.path().join("in");
    fs::create_dir_all(&input_dir).unwrap();

    let src = fixtures().join("flat_received.eml");
    let bytes = fs::read(&src).unwrap();
    fs::write(input_dir.join("a.eml"), &bytes).unwrap();
    fs::write(input_dir.join("b.eml"), &bytes).unwrap();

    let out = tmp.path().join("out");
    let (report, _) = convert(&[input_dir.as_path()], &out).unwrap();

    assert_eq!(report.extra("flat_eml"), 2);
    assert_eq!(report.extra("messages_before_dedupe"), 2);
    assert_eq!(report.messages, 1);
    assert_eq!(report.duplicates_dropped, 1);
    assert_eq!(report.conversations, 1);
}

#[test]
fn dedupe_collapses_archive_and_flat_despite_ms_mismatch() {
    use chrono::{Local, NaiveDateTime, TimeZone};

    let tmp = tempfile::tempdir().unwrap();
    let input_dir = tmp.path().join("in");
    fs::create_dir_all(&input_dir).unwrap();

    let naive = NaiveDateTime::parse_from_str("2020-01-01 12:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
    let local_ts = Local
        .from_local_datetime(&naive)
        .single()
        .unwrap()
        .timestamp();
    let ms = local_ts * 1000 + 488;

    fs::write(
        input_dir.join("archive.eml"),
        b"From: <4075551234@sms-backup-plus.local>\r\n\
To: me@example.com\r\n\
Subject: SMS archive Alice\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Alice\r\n\
2020-01-01 12:00:00 - Me\r\n\
Will do\r\n",
    )
    .unwrap();

    fs::write(
        input_dir.join("flat.eml"),
        format!(
            "From: me@example.com\r\n\
To: 4075551234@sms-backup-plus.local\r\n\
Subject: SMS with Alice\r\n\
X-smssync-type: 2\r\n\
X-smssync-address: 4075551234\r\n\
X-smssync-date: {ms}\r\n\
X-smssync-id: 999\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Will do\r\n"
        ),
    )
    .unwrap();

    let out = tmp.path().join("out");
    let (report, _) = convert(&[input_dir.as_path()], &out).unwrap();

    assert_eq!(report.extra("messages_before_dedupe"), 2);
    assert_eq!(report.messages, 1);
    assert_eq!(report.duplicates_dropped, 1);

    let csv = fs::read_to_string(out.join("+14075551234.csv")).unwrap();
    assert!(csv.contains("Will do"));
    // source_kind/smssync_id now live inside the source_fields_json cell.
    assert!(csv.contains("flat"));
    assert!(csv.contains("999"));
}

#[test]
fn jsonl_drains_the_write_queue_and_a_second_run_resumes_it() {
    let input = fixtures();
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    fs::create_dir_all(&out).expect("out dir");

    let run = |resume: bool| {
        convert_export(ConvertExportArgs {
            inputs: &[input.as_path()],
            output_dir: &out,
            owner_phones: &["+15555550100".into()],
            owner_emails: &["owner@example.com".into()],
            verbose: false,
            transforms: ExportTransforms::none(),
            output_format: OutputFormat::Jsonl,
            cancel: None,
            log: None,
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

/// Two different messages that share an `X-smssync-id`.
///
/// SMS Backup+ takes that header from the Android message id, which is unique
/// only within one device's database and is reused after a wipe or a restore,
/// so a mailbox holding two backups easily carries the same id on unrelated
/// messages. `identity::cover_identity` ignores the header for exactly that
/// reason, and `identity.rs` unit-tests the key it builds — but nothing put
/// two colliding files through the exporter, so a change that started keying
/// on the id would drop one of every colliding pair with the whole suite
/// green. `flat_smssync_276_alex.eml` and `flat_smssync_276_sam.eml` were
/// committed for this test and had none.
///
/// The two committed fixtures are in different chats, and the dedupe map is
/// per chat, so they cover the parse and the split but cannot themselves
/// collide. The same-chat pair written here is the case that can: two
/// messages one minute apart in one conversation, sharing id 276. Keying on
/// the id would collapse them into one.
#[test]
fn two_messages_sharing_an_smssync_id_both_survive() {
    let fixtures = fixtures();
    let tmp = tempfile::tempdir().expect("tempdir");
    let input = tmp.path().join("in");
    let out = tmp.path().join("out");
    fs::create_dir_all(&input).expect("input dir");
    fs::create_dir_all(&out).expect("out dir");
    for name in ["flat_smssync_276_alex.eml", "flat_smssync_276_sam.eml"] {
        let src = fixtures.join(name);
        assert!(src.is_file(), "missing fixture {}", src.display());
        fs::copy(&src, input.join(name)).expect("copy fixture");
    }
    // Two more in one chat, also both id 276.
    for (file, date, body) in [
        ("same_chat_first.eml", "1609460000000", "First to Dana"),
        ("same_chat_second.eml", "1609460060000", "Second to Dana"),
    ] {
        fs::write(
            input.join(file),
            format!(
                "From: dana@unknown.email\n\
                 To: me@example.com\n\
                 Subject: SMS with Dana\n\
                 X-smssync-type: 1\n\
                 X-smssync-address: +15555550133\n\
                 X-smssync-date: {date}\n\
                 X-smssync-id: 276\n\
                 Content-Type: text/plain; charset=utf-8\n\
                 \n\
                 {body}\n"
            ),
        )
        .expect("write fixture");
    }

    let (report, _) = convert(&[input.as_path()], &out).expect("convert");

    assert_eq!(
        report.messages, 4,
        "the shared X-smssync-id must not collapse unrelated messages"
    );
    assert_eq!(
        report.conversations, 3,
        "Alex, Sam and Dana are three conversations"
    );
    assert_eq!(report.duplicates_dropped, 0, "none of these is a duplicate");

    assert_csv_row(
        &out.join("+15555550111.csv"),
        &[
            ("text", "Hello from Alex"),
            ("direction", "outgoing"),
            ("timestamp_unix_ms", "1609459200313"),
        ],
    );
    assert_csv_row(
        &out.join("+15555550122.csv"),
        &[
            ("text", "Hello from Sam"),
            ("timestamp_unix_ms", "1609459300000"),
        ],
    );
    // The same-chat pair: both messages, in one conversation file.
    let dana = out.join("+15555550133.csv");
    assert_csv_row(&dana, &[("text", "First to Dana")]);
    assert_csv_row(&dana, &[("text", "Second to Dana")]);
    assert_eq!(
        csv_rows(&dana).len(),
        2,
        "both same-chat messages must survive the dedupe pass"
    );
}
