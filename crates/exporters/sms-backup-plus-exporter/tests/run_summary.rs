//! `run()`, the entry point the desktop app calls, and the counts its summary
//! shows. The smoke tests call `convert_export` directly, so a `run()` that
//! wrote nothing, or dropped its summary, passed them.

use message_vault_io_core::testutil::{assert_run_wrote_jsonl, jsonl_run_config};
use message_vault_io_core::{SmsBackupPlusConfig, SourceConfig};
use std::fs;
use std::path::Path;

/// A flat SMS Backup+ message from Alice sent at `date_ms`.
fn eml(date_ms: &str, text: &str) -> String {
    format!(
        "From: alice@unknown.email\n\
         To: me@example.com\n\
         Subject: SMS with Alice\n\
         X-smssync-type: 1\n\
         X-smssync-address: 4075551234\n\
         X-smssync-date: {date_ms}\n\
         Content-Type: text/plain; charset=utf-8\n\
         \n\
         {text}\n"
    )
}

/// A folder with one message and one dated past any calendar.
fn backup_folder(root: &Path) -> std::path::PathBuf {
    let input = root.join("backup");
    fs::create_dir_all(&input).unwrap();
    fs::write(
        input.join("1.eml"),
        eml("1609459200000", "Hello from Alice"),
    )
    .unwrap();
    fs::write(input.join("2.eml"), eml("99999999999999999", "Far future")).unwrap();
    input
}

fn source(include_summary: bool) -> SourceConfig {
    SourceConfig::SmsBackupPlus(SmsBackupPlusConfig {
        owner_phones: vec!["+15555550100".into()],
        owner_emails: vec!["me@example.com".into()],
        verbose: false,
        include_summary,
    })
}

#[test]
fn run_writes_the_conversation_and_counts_the_bad_date_row() {
    let tmp = tempfile::tempdir().unwrap();
    let input = backup_folder(tmp.path());
    let output = tmp.path().join("out");

    let result = crate::run(&jsonl_run_config(&[&input], &output, source(true))).expect("run");

    let written = assert_run_wrote_jsonl(&result, &output, 1);
    assert!(written.contains("Hello from Alice"), "{written}");
    assert!(!written.contains("Far future"), "{written}");
    assert!(
        result
            .messages
            .iter()
            .any(|l| l == "  skipped 1 invalid-date rows"),
        "{:?}",
        result.messages
    );
}

#[test]
fn run_without_a_summary_still_writes_the_export() {
    let tmp = tempfile::tempdir().unwrap();
    let input = backup_folder(tmp.path());
    let output = tmp.path().join("out");

    let result = crate::run(&jsonl_run_config(&[&input], &output, source(false))).expect("run");

    assert!(result.messages.is_empty(), "{:?}", result.messages);
    let files = fs::read_dir(&output)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
        .count();
    assert_eq!(files, 1);
}
