//! `run()`, the entry point the desktop app calls, and the counts its summary
//! shows. The smoke tests call `convert_export` directly, so a `run()` that
//! wrote nothing, or a report that dropped the bad-date count, passed them.

use message_vault_io_core::testutil::{assert_run_wrote_jsonl, jsonl_run_config};
use message_vault_io_core::{OpenExtractConfig, SourceConfig};
use std::fs;

const ALL_CONVERSATIONS_CSV: &str = "\
Date,Conversation,Direction,Sender,Text,Is From Me,Has Attachments
2020-01-01T17:00:00+00:00,Sam Example,Received,+15555550122,Hello from Sam,False,False
yesterday,Sam Example,Received,+15555550122,Bad date,False,False
2020-01-01T17:01:00+00:00,Sam Example,Sent,me,Hi Sam,True,False
";

#[test]
fn run_writes_the_conversation_and_counts_the_bad_date_row() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("all_conversations.csv");
    fs::write(&input, ALL_CONVERSATIONS_CSV).unwrap();
    let output = tmp.path().join("out");
    let config = jsonl_run_config(
        &[&input],
        &output,
        SourceConfig::OpenExtract(OpenExtractConfig {}),
    );

    let result = crate::run(&config).expect("run");

    let written = assert_run_wrote_jsonl(&result, &output, 1);
    assert!(written.contains("Hello from Sam"), "{written}");
    assert!(written.contains("Hi Sam"), "{written}");
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
