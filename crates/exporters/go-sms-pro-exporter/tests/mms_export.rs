//! The MMS half of a backup, converted by `convert_export`. Every PDU is
//! built by `go_sms_mms::testutil` the way a phone writes one, so the test
//! input has the shape a real backup has: a received message is an
//! `m-retrieve-conf` with From and To, a sent one an `m-send-req` in an
//! `S_` file, and a message never downloaded is a 17-byte stub.
//!
//! The peer is `+14075551234` and the owner `+15555550100`; the picture is
//! a JPEG magic followed by filler.

use crate::emit::{ConvertExportArgs, convert_export};
use go_sms_mms::testutil::{BuildPart, PduBuilder};
use message_vault_io_core::testutil::{csv_files, csv_rows};
use message_vault_io_core::{ExportReport, ExportTransforms, OutputFormat};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const PEER: &str = "+14075551234";
const OWNER: &str = "+15555550100";

/// The backup folder: five PDUs and two stray files.
fn write_backup(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    let received = PduBuilder::received(PEER)
        .to(OWNER)
        .date(Some(1_609_459_300))
        .text("Look at this")
        .part(BuildPart::jpeg("IMG_1.jpg", 200))
        .build();
    fs::write(dir.join("I_1609459300_1_0.pdu"), received).unwrap();
    let sent = PduBuilder::sent()
        .to(PEER)
        .date(Some(1_609_459_400))
        .text("Sent from me")
        .build();
    fs::write(dir.join("S_1609459400_1_0.pdu"), sent).unwrap();
    fs::write(dir.join("I_1609459500_1_0.pdu"), b"application/smil\0").unwrap();
    // A picture with no words: a message with a party but no text.
    let bare = PduBuilder::received(PEER)
        .to(OWNER)
        .date(Some(1_609_459_600))
        .part(BuildPart::jpeg("IMG_2.jpg", 100))
        .build();
    fs::write(dir.join("I_1609459600_1_0.pdu"), bare).unwrap();
    // From the owner to the owner: nobody to file it under.
    let owner_only = PduBuilder::received(OWNER)
        .to(OWNER)
        .date(Some(1_609_459_700))
        .text("note to self")
        .build();
    fs::write(dir.join("I_1609459700_1_0.pdu"), owner_only).unwrap();
    fs::write(dir.join("notes.txt"), "not part of the backup").unwrap();
    fs::write(dir.join("old.pdu"), [0u8; 16]).unwrap();
}

fn convert(input_dir: &Path, output_dir: &Path) -> ExportReport {
    convert_export(ConvertExportArgs {
        input_dir,
        output_dir,
        owner_phones: &[OWNER.into()],
        transforms: ExportTransforms::none(),
        output_format: OutputFormat::Csv,
        cancel: None,
        resume: false,
    })
    .expect("convert_export")
}

/// Convert the standard backup; returns the temp dir (kept alive), the
/// output dir, and the report.
fn convert_backup() -> (tempfile::TempDir, PathBuf, ExportReport) {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("backup");
    write_backup(&input);
    let output = tmp.path().join("out");
    let report = convert(&input, &output);
    (tmp, output, report)
}

/// Every row of the one conversation the export holds, keyed by text. The
/// `skipped_*.csv` files beside it are diagnostics, not conversations.
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
    let (_tmp, output, report) = convert_backup();

    assert_eq!(report.attachments_saved, 2, "{report:?}");
    let rows = rows_by_text(&output);
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

    // The bytes on disk are the picture from the PDU, whole.
    let bytes = fs::read(output.join(path)).unwrap();
    assert_eq!(bytes.len(), 200);
    assert_eq!(&bytes[..4], b"\xff\xd8\xff\xe0");
    assert!(bytes[10..].iter().all(|b| *b == 0x11));

    // The message without a picture has no attachment row.
    assert_eq!(rows["Sent from me"]["attachments_json"], "[]");
}

#[test]
fn a_received_pdu_names_its_sender_and_a_sent_pdu_names_the_owner() {
    let (_tmp, output, _) = convert_backup();

    let rows = rows_by_text(&output);
    let received = &rows["Look at this"];
    assert_eq!(received["direction"], "incoming");
    assert_eq!(received["sender_handle"], PEER);
    assert_eq!(received["chat_identifier"], PEER);
    assert_eq!(received["timestamp_unix_ms"], "1609459300000");
    assert_eq!(received["message_kind"], "mms");
    // The vendor bag names the PDU file and carries its headers.
    let bag: serde_json::Value = serde_json::from_str(&received["source_fields_json"]).unwrap();
    assert_eq!(bag["pdu_filename"], "I_1609459300_1_0.pdu");
    assert_eq!(bag["source_kind"], "pdu");
    assert_eq!(bag["pdu_fields"]["message-type"], "m-retrieve-conf");
    assert_eq!(bag["pdu_fields"]["message-id"], "MSG-1");
    assert_eq!(bag["pdu_fields"]["priority"], "Normal");

    let sent = &rows["Sent from me"];
    assert_eq!(sent["direction"], "outgoing");
    assert_eq!(sent["sender_handle"], OWNER);
    assert_eq!(sent["chat_identifier"], PEER);
    assert_eq!(sent["timestamp_unix_ms"], "1609459400000");
}

#[test]
fn a_stub_pdu_is_counted_as_skipped_and_writes_no_message() {
    let (_tmp, output, report) = convert_backup();

    assert_eq!(report.extra("skipped_empty_pdu"), 1, "{report:?}");
    assert_eq!(report.extra("pdu_messages"), 3, "{report:?}");
    let rows = rows_by_text(&output);
    // The one empty-text row is the picture with no words; the stub is not
    // a message.
    assert_eq!(
        rows.keys().collect::<Vec<_>>(),
        ["", "Look at this", "Sent from me"]
    );
    let bare = &rows[""];
    assert_eq!(bare["timestamp_unix_ms"], "1609459600000");
    assert_eq!(bare["direction"], "incoming");
    assert_eq!(bare["sender_handle"], PEER);
    assert_ne!(bare["attachments_json"], "[]");
    let skipped = fs::read_to_string(output.join("skipped_empty_pdu.csv")).unwrap();
    assert_eq!(
        skipped.lines().collect::<Vec<_>>(),
        ["pdu_filename", "I_1609459500_1_0.pdu"]
    );
}

#[test]
fn a_pdu_naming_only_the_owner_is_skipped_and_listed() {
    let (_tmp, output, report) = convert_backup();

    assert_eq!(report.extra("skipped_no_other_party"), 1, "{report:?}");
    let skipped = fs::read_to_string(output.join("skipped_no_party.csv")).unwrap();
    assert_eq!(
        skipped.lines().collect::<Vec<_>>(),
        [
            "pdu_filename,sender,recipients,is_sent",
            "I_1609459700_1_0.pdu,5555550100,5555550100,0"
        ]
    );
}

#[test]
fn only_xml_and_prefixed_pdu_files_are_read() {
    // `notes.txt` and `old.pdu` sit beside the backup. Reading either as XML
    // records a parse error; reading `old.pdu` as a PDU counts it unparseable.
    let (_tmp, _, report) = convert_backup();

    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(report.extra("skipped_unparseable_pdu"), 0, "{report:?}");
    assert_eq!(report.extra("xml_messages_seen"), 0, "{report:?}");
}

#[test]
fn a_sent_pdu_to_several_people_is_a_group_conversation() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("backup");
    fs::create_dir_all(&input).unwrap();
    let group = PduBuilder::sent()
        .to(PEER)
        .to("+14075559876")
        .to(OWNER)
        .text("hello all")
        .build();
    fs::write(input.join("S_1609459200_1_0.pdu"), group).unwrap();
    let output = tmp.path().join("out");
    let report = convert(&input, &output);

    assert_eq!(report.extra("pdu_group_messages"), 1, "{report:?}");
    let files = csv_files(&output);
    assert_eq!(files.len(), 1, "{files:?}");
    let name = files[0].file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(name, "group_+14075551234_+14075559876.csv");
    let rows = csv_rows(&files[0]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["direction"], "outgoing");
    assert_eq!(rows[0]["conversation_type"], "group");
    // The owner is on the To list but is not a member of the group.
    assert!(
        !rows[0]["participants_json"].contains("5555550100"),
        "{}",
        rows[0]["participants_json"]
    );
}

#[test]
fn a_pdu_that_breaks_the_mms_rules_is_counted_and_named() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("backup");
    fs::create_dir_all(&input).unwrap();
    // A message type with an unknown header code right after it.
    fs::write(input.join("I_1609459200_1_0.pdu"), [0x8c, 0x84, 0xff, 0x00]).unwrap();
    let output = tmp.path().join("out");
    let report = convert(&input, &output);

    assert_eq!(report.extra("skipped_unparseable_pdu"), 1, "{report:?}");
    assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
    assert!(
        report.errors[0].ends_with(
            "I_1609459200_1_0.pdu: malformed PDU: expected unknown header field code at byte 2"
        ),
        "{}",
        report.errors[0]
    );
}
