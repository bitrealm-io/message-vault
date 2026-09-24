use crate::emit::{ConvertRequest, convert_json};
use message_vault_io_core::testutil::assert_jsonl_resumes;
use message_vault_io_core::{ExportTransforms, OutputFormat};
use std::fs;
use std::path::PathBuf;

#[test]
fn convert_fixture_json_individual_and_group() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/result.json");
    assert!(fixture.is_file(), "missing {}", fixture.display());

    let tmp = tempfile::tempdir().expect("tempdir");
    let report = convert_json(ConvertRequest {
        json_path: &fixture,
        output: tmp.path(),
        transforms: ExportTransforms::none(),
        media_search_roots: &[],
        owner_handle: None,
        output_format: OutputFormat::Csv,
        cancel: None,
        resume: false,
    })
    .expect("convert");

    assert_eq!(report.conversations, 2);
    assert_eq!(report.messages, 4);
    assert_eq!(report.sent, 2);
    assert_eq!(report.received, 2);

    let individual = tmp.path().join("+15555550122__whatsapp.csv");
    assert!(individual.is_file(), "missing {}", individual.display());
    let body = fs::read_to_string(&individual).expect("read individual");
    assert!(body.contains("chat_identifier"));
    assert!(body.contains("whatsapp"));
    assert!(body.contains("Hello from Sam"));
    assert!(body.contains("WhatsApp Chat Exporter"));

    let group = tmp.path().join("Family_Chat__whatsapp.csv");
    assert!(group.is_file(), "missing {}", group.display());
    let gbody = fs::read_to_string(&group).expect("read group");
    assert!(gbody.contains("group"));
    assert!(gbody.contains("Family Chat"));
    assert!(gbody.contains("+15555550133") || gbody.contains("15555550133"));
}

#[test]
fn copies_ios_style_media_true_data_paths() {
    let media_root = tempfile::tempdir().expect("media root");
    let media_base = "AppDomainGroup-group.net.whatsapp.WhatsApp.shared";
    let rel = "Message/Media/chat/a/b/photo.jpg";
    let src = media_root.path().join(media_base).join(rel);
    fs::create_dir_all(src.parent().unwrap()).expect("mkdir");
    fs::write(&src, b"fake-jpeg").expect("write media");

    let json = serde_json::json!({
        "15555550999@s.whatsapp.net": {
            "name": "Media Peer",
            "type": "ios",
            "media_base": format!("{media_base}/"),
            "messages": {
                "M1": {
                    "from_me": false,
                    "timestamp": 1609459200,
                    "time": "00:00",
                    "key_id": "M1",
                    "data": rel,
                    "sender": null,
                    "media": true,
                    "mime": "image/jpeg",
                    "caption": "look at this",
                    "sticker": false,
                    "reply": null,
                    "reactions": {}
                }
            }
        }
    });
    let json_path = media_root.path().join("result.json");
    fs::write(&json_path, json.to_string()).expect("write json");

    let out = tempfile::tempdir().expect("out");
    let report = convert_json(ConvertRequest {
        json_path: &json_path,
        output: out.path(),
        transforms: ExportTransforms::none(),
        media_search_roots: &[media_root.path().to_path_buf()],
        owner_handle: None,
        output_format: OutputFormat::Csv,
        cancel: None,
        resume: false,
    })
    .expect("convert");

    assert_eq!(report.attachments_saved, 1);
    assert_eq!(
        report
            .extra
            .get("attachments_missing")
            .copied()
            .unwrap_or(0),
        0
    );
    let csv = out.path().join("+15555550999__whatsapp.csv");
    let body = fs::read_to_string(&csv).expect("csv");
    assert!(body.contains("look at this"));
    assert!(
        !body.contains(rel),
        "media path must not become message text"
    );
    assert!(body.contains("attachments/"));
    let att_dir = out.path().join("attachments");
    let files: Vec<_> = fs::read_dir(&att_dir)
        .expect("attachments dir")
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1);
    assert_eq!(fs::read(&files[0]).unwrap(), b"fake-jpeg");
}

#[test]
fn jsonl_drains_the_write_queue_and_a_second_run_resumes_it() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/result.json");
    let tmp = tempfile::tempdir().expect("tempdir");
    let report = assert_jsonl_resumes(tmp.path(), |resume| {
        convert_json(ConvertRequest {
            json_path: &fixture,
            output: tmp.path(),
            transforms: ExportTransforms::none(),
            media_search_roots: &[],
            owner_handle: None,
            output_format: OutputFormat::Jsonl,
            cancel: None,
            resume,
        })
    });
    assert_eq!(report.conversations, 2, "one individual chat and one group");
}

/// Convert `json` into a JSON export and read each conversation back,
/// keyed by chat identifier.
fn convert_to_documents(
    json: &serde_json::Value,
) -> (
    message_vault_io_core::ExportReport,
    std::collections::BTreeMap<String, message_ir::ConversationDocument>,
) {
    let dir = tempfile::tempdir().expect("tempdir");
    let json_path = dir.path().join("result.json");
    fs::write(&json_path, json.to_string()).expect("write json");
    let out = dir.path().join("out");
    let report = convert_json(ConvertRequest {
        json_path: &json_path,
        output: &out,
        transforms: ExportTransforms::none(),
        media_search_roots: &[dir.path().to_path_buf()],
        owner_handle: None,
        output_format: OutputFormat::Json,
        cancel: None,
        resume: false,
    })
    .expect("convert");
    let documents = fs::read_dir(&out)
        .expect("out")
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .map(|path| message_ir_format::read_conversation_json(&path).expect("read back"))
        .map(|doc| (doc.conversation.chat_identifier.clone(), doc))
        .collect();
    (report, documents)
}

/// One wtsexporter message row with the fields every row carries.
fn row(key_id: &str, timestamp: serde_json::Value, data: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "from_me": false,
        "timestamp": timestamp,
        "key_id": key_id,
        "data": data,
        "sender": null,
        "media": false,
        "mime": null,
        "caption": null,
        "sticker": false,
        "reply": null,
        "reactions": {}
    })
}

/// Each message keeps its exact time, whether wtsexporter wrote seconds or
/// milliseconds, and a time no calendar can hold is skipped. Two messages
/// sent in the same second with the same text are two messages, told
/// apart by their `key_id`.
#[test]
fn messages_keep_their_time_and_their_identity() {
    let mut ok_twice = row("K2", 1_609_459_300.into(), "ok".into());
    ok_twice["from_me"] = true.into();
    let mut ok_again = ok_twice.clone();
    ok_again["key_id"] = "K3".into();
    let json = serde_json::json!({
        "15555550122@s.whatsapp.net": {
            "name": "Sam Example",
            "messages": {
                "K1": row("K1", 1_609_459_200.into(), "Hello".into()),
                "K2": ok_twice,
                "K3": ok_again,
                "K4": row("K4", 1_609_459_400_123_i64.into(), "in milliseconds".into()),
                "K5": row("K5", 1e18.into(), "never".into()),
                // The largest stamp read as seconds, in 2286.
                "K6": row("K6", 9_999_999_999_i64.into(), "last second".into()),
            }
        }
    });

    let (report, documents) = convert_to_documents(&json);
    assert_eq!(report.skipped_invalid_date, 1);
    let doc = &documents["+15555550122"];
    let times: Vec<(i64, &str)> = doc
        .messages
        .iter()
        .map(|m| (m.timestamp_unix_ms, m.text.as_str()))
        .collect();
    assert_eq!(
        times,
        vec![
            (1_609_459_200_000, "Hello"),
            (1_609_459_300_000, "ok"),
            (1_609_459_300_000, "ok"),
            (1_609_459_400_123, "in milliseconds"),
            (9_999_999_999_000, "last second"),
        ]
    );
    assert_ne!(
        doc.messages[1].guid, doc.messages[2].guid,
        "the same text in the same second is two messages"
    );
}

/// An incoming one-to-one message is from the peer, named by the chat. A
/// group message is from the member's phone, and the group's roster lists
/// the members who wrote.
#[test]
fn senders_and_the_roster_are_the_peers_phones() {
    let mut group_row = row("G1", 1_609_632_000.into(), "Group hello".into());
    group_row["sender"] = "15555550133@s.whatsapp.net".into();
    // A row with a time no calendar can hold is dropped whole, so its
    // sender doesn't join the roster either.
    let mut dropped_row = row("G2", 1e18.into(), "never".into());
    dropped_row["sender"] = "15555550144@s.whatsapp.net".into();
    let json = serde_json::json!({
        "15555550122@s.whatsapp.net": {
            "name": "Sam Example",
            "messages": { "K1": row("K1", 1_609_459_200.into(), "Hello".into()) }
        },
        "120363042111111111@g.us": {
            "name": "Family Chat",
            "messages": { "G1": group_row, "G2": dropped_row }
        }
    });

    let (_, documents) = convert_to_documents(&json);
    let direct = &documents["+15555550122"].messages[0];
    assert_eq!(direct.sender_handle.as_deref(), Some("+15555550122"));
    assert_eq!(direct.sender_display_name.as_deref(), Some("Sam Example"));

    let group = documents
        .values()
        .find(|doc| doc.conversation.group_title.as_deref() == Some("Family Chat"))
        .expect("the group");
    assert_eq!(
        group.messages[0].sender_handle.as_deref(),
        Some("+15555550133")
    );
    let roster: Vec<_> = group
        .conversation
        .participants
        .iter()
        .filter_map(|p| p.handle.as_deref())
        .collect();
    assert_eq!(roster, vec!["+15555550133"]);
}

/// A number or a boolean in `data` is the message's text, a caption the
/// text doesn't already hold is added below it, and the text wtsexporter
/// writes for media that was not in the backup is neither text nor an
/// attachment.
#[test]
fn a_body_that_is_not_a_string_is_kept_and_missing_media_is_dropped() {
    let mut missing_flag = row("K3", 1_609_459_202.into(), "The media is missing".into());
    missing_flag["media"] = true.into();
    let mut missing_path = row("K4", 1_609_459_203.into(), "caption".into());
    missing_path["media"] = "The media is missing".into();
    let mut captioned = row("K5", 1_609_459_204.into(), "Look".into());
    captioned["caption"] = "the view".into();
    let mut caption_in_text = row("K6", 1_609_459_205.into(), "Look at the view".into());
    caption_in_text["caption"] = "the view".into();
    let json = serde_json::json!({
        "15555550122@s.whatsapp.net": {
            "name": "Sam Example",
            "messages": {
                "K1": row("K1", 1_609_459_200.into(), 42.into()),
                "K2": row("K2", 1_609_459_201.into(), true.into()),
                "K3": missing_flag,
                "K4": missing_path,
                "K5": captioned,
                "K6": caption_in_text,
            }
        }
    });

    let (report, documents) = convert_to_documents(&json);
    let messages = &documents["+15555550122"].messages;
    let texts: Vec<&str> = messages.iter().map(|m| m.text.as_str()).collect();
    assert_eq!(
        texts,
        vec![
            "42",
            "true",
            "",
            "caption",
            "Look\nthe view",
            "Look at the view"
        ]
    );
    assert!(messages.iter().all(|m| m.attachments.is_empty()));
    assert_eq!(report.extra.get("attachments_missing"), None);
}

/// Older dumps put the media path straight in `media` rather than setting
/// `media: true` and putting it in `data`; the file is copied all the same.
#[test]
fn a_media_path_in_the_media_field_is_copied() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("photo.jpg"), b"fake-jpeg").expect("write media");
    let mut photo = row("K1", 1_609_459_200.into(), serde_json::Value::Null);
    photo["media"] = "photo.jpg".into();
    let json = serde_json::json!({
        "15555550122@s.whatsapp.net": { "name": "Sam Example", "messages": { "K1": photo } }
    });
    let json_path = dir.path().join("result.json");
    fs::write(&json_path, json.to_string()).expect("write json");

    let out = dir.path().join("out");
    let report = convert_json(ConvertRequest {
        json_path: &json_path,
        output: &out,
        transforms: ExportTransforms::none(),
        media_search_roots: &[dir.path().to_path_buf()],
        owner_handle: None,
        output_format: OutputFormat::Csv,
        cancel: None,
        resume: false,
    })
    .expect("convert");
    assert_eq!(report.attachments_saved, 1);
}
