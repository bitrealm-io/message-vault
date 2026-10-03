//! Mock server tests for one pull: login, the Export Run it records, two
//! pages of messages, asset download, the journal a second run reads, and
//! the progress a caller sees.
//!
//! The mock answers the five routes `run` calls — `GET /v1/session`,
//! `POST /v1/exports`, `GET /v1/exports/{id}/messages`,
//! `POST /v1/exports/{id}/complete` or `/cancel`, and `GET /v1/assets/{sha256}`
//! — with the JSON the server serializes (`message-crate-api-types`,
//! `docs/src/assets/openapi.json`). Every request derives from
//! `PullConfig::base_url`, so the mock's address is the only seam.

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use httpmock::prelude::*;
use message_crate_pull::{ExportQueryList, ProgressEvent, PullConfig, PullReport, journal, run};
use message_ir_format::{EXPORT_SENTINEL, read_conversation_jsonl};
use serde_json::{Value, json};
use tempfile::tempdir;

/// SHA-256 of [`MENU_BYTES`], the menu attachment's fingerprint.
const MENU_SHA: &str = "9170c75b5d7058e5075b811f12e88ef3e7b0bd9018ff8b52bead945b6b1219f7";
/// SHA-256 of [`PHOTO_BYTES`], the fingerprint of the photo attachment, which
/// the server sends without a path.
const PHOTO_SHA: &str = "be04c407026cf352d54a051993ef2fec8153cc554f8ff7c697b225c87a6e5e03";
/// 13 bytes.
const MENU_BYTES: &[u8] = b"%PDF-1.4 menu";
/// 9 bytes.
const PHOTO_BYTES: &[u8] = b"PNG photo";
/// The file a pull of `+15555550101` from `sms-backup-restore` writes.
const CONVERSATION_FILE: &str = "+15555550101__sms-backup-restore.jsonl";
/// The id the mock server gives every run it records.
const EXPORT_ID: i64 = 7;

/// The menu attachment as the server serializes it, at `path`.
fn menu_attachment(path: Value) -> Value {
    json!({
        "path": path,
        "original_name": "menu.pdf",
        "mime_type": "application/pdf",
        "sha256": MENU_SHA
    })
}

/// The photo attachment: no `path`, so the pull files it under its fingerprint.
fn photo_attachment() -> Value {
    json!({
        "path": null,
        "original_name": "photo.png",
        "mime_type": "image/png",
        "sha256": PHOTO_SHA
    })
}

/// One exported message as `GET /v1/exports/{id}/messages` serializes it: an
/// individual SMS conversation with Sam, `service` on the message rather than
/// on the conversation, received at the holder's own `+15555550100`.
fn message(
    id: i64,
    source: &str,
    guid: &str,
    timestamp: &str,
    text: &str,
    attachments: Value,
) -> Value {
    json!({
        "id": id,
        "source": source,
        "service": "sms",
        "guid": guid,
        "timestamp": timestamp,
        "sort_order": id,
        "is_from_me": false,
        "sender": "+15555550101",
        "owner": "+15555550100",
        "subject": null,
        "text": text,
        "is_announcement": false,
        "is_reply": false,
        "thread_originator_guid": null,
        "thread_originator_part": null,
        "num_replies": 0,
        "conversation": {
            "id": 9,
            "chat_identifier": "+15555550101",
            "conversation_type": "individual",
            "group_title": null,
            "participants": [
                { "name": "Sam", "handle": "+15555550101", "service": "sms", "contact_id": 3 }
            ]
        },
        "attachments": attachments,
        "tapbacks": []
    })
}

/// The run the mock server records for `scope`, in `status`.
fn export_run(scope: Value, status: &str) -> Value {
    json!({
        "id": EXPORT_ID,
        "scope": scope,
        "tool": "message-crate-pull",
        "status": status,
        "started_at": "2026-09-08T12:00:00Z",
        "finished_at": if status == "running" { Value::Null } else { json!("2026-09-08T12:00:09Z") },
        "message_count": 3,
        "conversation_count": 1,
        "attachment_count": 2,
        "total_bytes": 22,
        "messages_delivered": if status == "running" { 0 } else { 3 }
    })
}

/// The login: the token resolves to account `1`, username `alice`.
fn mock_auth(server: &MockServer) -> httpmock::Mock<'_> {
    server.mock(|when, then| {
        when.method(GET).path("/v1/session");
        then.status(200)
            .json_body(json!({ "account_id": 1, "username": "alice" }));
    })
}

/// `POST /v1/exports` for exactly `scope`, recorded as `message-crate-pull`'s run.
fn mock_create<'a>(server: &'a MockServer, scope: Value) -> httpmock::Mock<'a> {
    let body = json!({ "scope": scope.clone(), "tool": "message-crate-pull" });
    server.mock(move |when, then| {
        when.method(POST)
            .path("/v1/exports")
            .json_body(body.clone());
        then.status(201)
            .header("location", format!("/v1/exports/{EXPORT_ID}"))
            .json_body(export_run(scope.clone(), "running"));
    })
}

/// `POST /v1/exports/{id}/complete`, answering the closed run.
fn mock_complete(server: &MockServer) -> httpmock::Mock<'_> {
    server.mock(|when, then| {
        when.method(POST)
            .path(format!("/v1/exports/{EXPORT_ID}/complete"));
        then.status(200)
            .json_body(export_run(json!({ "kind": "everything" }), "completed"));
    })
}

/// `POST /v1/exports/{id}/cancel`, answering the closed run.
fn mock_cancel(server: &MockServer) -> httpmock::Mock<'_> {
    server.mock(|when, then| {
        when.method(POST)
            .path(format!("/v1/exports/{EXPORT_ID}/cancel"));
        then.status(200)
            .json_body(export_run(json!({ "kind": "everything" }), "cancelled"));
    })
}

/// The run bookends every test below needs: the everything-scope create and
/// its complete.
fn mock_run(server: &MockServer) -> (httpmock::Mock<'_>, httpmock::Mock<'_>) {
    (
        mock_create(server, json!({ "kind": "everything" })),
        mock_complete(server),
    )
}

/// Two pages of `GET /v1/exports/{id}/messages` at two messages a page:
/// messages 1 and 2 with `total` 3, then message 3. The menu is on messages
/// 1 and 3, so its second mention must not download twice.
fn mock_pages<'a>(
    server: &'a MockServer,
    source: &str,
) -> (httpmock::Mock<'a>, httpmock::Mock<'a>) {
    let path = format!("/v1/exports/{EXPORT_ID}/messages");
    let first = server.mock(|when, then| {
        when.method(GET)
            .path(&path)
            .query_param("limit", "2")
            .query_param("offset", "0");
        then.status(200).json_body(json!({
            "items": [
                message(
                    1, source, "guid-1", "2015-03-12T18:05:01Z", "dinner at seven?",
                    json!([menu_attachment(json!("attachments/menu.pdf"))])
                ),
                message(
                    2, source, "guid-2", "2015-03-12T18:05:02Z", "here is the photo",
                    json!([photo_attachment()])
                )
            ],
            "total": 3,
            "limit": 2,
            "offset": 0
        }));
    });
    let second = server.mock(|when, then| {
        when.method(GET)
            .path(&path)
            .query_param("limit", "2")
            .query_param("offset", "2");
        then.status(200).json_body(json!({
            "items": [
                message(
                    3, source, "guid-3", "2015-03-12T18:05:03Z", "the menu again",
                    json!([menu_attachment(json!("attachments/menu.pdf"))])
                )
            ],
            "total": 3,
            "limit": 2,
            "offset": 2
        }));
    });
    (first, second)
}

/// `GET /v1/assets/{sha256}`, answering `bytes`. The fingerprint alone names
/// the attachment and the token names the account, and the server refuses a
/// parameter a route does not declare, so a download that still sent
/// `source=` or `account=` would not match.
fn mock_asset<'a>(server: &'a MockServer, sha256: &str, bytes: &[u8]) -> httpmock::Mock<'a> {
    server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v1/assets/{sha256}"))
            .query_param_missing("source")
            .query_param_missing("account");
        then.status(200)
            .header("content-type", "application/octet-stream")
            .body(bytes);
    })
}

/// A pull of every message into `out_dir`: two messages a page, one download
/// worker so the counts in the log are fixed.
fn config(out_dir: &Path, base_url: String) -> PullConfig {
    PullConfig {
        out_dir: out_dir.to_path_buf(),
        base_url,
        username: "alice".into(),
        token: "mc_test".into(),
        query: String::new(),
        list: ExportQueryList::Messages,
        skip_attachments: false,
        page_limit: 2,
        cancel: None,
        asset_download_workers: 1,
    }
}

/// The report `run` returns for the three-message fixture.
fn report_for(out_dir: &Path, downloaded: u64, skipped: u64) -> PullReport {
    PullReport {
        account: 1,
        export_id: EXPORT_ID,
        query: String::new(),
        conversations: 1,
        messages: 3,
        attachments_downloaded: downloaded,
        attachments_skipped: skipped,
        refused_attachment_paths: Vec::new(),
        out_dir: out_dir.display().to_string(),
    }
}

#[test]
fn a_pull_records_one_run_and_writes_the_conversation_and_every_asset_once_across_two_pages() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let (create, complete) = mock_run(&server);
    let (first, second) = mock_pages(&server, "sms-backup-restore");
    let menu = mock_asset(&server, MENU_SHA, MENU_BYTES);
    let photo = mock_asset(&server, PHOTO_SHA, PHOTO_BYTES);
    let dir = tempdir().unwrap();
    let out = dir.path().join("pulled");

    let report = run(&config(&out, server.base_url()), None).unwrap();

    assert_eq!(report, report_for(&out, 2, 0));
    create.assert();
    first.assert();
    second.assert();
    menu.assert();
    photo.assert();
    complete.assert();
    assert!(out.join(EXPORT_SENTINEL).is_file());
    assert_eq!(
        fs::read(out.join("attachments/menu.pdf")).unwrap(),
        MENU_BYTES
    );
    assert_eq!(
        fs::read(out.join("attachments").join(PHOTO_SHA)).unwrap(),
        PHOTO_BYTES
    );
    assert!(
        !out.join("attachments/menu.part").exists(),
        "the .part file is renamed into place"
    );
    let doc = read_conversation_jsonl(&out.join(CONVERSATION_FILE)).unwrap();
    assert_eq!(doc.export.source, "sms-backup-restore");
    assert_eq!(doc.conversation.chat_identifier, "+15555550101");
    assert_eq!(
        doc.conversation.participants[0].display_name.as_deref(),
        Some("Sam")
    );
    assert_eq!(doc.conversation.stats.message_count, 3);
    assert_eq!(doc.conversation.stats.attachment_count, 3);
    assert_eq!(
        doc.messages
            .iter()
            .map(|m| m.guid.as_str())
            .collect::<Vec<_>>(),
        ["guid-1", "guid-2", "guid-3"]
    );
    assert_eq!(doc.messages[0].text, "dinner at seven?");
    // The address the server holds each message at comes out on the message,
    // and on the header since every message shares it (#1098).
    assert_eq!(
        doc.messages
            .iter()
            .map(|m| m.owner_handle.as_deref())
            .collect::<Vec<_>>(),
        [Some("+15555550100"); 3]
    );
    assert_eq!(doc.export.owner_handle.as_deref(), Some("+15555550100"));
    assert_eq!(
        doc.messages[0].attachments[0].digest_sha256.as_deref(),
        Some(MENU_SHA)
    );
    assert_eq!(
        doc.messages[1].attachments[0].path.as_deref(),
        Some(format!("attachments/{PHOTO_SHA}").as_str())
    );
}

/// The server can hold an attachment path that climbs out of a folder or
/// names an absolute one, because an import that reuses a stored fingerprint
/// never read the file at that path. Joined onto the output folder, such a
/// path would write the download anywhere on disk. Each one is refused: the
/// file lands at `attachments/{sha256}`, the conversation file names that
/// path, and the report and the log name the refused path.
#[test]
fn an_attachment_path_that_leaves_the_output_folder_is_written_under_its_fingerprint_instead() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let (_create, complete) = mock_run(&server);
    let dir = tempdir().unwrap();
    let out = dir.path().join("pulled");
    let climbing = "../escape.pdf";
    let absolute = dir.path().join("absolute.png").display().to_string();
    let page = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v1/exports/{EXPORT_ID}/messages"))
            .query_param("offset", "0");
        then.status(200).json_body(json!({
            "items": [
                message(
                    1, "sms-backup-restore", "guid-1", "2015-03-12T18:05:01Z", "the menu",
                    json!([menu_attachment(json!(climbing))])
                ),
                message(
                    2, "sms-backup-restore", "guid-2", "2015-03-12T18:05:02Z", "the photo",
                    json!([{
                        "path": absolute,
                        "original_name": "photo.png",
                        "mime_type": "image/png",
                        "sha256": PHOTO_SHA
                    }])
                )
            ],
            "total": 2,
            "limit": 2,
            "offset": 0
        }));
    });
    let _menu = mock_asset(&server, MENU_SHA, MENU_BYTES);
    let _photo = mock_asset(&server, PHOTO_SHA, PHOTO_BYTES);
    let mut events = Vec::new();
    let mut on_progress = |event: ProgressEvent| events.push(event);

    let report = run(&config(&out, server.base_url()), Some(&mut on_progress)).unwrap();

    page.assert();
    complete.assert();
    assert!(!dir.path().join("escape.pdf").exists());
    assert!(!dir.path().join("absolute.png").exists());
    assert_eq!(
        fs::read(out.join("attachments").join(MENU_SHA)).unwrap(),
        MENU_BYTES
    );
    assert_eq!(
        fs::read(out.join("attachments").join(PHOTO_SHA)).unwrap(),
        PHOTO_BYTES
    );
    let doc = read_conversation_jsonl(&out.join(CONVERSATION_FILE)).unwrap();
    let written: Vec<Option<&str>> = doc
        .messages
        .iter()
        .map(|m| m.attachments[0].path.as_deref())
        .collect();
    assert_eq!(
        written,
        [
            Some(format!("attachments/{MENU_SHA}").as_str()),
            Some(format!("attachments/{PHOTO_SHA}").as_str()),
        ]
    );
    let mut refused = vec![climbing.to_string(), absolute.clone()];
    refused.sort();
    assert_eq!(report.refused_attachment_paths, refused);
    for (path, sha) in [(climbing, MENU_SHA), (absolute.as_str(), PHOTO_SHA)] {
        let line = format!(
            "warning: attachment path {path} would leave the output folder; written at attachments/{sha} instead"
        );
        assert!(
            events.contains(&ProgressEvent::Log(line.clone())),
            "the log names {path}: {events:?}"
        );
    }
}

#[test]
fn the_journal_lists_every_asset_and_marks_the_run_finished() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let _run = mock_run(&server);
    let _pages = mock_pages(&server, "sms-backup-restore");
    let _menu = mock_asset(&server, MENU_SHA, MENU_BYTES);
    let _photo = mock_asset(&server, PHOTO_SHA, PHOTO_BYTES);
    let dir = tempdir().unwrap();
    let out = dir.path().join("pulled");

    run(&config(&out, server.base_url()), None).unwrap();

    let state = journal::load(&journal::journal_path(&out), &server.base_url(), "alice").unwrap();
    assert_eq!(
        state.assets,
        HashSet::from([MENU_SHA.to_string(), PHOTO_SHA.to_string()])
    );
    assert!(state.backup_complete);
}

#[test]
fn a_second_run_over_the_same_folder_downloads_nothing_it_already_has() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let (create, complete) = mock_run(&server);
    let _pages = mock_pages(&server, "sms-backup-restore");
    let menu = mock_asset(&server, MENU_SHA, MENU_BYTES);
    let photo = mock_asset(&server, PHOTO_SHA, PHOTO_BYTES);
    let dir = tempdir().unwrap();
    let out = dir.path().join("pulled");
    let cfg = config(&out, server.base_url());
    run(&cfg, None).unwrap();

    let mut lines = Vec::new();
    let report = {
        let mut progress = |event| {
            if let ProgressEvent::Log(line) = event {
                lines.push(line);
            }
        };
        run(&cfg, Some(&mut progress)).unwrap()
    };

    assert_eq!(report, report_for(&out, 0, 2));
    assert_eq!(menu.calls(), 1);
    assert_eq!(photo.calls(), 1);
    // Every pull is its own run, whether or not it downloads anything.
    assert_eq!(create.calls(), 2);
    assert_eq!(complete.calls(), 2);
    assert_eq!(
        lines,
        [
            "Authenticated as alice (1)".to_string(),
            "Backup query: (all messages)".to_string(),
            "Previous backup completed successfully. Running to check for new messages…"
                .to_string(),
            "Export run 7: 3 message(s) in 1 conversation(s), 2 attachment(s) (22 B)".to_string(),
            format!("Wrote 1 conversation(s), 3 message(s) → {}", out.display()),
        ]
    );
}

#[test]
fn a_file_the_journal_lists_but_the_disk_lost_is_fetched_again() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let _run = mock_run(&server);
    let _pages = mock_pages(&server, "sms-backup-restore");
    let menu = mock_asset(&server, MENU_SHA, MENU_BYTES);
    let photo = mock_asset(&server, PHOTO_SHA, PHOTO_BYTES);
    let dir = tempdir().unwrap();
    let out = dir.path().join("pulled");
    let cfg = config(&out, server.base_url());
    run(&cfg, None).unwrap();
    fs::remove_file(out.join("attachments/menu.pdf")).unwrap();

    let report = run(&cfg, None).unwrap();

    assert_eq!(report, report_for(&out, 1, 1));
    assert_eq!(menu.calls(), 2);
    assert_eq!(photo.calls(), 1);
    assert_eq!(
        fs::read(out.join("attachments/menu.pdf")).unwrap(),
        MENU_BYTES
    );
}

#[test]
fn a_cancel_requested_before_the_run_records_nothing_on_the_server() {
    let server = MockServer::start();
    let auth = mock_auth(&server);
    let (create, complete) = mock_run(&server);
    let cancel = mock_cancel(&server);
    let (first, _second) = mock_pages(&server, "sms-backup-restore");
    let dir = tempdir().unwrap();
    let out = dir.path().join("pulled");
    let cfg = PullConfig {
        cancel: Some(Arc::new(AtomicBool::new(true))),
        ..config(&out, server.base_url())
    };

    let error = run(&cfg, None).unwrap_err();

    assert_eq!(error.to_string(), "cancelled");
    assert_eq!(auth.calls(), 1);
    assert_eq!(
        create.calls(),
        0,
        "a run the caller gave up on is never recorded"
    );
    assert_eq!(first.calls(), 0);
    assert_eq!(complete.calls(), 0);
    assert_eq!(cancel.calls(), 0);
    assert!(!out.join(CONVERSATION_FILE).exists());
    let state = journal::load(&journal::journal_path(&out), &server.base_url(), "alice").unwrap();
    assert!(!state.backup_complete);
}

#[test]
fn a_source_name_with_spaces_and_brackets_becomes_a_file_safe_suffix() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let _run = mock_run(&server);
    let _pages = mock_pages(&server, "whatsapp (phone 2)");
    let _menu = mock_asset(&server, MENU_SHA, MENU_BYTES);
    let _photo = mock_asset(&server, PHOTO_SHA, PHOTO_BYTES);
    let dir = tempdir().unwrap();
    let out = dir.path().join("pulled");

    run(&config(&out, server.base_url()), None).unwrap();

    let doc = read_conversation_jsonl(&out.join("+15555550101__whatsapp__phone_2_.jsonl")).unwrap();
    assert_eq!(doc.export.source, "whatsapp (phone 2)");
    assert_eq!(doc.conversation.stats.message_count, 3);
}

#[test]
fn two_groups_with_one_title_are_both_written() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let _run = mock_run(&server);
    // One message in each of two "Family" groups from the same source.
    let group_message = |id: i64, conversation_id: i64, chat: &str, text: &str| {
        let mut message = message(
            id,
            "imessage",
            &format!("guid-{id}"),
            "2015-03-12T18:05:01Z",
            text,
            json!([]),
        );
        message["conversation"]["id"] = json!(conversation_id);
        message["conversation"]["chat_identifier"] = json!(chat);
        message["conversation"]["conversation_type"] = json!("group");
        message["conversation"]["group_title"] = json!("Family");
        message
    };
    let _page = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v1/exports/{EXPORT_ID}/messages"))
            .query_param("offset", "0");
        then.status(200).json_body(json!({
            "items": [
                group_message(1, 9, "chat-one", "first family"),
                group_message(2, 10, "chat-two", "second family")
            ],
            "total": 2,
            "limit": 2,
            "offset": 0
        }));
    });
    let dir = tempdir().unwrap();
    let out = dir.path().join("pulled");

    run(&config(&out, server.base_url()), None).unwrap();

    let mut chats: Vec<String> = fs::read_dir(&out)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            // The pull's own journal is a hidden JSONL file beside them.
            let name = e.file_name().to_string_lossy().to_string();
            name.ends_with(".jsonl") && !name.starts_with('.')
        })
        .map(|e| {
            read_conversation_jsonl(&e.path())
                .unwrap()
                .conversation
                .chat_identifier
        })
        .collect();
    chats.sort();
    assert_eq!(
        chats,
        ["chat-one", "chat-two"],
        "neither group's file replaces the other's"
    );
}

#[test]
fn skipping_attachments_writes_messages_without_files_or_downloads() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let _run = mock_run(&server);
    let _pages = mock_pages(&server, "sms-backup-restore");
    let menu = mock_asset(&server, MENU_SHA, MENU_BYTES);
    let photo = mock_asset(&server, PHOTO_SHA, PHOTO_BYTES);
    let dir = tempdir().unwrap();
    let out = dir.path().join("pulled");
    let cfg = PullConfig {
        skip_attachments: true,
        ..config(&out, server.base_url())
    };

    let report = run(&cfg, None).unwrap();

    assert_eq!(report, report_for(&out, 0, 0));
    assert_eq!(menu.calls(), 0);
    assert_eq!(photo.calls(), 0);
    assert!(!out.join("attachments").exists());
    let doc = read_conversation_jsonl(&out.join(CONVERSATION_FILE)).unwrap();
    assert_eq!(doc.conversation.stats.attachment_count, 0);
    assert!(doc.messages[0].attachments.is_empty());
}

#[test]
fn a_query_becomes_the_runs_query_scope_and_progress_narrates_the_run() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let create = mock_create(
        &server,
        json!({ "kind": "query", "list": "messages", "q": "from:sam" }),
    );
    let complete = mock_complete(&server);
    let _pages = mock_pages(&server, "sms-backup-restore");
    let _menu = mock_asset(&server, MENU_SHA, MENU_BYTES);
    let _photo = mock_asset(&server, PHOTO_SHA, PHOTO_BYTES);
    let dir = tempdir().unwrap();
    let out = dir.path().join("pulled");
    let cfg = PullConfig {
        query: " from:sam ".into(),
        ..config(&out, server.base_url())
    };

    let mut events = Vec::new();
    let report = {
        let mut progress = |event| events.push(event);
        run(&cfg, Some(&mut progress)).unwrap()
    };

    create.assert();
    complete.assert();
    assert_eq!(
        events,
        vec![
            ProgressEvent::Auth {
                account_id: 1,
                username: "alice".into(),
            },
            ProgressEvent::Log("Authenticated as alice (1)".into()),
            ProgressEvent::Log("Backup query: from:sam".into()),
            ProgressEvent::Log(
                "Export run 7: 3 message(s) in 1 conversation(s), 2 attachment(s) (22 B)".into()
            ),
            ProgressEvent::Page {
                messages: 2,
                total_so_far: 2,
            },
            ProgressEvent::Page {
                messages: 1,
                total_so_far: 3,
            },
            ProgressEvent::Log(
                "Downloading 2 unique asset(s) with 1 worker(s) (0 skipped from journal)…".into()
            ),
            ProgressEvent::Log("Assets: 2 downloaded, 0 skipped (22 B total bytes)".into()),
            ProgressEvent::Log(format!(
                "Wrote 1 conversation(s), 3 message(s) → {}",
                out.display()
            )),
            ProgressEvent::Done(report),
        ]
    );
}

/// A query from the Conversations list must reach the server as one: sent
/// for the Messages list it would be refused or hand over fewer messages.
#[test]
fn a_query_for_the_conversations_list_names_that_list_in_the_scope() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let create = mock_create(
        &server,
        json!({ "kind": "query", "list": "conversations", "q": "messages:>100" }),
    );
    let complete = mock_complete(&server);
    let _pages = mock_pages(&server, "sms-backup-restore");
    let dir = tempdir().unwrap();
    let out = dir.path().join("pulled");
    let cfg = PullConfig {
        query: "messages:>100".into(),
        list: ExportQueryList::Conversations,
        skip_attachments: true,
        ..config(&out, server.base_url())
    };

    let mut events = Vec::new();
    {
        let mut progress = |event| events.push(event);
        run(&cfg, Some(&mut progress)).unwrap();
    }

    create.assert();
    complete.assert();
    assert!(events.contains(&ProgressEvent::Log(
        "Backup query: messages:>100 (every message of the conversations it finds)".into()
    )));
}

#[test]
fn an_asset_the_server_does_not_have_fails_the_run_and_cancels_it_on_the_server() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let (create, complete) = mock_run(&server);
    let cancel = mock_cancel(&server);
    let _pages = mock_pages(&server, "sms-backup-restore");
    let menu = server.mock(|when, then| {
        when.method(GET).path(format!("/v1/assets/{MENU_SHA}"));
        then.status(404).json_body(json!({
            "type": "https://messagecrate.app/docs/developer/reference/errors/not-found",
            "title": "Not found",
            "status": 404,
            "detail": "asset not found"
        }));
    });
    let _photo = mock_asset(&server, PHOTO_SHA, PHOTO_BYTES);
    let dir = tempdir().unwrap();
    let out = dir.path().join("pulled");

    let error = run(&config(&out, server.base_url()), None).unwrap_err();

    assert_eq!(
        error.to_string(),
        format!("asset download failed: asset not found: {MENU_SHA}")
    );
    assert_eq!(menu.calls(), 1, "a 404 Not Found is not retried");
    create.assert();
    cancel.assert();
    assert_eq!(complete.calls(), 0);
    assert!(!out.join("attachments/menu.pdf").exists());
    assert!(!out.join(CONVERSATION_FILE).exists());
    let state = journal::load(&journal::journal_path(&out), &server.base_url(), "alice").unwrap();
    assert!(!state.backup_complete);
}

/// An access proxy whose session has expired answers the asset request with
/// its login page and `200 OK`. Those bytes are not the photo, so the run
/// fails naming the photo's fingerprint, and nothing at the photo's path or in
/// the journal lets a later run skip it.
#[test]
fn bytes_whose_sha256_is_not_the_one_asked_for_fail_the_run_and_are_not_kept() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let (create, complete) = mock_run(&server);
    let cancel = mock_cancel(&server);
    let _pages = mock_pages(&server, "sms-backup-restore");
    let _menu = mock_asset(&server, MENU_SHA, MENU_BYTES);
    let photo = mock_asset(
        &server,
        PHOTO_SHA,
        b"<html><body>Sign in to continue</body></html>",
    );
    let dir = tempdir().unwrap();
    let out = dir.path().join("pulled");

    let error = run(&config(&out, server.base_url()), None).unwrap_err();

    let message = error.to_string();
    assert!(
        message.starts_with("asset download failed: ") && message.contains(PHOTO_SHA),
        "{message}"
    );
    assert_eq!(
        photo.calls(),
        1,
        "the same answer would come back, so it is not retried"
    );
    create.assert();
    cancel.assert();
    assert_eq!(complete.calls(), 0);
    let photo_path = out.join("attachments").join(PHOTO_SHA);
    assert!(
        !photo_path.exists(),
        "the login page is not kept as the photo"
    );
    assert!(
        !photo_path.with_extension("part").exists(),
        "the .part file is removed"
    );
    let state = journal::load(&journal::journal_path(&out), &server.base_url(), "alice").unwrap();
    assert!(
        !state.assets.contains(PHOTO_SHA),
        "the photo is not journalled"
    );
    assert!(!state.backup_complete);
}

#[test]
fn a_scope_the_server_refuses_fails_the_run_with_the_servers_sentence() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let create = server.mock(|when, then| {
        when.method(POST).path("/v1/exports");
        then.status(400)
            .header("content-type", "application/problem+json")
            .json_body(json!({
                "type": "https://messagecrate.app/docs/developer/reference/errors/search-query-invalid",
                "title": "Search query invalid",
                "status": 400,
                "detail": "wibble: is not a word the Messages list has"
            }));
    });
    let (first, _second) = mock_pages(&server, "sms-backup-restore");
    let dir = tempdir().unwrap();
    let out = dir.path().join("pulled");
    let cfg = PullConfig {
        query: "wibble:yes".into(),
        ..config(&out, server.base_url())
    };

    let error = run(&cfg, None).unwrap_err();

    assert_eq!(
        error.to_string(),
        "create export failed (HTTP 400 Bad Request): wibble: is not a word the Messages list has"
    );
    assert_eq!(create.calls(), 1, "a 400 is not retried");
    assert_eq!(first.calls(), 0);
}

#[test]
fn a_blank_token_or_output_folder_is_refused_before_login() {
    let dir = tempdir().unwrap();
    let base_url = "http://127.0.0.1:1".to_string();
    let blank_key = PullConfig {
        token: "  ".into(),
        ..config(dir.path(), base_url.clone())
    };
    let blank_out_dir = PullConfig {
        out_dir: Path::new("").to_path_buf(),
        ..config(dir.path(), base_url)
    };

    assert_eq!(
        run(&blank_key, None).unwrap_err().to_string(),
        "session token is required"
    );
    assert_eq!(
        run(&blank_out_dir, None).unwrap_err().to_string(),
        "output directory is required"
    );
}

/// The desktop app sends a session token, so a session the server refuses
/// (expired, or revoked from another window) says to log in again and names
/// no API key (#1400).
#[test]
fn a_refused_session_says_to_log_in_again() {
    let server = MockServer::start();
    let _session = server.mock(|when, then| {
        when.method(GET).path("/v1/session");
        then.status(401).body("unauthorized");
    });
    let dir = tempdir().unwrap();

    let message = format!(
        "{:#}",
        run(&config(dir.path(), server.base_url()), None).unwrap_err()
    );

    assert!(message.contains("Log in again"), "{message}");
    assert!(!message.contains("API key"), "{message}");
}

/// Staging names a file by date and fingerprint, so one menu sent on two days
/// has two paths on the server. The menu downloads once, and every path a
/// message names exists after the pull holding the menu's bytes.
#[test]
fn every_path_a_message_names_exists_after_a_pull() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let (_create, _complete) = mock_run(&server);
    let _page = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v1/exports/{EXPORT_ID}/messages"))
            .query_param("limit", "2")
            .query_param("offset", "0");
        then.status(200).json_body(json!({
            "items": [
                message(
                    1, "sms-backup-restore", "guid-1", "2015-03-12T18:05:01Z", "menu",
                    json!([menu_attachment(json!("attachments/20150312_180501-0a0a0a0a0a0a0a0a.pdf"))])
                ),
                message(
                    2, "sms-backup-restore", "guid-2", "2016-04-01T10:00:00Z", "menu again",
                    json!([menu_attachment(json!("attachments/20160401_100000-0a0a0a0a0a0a0a0a.pdf"))])
                )
            ],
            "total": 2,
            "limit": 2,
            "offset": 0
        }));
    });
    let menu = mock_asset(&server, MENU_SHA, MENU_BYTES);
    let dir = tempdir().unwrap();
    let out = dir.path().join("pulled");

    run(&config(&out, server.base_url()), None).unwrap();

    menu.assert_calls(1);
    let doc = read_conversation_jsonl(&out.join(CONVERSATION_FILE)).unwrap();
    for msg in &doc.messages {
        let rel = msg.attachments[0].path.as_deref().unwrap();
        assert_eq!(
            fs::read(out.join(rel)).ok().as_deref(),
            Some(MENU_BYTES),
            "message {} names {rel}, which the pull never wrote",
            msg.guid
        );
    }
}
