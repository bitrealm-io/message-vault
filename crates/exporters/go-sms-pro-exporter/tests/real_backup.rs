//! The exporter over a real GO SMS Pro backup, when one is on the machine.
//!
//! Run by hand with the backup folder and the owner's number:
//!
//! ```text
//! GO_SMS_PRO_BACKUP=/path/to/backup GO_SMS_PRO_OWNER=+15555550100 \
//!   cargo test -p go-sms-pro-exporter real_backup -- --ignored --nocapture
//! ```
//!
//! It asserts what only real data can show: every PDU is a message or a
//! stub, nothing is unparseable, and every conversation's members are
//! phone numbers, not digit runs scraped from image bytes. Set
//! `GO_SMS_PRO_OUT` to keep the export in that folder for a closer look.

use crate::emit::{ConvertExportArgs, convert_export};
use message_vault_io_core::testutil::{csv_files, csv_rows};
use message_vault_io_core::{ExportTransforms, OutputFormat};
use std::path::PathBuf;

#[test]
#[ignore = "needs GO_SMS_PRO_BACKUP and GO_SMS_PRO_OWNER"]
fn real_backup_exports_clean_conversations() {
    let input = PathBuf::from(std::env::var("GO_SMS_PRO_BACKUP").unwrap());
    let owner = std::env::var("GO_SMS_PRO_OWNER").unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let output =
        std::env::var("GO_SMS_PRO_OUT").map_or_else(|_| tmp.path().to_path_buf(), PathBuf::from);
    let report = convert_export(ConvertExportArgs {
        input_dir: &input,
        output_dir: &output,
        owner_phones: &[owner],
        transforms: ExportTransforms::none(),
        output_format: OutputFormat::Csv,
        cancel: None,
        resume: false,
    })
    .unwrap();
    println!("{report:#?}");
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(report.extra("skipped_unparseable_pdu"), 0);

    for file in csv_files(&output) {
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        if name.starts_with("skipped_") {
            continue;
        }
        for row in csv_rows(&file) {
            let bag: serde_json::Value = serde_json::from_str(&row["source_fields_json"]).unwrap();
            if bag["source_kind"] != "pdu" {
                // An SMS may come from a short code; only MMS addresses are
                // full numbers.
                continue;
            }
            let members: Vec<serde_json::Value> =
                serde_json::from_str(&row["participants_json"]).unwrap();
            for m in members {
                let handle = m["handle"].as_str().unwrap();
                let digits = handle.trim_start_matches('+');
                assert!(
                    (10..=11).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit()),
                    "{name}: {handle} is not a phone number"
                );
            }
        }
    }
}
