//! `run()`, the entry point the desktop app calls, and the counts its summary
//! shows. The smoke tests call `convert_export` directly, so a `run()` that
//! wrote nothing, or a report that dropped the bad-date count, passed them.

use message_vault_io_core::testutil::{assert_run_wrote_jsonl, jsonl_run_config};
use message_vault_io_core::{ImazingConfig, SourceConfig};
use std::fs;

const MESSAGES_CSV: &str = "\
Chat Session,Message Date,Delivered Date,Read Date,Edited Date,Deleted Date,Service,Type,Sender ID,Sender Name,Status,Replying to,Subject,Text,Reactions,Attachment,Attachment type
Bob McRoy,2020-01-01 12:00:00,,,,,SMS,Incoming,+13212462167,Bob McRoy,Read,,,Hello from Bob,,,
Bob McRoy,sometime,,,,,SMS,Incoming,+13212462167,Bob McRoy,Read,,,Bad date,,,
Bob McRoy,2020-01-01 12:01:00,,,,,SMS,Outgoing,,,Read,,,Hi Bob,,,
";

#[test]
fn run_writes_the_conversation_and_counts_the_bad_date_row() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("messages.csv");
    fs::write(&input, MESSAGES_CSV).unwrap();
    let output = tmp.path().join("out");
    let mut config = jsonl_run_config(&[&input], &output, SourceConfig::Imazing(ImazingConfig {}));
    config.timezone = Some("UTC".into());

    let result = crate::run(&config).expect("run");

    let written = assert_run_wrote_jsonl(&result, &output, 1);
    assert!(written.contains("Hello from Bob"), "{written}");
    assert!(written.contains("Hi Bob"), "{written}");
    assert!(!written.contains("Bad date"), "{written}");
    assert!(
        result
            .messages
            .iter()
            .any(|l| l == "  skipped 1 invalid-date rows"),
        "{:?}",
        result.messages
    );
}
