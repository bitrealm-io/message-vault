//! Round-trip tests for reading and writing conversation documents.

use super::*;
use mail::clean_previous_mail_output;
use message_ir::{
    ConversationDocument, IrDirection, IrImessage, IrMessage, IrMessageKind, IrService,
};
use message_vault_io_core::OutputFormat;
use serde_json::{Value, json};
use std::fs;

#[test]
fn writes_json_csv_jsonl_and_eml() {
    let tmp = tempfile::tempdir().unwrap();
    let doc = message_ir::testutil::sample_document("hello ir");

    let json_path = write_format(tmp.path(), OutputFormat::Json, doc.clone()).unwrap();
    assert!(json_path.ends_with("+15555550101.json"));
    let raw = fs::read_to_string(&json_path).unwrap();
    let parsed: ConversationDocument = serde_json::from_str(&raw).unwrap();
    assert_eq!(parsed.schema_version, 4);
    assert_eq!(parsed.messages[0].text, "hello ir");
    assert!(parsed.messages[0].attachments.is_empty());
    assert_eq!(
        parsed.messages[0].source.as_ref().unwrap().android_type,
        Some(1)
    );
    assert!(
        parsed.messages[0]
            .source
            .as_ref()
            .unwrap()
            .fields
            .contains_key("address")
    );
    assert_eq!(
        parsed.messages[0].sender_handle.as_deref(),
        Some("+15555550101")
    );
    assert_eq!(parsed.conversation.stats.message_count, 1);
    assert!(!raw.contains("filename_suffix"));
    assert!(!raw.contains("\"bytes\""));
    // Stable null keys present.
    assert!(raw.contains("\"group_title\": null") || raw.contains("\"group_title\":null"));
    assert!(raw.contains("\"imessage\": null") || raw.contains("\"imessage\":null"));

    let jsonl_path = write_format(tmp.path(), OutputFormat::Jsonl, doc.clone()).unwrap();
    assert!(jsonl_path.ends_with("+15555550101.jsonl"));
    let jsonl = fs::read_to_string(&jsonl_path).unwrap();
    let mut lines = jsonl.lines();
    let header: Value = serde_json::from_str(lines.next().unwrap()).unwrap();
    assert_eq!(header["schema_version"], 4);
    assert!(header.get("messages").is_none());
    assert_eq!(header["conversation"]["stats"]["message_count"], 1);
    let msg_line: Value = serde_json::from_str(lines.next().unwrap()).unwrap();
    assert_eq!(msg_line["text"], "hello ir");
    assert!(msg_line["source"]["fields"].is_object());
    assert_eq!(msg_line["source"]["android_type"], 1);

    let csv_path = write_format(tmp.path(), OutputFormat::Csv, doc.clone()).unwrap();
    let csv = fs::read_to_string(&csv_path).unwrap();
    assert!(csv.contains("hello ir"));
    assert!(csv.contains("sms-backup-restore"));
    assert!(csv.contains("source_fields_json"));
    assert!(csv.contains("timestamp_unix_ms"));
    assert!(csv.contains("+15555550100")); // owner handle filled

    let _ = clean_previous_mail_output(tmp.path());
    let eml_dir = write_format(tmp.path(), OutputFormat::Eml, doc).unwrap();
    assert!(eml_dir.is_dir());
}

#[test]
fn imessage_bag_restores_mail_extension_headers() {
    let tmp = tempfile::tempdir().unwrap();
    let doc = message_ir::testutil::sample_imessage_document();
    let mail_messages = document_to_mail_messages(&doc, tmp.path()).unwrap();

    let reply = &mail_messages[0];
    let reply_im = reply.message.imessage.as_ref().unwrap();
    assert!(reply_im.is_reply);
    assert_eq!(
        reply_im.in_reply_to_guid.as_deref(),
        Some("parent-guid-1111")
    );
    assert_eq!(reply_im.thread_originator_part, Some(0));
    assert_eq!(reply_im.num_replies, Some(2));
    assert_eq!(reply_im.send_effect.as_deref(), Some("Sent with Balloons"));
    assert!(
        reply_im
            .tapbacks
            .as_ref()
            .unwrap()
            .to_string()
            .contains("loved")
    );
    assert!(
        reply_im
            .parts
            .as_ref()
            .unwrap()
            .to_string()
            .contains("hello imessage")
    );
    assert_eq!(reply.owner_display_name.as_deref(), Some("Me"));

    let tapback = &mail_messages[1];
    let tapback_im = tapback.message.imessage.as_ref().unwrap();
    assert_eq!(
        tapback_im.associated_guid.as_deref(),
        Some("parent-guid-1111")
    );
    assert_eq!(tapback_im.associated_part, Some(0));
    assert_eq!(tapback_im.tapback_kind.as_deref(), Some("loved"));
    assert_eq!(tapback_im.tapback_action.as_deref(), Some("add"));
    assert_eq!(tapback.owner_display_name.as_deref(), Some("Me"));
    assert_eq!(
        tapback.message.sender_handle.as_deref(),
        Some("+15555550100")
    );
}

#[test]
fn unified_csv_headers_for_all_sources() {
    let tmp = tempfile::tempdir().unwrap();
    let doc = message_ir::testutil::sample_imessage_document();

    let csv_path = write_format(tmp.path(), OutputFormat::Csv, doc).unwrap();
    let csv = fs::read_to_string(&csv_path).unwrap();
    let header_line = csv.lines().next().unwrap();
    assert_eq!(header_line, CSV_HEADERS.join(","));
    assert!(csv.contains("timestamp_unix_ms"));
    assert!(csv.contains("source_fields_json"));
    assert!(csv.contains("owner_handle"));
    assert!(!header_line.contains("date_ms"));
    assert!(!header_line.contains("contact_name"));
    assert!(!header_line.contains("xml_fields_json"));
    assert!(csv.contains("hello imessage"));
    assert!(csv.contains("Loved a message"));
    assert!(csv.contains("Sent with Balloons"));
    assert!(csv.contains("true")); // is_reply
    assert!(csv.contains("loved"));
    assert!(csv.contains("+15555550100")); // outgoing sender / owner

    let sbr_doc = message_ir::testutil::sample_document("hello ir");
    let sbr_csv_path = write_format(tmp.path(), OutputFormat::Csv, sbr_doc).unwrap();
    let sbr_csv = fs::read_to_string(&sbr_csv_path).unwrap();
    assert_eq!(sbr_csv.lines().next().unwrap(), CSV_HEADERS.join(","));
    assert!(!sbr_csv.contains("xml_fields_json"));
    assert!(sbr_csv.contains("source_fields_json"));
}

#[test]
fn packaging_stem_suffix_affects_filename_not_json() {
    let mut doc = message_ir::testutil::sample_document("hello ir");
    doc.packaging_stem_suffix = Some("__whatsapp".into());
    let tmp = tempfile::tempdir().unwrap();
    let path = write_format(tmp.path(), OutputFormat::Json, doc).unwrap();
    assert!(
        path.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .contains("__whatsapp")
    );
    let raw = fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("filename_suffix"));
    assert!(!raw.contains("__whatsapp"));
}

fn assert_docs_equal_after_normalize(mut a: ConversationDocument, mut b: ConversationDocument) {
    normalize_document_for_compare(&mut a);
    normalize_document_for_compare(&mut b);
    let va = serde_json::to_value(&a).unwrap();
    let vb = serde_json::to_value(&b).unwrap();
    assert_eq!(va, vb);
}

#[test]
fn roundtrip_csv_sms_and_imessage() {
    for doc in [
        message_ir::testutil::sample_document("hello ir"),
        message_ir::testutil::sample_imessage_document(),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let csv_path = write_conversation_csv(tmp.path(), &doc).unwrap();
        let csv = fs::read_to_string(&csv_path).unwrap();
        // Nested bag cells are empty strings, never the literal `null`.
        let header = csv.lines().next().unwrap();
        let cols: Vec<&str> = header.split(',').collect();
        let bag_names = [
            "source_fields_json",
            "parts_json",
            "edits_json",
            "tapbacks_json",
            "app_json",
        ];
        for line in csv.lines().skip(1) {
            let mut rdr = csv::ReaderBuilder::new()
                .has_headers(false)
                .from_reader(line.as_bytes());
            let record = rdr.records().next().unwrap().unwrap();
            for name in bag_names {
                let idx = cols.iter().position(|c| *c == name).unwrap();
                assert_ne!(
                    record.get(idx),
                    Some("null"),
                    "column {name} must not be literal null"
                );
            }
        }
        assert!(!csv_path.with_extension("meta.json").is_file());

        let back = read_conversation_csv(&csv_path).unwrap();
        assert_docs_equal_after_normalize(doc, back);
    }
}

#[test]
fn csv_omits_trivial_parts_json_keeps_rich_parts() {
    let tmp = tempfile::tempdir().unwrap();
    let mut doc = message_ir::testutil::sample_imessage_document();
    // First message has a single run equal to text → omit parts_json.
    // Add a second body with multi-part parts that must be kept.
    doc.messages.push(IrMessage {
        guid: "MULTI-PART-GUID".into(),
        timestamp_unix_ms: 1_400_773_263_000,
        direction: IrDirection::Incoming,
        service: IrService::IMessage,
        message_kind: IrMessageKind::IMessage,
        sender_handle: Some("+15555550101".into()),
        sender_display_name: Some("Sam".into()),
        owner_handle: None,
        subject: None,
        text: "hello".into(),
        attachments: vec![],
        imessage: Some(IrImessage {
            parts: Some(json!([
                {"index": 0, "kind": "run", "text": "hello"},
                {"index": 1, "kind": "attachment", "transfer_name": "a.jpg"}
            ])),
            ..IrImessage::default()
        }),
        source: None,
    });
    doc.finalize_stats();

    let csv_path = write_conversation_csv(tmp.path(), &doc).unwrap();
    let csv = fs::read_to_string(&csv_path).unwrap();
    let header = csv.lines().next().unwrap();
    let cols: Vec<&str> = header.split(',').collect();
    let parts_idx = cols.iter().position(|c| *c == "parts_json").unwrap();
    let text_idx = cols.iter().position(|c| *c == "text").unwrap();

    let mut saw_trivial_empty = false;
    let mut saw_rich = false;
    for line in csv.lines().skip(1) {
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(line.as_bytes());
        let record = rdr.records().next().unwrap().unwrap();
        let text = record.get(text_idx).unwrap_or("");
        let parts = record.get(parts_idx).unwrap_or("");
        if text == "hello imessage" {
            assert!(parts.is_empty(), "trivial parts_json should be empty");
            saw_trivial_empty = true;
        }
        if text == "hello" {
            assert!(
                parts.contains("attachment"),
                "rich parts_json should be kept: {parts}"
            );
            saw_rich = true;
        }
    }
    assert!(saw_trivial_empty && saw_rich);
}

#[test]
fn roundtrip_json_and_jsonl() {
    for doc in [
        message_ir::testutil::sample_document("hello ir"),
        message_ir::testutil::sample_imessage_document(),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = write_format(tmp.path(), OutputFormat::Json, doc.clone()).unwrap();
        let back_json = read_conversation_json(&json_path).unwrap();
        assert_docs_equal_after_normalize(doc.clone(), back_json);

        let _ = clean_previous_ir_output(tmp.path());
        let jsonl_path = write_format(tmp.path(), OutputFormat::Jsonl, doc.clone()).unwrap();
        let back_jsonl = read_conversation_jsonl(&jsonl_path).unwrap();
        assert_docs_equal_after_normalize(doc, back_jsonl);
    }
}

#[test]
fn csv_serializes_handle_type_in_cell_and_column() {
    fn first_row_cols(csv: &str) -> (Vec<String>, csv::StringRecord) {
        let mut lines = csv.lines();
        let headers = lines.next().unwrap().to_string();
        let cols: Vec<String> = headers.split(',').map(str::to_string).collect();
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(lines.next().unwrap().as_bytes());
        let row = rdr.records().next().unwrap().unwrap();
        (cols, row)
    }

    let tmp = tempfile::tempdir().unwrap();
    let csv_path = write_conversation_csv(
        tmp.path(),
        &message_ir::testutil::sample_document("hello ir"),
    )
    .unwrap();
    let csv = fs::read_to_string(&csv_path).unwrap();
    let (cols, row) = first_row_cols(&csv);
    let participants_idx = cols.iter().position(|c| c == "participants_json").unwrap();
    let handle_type_idx = cols.iter().position(|c| c == "handle_type").unwrap();
    // Participants cell carries the typed participant.
    assert!(
        row.get(participants_idx)
            .unwrap()
            .contains(r#""handle_type":"phone""#),
        "participants_json must carry handle_type"
    );
    // Dedicated column carries the sender handle type.
    assert_eq!(row.get(handle_type_idx).unwrap(), "phone");
    // Empty sender handle yields an empty cell, never "other".
    let mut doc = message_ir::testutil::sample_document("hello ir");
    doc.messages[0].sender_handle = None;
    let csv_path = write_conversation_csv(tmp.path(), &doc).unwrap();
    let csv = fs::read_to_string(&csv_path).unwrap();
    let (cols, row) = first_row_cols(&csv);
    let handle_type_idx = cols.iter().position(|c| c == "handle_type").unwrap();
    assert_eq!(row.get(handle_type_idx).unwrap(), "");
}

#[test]
fn roundtrip_eml_and_mbox() {
    for doc in [
        message_ir::testutil::sample_document("hello ir"),
        message_ir::testutil::sample_imessage_document(),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let _ = clean_previous_mail_output(tmp.path());
        let eml_dir = write_format(tmp.path(), OutputFormat::Eml, doc.clone()).unwrap();
        let back_eml = read_conversation_eml_dir(&eml_dir).unwrap();
        // Outgoing EML must carry sender + owner identity headers. Only the
        // iMessage fixture has an outgoing row (the tapback).
        if doc
            .messages
            .iter()
            .any(|m| m.direction == IrDirection::Outgoing)
        {
            let outgoing_eml = fs::read_dir(&eml_dir)
                .unwrap()
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .find(|p| {
                    let bytes = fs::read(p).unwrap();
                    let text = String::from_utf8_lossy(&bytes);
                    text.contains("X-ME-Direction: outgoing")
                })
                .expect("outgoing eml");
            let outgoing_text = fs::read_to_string(&outgoing_eml).unwrap();
            assert!(outgoing_text.contains("X-ME-Sender-Handle:"));
            assert!(outgoing_text.contains("X-ME-Owner-Handle:"));
        }

        assert_docs_equal_after_normalize(doc.clone(), back_eml);

        let _ = clean_previous_mail_output(tmp.path());
        let mbox_path = write_format(tmp.path(), OutputFormat::Mbox, doc.clone()).unwrap();
        let back_mbox = read_conversation_mbox(&mbox_path).unwrap();
        assert_docs_equal_after_normalize(doc, back_mbox);
    }
}

/// A version-3 file is refused by its version, not by whichever field fails
/// to parse first: the reader peeks at `schema_version` before parsing the
/// rest, so the fields below are deliberately not a valid version-4 shape.
#[test]
fn json_refuses_a_version_3_file_by_name() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("old.json");
    fs::write(
        &path,
        r#"{"schema_version":3,"export":{},"conversation":{},"messages":[]}"#,
    )
    .unwrap();
    let err = read_conversation_json(&path).unwrap_err();
    let refusal = err
        .downcast_ref::<message_ir::UnsupportedSchemaVersion>()
        .expect("typed refusal");
    assert_eq!(refusal.found, 3);
    assert_eq!(
        refusal.to_string(),
        "This file is schema version 3; the vault reads version 4"
    );
}

#[test]
fn jsonl_refuses_a_version_3_file_by_name() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("old.jsonl");
    fs::write(
        &path,
        "{\"schema_version\":3,\"export\":{},\"conversation\":{}}\n{\"not\":\"a message\"}\n",
    )
    .unwrap();
    let err = read_conversation_jsonl(&path).unwrap_err();
    let refusal = err
        .downcast_ref::<message_ir::UnsupportedSchemaVersion>()
        .expect("typed refusal");
    assert_eq!(refusal.found, 3);
    assert!(format!("{err:#}").contains("schema version 3"), "{err:#}");
}

/// Message texts that a careless writer or reader damages: line endings a
/// normaliser rewrites, an mbox `From ` line, CSV quoting, multi-code-point
/// emoji, and no text at all beside an attachment. Every per-conversation
/// format writes each one and reads it back byte for byte.
#[test]
fn hard_texts_survive_every_format() {
    struct Case {
        name: &'static str,
        text: String,
        with_attachment: bool,
    }
    let case = |name: &'static str, text: &str, with_attachment: bool| Case {
        name,
        text: text.to_string(),
        with_attachment,
    };
    let cases = [
        case("trailing newline", "ends with one newline\n", false),
        case(
            "multiple trailing newlines",
            "ends with three newlines\n\n\n",
            false,
        ),
        case("CRLF line endings", "first line\r\nsecond line\r\n", false),
        case("bare CR", "first\rsecond", false),
        case("leading and trailing blank lines", "\n\nmiddle\n\n", false),
        case(
            "From line",
            "From the top\nFrom here on\n>From quoted\n>>From twice",
            false,
        ),
        case(
            "CSV quotes, commas and newlines",
            "she said \"hi\", then, \"\"twice\"\"\r\nnext,line\n\"",
            false,
        ),
        case(
            "emoji joiners and skin tones",
            "\u{1F469}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466} \u{1F44D}\u{1F3FD} \
             \u{1F9D1}\u{1F3FF}\u{200D}\u{1F680} \u{1F3F3}\u{FE0F}\u{200D}\u{1F308}",
            false,
        ),
        case(
            "leading and trailing spaces and tabs",
            "  indented\t\nends with spaces   \n\t",
            false,
        ),
        case(
            "line longer than an RFC 5322 line",
            &"x".repeat(1500),
            false,
        ),
        case("empty text with an attachment", "", true),
    ];

    let formats = [
        OutputFormat::Json,
        OutputFormat::Jsonl,
        OutputFormat::Csv,
        OutputFormat::Eml,
        OutputFormat::Mbox,
    ];

    let mut failures = Vec::new();
    for case in &cases {
        for format in formats {
            let mut doc = message_ir::testutil::sample_document(&case.text);
            if case.with_attachment {
                doc.messages[0].attachments.push(message_ir::IrAttachment {
                    path: None,
                    original_name: Some("photo.jpg".into()),
                    mime_type: Some("image/jpeg".into()),
                    digest_sha256: None,
                    is_sticker: false,
                    transcription: None,
                    sticker_effect: None,
                    size_bytes: None,
                    missing_reason: None,
                    bytes: Some(b"\xff\xd8\xfffakejpeg".to_vec()),
                });
            }
            let tmp = tempfile::tempdir().unwrap();
            let path = write_format(tmp.path(), format, doc.clone()).unwrap();
            let back = match format {
                OutputFormat::Json => read_conversation_json(&path),
                OutputFormat::Jsonl => read_conversation_jsonl(&path),
                OutputFormat::Csv => read_conversation_csv(&path),
                OutputFormat::Eml => read_conversation_eml_dir(&path),
                OutputFormat::Mbox => read_conversation_mbox(&path),
                OutputFormat::Xml => unreachable!(),
            };
            let back = match back {
                Ok(back) => back,
                Err(err) => {
                    failures.push(format!(
                        "{} / {}: read failed: {err:#}",
                        format.as_str(),
                        case.name
                    ));
                    continue;
                }
            };
            let got = &back.messages[0];
            if got.text != case.text {
                failures.push(format!(
                    "{} / {}: text {:?} came back as {:?}",
                    format.as_str(),
                    case.name,
                    case.text,
                    got.text
                ));
            }
            if got.attachments.len() != doc.messages[0].attachments.len() {
                failures.push(format!(
                    "{} / {}: {} attachments came back as {}",
                    format.as_str(),
                    case.name,
                    doc.messages[0].attachments.len(),
                    got.attachments.len()
                ));
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
