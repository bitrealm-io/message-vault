//! The `tests/fixtures/mms_export` backup, converted by `convert_export`:
//!
//! - `I_1609459300_recv_img.pdu`: received from the peer, a text part and a
//!   picture part.
//! - `I_1609459400_sent.pdu`: sent by the owner to the peer, text only.
//! - `I_1609459500_stub.pdu`: sixteen zero bytes, the hollow stub GO SMS Pro
//!   leaves behind.
//! - `I_1609459600_bare.pdu`: the peer's number and nothing else, a message
//!   with a party but no text.
//! - `I_1609459700_owner_only.pdu`: the owner's number and nothing else, a
//!   message with nobody to file it under.
//! - `notes.txt` and `old.pdu`: files beside the backup that are not part
//!   of it.
//!
//! The smoke test's one received PDU carries no picture, names no sender and
//! has no stub or stray file beside it, so an exporter that dropped every
//! attachment, wrote an empty sender handle, kept hollow PDUs as empty
//! messages, or read every file as input passed it.
//!
//! The PDUs are synthetic: `+14075551234` is the peer, `+15555550100` the
//! owner, and the picture is a JPEG magic followed by filler.

use crate::emit::{ConvertExportArgs, convert_export};
use message_vault_io_core::testutil::{csv_files, csv_rows};
use message_vault_io_core::{ExportReport, ExportTransforms, OutputFormat};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const PEER: &str = "+14075551234";

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mms_export")
}

fn convert(input_dir: &Path, output_dir: &Path) -> ExportReport {
    convert_export(ConvertExportArgs {
        input_dir,
        output_dir,
        owner_phones: &["+15555550100".into()],
        transforms: ExportTransforms::none(),
        output_format: OutputFormat::Csv,
        cancel: None,
        resume: false,
    })
    .expect("convert_export")
}

/// Every row of the one conversation the export holds, keyed by text. The
/// conversation is named by its chat identifier; the `skipped_*.csv` files
/// beside it are diagnostics, not conversations.
fn rows_by_text(output_dir: &Path) -> BTreeMap<String, BTreeMap<String, String>> {
    let conversations: Vec<PathBuf> = csv_files(output_dir)
        .into_iter()
        .filter(|p| {
            !p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("skipped_")
        })
        .collect();
    assert_eq!(
        conversations,
        [output_dir.join(format!("{PEER}.csv"))],
        "one conversation, the peer's"
    );
    csv_rows(&conversations[0])
        .into_iter()
        .map(|row| (row["text"].clone(), row))
        .collect()
}

#[test]
fn a_pdu_picture_is_one_attachment_with_its_bytes_staged() {
    let tmp = tempfile::tempdir().unwrap();
    let report = convert(&fixture(), tmp.path());

    assert_eq!(report.attachments_saved, 1, "{report:?}");
    let rows = rows_by_text(tmp.path());
    let row = &rows["Look at this"];
    let attachments: Vec<serde_json::Value> =
        serde_json::from_str(&row["attachments_json"]).expect("attachments_json");
    assert_eq!(attachments.len(), 1, "{attachments:?}");
    let att = &attachments[0];
    assert_eq!(att["mime_type"], "image/jpeg");
    assert_eq!(att["original_name"], "IMG_1.jpg");
    let path = att["path"].as_str().expect("path");
    assert!(
        path.starts_with("attachments/") && path.ends_with(".jpg"),
        "{path}"
    );

    // The bytes are on disk at the path the row names, and they are the
    // picture from the PDU.
    let staged: Vec<PathBuf> = fs::read_dir(tmp.path().join("attachments"))
        .expect("attachments dir")
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(staged, [tmp.path().join(path)]);
    let bytes = fs::read(&staged[0]).unwrap();
    assert_eq!(bytes.len(), 200);
    assert_eq!(&bytes[..4], b"\xff\xd8\xff\xe0");
    assert_eq!(&bytes[198..], b"\xff\xd9");

    // The message without a picture has no attachment row.
    assert_eq!(rows["Sent from me"]["attachments_json"], "[]");
}

#[test]
fn incoming_pdu_names_its_sender_and_a_sent_pdu_names_the_owner() {
    let tmp = tempfile::tempdir().unwrap();
    convert(&fixture(), tmp.path());

    let rows = rows_by_text(tmp.path());
    let received = &rows["Look at this"];
    assert_eq!(received["direction"], "incoming");
    assert_eq!(received["sender_handle"], PEER);
    assert_eq!(received["chat_identifier"], PEER);
    assert_eq!(received["timestamp_unix_ms"], "1609459300000");
    // The vendor bag names the PDU file and how well it decoded, and nothing
    // else: the PDU has no Subject or other optional header to carry.
    assert_eq!(
        received["source_fields_json"],
        r#"{"pdu_decode":"structured","pdu_filename":"I_1609459300_recv_img.pdu","source_kind":"pdu"}"#
    );

    let sent = &rows["Sent from me"];
    assert_eq!(sent["direction"], "outgoing");
    assert_eq!(sent["sender_handle"], "+15555550100");
    assert_eq!(sent["chat_identifier"], PEER);
}

#[test]
fn a_stub_pdu_is_counted_as_skipped_and_writes_no_message() {
    let tmp = tempfile::tempdir().unwrap();
    let report = convert(&fixture(), tmp.path());

    assert_eq!(report.extra("skipped_empty_pdu"), 1, "{report:?}");
    assert_eq!(report.extra("pdu_messages"), 3, "{report:?}");
    let rows = rows_by_text(tmp.path());
    // The one empty-text row is the bare PDU, which names a party and so is
    // a message; the stub names nobody and is not.
    assert_eq!(
        rows.keys().collect::<Vec<_>>(),
        ["", "Look at this", "Sent from me"]
    );
    let bare = &rows[""];
    assert_eq!(bare["timestamp_unix_ms"], "1609459600000");
    assert_eq!(bare["direction"], "incoming");
    assert_eq!(bare["sender_handle"], PEER);
    assert_eq!(bare["attachments_json"], "[]");
    let skipped = fs::read_to_string(tmp.path().join("skipped_empty_pdu.csv")).unwrap();
    assert_eq!(
        skipped.lines().collect::<Vec<_>>(),
        ["pdu_filename", "I_1609459500_stub.pdu"]
    );
}

#[test]
fn a_pdu_naming_only_the_owner_is_skipped_and_listed() {
    let tmp = tempfile::tempdir().unwrap();
    let report = convert(&fixture(), tmp.path());

    assert_eq!(report.extra("skipped_no_other_party"), 1, "{report:?}");
    let skipped = fs::read_to_string(tmp.path().join("skipped_no_party.csv")).unwrap();
    assert_eq!(
        skipped.lines().collect::<Vec<_>>(),
        [
            "pdu_filename,participants,is_sent,has_from,has_to",
            "I_1609459700_owner_only.pdu,5555550100,1,0,0"
        ]
    );
}

#[test]
fn only_xml_and_i_prefixed_pdu_files_are_read() {
    // `notes.txt` and `old.pdu` sit beside the backup. Reading either as XML
    // records a parse error; reading either as a PDU counts it unparseable.
    let tmp = tempfile::tempdir().unwrap();
    let report = convert(&fixture(), tmp.path());

    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(report.extra("skipped_unparseable_pdu"), 0, "{report:?}");
    assert_eq!(report.extra("xml_messages_seen"), 0, "{report:?}");
}

#[test]
fn a_voicemail_notice_is_attributed_to_the_caller() {
    // A Google Voice voicemail SMS arrives from the service, not from the
    // caller, so its address is unusable; the body names who called.
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("backup");
    fs::create_dir_all(&input).unwrap();
    fs::write(
        input.join("gosms_sys_1.xml"),
        r#"<?xml version="1.0"?>
<GoSms>
  <SMSCount>2</SMSCount>
  <SMS>
    <address>Google Voice</address>
    <date>1609459200000</date>
    <type>1</type>
    <body>You've got a new voicemail from (407) 555-1234. Call to listen.</body>
  </SMS>
  <SMS>
    <address>+14075551234</address>
    <contactName>Alice</contactName>
    <date>1609459260000</date>
    <type>1</type>
    <body>plain incoming</body>
  </SMS>
</GoSms>
"#,
    )
    .unwrap();
    let output = tmp.path().join("out");
    let report = convert(&input, &output);

    assert_eq!(report.extra("skipped_unknown_address"), 0, "{report:?}");
    let rows = rows_by_text(&output);
    let voicemail = &rows["You've got a new voicemail from (407) 555-1234. Call to listen."];
    assert_eq!(voicemail["direction"], "incoming");
    assert_eq!(voicemail["sender_handle"], PEER);
    assert_eq!(voicemail["chat_identifier"], PEER);
    // An ordinary incoming SMS names its sender the same way.
    let plain = &rows["plain incoming"];
    assert_eq!(plain["sender_handle"], PEER);
    assert_eq!(plain["sender_display_name"], "Alice");
}
