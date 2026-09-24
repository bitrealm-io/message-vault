use super::*;
use media::MediaMode;
use message_ir::{
    ConversationMeta, ConversationStats, ExportMeta, HandleType, IrConversationType, IrImessage,
    IrMessage, IrMessageKind, IrParticipant, IrService, IrSource, SCHEMA_VERSION,
};
use message_vault_io_core::{ExportReport, OutputFormat};
use serde_json::{Value, json};
use std::fs;

fn doc_with_image_attachment() -> ConversationDocument {
    ConversationDocument {
        schema_version: SCHEMA_VERSION,
        export: ExportMeta {
            source: "test".into(),
            tool: "test".into(),
            tool_version: "0".into(),
            owner_handle: None,
            owner_display_name: None,
        },
        conversation: ConversationMeta {
            chat_identifier: "+15555550101".into(),
            conversation_type: IrConversationType::Individual,
            group_title: None,
            participants: vec![IrParticipant {
                handle: Some("+15555550101".into()),
                display_name: Some("Sam".into()),
                handle_type: None,
            }],
            stats: ConversationStats::default(),
        },
        messages: vec![IrMessage {
            guid: "guid-1".into(),
            timestamp_unix_ms: 1_400_773_261_000,
            direction: IrDirection::Incoming,
            service: IrService::Sms,
            message_kind: IrMessageKind::Sms,
            sender_handle: Some("+15555550101".into()),
            sender_display_name: Some("Sam".into()),
            owner_handle: None,
            subject: None,
            text: "hi".into(),
            attachments: vec![IrAttachment {
                path: Some("photo.jpg".into()),
                original_name: Some("photo.jpg".into()),
                mime_type: Some("image/jpeg".into()),
                digest_sha256: None,
                is_sticker: false,
                transcription: None,
                sticker_effect: None,
                size_bytes: None,
                missing_reason: None,
                bytes: None,
            }],
            imessage: None,
            source: None,
        }],
        packaging_stem_suffix: None,
    }
}

#[test]
fn obfuscate_drops_the_vendor_bag() {
    let mut doc = message_ir::testutil::sample_document("secret");
    assert!(doc.messages[0].source.is_some());
    let mut anon = Obfuscator::new([7u8; 32]);
    obfuscate_document(&mut doc, &mut anon);
    assert!(
        doc.messages[0].source.is_none(),
        "obfuscated output must not carry vendor fields"
    );
}

#[test]
fn obfuscate_replaces_the_owner_address_on_each_message() {
    let mut doc = message_ir::testutil::sample_document("secret");
    doc.messages[0].owner_handle = Some("+15555550100".into());
    let mut anon = Obfuscator::new([7u8; 32]);
    obfuscate_document(&mut doc, &mut anon);
    let owner = doc.messages[0].owner_handle.as_deref();
    assert_ne!(owner, Some("+15555550100"));
    assert_eq!(
        owner,
        doc.export.owner_handle.as_deref(),
        "one address becomes one fake address wherever it appears"
    );
}

#[test]
fn obfuscate_skips_staged_media_and_writes_placeholders() {
    let tmp = tempfile::tempdir().unwrap();
    let att = tmp.path().join("attachments");
    fs::create_dir_all(&att).unwrap();
    // Pretend an exporter staged a real file (should be removed).
    fs::write(att.join("real-photo.jpg"), b"REAL_JPEG_BYTES_SHOULD_GO").unwrap();

    let mut docs = vec![doc_with_image_attachment()];
    let transforms = ExportTransforms {
        media: MediaMode::Convert,
        obfuscate: true,
        obfuscate_seed: Some(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        ),
        ..ExportTransforms::none()
    };
    let outcome = apply_transforms(&mut docs, tmp.path(), &transforms).unwrap();
    assert_eq!(outcome.obfuscated_docs, 1);

    assert!(!att.join("real-photo.jpg").exists());
    assert!(att.join("placeholder.jpg").is_file());
    assert!(att.join("placeholder.mp4").is_file());
    assert!(att.join("placeholder.bin").is_file());
    assert_eq!(
        docs[0].messages[0].attachments[0].path.as_deref(),
        Some("attachments/placeholder.jpg")
    );
}

#[test]
fn obfuscate_keeps_mime_when_media_disabled() {
    let tmp = tempfile::tempdir().unwrap();
    let mut docs = vec![doc_with_image_attachment()];
    let transforms = ExportTransforms {
        media: MediaMode::Disabled,
        obfuscate: true,
        obfuscate_seed: Some(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        ),
        ..ExportTransforms::none()
    };
    apply_transforms(&mut docs, tmp.path(), &transforms).unwrap();
    assert_eq!(
        docs[0].messages[0].attachments[0].path.as_deref(),
        Some("attachments/placeholder.jpg")
    );
}

#[test]
fn convert_at_finish_leaves_cloned_file() {
    let tmp = tempfile::tempdir().unwrap();
    let att = tmp.path().join("attachments");
    fs::create_dir_all(&att).unwrap();
    fs::write(att.join("keep.bin"), b"already-cloned").unwrap();
    let mut docs = vec![doc_with_image_attachment()];
    docs[0].messages[0].attachments[0].path = Some("attachments/keep.bin".into());
    let transforms = ExportTransforms {
        media: MediaMode::Convert,
        ..ExportTransforms::none()
    };
    let outcome = apply_transforms(&mut docs, tmp.path(), &transforms).unwrap();
    assert_eq!(outcome.obfuscated_docs, 0);
    assert_eq!(
        docs[0].messages[0].attachments[0].path.as_deref(),
        Some("attachments/keep.bin")
    );
    assert_eq!(fs::read(att.join("keep.bin")).unwrap(), b"already-cloned");
}

/// A group chat in which every string holds its own `LEAK-<n>` marker, except
/// the values listed in [`KEPT_AS_IS`]. Every `Option` is set, so a field
/// added to the IR later has to be filled in here and the walk then checks it.
fn doc_with_a_marker_in_every_field() -> ConversationDocument {
    ConversationDocument {
        schema_version: SCHEMA_VERSION,
        export: ExportMeta {
            source: "sms-backup-restore".into(),
            tool: "SMS Backup & Restore".into(),
            tool_version: "10.20".into(),
            owner_handle: Some("LEAK-01".into()),
            owner_display_name: Some("LEAK-02".into()),
        },
        conversation: ConversationMeta {
            chat_identifier: "LEAK-03".into(),
            conversation_type: IrConversationType::Group,
            group_title: Some("LEAK-04".into()),
            participants: vec![IrParticipant {
                handle: Some("LEAK-05".into()),
                display_name: Some("LEAK-06".into()),
                handle_type: Some(HandleType::Phone),
            }],
            stats: ConversationStats {
                message_count: 1,
                attachment_count: 1,
                first_timestamp_unix_ms: Some(1_400_773_261_000),
                last_timestamp_unix_ms: Some(1_400_773_261_000),
            },
        },
        messages: vec![IrMessage {
            guid: "6A1B2C3D-0000-4000-8000-000000000001".into(),
            timestamp_unix_ms: 1_400_773_261_000,
            direction: IrDirection::Incoming,
            service: IrService::IMessage,
            message_kind: IrMessageKind::IMessage,
            sender_handle: Some("LEAK-07".into()),
            sender_display_name: Some("LEAK-08".into()),
            owner_handle: Some("LEAK-09".into()),
            subject: Some("LEAK-10".into()),
            text: "LEAK-11".into(),
            attachments: vec![IrAttachment {
                path: Some("attachments/LEAK-12.jpg".into()),
                original_name: Some("LEAK-13.jpg".into()),
                mime_type: Some("image/jpeg".into()),
                digest_sha256: Some("LEAK-14".into()),
                is_sticker: false,
                transcription: Some("LEAK-15".into()),
                sticker_effect: Some("shiny".into()),
                size_bytes: Some(4),
                missing_reason: Some("file_missing".into()),
                bytes: None,
            }],
            imessage: Some(IrImessage {
                is_reply: true,
                in_reply_to_guid: Some("6A1B2C3D-0000-4000-8000-000000000002".into()),
                thread_originator_part: Some(0),
                num_replies: Some(1),
                is_deleted: false,
                send_effect: Some("slam".into()),
                shared_location: Some("LEAK-16".into()),
                announcement: Some("LEAK-17".into()),
                read_receipt_rfc3339: Some("2014-05-22T12:21:01Z".into()),
                parts: Some(json!([{ "text": "LEAK-18", "attachment_indices": [0] }])),
                edits: Some(json!([{ "text": "LEAK-19", "timestamp_unix_ms": 1 }])),
                tapbacks: Some(json!([{
                    "part_index": 0,
                    "kind": "emoji",
                    "emoji": "👍",
                    "is_from_me": false,
                    "reactor_handle": "LEAK-20",
                    "reactor_display_name": "LEAK-21",
                    "sender": "LEAK-22",
                    "unknown_key": "LEAK-23",
                }])),
                app: Some(json!({ "url": "https://LEAK-24.example/", "title": "LEAK-25" })),
                balloon_bundle_id: Some("com.apple.DigitalTouchBalloonProvider".into()),
                balloon_kind: Some("sketch".into()),
                associated_guid: Some("6A1B2C3D-0000-4000-8000-000000000003".into()),
                associated_part: Some(0),
                tapback_kind: Some("emoji".into()),
                tapback_emoji: Some("👍".into()),
                tapback_action: Some("added".into()),
            }),
            source: Some(IrSource {
                android_type: Some(1),
                fields: serde_json::Map::from_iter([("address".into(), json!("LEAK-26"))]),
            }),
        }],
        packaging_stem_suffix: None,
    }
}

/// The strings obfuscation passes through unchanged, by JSON path, each with
/// the reason that is safe. Every other string must be replaced or dropped.
const KEPT_AS_IS: &[(&str, &str)] = &[
    ("export.source", "names the backup tool, not the person"),
    ("export.tool", "names the backup tool, not the person"),
    (
        "export.tool_version",
        "names the backup tool, not the person",
    ),
    ("conversation.conversation_type", "enum value"),
    ("conversation.participants[].handle_type", "enum value"),
    ("messages[].guid", "opaque message id"),
    ("messages[].direction", "enum value"),
    ("messages[].service", "enum value"),
    ("messages[].message_kind", "enum value"),
    (
        "messages[].attachments[].mime_type",
        "picks the placeholder file",
    ),
    (
        "messages[].attachments[].sticker_effect",
        "Apple effect label",
    ),
    (
        "messages[].attachments[].missing_reason",
        "closed set of reasons",
    ),
    ("messages[].imessage.in_reply_to_guid", "opaque message id"),
    ("messages[].imessage.associated_guid", "opaque message id"),
    ("messages[].imessage.send_effect", "Apple effect label"),
    (
        "messages[].imessage.read_receipt_rfc3339",
        "a time, and times are kept",
    ),
    ("messages[].imessage.balloon_bundle_id", "Apple bundle id"),
    ("messages[].imessage.balloon_kind", "Apple balloon label"),
    ("messages[].imessage.tapback_kind", "tapback label"),
    (
        "messages[].imessage.tapback_emoji",
        "the reaction, not who made it",
    ),
    ("messages[].imessage.tapback_action", "tapback label"),
    ("messages[].imessage.tapbacks[].kind", "tapback label"),
    (
        "messages[].imessage.tapbacks[].emoji",
        "the reaction, not who made it",
    ),
];

/// Push every string in `value` with its path (`a.b[].c`) onto `out`.
///
/// # Panics
///
/// Panics on a null, because the fixture must set every field.
fn string_leaves(value: &Value, path: &str, out: &mut Vec<(String, String)>) {
    match value {
        Value::String(s) => out.push((path.to_string(), s.clone())),
        Value::Array(items) => {
            for item in items {
                string_leaves(item, &format!("{path}[]"), out);
            }
        }
        Value::Object(map) => {
            for (k, v) in map {
                let child = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                string_leaves(v, &child, out);
            }
        }
        Value::Null => panic!("{path} is null; the fixture must set every field"),
        Value::Bool(_) | Value::Number(_) => {}
    }
}

#[test]
fn obfuscated_export_keeps_no_string_from_the_source() {
    let doc = doc_with_a_marker_in_every_field();
    let mut leaves = Vec::new();
    string_leaves(&serde_json::to_value(&doc).unwrap(), "", &mut leaves);
    for (path, value) in &leaves {
        let kept = KEPT_AS_IS.iter().any(|(p, _)| p == path);
        assert_eq!(
            value.contains("LEAK-"),
            !kept,
            "{path} = {value:?}: a string not in KEPT_AS_IS must hold a LEAK marker"
        );
    }

    let tmp = tempfile::tempdir().unwrap();
    let transforms = ExportTransforms {
        media: MediaMode::Disabled,
        obfuscate: true,
        obfuscate_seed: Some(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        ),
        ..ExportTransforms::none()
    };
    let mut sink = crate::FormatSink::open(tmp.path(), OutputFormat::Json, transforms).unwrap();
    sink.write_document(doc).unwrap();
    sink.finish(&mut ExportReport::default()).unwrap();

    let mut written = String::new();
    for entry in fs::read_dir(tmp.path()).unwrap() {
        let path = entry.unwrap().path();
        // A group chat's file is named after its title.
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(!name.contains("LEAK-"), "obfuscated export wrote {name}");
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            written.push_str(&fs::read_to_string(&path).unwrap());
        }
    }
    assert!(!written.is_empty(), "the export wrote no JSON file");
    let leaked: Vec<&str> = leaves
        .iter()
        .filter(|(_, value)| value.contains("LEAK-") && written.contains(value.as_str()))
        .map(|(path, _)| path.as_str())
        .collect();
    assert!(leaked.is_empty(), "obfuscated export kept {leaked:?}");
}
