//! `run()`, the entry point the desktop app calls, and the counts its summary
//! shows. The smoke tests call `convert_export` directly, so a `run()` that
//! wrote nothing, or a report that dropped a skip count or a file's parse
//! error, passed them.

use crate::emit::{ConvertExportArgs, convert_export};
use message_vault_io_core::testutil::{assert_run_wrote_jsonl, jsonl_run_config};
use message_vault_io_core::{ExportTransforms, OutputFormat, SmsBackupRestoreConfig, SourceConfig};
use std::fs;
use std::path::Path;

/// One row for every reason a record is skipped, around two good messages.
const SKIPS_XML: &str = r#"<?xml version='1.0' encoding='UTF-8' standalone='yes' ?>
<smses count="8">
  <sms address="+15555550101" date="1400773261000" type="1" body="hello" contact_name="Sam" />
  <sms address="+15555550101" date="1400773321000" type="2" body="hey" contact_name="Sam" />
  <sms address="+15555550101" date="soon" type="1" body="bad date" />
  <sms address="" date="1400773500000" type="1" body="no address" />
  <sms address="+15555550101" date="1400773600000" type="9" body="unknown type" />
  <sms address="+15555550101" date="1400773700000" type="3" body="draft" />
  <mms date="1400773800000" msg_box="1">
    <parts><part seq="0" ct="text/plain" text="nobody" /></parts>
  </mms>
  <mms date="1400773900000" msg_box="1" address="+15555550101" contact_name="Sam">
    <parts>
      <part seq="0" ct="text/plain" text="broken photo" />
      <part seq="1" ct="image/jpeg" name="pic.jpg" data="%%% not base64 %%%" />
    </parts>
    <addrs><addr address="+15555550101" type="137" charset="106" /></addrs>
  </mms>
</smses>
"#;

/// A backup folder holding `SKIPS_XML` and a file cut off mid-element.
fn backup_folder(root: &Path) -> std::path::PathBuf {
    let input = root.join("backup");
    fs::create_dir_all(&input).unwrap();
    fs::write(input.join("sms-1.xml"), SKIPS_XML).unwrap();
    fs::write(
        input.join("sms-2-broken.xml"),
        "<smses count=\"1\">\n  <sms address=\"+15555550102\" date=\"1\" body=\"cut",
    )
    .unwrap();
    input
}

#[test]
fn run_writes_the_conversation_and_reports_every_skip_and_error() {
    let tmp = tempfile::tempdir().unwrap();
    let input = backup_folder(tmp.path());
    let output = tmp.path().join("out");
    let config = jsonl_run_config(
        &[&input],
        &output,
        SourceConfig::SmsBackupRestore(SmsBackupRestoreConfig {
            owner_phones: vec!["+15555550100".into()],
        }),
    );

    let result = crate::run(&config).expect("run");

    let written = assert_run_wrote_jsonl(&result, &output, 1);
    assert!(written.contains("\"hello\""), "{written}");
    assert!(written.contains("\"hey\""), "{written}");
    assert!(written.contains("broken photo"), "{written}");
    for line in [
        "  skipped 1 invalid-date rows",
        "  mms_seen: 2",
        "  skipped_bad_attachment: 1",
        "  skipped_draft_or_outbox: 1",
        "  skipped_empty_participants: 1",
        "  skipped_unknown_address: 1",
        "  skipped_unknown_type: 1",
        "  sms_seen: 6",
    ] {
        assert!(
            result.messages.iter().any(|l| l == line),
            "{line:?} missing from {:?}",
            result.messages
        );
    }
    let errors: Vec<&String> = result
        .messages
        .iter()
        .filter(|l| l.starts_with("  error: "))
        .collect();
    assert_eq!(errors.len(), 1, "{:?}", result.messages);
    assert!(errors[0].contains("sms-2-broken.xml"), "{}", errors[0]);
}

#[test]
fn the_report_counts_conversations_and_directions() {
    let tmp = tempfile::tempdir().unwrap();
    let input = backup_folder(tmp.path());
    let report = convert_export(ConvertExportArgs {
        input: &input,
        output_dir: &tmp.path().join("out"),
        owner_phones: &["+15555550100".into()],
        transforms: ExportTransforms::none(),
        output_format: OutputFormat::Jsonl,
        cancel: None,
        resume: false,
    })
    .expect("convert");

    assert_eq!(report.conversations, 1);
    assert_eq!(report.sent, 1);
    assert_eq!(report.received, 2);
    assert_eq!(report.skipped_invalid_date, 1);
    assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
}
