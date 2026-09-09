use crate::emit::convert_json;
use message_ir_format::ExportTransforms;
use message_vault_io_core::OutputFormat;
use std::fs;
use std::path::PathBuf;

#[test]
fn convert_fixture_json_individual_and_group() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/result.json");
    assert!(fixture.is_file(), "missing {}", fixture.display());

    let tmp = tempfile::tempdir().expect("tempdir");
    let (report, _) = convert_json(
        &fixture,
        tmp.path(),
        ExportTransforms::none(),
        &[],
        OutputFormat::Csv,
        None,
        false,
    )
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
    let (report, _) = convert_json(
        &json_path,
        out.path(),
        ExportTransforms::none(),
        &[media_root.path().to_path_buf()],
        OutputFormat::Csv,
        None,
        false,
    )
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

    let (report, _) = convert_json(
        &fixture,
        tmp.path(),
        ExportTransforms::none(),
        &[],
        OutputFormat::Jsonl,
        None,
        false,
    )
    .expect("convert");
    assert_eq!(report.conversations, 2);

    let jsonl_files = |dir: &std::path::Path| -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .expect("read output")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".jsonl"))
            .collect();
        names.sort();
        names
    };
    let first = jsonl_files(tmp.path());
    assert_eq!(first.len(), 2, "the queue wrote a file per conversation");
    let bodies: Vec<String> = first
        .iter()
        .map(|n| fs::read_to_string(tmp.path().join(n)).expect("read jsonl"))
        .collect();

    // Resuming into the same folder finds both conversations already written
    // and leaves them exactly as they were.
    let (resumed, _) = convert_json(
        &fixture,
        tmp.path(),
        ExportTransforms::none(),
        &[],
        OutputFormat::Jsonl,
        None,
        true,
    )
    .expect("resume convert");

    assert_eq!(resumed.conversations, 2, "resume still accounts for both");
    // The file bytes alone prove nothing here: the writer is deterministic, so
    // a resumed run that quietly rewrote both conversations would produce the
    // same bytes and this test would still pass. `conversations_skipped` is
    // the only observable difference between resuming and starting over.
    assert_eq!(
        resumed.conversations_skipped, 2,
        "both conversations were already written, so the resume skipped both"
    );
    assert_eq!(jsonl_files(tmp.path()), first, "same file set");
    for (name, before) in first.iter().zip(bodies) {
        assert_eq!(
            fs::read_to_string(tmp.path().join(name)).expect("reread"),
            before,
            "a resumed run must not rewrite {name}"
        );
    }
}
