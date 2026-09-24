//! `run()`, the entry point the desktop app calls, and the counts its summary
//! shows. The smoke tests call `convert_export` directly, so a `run()` that
//! wrote nothing, or a report that dropped a skip count or a file's parse
//! error, passed them.

use message_vault_io_core::testutil::{assert_run_wrote_jsonl, jsonl_run_config};
use message_vault_io_core::{GoSmsProConfig, SourceConfig};
use std::fs;

/// One row for every reason an SMS is skipped, around two good messages.
const SKIPS_XML: &str = r#"<?xml version="1.0"?>
<GoSms>
  <SMSCount>6</SMSCount>
  <SMS>
    <address>+14075551234</address>
    <contactName>Alice</contactName>
    <date>1609459200000</date>
    <type>1</type>
    <body>hello</body>
  </SMS>
  <SMS>
    <address>+14075551234</address>
    <contactName>Alice</contactName>
    <date>1609459260000</date>
    <type>2</type>
    <body>reply</body>
  </SMS>
  <SMS>
    <address>+14075551234</address>
    <date>soon</date>
    <type>1</type>
    <body>bad date</body>
  </SMS>
  <SMS>
    <address>+14075551234</address>
    <type>1</type>
    <body>no date</body>
  </SMS>
  <SMS>
    <address>Carrier</address>
    <contactName>Nobody</contactName>
    <date>1609459300000</date>
    <type>1</type>
    <body>no address</body>
  </SMS>
  <SMS>
    <address>+14075551234</address>
    <date>1609459400000</date>
    <type>5</type>
    <body>unknown type</body>
  </SMS>
</GoSms>
"#;

#[test]
fn run_writes_the_conversation_and_reports_every_skip_and_error() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("backup");
    fs::create_dir_all(&input).unwrap();
    fs::write(input.join("gosms_sys_1.xml"), SKIPS_XML).unwrap();
    fs::write(
        input.join("gosms_sys_2_broken.xml"),
        "<GoSms><SMS><address>",
    )
    .unwrap();
    let output = tmp.path().join("out");
    let config = jsonl_run_config(
        &[&input],
        &output,
        SourceConfig::GoSmsPro(GoSmsProConfig {
            owner_phones: vec!["+15555550100".into()],
        }),
    );

    let result = crate::run(&config).expect("run");

    let written = assert_run_wrote_jsonl(&result, &output, 1);
    assert!(written.contains("\"hello\""), "{written}");
    assert!(written.contains("\"reply\""), "{written}");
    for line in [
        "  skipped 2 invalid-date rows",
        "  skipped_unknown_address: 1",
        "  skipped_unknown_type: 1",
        "  xml_messages_seen: 6",
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
    assert!(
        errors[0].contains("gosms_sys_2_broken.xml"),
        "{}",
        errors[0]
    );

    // The row dropped for its address is named, so the person can find it.
    let skipped = fs::read_to_string(output.join("skipped_invalid_address.csv"))
        .expect("skipped_invalid_address.csv");
    let rows: Vec<&str> = skipped.lines().collect();
    assert_eq!(rows.len(), 2, "{skipped}");
    assert_eq!(
        rows[1],
        "gosms_sys_1.xml,Carrier,Nobody,1,1609459300000,no address"
    );
}

#[test]
fn run_names_the_first_twenty_bad_address_rows_and_counts_the_rest() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("backup");
    fs::create_dir_all(&input).unwrap();
    let bad_rows: String = (0..22)
        .map(|i| {
            format!(
                "<SMS><address>Carrier</address><date>16094593{i:05}</date>\
                 <type>1</type><body>no address {i}</body></SMS>\n"
            )
        })
        .collect();
    fs::write(
        input.join("gosms_sys_1.xml"),
        format!(
            "<?xml version=\"1.0\"?>\n<GoSms>\n<SMS><address>+14075551234</address>\
             <date>1609459200000</date><type>1</type><body>hello</body></SMS>\n\
             {bad_rows}</GoSms>\n"
        ),
    )
    .unwrap();
    let output = tmp.path().join("out");
    let config = jsonl_run_config(
        &[&input],
        &output,
        SourceConfig::GoSmsPro(GoSmsProConfig {
            owner_phones: vec!["+15555550100".into()],
        }),
    );

    let result = crate::run(&config).expect("run");

    assert!(
        result
            .messages
            .iter()
            .any(|l| l == "  skipped_unknown_address: 22"),
        "{:?}",
        result.messages
    );
    let skipped = fs::read_to_string(output.join("skipped_invalid_address.csv")).unwrap();
    let rows: Vec<&str> = skipped.lines().collect();
    // The header, twenty named rows, and one line for the rest.
    assert_eq!(rows.len(), 22, "{skipped}");
    assert!(rows[20].ends_with(",no address 19"), "{}", rows[20]);
    assert_eq!(rows[21], ",,,,,...and 2 more entries not shown");
}
