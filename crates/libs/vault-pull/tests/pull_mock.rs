//! Mock vault tests for one pull: login, two pages of messages, asset
//! download, the journal a second run reads, and the progress a caller sees.
//!
//! The mock answers the three routes `run` calls — `GET /v1/session`,
//! `GET /v1/export/messages`, and `GET /v1/assets/{sha256}` — with the JSON
//! the vault serializes (`vault-api-types`, `docs/src/assets/openapi.json`).
//! Every request derives from `VaultPullConfig::base_url`, so the mock's
//! address is the only seam.

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use httpmock::prelude::*;
use message_ir_format::{EXPORT_SENTINEL, read_conversation_jsonl};
use serde_json::{Value, json};
use tempfile::tempdir;
use vault_pull::{ProgressEvent, PullReport, VaultPullConfig, journal, run};

/// Fingerprint of the menu attachment. The pull never hashes what it
/// downloads, so any 64 hex characters name an asset.
const MENU_SHA: &str = "0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a";
/// Fingerprint of the photo attachment, which the vault sends without a path.
const PHOTO_SHA: &str = "0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b";
/// 13 bytes.
const MENU_BYTES: &[u8] = b"%PDF-1.4 menu";
/// 9 bytes.
const PHOTO_BYTES: &[u8] = b"PNG photo";
/// The file a pull of `+15555550101` from `sms-backup-restore` writes.
const CONVERSATION_FILE: &str = "+15555550101__sms-backup-restore.jsonl";

/// The menu attachment as the vault serializes it, at `path`.
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

/// One exported message as `GET /v1/export/messages` serializes it: an
/// individual SMS conversation with Sam, `service` on the message rather than
/// on the conversation.
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

/// The login: the key resolves to account `1`, username `alice`.
fn mock_auth(server: &MockServer) -> httpmock::Mock<'_> {
    server.mock(|when, then| {
        when.method(GET).path("/v1/session");
        then.status(200)
            .json_body(json!({ "account_id": 1, "username": "alice" }));
    })
}

/// Two pages of `GET /v1/export/messages` for `q` at two messages a page:
/// messages 1 and 2 with `total` 3, then message 3. The menu is on messages
/// 1 and 3, so its second mention must not download twice.
fn mock_pages<'a>(
    server: &'a MockServer,
    source: &str,
    q: &str,
) -> (httpmock::Mock<'a>, httpmock::Mock<'a>) {
    let first = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/export/messages")
            .query_param("q", q)
            .query_param("limit", "2")
            .query_param("offset", "0")
            .query_param("account", "1");
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
            .path("/v1/export/messages")
            .query_param("q", q)
            .query_param("limit", "2")
            .query_param("offset", "2")
            .query_param("account", "1");
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

/// `GET /v1/assets/{sha256}` for `source`, answering `bytes`.
fn mock_asset<'a>(
    server: &'a MockServer,
    sha256: &str,
    source: &str,
    bytes: &[u8],
) -> httpmock::Mock<'a> {
    server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v1/assets/{sha256}"))
            .query_param("source", source)
            .query_param("account", "1");
        then.status(200)
            .header("content-type", "application/octet-stream")
            .body(bytes);
    })
}

/// A pull of every message into `out_dir`: two messages a page, one download
/// worker so the counts in the log are fixed.
fn config(out_dir: &Path, base_url: String) -> VaultPullConfig {
    VaultPullConfig {
        out_dir: out_dir.to_path_buf(),
        base_url,
        username: "alice".into(),
        key: "mv_test".into(),
        query: String::new(),
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
        query: String::new(),
        conversations: 1,
        messages: 3,
        attachments_downloaded: downloaded,
        attachments_skipped: skipped,
        out_dir: out_dir.display().to_string(),
    }
}

#[test]
fn a_pull_writes_the_conversation_and_every_asset_once_across_two_pages() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let (first, second) = mock_pages(&server, "sms-backup-restore", "");
    let menu = mock_asset(&server, MENU_SHA, "sms-backup-restore", MENU_BYTES);
    let photo = mock_asset(&server, PHOTO_SHA, "sms-backup-restore", PHOTO_BYTES);
    let dir = tempdir().unwrap();
    let out = dir.path().join("pulled");

    let report = run(&config(&out, server.base_url()), None).unwrap();

    assert_eq!(report, report_for(&out, 2, 0));
    first.assert();
    second.assert();
    menu.assert();
    photo.assert();
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
    assert_eq!(
        doc.messages[0].attachments[0].digest_sha256.as_deref(),
        Some(MENU_SHA)
    );
    assert_eq!(
        doc.messages[1].attachments[0].path.as_deref(),
        Some(format!("attachments/{PHOTO_SHA}").as_str())
    );
}

#[test]
fn the_journal_lists_every_asset_and_marks_the_run_finished() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let _pages = mock_pages(&server, "sms-backup-restore", "");
    let _menu = mock_asset(&server, MENU_SHA, "sms-backup-restore", MENU_BYTES);
    let _photo = mock_asset(&server, PHOTO_SHA, "sms-backup-restore", PHOTO_BYTES);
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
    let _pages = mock_pages(&server, "sms-backup-restore", "");
    let menu = mock_asset(&server, MENU_SHA, "sms-backup-restore", MENU_BYTES);
    let photo = mock_asset(&server, PHOTO_SHA, "sms-backup-restore", PHOTO_BYTES);
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
    assert_eq!(
        lines,
        [
            "Authenticated as alice (1)".to_string(),
            "Backup query: (all messages)".to_string(),
            "Previous backup completed successfully. Running to check for new messages…"
                .to_string(),
            format!("Wrote 1 conversation(s), 3 message(s) → {}", out.display()),
        ]
    );
}

#[test]
fn a_file_the_journal_lists_but_the_disk_lost_is_fetched_again() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let _pages = mock_pages(&server, "sms-backup-restore", "");
    let menu = mock_asset(&server, MENU_SHA, "sms-backup-restore", MENU_BYTES);
    let photo = mock_asset(&server, PHOTO_SHA, "sms-backup-restore", PHOTO_BYTES);
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
fn a_cancel_requested_before_the_run_stops_it_before_the_first_page() {
    let server = MockServer::start();
    let auth = mock_auth(&server);
    let (first, _second) = mock_pages(&server, "sms-backup-restore", "");
    let dir = tempdir().unwrap();
    let out = dir.path().join("pulled");
    let cfg = VaultPullConfig {
        cancel: Some(Arc::new(AtomicBool::new(true))),
        ..config(&out, server.base_url())
    };

    let error = run(&cfg, None).unwrap_err();

    assert_eq!(error.to_string(), "cancelled");
    assert_eq!(auth.calls(), 1);
    assert_eq!(first.calls(), 0);
    assert!(!out.join(CONVERSATION_FILE).exists());
    let state = journal::load(&journal::journal_path(&out), &server.base_url(), "alice").unwrap();
    assert!(!state.backup_complete);
}

#[test]
fn a_source_name_with_spaces_and_brackets_becomes_a_file_safe_suffix() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let _pages = mock_pages(&server, "whatsapp (phone 2)", "");
    let _menu = mock_asset(&server, MENU_SHA, "whatsapp (phone 2)", MENU_BYTES);
    let _photo = mock_asset(&server, PHOTO_SHA, "whatsapp (phone 2)", PHOTO_BYTES);
    let dir = tempdir().unwrap();
    let out = dir.path().join("pulled");

    run(&config(&out, server.base_url()), None).unwrap();

    let doc = read_conversation_jsonl(&out.join("+15555550101__whatsapp__phone_2_.jsonl")).unwrap();
    assert_eq!(doc.export.source, "whatsapp (phone 2)");
    assert_eq!(doc.conversation.stats.message_count, 3);
}

#[test]
fn skipping_attachments_writes_messages_without_files_or_downloads() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let _pages = mock_pages(&server, "sms-backup-restore", "");
    let menu = mock_asset(&server, MENU_SHA, "sms-backup-restore", MENU_BYTES);
    let photo = mock_asset(&server, PHOTO_SHA, "sms-backup-restore", PHOTO_BYTES);
    let dir = tempdir().unwrap();
    let out = dir.path().join("pulled");
    let cfg = VaultPullConfig {
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
fn progress_events_narrate_login_paging_downloads_and_the_report() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let _pages = mock_pages(&server, "sms-backup-restore", "from:sam");
    let _menu = mock_asset(&server, MENU_SHA, "sms-backup-restore", MENU_BYTES);
    let _photo = mock_asset(&server, PHOTO_SHA, "sms-backup-restore", PHOTO_BYTES);
    let dir = tempdir().unwrap();
    let out = dir.path().join("pulled");
    let cfg = VaultPullConfig {
        query: " from:sam ".into(),
        ..config(&out, server.base_url())
    };

    let mut events = Vec::new();
    let report = {
        let mut progress = |event| events.push(event);
        run(&cfg, Some(&mut progress)).unwrap()
    };

    assert_eq!(
        events,
        vec![
            ProgressEvent::Auth {
                account_id: 1,
                username: "alice".into(),
            },
            ProgressEvent::Log("Authenticated as alice (1)".into()),
            ProgressEvent::Log("Backup query: from:sam".into()),
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

#[test]
fn an_asset_the_vault_does_not_have_fails_the_run_by_fingerprint() {
    let server = MockServer::start();
    let _auth = mock_auth(&server);
    let _pages = mock_pages(&server, "sms-backup-restore", "");
    let menu = server.mock(|when, then| {
        when.method(GET).path(format!("/v1/assets/{MENU_SHA}"));
        then.status(404).json_body(json!({
            "type": "https://bitrealm.io/vault/developer/reference/errors/not-found",
            "title": "Not found",
            "status": 404,
            "detail": "asset not found"
        }));
    });
    let _photo = mock_asset(&server, PHOTO_SHA, "sms-backup-restore", PHOTO_BYTES);
    let dir = tempdir().unwrap();
    let out = dir.path().join("pulled");

    let error = run(&config(&out, server.base_url()), None).unwrap_err();

    assert_eq!(
        error.to_string(),
        format!("asset download failed: asset not found: {MENU_SHA} (source=sms-backup-restore)")
    );
    assert_eq!(menu.calls(), 1, "a 404 Not Found is not retried");
    assert!(!out.join("attachments/menu.pdf").exists());
    assert!(!out.join(CONVERSATION_FILE).exists());
    let state = journal::load(&journal::journal_path(&out), &server.base_url(), "alice").unwrap();
    assert!(!state.backup_complete);
}

#[test]
fn a_blank_key_or_output_folder_is_refused_before_login() {
    let dir = tempdir().unwrap();
    let base_url = "http://127.0.0.1:1".to_string();
    let blank_key = VaultPullConfig {
        key: "  ".into(),
        ..config(dir.path(), base_url.clone())
    };
    let blank_out_dir = VaultPullConfig {
        out_dir: Path::new("").to_path_buf(),
        ..config(dir.path(), base_url)
    };

    assert_eq!(
        run(&blank_key, None).unwrap_err().to_string(),
        "vault key is required"
    );
    assert_eq!(
        run(&blank_out_dir, None).unwrap_err().to_string(),
        "output directory is required"
    );
}
