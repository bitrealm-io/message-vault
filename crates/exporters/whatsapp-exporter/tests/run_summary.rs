//! `run()`, the entry point the desktop app calls, and the counts its summary
//! shows. The smoke tests call `convert_json` directly, so a `run()` that
//! wrote nothing, or a report that dropped the bad-date count, passed them.

use message_vault_io_core::testutil::{assert_run_wrote_jsonl, jsonl_run_config};
use message_vault_io_core::{SourceConfig, WhatsappConfig};
use std::fs;

/// A message with the given `timestamp` JSON value.
fn message(key: &str, timestamp: &str, text: &str) -> String {
    format!(
        r#""{key}": {{
        "from_me": false, "timestamp": {timestamp}, "time": "00:00", "key_id": "{key}",
        "data": "{text}", "sender": null, "media": false, "mime": null,
        "caption": null, "sticker": false, "reply": null, "reactions": {{}}
      }}"#
    )
}

#[test]
fn run_writes_the_conversation_and_counts_the_bad_date_rows() {
    let tmp = tempfile::tempdir().unwrap();
    let json = tmp.path().join("result.json");
    // One message without a timestamp, and one whose timestamp no calendar
    // can show.
    let messages = [
        message("AAA", "1609459200", "Hello from Sam"),
        message("BBB", "null", "No time"),
        message("CCC", "1e300", "Far future"),
    ]
    .join(",\n");
    fs::write(
        &json,
        format!(
            r#"{{ "15555550122@s.whatsapp.net": {{
    "name": "Sam Example", "type": "ANDROID",
    "messages": {{ {messages} }}
  }} }}"#
        ),
    )
    .unwrap();
    let output = tmp.path().join("out");
    let config = jsonl_run_config(
        &[],
        &output,
        SourceConfig::Whatsapp(WhatsappConfig {
            json: Some(json),
            ..WhatsappConfig::default()
        }),
    );

    let result = crate::run(&config).expect("run");

    let written = assert_run_wrote_jsonl(&result, &output, 1);
    assert!(written.contains("Hello from Sam"), "{written}");
    assert!(!written.contains("No time"), "{written}");
    assert!(!written.contains("Far future"), "{written}");
    assert!(
        result
            .messages
            .iter()
            .any(|l| l == "  skipped 2 invalid-date rows"),
        "{:?}",
        result.messages
    );
    // No owner was given, so the header records none.
    assert!(written.contains(r#""owner_handle":null"#), "{written}");
}

/// The number from the form is stamped on the export header, under the
/// vault's handle key, so every message is held at it (ADR 0015). It is a
/// header value, not a participant: the holder is never listed there.
#[test]
fn run_records_the_form_owner_on_the_header() {
    let tmp = tempfile::tempdir().unwrap();
    let json = tmp.path().join("result.json");
    fs::write(
        &json,
        format!(
            r#"{{ "15555550122@s.whatsapp.net": {{
    "name": "Sam Example", "type": "ANDROID",
    "messages": {{ {} }}
  }} }}"#,
            message("AAA", "1609459200", "Hello from Sam")
        ),
    )
    .unwrap();
    let output = tmp.path().join("out");
    let config = jsonl_run_config(
        &[],
        &output,
        SourceConfig::Whatsapp(WhatsappConfig {
            json: Some(json),
            owner_phone: Some("+1 555 555 0100".into()),
            ..WhatsappConfig::default()
        }),
    );

    let result = crate::run(&config).expect("run");

    let written = assert_run_wrote_jsonl(&result, &output, 1);
    assert!(
        written.contains(r#""owner_handle":"+15555550100""#),
        "{written}"
    );
    let header = written.lines().next().unwrap();
    assert!(!header.contains(r#""handle":"+15555550100""#), "{header}");
}
