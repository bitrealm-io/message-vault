//! `run()`, the entry point the desktop app calls, and the counts its summary
//! shows. The smoke tests call `convert_export` directly, so a `run()` that
//! wrote nothing, or dropped its summary, passed them.

use message_crate_core::testutil::{assert_run_wrote_jsonl, jsonl_run_config};
use message_crate_core::{SmsBackupPlusConfig, SourceConfig};
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

/// A call-log mail SMS Backup+ wrote into its "Call log" label.
const CALL_LOG_EML: &str = "From: alice@unknown.email\n\
     To: me@example.com\n\
     Subject: Call with Alice\n\
     X-smssync-datatype: CALLLOG\n\
     X-smssync-type: 1\n\
     X-smssync-duration: 123\n\
     X-smssync-address: 4075551234\n\
     X-smssync-date: 1609459300000\n\
     Content-Type: text/plain; charset=utf-8\n\
     \n\
     123s (00:02:03)\n\
     4075551234 (incoming call)\n";

/// SMS Backup+ has no text in a call-log mail, only the call's length and
/// number, and Message Crate has no model for a call.
#[test]
fn a_call_log_mail_is_skipped_and_counted_in_the_summary() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("backup");
    fs::create_dir_all(&input).unwrap();
    fs::write(
        input.join("1.eml"),
        eml("1609459200000", "Hello from Alice"),
    )
    .unwrap();
    fs::write(input.join("2.eml"), CALL_LOG_EML).unwrap();
    let output = tmp.path().join("out");

    let result = crate::run(&jsonl_run_config(&[&input], &output, source(true))).expect("run");

    let written = assert_run_wrote_jsonl(&result, &output, 1);
    assert!(written.contains("Hello from Alice"), "{written}");
    assert!(!written.contains("123s"), "{written}");
    assert!(
        result.messages.iter().any(|l| l == "  skipped_call_log: 1"),
        "{:?}",
        result.messages
    );
}

/// A received group MMS whose `From` names nobody in the group is written
/// with no sender, and the summary and the Import Run's issues count it once,
/// even when the archive holds two copies of the mail.
#[test]
fn a_group_message_with_no_readable_sender_is_kept_and_counted_once() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("backup");
    fs::create_dir_all(&input).unwrap();
    let mail = "From: Bob <bob@example.org>\n\
         To: me@example.com\n\
         Subject: SMS with group\n\
         X-smssync-type: 132\n\
         X-smssync-address: 4075551111~4075555678\n\
         X-smssync-date: 1609459200000\n\
         Content-Type: text/plain; charset=utf-8\n\
         \n\
         Hello group\n";
    fs::write(input.join("1.eml"), mail).unwrap();
    fs::write(input.join("2.eml"), mail).unwrap();
    let output = tmp.path().join("out");

    let result = crate::run(&jsonl_run_config(&[&input], &output, source(true))).expect("run");

    let written = assert_run_wrote_jsonl(&result, &output, 1);
    assert!(written.contains("Hello group"), "{written}");
    assert!(
        result
            .messages
            .iter()
            .any(|l| l == "  group_messages_without_sender: 1"),
        "{:?}",
        result.messages
    );
    assert_eq!(result.issues.len(), 1, "{:?}", result.issues);
    let issue = &result.issues[0];
    assert_eq!(
        (
            issue.kind.as_str(),
            issue.step.as_str(),
            issue.item.as_str()
        ),
        ("skip", "parse", "1.eml (sender)")
    );
    assert!(issue.reason.contains("left out"), "{}", issue.reason);
    assert!(issue.reason.contains("kept"), "{}", issue.reason);
}

/// A received group MMS whose `To` names none of the owner's addresses is
/// filed one-to-one under its `From`, and the summary counts it once, even
/// when the archive holds two copies of the mail. Its sender was read, so it
/// is not an issue.
#[test]
fn a_group_message_not_naming_the_owner_is_counted_once() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("backup");
    fs::create_dir_all(&input).unwrap();
    let mail = "From: carol@example.org\n\
         To: <me@icloud.example>, <+14075551111@unknown.email>\n\
         Subject: SMS with Alice\n\
         X-smssync-type: 132\n\
         X-smssync-address: 4075551111\n\
         X-smssync-date: 1609459200000\n\
         Content-Type: text/plain; charset=utf-8\n\
         \n\
         Hello from Carol\n";
    fs::write(input.join("1.eml"), mail).unwrap();
    fs::write(input.join("2.eml"), mail).unwrap();
    let output = tmp.path().join("out");

    let result = crate::run(&jsonl_run_config(&[&input], &output, source(true))).expect("run");

    let written = assert_run_wrote_jsonl(&result, &output, 1);
    assert!(written.contains("Hello from Carol"), "{written}");
    assert!(
        result
            .messages
            .iter()
            .any(|l| l == "  group_messages_owner_not_named: 1"),
        "{:?}",
        result.messages
    );
    assert!(result.issues.is_empty(), "{:?}", result.issues);
}
