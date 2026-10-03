//! Mock HTTP server tests for login, JSON Lines push, and journal skip.
//!
//! JSON Lines means one JSON object per line. The journal is
//! `.import-state.jsonl`, a local log of which conversations and files
//! were already uploaded.

use message_crate_push::ImportMode;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use httpmock::prelude::*;
use message_crate_push::{AuthError, ProgressEvent, PushConfig, authenticate, run};
use message_ir::{
    ConversationDocument, ConversationMeta, ConversationStats, ExportMeta, IrAttachment,
    IrConversationType, IrDirection, IrMessage, IrMessageKind, IrParticipant, IrService,
    SCHEMA_VERSION,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use tempfile::tempdir;

/// One SMS conversation used by most mock-server tests.
fn sample_doc() -> ConversationDocument {
    ConversationDocument {
        schema_version: SCHEMA_VERSION,
        export: ExportMeta {
            source: "sms-backup-restore".into(),
            tool: "SMS Backup & Restore".into(),
            tool_version: "10.26.003".into(),
            owner_handle: Some("+15555550100".into()),
            owner_display_name: Some("Me".into()),
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
            text: "hello there".into(),
            attachments: vec![],
            imessage: None,
            source: None,
        }],
        packaging_stem_suffix: None,
    }
}

/// Same fixture as [`sample_doc`], with a different chat handle and message guid.
fn sample_doc_for(handle: &str, guid: &str) -> ConversationDocument {
    let mut doc = sample_doc();
    doc.conversation.chat_identifier = handle.into();
    doc.conversation.participants[0].handle = Some(handle.into());
    doc.messages[0].guid = guid.into();
    doc.messages[0].sender_handle = Some(handle.into());
    doc
}

/// Write `doc` as a JSON Lines file under `dir`.
fn write_jsonl(dir: &Path, doc: &ConversationDocument) {
    let stem = doc.filename_stem();
    let path = dir.join(format!("{stem}.jsonl"));
    let mut f = fs::File::create(&path).unwrap();
    let header = json!({
        "schema_version": doc.schema_version,
        "export": doc.export,
        "conversation": doc.conversation,
    });
    writeln!(f, "{}", serde_json::to_string(&header).unwrap()).unwrap();
    for msg in &doc.messages {
        writeln!(f, "{}", serde_json::to_string(msg).unwrap()).unwrap();
    }
}

/// The server's answer to `POST /v1/imports`: an Import Run with `id`. Every
/// push starts one, so every test mocks it; the batches then go to
/// `/v1/imports/{id}/batches`.
fn mock_import_run(server: &MockServer, id: i64) -> httpmock::Mock<'_> {
    server.mock(|when, then| {
        when.method(POST).path("/v1/imports");
        then.status(201)
            .header("Location", format!("/v1/imports/{id}"))
            .json_body(json!({ "id": id }));
    })
}

/// Push config with no retries, pointed at a mock server URL. Attachments are
/// not skipped (`skip_attachments: false`).
fn text_only_config(dir: &Path, base_url: String) -> PushConfig {
    PushConfig {
        input: dir.to_path_buf(),
        base_url,
        username: "alice".into(),
        key: "mc_test".into(),
        mode: ImportMode::Append,
        force: false,
        skip_attachments: false,
        verify_digests: false,
        trust_export: false,

        max_retries: 0,
        batch_size: 50,
        asset_upload_workers: 1,
        prepare_ahead: message_crate_push::DEFAULT_PREPARE_AHEAD,
        prepare_workers: message_crate_push::DEFAULT_PREPARE_WORKERS,
        asset_multipart_threshold: message_crate_push::MAX_PROXY_BODY_BYTES,
        asset_max_bytes: message_crate_push::DEFAULT_ASSET_MAX_BYTES,
        report_path: Some(dir.join("message-crate-push-report.json")),
        log_path: Some(dir.join("message-crate-push.log")),
        journal_path: Some(dir.join(".import-state.jsonl")),
        cancel: None,
        import_id: None,
    }
}

#[test]
fn authenticate_and_push_text_only_conversation() {
    let server = MockServer::start();
    let _auth = server.mock(|when, then| {
        when.method(GET).path("/v1/session");
        then.status(200).json_body(json!({
            "account_id": 1,
            "username": "alice",
            "sources": ["sms-backup-restore"]
        }));
    });
    let _start_import = server.mock(|when, then| {
        when.method(POST).path("/v1/imports");
        then.status(201)
            .header("Location", "/v1/imports/42")
            .json_body(json!({ "id": 42 }));
    });
    let _complete_import = server.mock(|when, then| {
        when.method(POST).path("/v1/imports/42/complete");
        then.status(200).json_body(json!({
            "id": 42,
            "status": "completed",
            "message_count": 1,
            "attachment_count": 0,
            "bytes_uploaded": 0
        }));
    });
    let import = server.mock(|when, then| {
        when.method(POST).path("/v1/imports/42/batches");
        then.status(200).json_body(json!({
            "source": "sms-backup-restore",
            "account": 1,
            "messages": 1,
            "messages_appended": 1,
            "conversations": 1,
            "attachments": 0,
            "assets_copied": 0,
            "assets_missing": 0,
            "mode": "append"
        }));
    });

    let info = authenticate(&server.base_url(), "mc_test").unwrap();
    assert_eq!(info.account_id, 1);

    let dir = tempdir().unwrap();
    write_jsonl(dir.path(), &sample_doc());

    let cfg = text_only_config(dir.path(), server.base_url());
    let report = run(&cfg, None).unwrap();
    assert!(report.ok);
    assert_eq!(report.conversations_ok, 1);
    let report_json = serde_json::to_value(&report).unwrap();
    assert!(
        report_json
            .get("elapsed_ms")
            .and_then(|v| v.as_u64())
            .is_some()
    );
    import.assert();

    // Second run should skip via journal.
    let report2 = run(&cfg, None).unwrap();
    assert!(report2.ok);
    assert_eq!(report2.conversations_skipped, 1);
}

#[test]
fn reuses_supplied_import_session_without_starting_or_completing_one() {
    let server = MockServer::start();
    let _auth = server.mock(|when, then| {
        when.method(GET).path("/v1/session");
        then.status(200).json_body(json!({
            "account_id": 1,
            "username": "alice",
        }));
    });
    let start = server.mock(|when, then| {
        when.method(POST).path("/v1/imports");
        then.status(201)
            .header("Location", "/v1/imports/99")
            .json_body(json!({
                "id": 42
            }));
    });
    let complete = server.mock(|when, then| {
        when.method(POST).path("/v1/imports/99/complete");
        then.status(200).json_body(json!({
            "id": 99,
            "status": "completed",
            "message_count": 1,
            "attachment_count": 0,
            "bytes_uploaded": 0
        }));
    });
    let import = server.mock(|when, then| {
        when.method(POST).path("/v1/imports/99/batches");
        then.status(200).json_body(json!({
            "source": "sms-backup-restore",
            "account": 1,
            "messages": 1,
            "messages_appended": 1,
            "conversations": 1,
            "attachments": 0,
            "assets_copied": 0,
            "assets_missing": 0,
            "mode": "append"
        }));
    });

    let dir = tempdir().unwrap();
    write_jsonl(dir.path(), &sample_doc());

    let cfg = PushConfig {
        import_id: Some(99),
        ..text_only_config(dir.path(), server.base_url())
    };
    let report = run(&cfg, None).unwrap();

    assert!(report.ok);
    assert_eq!(report.conversations_ok, 1);
    assert_eq!(start.calls(), 0, "push must not start a new session");
    assert_eq!(
        complete.calls(),
        0,
        "push must not complete a reused session"
    );
    import.assert();
}

/// A push that started its own Import Run completes it once, with the
/// bytes the push sent. The server counts the run's messages and
/// attachments itself.
#[test]
fn a_push_completes_its_import_run_with_the_bytes_it_sent() {
    const PHOTO: &[u8] = b"photo bytes";
    let server = MockServer::start();
    let _auth = mock_session(&server);
    let _run = mock_import_run(&server, 42);
    let digest = hex::encode(Sha256::digest(PHOTO));
    let _head = server.mock(|when, then| {
        when.method("HEAD").path(format!("/v1/assets/{digest}"));
        then.status(404);
    });
    let _put = server.mock(|when, then| {
        when.method(PUT).path(format!("/v1/assets/{digest}"));
        then.status(200)
            .json_body(json!({ "already_present": false }));
    });
    let _import = server.mock(|when, then| {
        when.method(POST).path("/v1/imports/42/batches");
        then.status(200).json_body(json!({
            "messages": 1,
            "messages_appended": 1,
            "conversations": 1
        }));
    });
    let complete = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/imports/42/complete")
            .json_body(json!({
                "status": "completed",
                "bytes_uploaded": PHOTO.len(),
            }));
        then.status(200).json_body(json!({ "id": 42 }));
    });

    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("attachments")).unwrap();
    fs::write(dir.path().join("attachments/photo.txt"), PHOTO).unwrap();
    let mut doc = sample_doc();
    doc.messages[0].attachments = vec![ir_attachment("attachments/photo.txt", digest.clone())];
    write_jsonl(dir.path(), &doc);

    let report = run(&text_only_config(dir.path(), server.base_url()), None).unwrap();

    assert!(report.ok, "{:?}", report.results);
    assert_eq!(complete.calls(), 1, "the Import Run is completed once");
}

/// A push whose only batch fails still completes its Import Run, as failed,
/// so the server does not show it as running.
#[test]
fn a_push_where_nothing_lands_completes_its_import_run_as_failed() {
    let server = MockServer::start();
    let _auth = mock_session(&server);
    let _run = mock_import_run(&server, 42);
    let _import = server.mock(|when, then| {
        when.method(POST).path("/v1/imports/42/batches");
        then.status(500).json_body(json!({
            "type": "about:blank",
            "title": "Internal server error",
            "status": 500,
            "detail": "intentional batch failure"
        }));
    });
    let complete = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/imports/42/complete")
            .json_body_includes(r#"{ "status": "failed" }"#);
        then.status(200).json_body(json!({ "id": 42 }));
    });

    let dir = tempdir().unwrap();
    write_jsonl(dir.path(), &sample_doc());
    let report = run(&text_only_config(dir.path(), server.base_url()), None).unwrap();

    assert!(!report.ok);
    assert_eq!(complete.calls(), 1, "the Import Run is completed as failed");
}

#[test]
fn aggregates_multiple_conversations_into_one_import_request() {
    let server = MockServer::start();
    let _auth = server.mock(|when, then| {
        when.method(GET).path("/v1/session");
        then.status(200).json_body(json!({
            "account_id": 1,
            "username": "alice",
        }));
    });
    let _run = mock_import_run(&server, 7);
    let import = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/imports/7/batches")
            .body_includes("+15555550101")
            .body_includes("+15555550102");
        then.status(200).json_body(json!({
            "messages": 1,
            "messages_appended": 1,
            "messages_deduped": 1,
            "conversations": 2
        }));
    });

    let dir = tempdir().unwrap();
    let first = sample_doc();
    let second = sample_doc_for("+15555550102", "guid-2");
    write_jsonl(dir.path(), &first);
    write_jsonl(dir.path(), &second);

    let report = run(&text_only_config(dir.path(), server.base_url()), None).unwrap();

    assert!(report.ok);
    assert_eq!(report.conversations_ok, 2);
    assert_eq!(report.messages, 2);
    assert_eq!(report.messages_attempted, 2);
    assert_eq!(report.messages_inserted, 1);
    assert_eq!(report.messages_deduped, 1);
    assert_eq!(report.messages_failed, 0);
    assert_eq!(
        report.messages_attempted,
        report.messages_inserted + report.messages_deduped + report.messages_failed
    );
    assert_eq!(import.calls(), 1);
    let log = fs::read_to_string(dir.path().join("message-crate-push.log")).unwrap();
    assert!(log.contains("IMPORT_REQUEST ok"));
    assert!(log.contains("conversations=2 messages=2"));
}

#[test]
fn flushes_at_message_limit_across_two_batches_of_one_run() {
    let server = MockServer::start();
    let _auth = server.mock(|when, then| {
        when.method(GET).path("/v1/session");
        then.status(200).json_body(json!({
            "account_id": 1,
            "username": "alice",
        }));
    });
    let _run = mock_import_run(&server, 7);
    let replace = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/imports/7/batches")
            .body_includes("+15555550101")
            .body_includes("+15555550102");
        then.status(200).json_body(json!({
            "messages": 2,
            "messages_appended": 2,
            "conversations": 2
        }));
    });
    let append = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/imports/7/batches")
            .body_includes("+15555550103");
        then.status(200).json_body(json!({
            "messages": 1,
            "messages_appended": 1,
            "conversations": 1
        }));
    });

    let dir = tempdir().unwrap();
    write_jsonl(dir.path(), &sample_doc());
    write_jsonl(dir.path(), &sample_doc_for("+15555550102", "guid-2"));
    write_jsonl(dir.path(), &sample_doc_for("+15555550103", "guid-3"));
    let mut cfg = text_only_config(dir.path(), server.base_url());
    cfg.mode = ImportMode::Replace;
    cfg.batch_size = 2;

    let report = run(&cfg, None).unwrap();

    assert!(report.ok);
    assert_eq!(report.conversations_ok, 3);
    assert_eq!(replace.calls(), 1);
    assert_eq!(append.calls(), 1);
}

#[test]
fn failed_combined_request_only_fails_its_files() {
    let server = MockServer::start();
    let _auth = server.mock(|when, then| {
        when.method(GET).path("/v1/session");
        then.status(200).json_body(json!({
            "account_id": 1,
            "username": "alice",
        }));
    });
    let _run = mock_import_run(&server, 7);
    let failed = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/imports/7/batches")
            .body_includes("+15555550101")
            .body_includes("+15555550102");
        then.status(500).json_body(json!({
            "type": "about:blank",
            "title": "Internal server error",
            "status": 500,
            "detail": "intentional batch failure"
        }));
    });
    let succeeded = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/imports/7/batches")
            .body_includes("+15555550103");
        then.status(200).json_body(json!({
            "messages": 1,
            "messages_appended": 1,
            "conversations": 1
        }));
    });

    let dir = tempdir().unwrap();
    write_jsonl(dir.path(), &sample_doc());
    write_jsonl(dir.path(), &sample_doc_for("+15555550102", "guid-2"));
    write_jsonl(dir.path(), &sample_doc_for("+15555550103", "guid-3"));
    let mut cfg = text_only_config(dir.path(), server.base_url());
    cfg.batch_size = 2;

    let report = run(&cfg, None).unwrap();

    assert!(!report.ok);
    assert_eq!(report.conversations_failed, 2);
    assert_eq!(report.conversations_ok, 1);
    assert_eq!(report.messages_attempted, 3);
    assert_eq!(report.messages_inserted, 1);
    assert_eq!(report.messages_deduped, 0);
    assert_eq!(report.messages_failed, 2);
    assert_eq!(
        report.messages_attempted,
        report.messages_inserted + report.messages_deduped + report.messages_failed
    );
    assert_eq!(failed.calls(), 1);
    assert_eq!(succeeded.calls(), 1);
    assert_eq!(
        report
            .results
            .iter()
            .filter(|result| result.status == "failed")
            .count(),
        2
    );
}

/// The server counts lines of the batch, which Upload packs from several
/// staged files. A refusal of the batch's fourth line is reported as the
/// line of the staged file that line came from: the second file's first
/// message, on its line 3 because of the blank line above it.
#[test]
fn a_refused_line_of_a_batch_is_reported_as_the_line_of_its_staged_file() {
    let server = MockServer::start();
    let _auth = mock_session(&server);
    let _run = mock_import_run(&server, 7);
    let refused = server.mock(|when, then| {
        when.method(POST).path("/v1/imports/7/batches");
        then.status(400)
            .header("Content-Type", "application/problem+json")
            .json_body(json!({
                "type": "https://messagecrate.app/docs/developer/reference/errors/malformed-body",
                "title": "Malformed body",
                "status": 400,
                "detail": "Could not read line 4 of the batch: the message is not valid: boom.",
                "request_id": "3f2b1c0e-8d4a-4b6e-9f21-5c7d8e9a0b1c",
                "line": 4
            }));
    });

    let dir = tempdir().unwrap();
    // Batch lines 1 and 2: the first file's header and its one message.
    write_jsonl(dir.path(), &sample_doc());
    // Batch lines 3 to 5: the second file's header and its two messages,
    // which are lines 3 and 4 of the file because line 2 is blank.
    let mut second = sample_doc_for("+15555550102", "guid-2");
    let mut later = second.messages[0].clone();
    later.guid = "guid-3".into();
    second.messages.push(later);
    let second_name = format!("{}.jsonl", second.filename_stem());
    let header = json!({
        "schema_version": second.schema_version,
        "export": second.export,
        "conversation": second.conversation,
    });
    let mut text = format!("{header}\n\n");
    for message in &second.messages {
        text.push_str(&serde_json::to_string(message).unwrap());
        text.push('\n');
    }
    fs::write(dir.path().join(&second_name), text).unwrap();

    let report = run(&text_only_config(dir.path(), server.base_url()), None).unwrap();

    assert!(!report.ok);
    assert_eq!(refused.calls(), 1);
    assert_eq!(report.conversations_failed, 2);
    for result in &report.results {
        let error = result.error.as_deref().unwrap();
        assert!(
            error.starts_with(&format!("line 3 of {second_name}: ")),
            "{}: {error}",
            result.file
        );
        assert!(
            error.contains("Could not read line 4 of the batch"),
            "the server's own sentence stays: {error}"
        );
    }
}

/// Three conversations, the middle one unreadable, in name order.
fn folder_with_a_bad_middle_file(dir: &Path) {
    write_jsonl(dir, &sample_doc());
    let bad = sample_doc_for("+15555550102", "guid-2");
    fs::write(
        dir.join(format!("{}.jsonl", bad.filename_stem())),
        "not a conversation\n",
    )
    .unwrap();
    write_jsonl(dir, &sample_doc_for("+15555550103", "guid-3"));
}

/// A file that fails to prepare is reported and the push goes on: the good
/// files around it still share one request.
#[test]
fn a_push_imports_the_files_after_a_bad_one() {
    let server = MockServer::start();
    let _auth = mock_session(&server);
    let _run = mock_import_run(&server, 7);
    let import = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/imports/7/batches")
            .body_includes("guid-1")
            .body_includes("guid-3");
        then.status(200).json_body(json!({
            "messages": 2,
            "messages_appended": 2,
            "conversations": 2
        }));
    });

    let dir = tempdir().unwrap();
    folder_with_a_bad_middle_file(dir.path());

    let report = run(&text_only_config(dir.path(), server.base_url()), None).unwrap();

    assert!(!report.ok);
    assert_eq!(import.calls(), 1);
    assert_eq!(report.conversations_ok, 2);
    assert_eq!(report.conversations_failed, 1);
}

#[test]
fn resumes_message_batches_from_compacted_journal() {
    let server = MockServer::start();
    let _auth = server.mock(|when, then| {
        when.method(GET).path("/v1/session");
        then.status(200).json_body(json!({
            "account_id": 1,
            "username": "alice",
        }));
    });
    let _run = mock_import_run(&server, 7);
    let import = server.mock(|when, then| {
        when.method(POST).path("/v1/imports/7/batches");
        then.status(200).json_body(json!({
            "messages": 1,
            "messages_appended": 1,
            "conversations": 1
        }));
    });

    let dir = tempdir().unwrap();
    write_jsonl(dir.path(), &sample_doc());
    let cfg = text_only_config(dir.path(), server.base_url());
    run(&cfg, None).unwrap();
    assert_eq!(import.calls(), 1);

    let journal_path = dir.path().join(".import-state.jsonl");
    let compacted = fs::read_to_string(&journal_path).unwrap();
    assert!(compacted.contains("\"event\":\"message_batch_ok\""));
    let without_file_success = compacted
        .lines()
        .filter(|line| !line.contains("\"event\":\"file_ok\""))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&journal_path, format!("{without_file_success}\n")).unwrap();

    let resumed = run(&cfg, None).unwrap();
    assert!(resumed.ok);
    assert_eq!(resumed.conversations_ok, 1);
    assert_eq!(resumed.messages, 0);
    assert_eq!(import.calls(), 1);
}

/// A replace push wipes the source on the server, so it ignores the journal
/// even without `force`: every message already sent goes out again.
/// Otherwise the journaled messages would be skipped and the wipe would
/// leave them missing.
#[test]
fn a_replace_push_ignores_the_journal_and_sends_every_message_again() {
    let server = MockServer::start();
    let _auth = server.mock(|when, then| {
        when.method(GET).path("/v1/session");
        then.status(200).json_body(json!({
            "account_id": 1,
            "username": "alice",
        }));
    });
    let _run = mock_import_run(&server, 7);
    let import = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/imports/7/batches")
            .body_includes("guid-1")
            .body_includes("guid-2");
        then.status(200).json_body(json!({
            "messages": 2,
            "messages_appended": 2,
            "conversations": 2
        }));
    });

    let dir = tempdir().unwrap();
    write_jsonl(dir.path(), &sample_doc());
    write_jsonl(dir.path(), &sample_doc_for("+15555550102", "guid-2"));
    let cfg = text_only_config(dir.path(), server.base_url());
    assert!(run(&cfg, None).unwrap().ok);
    assert_eq!(import.calls(), 1);

    let replace = PushConfig {
        mode: ImportMode::Replace,
        force: false,
        ..cfg
    };
    let report = run(&replace, None).unwrap();

    assert!(report.ok, "{:?}", report.results);
    assert_eq!(import.calls(), 2, "the replace push sends both messages");
    assert_eq!(report.conversations_skipped, 0);
    assert_eq!(report.messages_attempted, 2);
}

#[test]
fn profiles_attachment_upload_phases() {
    const ASSET_BYTES: &[u8] = b"attachment profile fixture";

    let server = MockServer::start();
    let _auth = server.mock(|when, then| {
        when.method(GET).path("/v1/session");
        then.status(200).json_body(json!({
            "account_id": 1,
            "username": "alice",
            "sources": ["sms-backup-restore"]
        }));
    });
    let _run = mock_import_run(&server, 7);
    let digest = hex::encode(Sha256::digest(ASSET_BYTES));
    let head = server.mock(|when, then| {
        when.method("HEAD").path(format!("/v1/assets/{digest}"));
        then.status(404).json_body(json!({
            "type": "https://messagecrate.app/docs/developer/reference/errors/not-found",
            "title": "Not found",
            "status": 404,
            "detail": "asset not found"
        }));
    });
    let asset = server.mock(|when, then| {
        when.method(PUT).path(format!("/v1/assets/{digest}"));
        then.status(200).json_body(json!({
            "already_present": false
        }));
    });
    let import = server.mock(|when, then| {
        when.method(POST).path("/v1/imports/7/batches");
        then.status(200).json_body(json!({
            "messages": 1,
            "messages_appended": 1
        }));
    });

    let dir = tempdir().unwrap();
    let attachment_dir = dir.path().join("attachments");
    fs::create_dir(&attachment_dir).unwrap();
    fs::write(attachment_dir.join("fixture.txt"), ASSET_BYTES).unwrap();
    let mut doc = sample_doc();
    doc.messages[0].attachments.push(IrAttachment {
        path: Some("attachments/fixture.txt".into()),
        original_name: Some("fixture.txt".into()),
        mime_type: Some("text/plain".into()),
        digest_sha256: None,
        is_sticker: false,
        transcription: None,
        sticker_effect: None,
        size_bytes: None,
        missing_reason: None,
        bytes: None,
    });
    write_jsonl(dir.path(), &doc);

    let report_path = dir.path().join("message-crate-push-report.json");
    let log_path = dir.path().join("message-crate-push.log");
    let cfg = PushConfig {
        input: dir.path().to_path_buf(),
        base_url: server.base_url(),
        username: "alice".into(),
        key: "mc_test".into(),
        mode: ImportMode::Append,
        force: false,
        skip_attachments: false,
        verify_digests: false,
        trust_export: false,

        max_retries: 0,
        batch_size: 50,
        asset_upload_workers: 2,
        prepare_ahead: message_crate_push::DEFAULT_PREPARE_AHEAD,
        prepare_workers: message_crate_push::DEFAULT_PREPARE_WORKERS,
        asset_multipart_threshold: message_crate_push::MAX_PROXY_BODY_BYTES,
        asset_max_bytes: message_crate_push::DEFAULT_ASSET_MAX_BYTES,
        report_path: Some(report_path.clone()),
        log_path: Some(log_path.clone()),
        journal_path: Some(dir.path().join(".import-state.jsonl")),
        cancel: None,
        import_id: None,
    };
    let mut progress_lines = Vec::new();
    let report = {
        let mut progress = |event| {
            if let ProgressEvent::Log(line) = event {
                progress_lines.push(line);
            }
        };
        run(&cfg, Some(&mut progress)).unwrap()
    };

    assert!(
        head.calls() >= 1,
        "first upload pays one preflight HEAD of the queued digest"
    );
    asset.assert();
    import.assert();
    let profile = report.results[0].profile.as_ref().unwrap();
    assert_eq!(profile.unique_assets, 1);
    assert_eq!(profile.asset_bytes, ASSET_BYTES.len() as u64);
    assert!(
        progress_lines
            .iter()
            .any(|line| line.starts_with("files ") && line.contains("import time="))
    );

    let persisted_report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(report_path).unwrap()).unwrap();
    assert_eq!(
        persisted_report["results"][0]["profile"]["asset_bytes"],
        ASSET_BYTES.len() as u64
    );
    let persisted_log = fs::read_to_string(log_path).unwrap();
    assert!(persisted_log.contains("attachment_scan_hash_ms="));
    assert!(persisted_log.contains("Import "));
}

fn ir_attachment(rel: &str, digest: String) -> IrAttachment {
    IrAttachment {
        path: Some(rel.into()),
        original_name: Some(rel.rsplit('/').next().unwrap_or(rel).into()),
        mime_type: Some("text/plain".into()),
        digest_sha256: Some(digest),
        is_sticker: false,
        transcription: None,
        sticker_effect: None,
        size_bytes: None,
        missing_reason: None,
        bytes: None,
    }
}

/// Two unique files in one conversation; one worker so upload order is stable.
fn two_attachment_docs(
    dir: &Path,
    a_name: &str,
    a_bytes: &[u8],
    b_name: &str,
    b_bytes: &[u8],
) -> (String, String) {
    let attachment_dir = dir.join("attachments");
    fs::create_dir(&attachment_dir).unwrap();
    fs::write(attachment_dir.join(a_name), a_bytes).unwrap();
    fs::write(attachment_dir.join(b_name), b_bytes).unwrap();
    let digest_a = hex::encode(Sha256::digest(a_bytes));
    let digest_b = hex::encode(Sha256::digest(b_bytes));
    let mut doc = sample_doc();
    doc.messages[0].attachments = vec![
        ir_attachment(&format!("attachments/{a_name}"), digest_a.clone()),
        ir_attachment(&format!("attachments/{b_name}"), digest_b.clone()),
    ];
    write_jsonl(dir, &doc);
    (digest_a, digest_b)
}

#[test]
fn puts_two_new_assets_after_one_preflight_head() {
    const A: &[u8] = b"new-asset-alpha";
    const B: &[u8] = b"new-asset-bravo";

    let server = MockServer::start();
    let _auth = server.mock(|when, then| {
        when.method(GET).path("/v1/session");
        then.status(200).json_body(json!({
            "account_id": 1,
            "username": "alice",
            "sources": ["sms-backup-restore"]
        }));
    });
    let _run = mock_import_run(&server, 7);
    let dir = tempdir().unwrap();
    let (digest_a, digest_b) = two_attachment_docs(dir.path(), "a.txt", A, "b.txt", B);
    let head_a = server.mock(|when, then| {
        when.method("HEAD").path(format!("/v1/assets/{digest_a}"));
        then.status(404);
    });
    let head_b = server.mock(|when, then| {
        when.method("HEAD").path(format!("/v1/assets/{digest_b}"));
        then.status(404);
    });
    let put_a = server.mock(|when, then| {
        // The server records the media type the upload names, so each PUT
        // must carry the attachment's own type.
        when.method(PUT)
            .path(format!("/v1/assets/{digest_a}"))
            .header("Content-Type", "text/plain");
        then.status(200)
            .json_body(json!({ "already_present": false }));
    });
    let put_b = server.mock(|when, then| {
        when.method(PUT)
            .path(format!("/v1/assets/{digest_b}"))
            .header("Content-Type", "text/plain");
        then.status(200)
            .json_body(json!({ "already_present": false }));
    });
    let _import = server.mock(|when, then| {
        when.method(POST).path("/v1/imports/7/batches");
        then.status(200).json_body(json!({
            "messages": 1,
            "messages_appended": 1
        }));
    });

    let mut cfg = text_only_config(dir.path(), server.base_url());
    cfg.force = true;
    cfg.asset_upload_workers = 1;
    let report = run(&cfg, None).unwrap();

    // One preflight HEAD of the first queued digest (sha256 order); later new
    // files still PUT without HEAD.
    let head_first = if digest_a < digest_b {
        &head_a
    } else {
        &head_b
    };
    let head_second = if digest_a < digest_b {
        &head_b
    } else {
        &head_a
    };
    assert!(
        head_first.calls() >= 1,
        "first import pays one preflight HEAD"
    );
    assert_eq!(head_second.calls(), 0);
    assert_eq!(put_a.calls(), 1);
    assert_eq!(put_b.calls(), 1);
    assert_eq!(report.assets_uploaded, 2);
}

#[test]
fn heads_later_assets_after_put_reports_already_present() {
    const FIRST: &[u8] = b"already-on-server-first";
    const SECOND: &[u8] = b"already-on-server-second";

    let server = MockServer::start();
    let _auth = server.mock(|when, then| {
        when.method(GET).path("/v1/session");
        then.status(200).json_body(json!({
            "account_id": 1,
            "username": "alice",
            "sources": ["sms-backup-restore"]
        }));
    });
    let _run = mock_import_run(&server, 7);
    let dir = tempdir().unwrap();
    let (digest_a, digest_b) =
        two_attachment_docs(dir.path(), "first.txt", FIRST, "second.txt", SECOND);
    // Jobs run in sha256 order. The lexicographically first digest PUTs;
    // after already_present, the second digest HEADs and skips PUT.
    let (first_digest, second_digest) = if digest_a < digest_b {
        (digest_a, digest_b)
    } else {
        (digest_b, digest_a)
    };
    // Preflight HEAD of the first digest misses (404). The PUT then reports
    // already_present and later files HEAD-skip. Sequential workers are
    // required so the second job runs after that store.
    let head_first = server.mock(|when, then| {
        when.method("HEAD")
            .path(format!("/v1/assets/{first_digest}"));
        then.status(404);
    });
    let put_first = server.mock(|when, then| {
        when.method(PUT).path(format!("/v1/assets/{first_digest}"));
        then.status(200)
            .json_body(json!({ "already_present": true }));
    });
    let head_second = server.mock(|when, then| {
        when.method("HEAD")
            .path(format!("/v1/assets/{second_digest}"));
        then.status(200);
    });
    let put_second = server.mock(|when, then| {
        when.method(PUT).path(format!("/v1/assets/{second_digest}"));
        then.status(200)
            .json_body(json!({ "already_present": false }));
    });
    let _import = server.mock(|when, then| {
        when.method(POST).path("/v1/imports/7/batches");
        then.status(200).json_body(json!({
            "messages": 1,
            "messages_appended": 1
        }));
    });

    let mut cfg = text_only_config(dir.path(), server.base_url());
    cfg.force = true;
    cfg.asset_upload_workers = 1;
    let report = run(&cfg, None).unwrap();

    assert!(
        head_first.calls() >= 1,
        "preflight HEAD of the first digest"
    );
    assert_eq!(put_first.calls(), 1);
    assert!(head_second.calls() >= 1);
    assert_eq!(put_second.calls(), 0);
    assert_eq!(report.assets_skipped, 2);
}

#[test]
fn preflight_head_skips_puts_when_first_asset_already_present() {
    const A: &[u8] = b"server-already-has-alpha";
    const B: &[u8] = b"server-already-has-bravo";

    let server = MockServer::start();
    let _auth = server.mock(|when, then| {
        when.method(GET).path("/v1/session");
        then.status(200).json_body(json!({
            "account_id": 1,
            "username": "alice",
            "sources": ["sms-backup-restore"]
        }));
    });
    let _run = mock_import_run(&server, 7);
    let dir = tempdir().unwrap();
    let (digest_a, digest_b) = two_attachment_docs(dir.path(), "a.txt", A, "b.txt", B);
    let head_a = server.mock(|when, then| {
        when.method("HEAD").path(format!("/v1/assets/{digest_a}"));
        then.status(200).json_body(json!({
            "already_present": true
        }));
    });
    let head_b = server.mock(|when, then| {
        when.method("HEAD").path(format!("/v1/assets/{digest_b}"));
        then.status(200).json_body(json!({
            "already_present": true
        }));
    });
    let put_a = server.mock(|when, then| {
        when.method(PUT).path(format!("/v1/assets/{digest_a}"));
        then.status(200)
            .json_body(json!({ "already_present": false }));
    });
    let put_b = server.mock(|when, then| {
        when.method(PUT).path(format!("/v1/assets/{digest_b}"));
        then.status(200)
            .json_body(json!({ "already_present": false }));
    });
    let _import = server.mock(|when, then| {
        when.method(POST).path("/v1/imports/7/batches");
        then.status(200).json_body(json!({
            "messages": 1,
            "messages_appended": 1
        }));
    });

    let mut cfg = text_only_config(dir.path(), server.base_url());
    cfg.force = true;
    // Parallel workers would both PUT before a post-PUT flag is visible.
    cfg.asset_upload_workers = 2;
    let report = run(&cfg, None).unwrap();

    assert_eq!(put_a.calls(), 0, "preflight HEAD must skip PUT for A");
    assert_eq!(put_b.calls(), 0, "preflight HEAD must skip PUT for B");
    assert!(head_a.calls() + head_b.calls() >= 1);
    assert_eq!(report.assets_skipped, 2);
}

#[test]
fn multipart_upload_when_over_proxy_threshold() {
    // File larger than the test threshold; mock server returns a tiny part_size.
    const ASSET_BYTES: &[u8] = b"0123456789abcdef0123456789abcdef01234567"; // 40 bytes

    let server = MockServer::start();
    let _auth = server.mock(|when, then| {
        when.method(GET).path("/v1/session");
        then.status(200).json_body(json!({
            "account_id": 1,
            "username": "alice",
            "sources": ["sms-backup-restore"]
        }));
    });
    let _run = mock_import_run(&server, 7);
    let digest = hex::encode(Sha256::digest(ASSET_BYTES));
    let head = server.mock(|when, then| {
        when.method("HEAD").path(format!("/v1/assets/{digest}"));
        then.status(404).json_body(json!({
            "type": "https://messagecrate.app/docs/developer/reference/errors/not-found",
            "title": "Not found",
            "status": 404,
            "detail": "asset not found"
        }));
    });
    let start = server.mock(|when, then| {
        // The start request is the only one that names the media type.
        when.method(POST)
            .path(format!("/v1/assets/{digest}/uploads"))
            .json_body(json!({ "bytes": 40, "mime": "video/mp4" }));
        then.status(200).json_body(json!({
            "upload_id": "up-1",
            "part_size": 16
        }));
    });
    let part1 = server.mock(|when, then| {
        when.method(PUT)
            .path(format!("/v1/assets/{digest}/uploads/up-1/parts/1"));
        then.status(200)
            .json_body(json!({ "part": 1, "bytes": 16 }));
    });
    let part2 = server.mock(|when, then| {
        when.method(PUT)
            .path(format!("/v1/assets/{digest}/uploads/up-1/parts/2"));
        then.status(200)
            .json_body(json!({ "part": 2, "bytes": 16 }));
    });
    let part3 = server.mock(|when, then| {
        when.method(PUT)
            .path(format!("/v1/assets/{digest}/uploads/up-1/parts/3"));
        then.status(200).json_body(json!({ "part": 3, "bytes": 8 }));
    });
    let complete = server.mock(|when, then| {
        when.method(POST)
            .path(format!("/v1/assets/{digest}/uploads/up-1/complete"));
        then.status(200).json_body(json!({
            "sha256": digest,
            "assets_path": format!("ab/{digest}"),
            "already_present": false
        }));
    });
    let single_put = server.mock(|when, then| {
        when.method(PUT).path(format!("/v1/assets/{digest}"));
        then.status(200).json_body(json!({
            "already_present": false
        }));
    });
    let import = server.mock(|when, then| {
        when.method(POST).path("/v1/imports/7/batches");
        then.status(200).json_body(json!({
            "messages": 1,
            "messages_appended": 1
        }));
    });

    let dir = tempdir().unwrap();
    let attachment_dir = dir.path().join("attachments");
    fs::create_dir(&attachment_dir).unwrap();
    fs::write(attachment_dir.join("large.bin"), ASSET_BYTES).unwrap();
    let mut doc = sample_doc();
    doc.messages[0].attachments.push(IrAttachment {
        path: Some("attachments/large.bin".into()),
        original_name: Some("large.bin".into()),
        mime_type: Some("video/mp4".into()),
        digest_sha256: Some(digest.clone()),
        is_sticker: false,
        transcription: None,
        sticker_effect: None,
        size_bytes: None,
        missing_reason: None,
        bytes: None,
    });
    write_jsonl(dir.path(), &doc);

    let mut cfg = text_only_config(dir.path(), server.base_url());
    cfg.force = true;
    cfg.asset_multipart_threshold = 20; // force multipart for 40-byte file
    let report = run(&cfg, None).unwrap();

    assert!(
        head.calls() >= 1,
        "first upload pays one preflight HEAD of the queued digest"
    );
    start.assert();
    part1.assert();
    part2.assert();
    part3.assert();
    complete.assert();
    assert_eq!(
        single_put.calls(),
        0,
        "single PUT must not be used for multipart path"
    );
    import.assert();
    assert_eq!(report.assets_uploaded, 1);
}

#[test]
fn multipart_aborts_on_hash_mismatch_complete() {
    const ASSET_BYTES: &[u8] = b"mismatch-fixture-bytes!!";

    let server = MockServer::start();
    let _auth = server.mock(|when, then| {
        when.method(GET).path("/v1/session");
        then.status(200).json_body(json!({
            "account_id": 1,
            "username": "alice",
            "sources": ["sms-backup-restore"]
        }));
    });
    let _run = mock_import_run(&server, 7);
    let digest = hex::encode(Sha256::digest(ASSET_BYTES));
    let _head = server.mock(|when, then| {
        when.method("HEAD").path(format!("/v1/assets/{digest}"));
        then.status(404);
    });
    let _start = server.mock(|when, then| {
        when.method(POST)
            .path(format!("/v1/assets/{digest}/uploads"));
        then.status(200).json_body(json!({
            "upload_id": "up-bad",
            "part_size": 64
        }));
    });
    let _part = server.mock(|when, then| {
        when.method(PUT)
            .path_includes(format!("/v1/assets/{digest}/uploads/up-bad/parts/"));
        then.status(200)
            .json_body(json!({ "part": 1, "bytes": 24 }));
    });
    let _complete = server.mock(|when, then| {
        when.method(POST)
            .path(format!("/v1/assets/{digest}/uploads/up-bad/complete"));
        then.status(400).json_body(json!({
            "type": "https://messagecrate.app/docs/developer/reference/errors/asset-upload-invalid",
            "title": "Asset upload invalid",
            "status": 400,
            "detail": "sha256 mismatch: claimed abc, got def"
        }));
    });
    let abort = server.mock(|when, then| {
        when.method(DELETE)
            .path(format!("/v1/assets/{digest}/uploads/up-bad"));
        then.status(204);
    });

    let dir = tempdir().unwrap();
    let attachment_dir = dir.path().join("attachments");
    fs::create_dir(&attachment_dir).unwrap();
    fs::write(attachment_dir.join("bad.bin"), ASSET_BYTES).unwrap();
    let mut doc = sample_doc();
    doc.messages[0].attachments.push(IrAttachment {
        path: Some("attachments/bad.bin".into()),
        original_name: Some("bad.bin".into()),
        mime_type: Some("application/octet-stream".into()),
        digest_sha256: Some(digest),
        is_sticker: false,
        transcription: None,
        sticker_effect: None,
        size_bytes: None,
        missing_reason: None,
        bytes: None,
    });
    write_jsonl(dir.path(), &doc);

    let mut cfg = text_only_config(dir.path(), server.base_url());
    cfg.force = true;
    cfg.asset_multipart_threshold = 8;
    let report = run(&cfg, None).unwrap();
    assert!(!report.ok);
    abort.assert();
}

#[test]
fn authenticate_maps_http_failures_to_typed_errors() {
    let server = MockServer::start();

    let _unauthorized = server.mock(|when, then| {
        when.method(GET).path("/v1/session");
        then.status(401).body("unauthorized");
    });
    let err = authenticate(&server.base_url(), "bad").unwrap_err();
    assert_eq!(err.kind(), "invalid_key");
    assert!(!err.user_message().contains("unauthorized"));
    assert!(err.detail().contains("invalid API key"));
}

#[test]
fn authenticate_maps_html_and_status_failures() {
    let server = MockServer::start();
    let _html = server.mock(|when, then| {
        when.method(GET).path("/v1/session");
        then.status(200)
            .body("<!DOCTYPE html><html><body>browse ui</body></html>");
    });
    let err = authenticate(&server.base_url(), "mc_test").unwrap_err();
    assert_eq!(err.kind(), "wrong_host");
    assert!(err.user_message().contains("website"));
    assert!(err.detail().contains("HTML"));

    // Fresh server for a non-401 status.
    let server = MockServer::start();
    let _forbidden = server.mock(|when, then| {
        when.method(GET).path("/v1/session");
        then.status(403).body("username does not match API key");
    });
    let err = authenticate(&server.base_url(), "mc_test").unwrap_err();
    assert_eq!(err.kind(), "forbidden");
    assert!(!err.user_message().contains("username does not match"));
    assert!(err.detail().contains("username does not match API key"));
}

#[test]
fn authenticate_rejects_invalid_url() {
    let err = authenticate("not a url", "mc_test").unwrap_err();
    assert_eq!(err.kind(), "invalid_url");
    assert!(matches!(err, AuthError::InvalidUrl { .. }));
}

#[test]
fn verify_digests_fails_on_mismatch() {
    const ASSET_BYTES: &[u8] = b"on-disk bytes";
    let wrong_digest = hex::encode(Sha256::digest(b"other bytes"));

    let server = MockServer::start();
    let _auth = server.mock(|when, then| {
        when.method(GET).path("/v1/session");
        then.status(200).json_body(json!({
            "account_id": 1,
            "username": "alice",
            "sources": ["sms-backup-restore"]
        }));
    });
    let _run = mock_import_run(&server, 7);
    let put = server.mock(|when, then| {
        when.method(PUT).path_includes("/v1/assets/");
        then.status(200)
            .json_body(json!({ "already_present": false }));
    });

    let dir = tempdir().unwrap();
    let attachment_dir = dir.path().join("attachments");
    fs::create_dir(&attachment_dir).unwrap();
    fs::write(attachment_dir.join("fixture.txt"), ASSET_BYTES).unwrap();
    let mut doc = sample_doc();
    doc.messages[0].attachments.push(IrAttachment {
        path: Some("attachments/fixture.txt".into()),
        original_name: Some("fixture.txt".into()),
        mime_type: Some("text/plain".into()),
        digest_sha256: Some(wrong_digest),
        is_sticker: false,
        transcription: None,
        sticker_effect: None,
        size_bytes: None,
        missing_reason: None,
        bytes: None,
    });
    write_jsonl(dir.path(), &doc);

    let mut cfg = text_only_config(dir.path(), server.base_url());
    cfg.verify_digests = true;
    let report = run(&cfg, None).unwrap();
    assert!(!report.ok);
    assert_eq!(report.conversations_failed, 1);
    assert_eq!(put.calls(), 0, "mismatch must fail before upload");
}

#[test]
fn shared_attachment_uploaded_once_across_conversations() {
    const ASSET_BYTES: &[u8] = b"shared attachment bytes";
    let digest = hex::encode(Sha256::digest(ASSET_BYTES));

    let server = MockServer::start();
    let _auth = server.mock(|when, then| {
        when.method(GET).path("/v1/session");
        then.status(200).json_body(json!({
            "account_id": 1,
            "username": "alice",
            "sources": ["sms-backup-restore"]
        }));
    });
    let _run = mock_import_run(&server, 7);
    let head = server.mock(|when, then| {
        when.method("HEAD").path(format!("/v1/assets/{digest}"));
        then.status(404).json_body(json!({
            "type": "https://messagecrate.app/docs/developer/reference/errors/not-found",
            "title": "Not found",
            "status": 404,
            "detail": "asset not found"
        }));
    });
    let put = server.mock(|when, then| {
        when.method(PUT).path(format!("/v1/assets/{digest}"));
        then.status(200)
            .json_body(json!({ "already_present": false }));
    });
    let import = server.mock(|when, then| {
        when.method(POST).path("/v1/imports/7/batches");
        then.status(200).json_body(json!({
            "messages": 2,
            "messages_appended": 2
        }));
    });

    let dir = tempdir().unwrap();
    let attachment_dir = dir.path().join("attachments");
    fs::create_dir(&attachment_dir).unwrap();
    fs::write(attachment_dir.join("shared.txt"), ASSET_BYTES).unwrap();

    let mut doc_a = sample_doc_for("+15555550101", "guid-a");
    doc_a.messages[0].attachments.push(IrAttachment {
        path: Some("attachments/shared.txt".into()),
        original_name: Some("shared.txt".into()),
        mime_type: Some("text/plain".into()),
        digest_sha256: Some(digest.clone()),
        is_sticker: false,
        transcription: None,
        sticker_effect: None,
        size_bytes: None,
        missing_reason: None,
        bytes: None,
    });
    let mut doc_b = sample_doc_for("+15555550102", "guid-b");
    doc_b.messages[0].attachments.push(IrAttachment {
        path: Some("attachments/shared.txt".into()),
        original_name: Some("shared.txt".into()),
        mime_type: Some("text/plain".into()),
        digest_sha256: Some(digest),
        is_sticker: false,
        transcription: None,
        sticker_effect: None,
        size_bytes: None,
        missing_reason: None,
        bytes: None,
    });
    write_jsonl(dir.path(), &doc_a);
    write_jsonl(dir.path(), &doc_b);

    let cfg = text_only_config(dir.path(), server.base_url());
    let report = run(&cfg, None).unwrap();
    assert!(report.ok);
    assert_eq!(report.conversations_ok, 2);
    assert_eq!(put.calls(), 1, "shared digest must upload once");
    assert!(
        head.calls() >= 1,
        "first upload pays one preflight HEAD of the queued digest"
    );
    import.assert();
    assert_eq!(report.assets_uploaded, 1);
}

/// When the first conversation's upload of a shared file fails, its claim on
/// the file is released, so the next conversation uploads it in the same
/// push rather than skipping it as already in flight.
///
/// The two conversations come from different backup sources, so the mock
/// server can refuse the first upload and accept the second by the `source`
/// each PUT carries.
#[test]
fn a_failed_upload_frees_a_shared_file_for_the_next_conversation() {
    const ASSET_BYTES: &[u8] = b"shared attachment bytes";
    let digest = hex::encode(Sha256::digest(ASSET_BYTES));

    let server = MockServer::start();
    let _auth = mock_session(&server);
    let _run = mock_import_run(&server, 7);
    let _head = server.mock(|when, then| {
        when.method("HEAD").path(format!("/v1/assets/{digest}"));
        then.status(404);
    });
    let refused = server.mock(|when, then| {
        when.method(PUT)
            .path(format!("/v1/assets/{digest}"))
            .query_param("source", "sms-backup-restore");
        then.status(503).json_body(json!({
            "type": "about:blank",
            "title": "Service unavailable",
            "status": 503,
            "detail": "the server is busy"
        }));
    });
    let accepted = server.mock(|when, then| {
        when.method(PUT)
            .path(format!("/v1/assets/{digest}"))
            .query_param("source", "whatsapp");
        then.status(200)
            .json_body(json!({ "already_present": false }));
    });
    let import = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/imports/7/batches")
            .body_includes("guid-2");
        then.status(200).json_body(json!({
            "messages": 1,
            "messages_appended": 1
        }));
    });

    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("attachments")).unwrap();
    fs::write(dir.path().join("attachments/shared.txt"), ASSET_BYTES).unwrap();
    let mut first = sample_doc();
    first.messages[0].attachments = vec![ir_attachment("attachments/shared.txt", digest.clone())];
    write_jsonl(dir.path(), &first);
    let mut second = sample_doc_for("+15555550102", "guid-2");
    second.export.source = "whatsapp".into();
    second.messages[0].attachments = vec![ir_attachment("attachments/shared.txt", digest.clone())];
    write_jsonl(dir.path(), &second);
    let cfg = PushConfig {
        prepare_workers: 1,
        ..text_only_config(dir.path(), server.base_url())
    };

    let report = run(&cfg, None).unwrap();

    assert_eq!(refused.calls(), 1);
    assert_eq!(
        accepted.calls(),
        1,
        "the second conversation uploads the file"
    );
    assert_eq!(report.assets_uploaded, 1);
    assert_eq!(report.conversations_failed, 1);
    assert_eq!(report.conversations_ok, 1);
    assert_eq!(import.calls(), 1);
}

#[test]
fn skips_oversized_attachment_keeps_conversation_ok() {
    const SMALL: &[u8] = b"ok-bytes";
    const BIG: &[u8] = b"this-file-is-too-large-for-the-test-limit!!";

    let server = MockServer::start();
    let _auth = server.mock(|when, then| {
        when.method(GET).path("/v1/session");
        then.status(200).json_body(json!({
            "account_id": 1,
            "username": "alice",
            "sources": ["sms-backup-restore"]
        }));
    });
    let _run = mock_import_run(&server, 7);
    let small_digest = hex::encode(Sha256::digest(SMALL));
    let big_digest = hex::encode(Sha256::digest(BIG));
    let _small_head = server.mock(|when, then| {
        when.method("HEAD")
            .path(format!("/v1/assets/{small_digest}"));
        then.status(404);
    });
    let small_put = server.mock(|when, then| {
        when.method(PUT).path(format!("/v1/assets/{small_digest}"));
        then.status(200).json_body(json!({
            "already_present": false
        }));
    });
    let big_put = server.mock(|when, then| {
        when.method(PUT).path(format!("/v1/assets/{big_digest}"));
        then.status(200).json_body(json!({
            "already_present": false
        }));
    });
    let import = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/imports/7/batches")
            .body_includes(r#""missing_reason":"too_large""#)
            .body_includes("big.bin")
            .body_includes(&small_digest);
        then.status(200).json_body(json!({
            "messages": 1,
            "messages_appended": 1
        }));
    });

    let dir = tempdir().unwrap();
    let attachment_dir = dir.path().join("attachments");
    fs::create_dir(&attachment_dir).unwrap();
    fs::write(attachment_dir.join("small.txt"), SMALL).unwrap();
    fs::write(attachment_dir.join("big.bin"), BIG).unwrap();

    let mut doc = sample_doc();
    doc.messages[0].attachments = vec![
        IrAttachment {
            path: Some("attachments/big.bin".into()),
            original_name: Some("big.bin".into()),
            mime_type: Some("application/octet-stream".into()),
            digest_sha256: Some(big_digest),
            is_sticker: false,
            transcription: None,
            sticker_effect: None,
            size_bytes: Some(BIG.len() as u64),
            missing_reason: None,
            bytes: None,
        },
        IrAttachment {
            path: Some("attachments/small.txt".into()),
            original_name: Some("small.txt".into()),
            mime_type: Some("text/plain".into()),
            digest_sha256: Some(small_digest),
            is_sticker: false,
            transcription: None,
            sticker_effect: None,
            size_bytes: Some(SMALL.len() as u64),
            missing_reason: None,
            bytes: None,
        },
    ];
    write_jsonl(dir.path(), &doc);

    let mut cfg = text_only_config(dir.path(), server.base_url());
    cfg.force = true;
    cfg.asset_max_bytes = 16; // BIG exceeds; SMALL does not

    let mut issues = Vec::new();
    let report = {
        let mut progress = |event: ProgressEvent| {
            if let ProgressEvent::Issue {
                kind, item, reason, ..
            } = event
            {
                issues.push((kind, item, reason));
            }
        };
        run(&cfg, Some(&mut progress)).unwrap()
    };

    assert!(
        report.ok,
        "oversized attachment must not fail the conversation"
    );
    assert_eq!(report.conversations_ok, 1);
    assert_eq!(report.conversations_failed, 0);
    assert_eq!(report.messages_attempted, 1);
    assert_eq!(small_put.calls(), 1, "normal attachment must still upload");
    assert_eq!(big_put.calls(), 0, "oversized attachment must not be PUT");
    import.assert();
    assert!(
        issues.iter().any(|(kind, item, reason)| {
            kind == "skip"
                && item.contains("big.bin")
                && reason.contains("over the configured asset max")
        }),
        "expected skip issue for oversized attachment, got {issues:?}"
    );
}

#[test]
fn skips_missing_attachment_file_keeps_conversation_ok() {
    const SMALL: &[u8] = b"present-bytes";

    let server = MockServer::start();
    let _auth = server.mock(|when, then| {
        when.method(GET).path("/v1/session");
        then.status(200).json_body(json!({
            "account_id": 1,
            "username": "alice",
            "sources": ["sms-backup-restore"]
        }));
    });
    let _run = mock_import_run(&server, 7);
    let small_digest = hex::encode(Sha256::digest(SMALL));
    let _small_head = server.mock(|when, then| {
        when.method("HEAD")
            .path(format!("/v1/assets/{small_digest}"));
        then.status(404);
    });
    let small_put = server.mock(|when, then| {
        when.method(PUT).path(format!("/v1/assets/{small_digest}"));
        then.status(200).json_body(json!({
            "already_present": false
        }));
    });
    let import = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/imports/7/batches")
            .body_includes(r#""missing_reason":"file_missing""#)
            .body_includes("gone.bin")
            .body_includes(&small_digest);
        then.status(200).json_body(json!({
            "messages": 1,
            "messages_appended": 1
        }));
    });

    let dir = tempdir().unwrap();
    let attachment_dir = dir.path().join("attachments");
    fs::create_dir(&attachment_dir).unwrap();
    fs::write(attachment_dir.join("small.txt"), SMALL).unwrap();
    // Intentionally do not write gone.bin

    let mut doc = sample_doc();
    doc.messages[0].attachments = vec![
        IrAttachment {
            path: Some("attachments/gone.bin".into()),
            original_name: Some("gone.bin".into()),
            mime_type: Some("application/octet-stream".into()),
            digest_sha256: None,
            is_sticker: false,
            transcription: None,
            sticker_effect: None,
            size_bytes: Some(1234),
            missing_reason: None,
            bytes: None,
        },
        IrAttachment {
            path: Some("attachments/small.txt".into()),
            original_name: Some("small.txt".into()),
            mime_type: Some("text/plain".into()),
            digest_sha256: Some(small_digest),
            is_sticker: false,
            transcription: None,
            sticker_effect: None,
            size_bytes: Some(SMALL.len() as u64),
            missing_reason: None,
            bytes: None,
        },
    ];
    write_jsonl(dir.path(), &doc);

    let mut cfg = text_only_config(dir.path(), server.base_url());
    cfg.force = true;

    let mut issues = Vec::new();
    let report = {
        let mut progress = |event: ProgressEvent| {
            if let ProgressEvent::Issue {
                kind, item, reason, ..
            } = event
            {
                issues.push((kind, item, reason));
            }
        };
        run(&cfg, Some(&mut progress)).unwrap()
    };

    assert!(report.ok);
    assert_eq!(report.conversations_ok, 1);
    assert_eq!(small_put.calls(), 1);
    import.assert();
    assert!(
        issues.iter().any(|(kind, item, reason)| {
            kind == "skip" && item.contains("gone.bin") && reason.contains("not found")
        }),
        "expected skip issue for missing file, got {issues:?}"
    );
}

/// "Do not copy" leaves attachments with no path but a reason. Importing must
/// keep the metadata rather than failing the whole conversation.
#[test]
fn keeps_conversation_ok_when_skipped_attachment_has_no_path() {
    let server = MockServer::start();
    let _auth = server.mock(|when, then| {
        when.method(GET).path("/v1/session");
        then.status(200).json_body(json!({
            "account_id": 1,
            "username": "alice",
            "sources": ["sms-backup-restore"]
        }));
    });
    let _run = mock_import_run(&server, 7);
    let import = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/imports/7/batches")
            .body_includes(r#""missing_reason":"skipped""#)
            .body_includes("IMG_0421.HEIC")
            .body_includes("image/heic");
        then.status(200).json_body(json!({
            "messages": 1,
            "messages_appended": 1
        }));
    });

    let dir = tempdir().unwrap();
    let mut doc = sample_doc();
    doc.messages[0].attachments = vec![IrAttachment {
        path: None,
        original_name: Some("IMG_0421.HEIC".into()),
        mime_type: Some("image/heic".into()),
        digest_sha256: None,
        is_sticker: false,
        transcription: None,
        sticker_effect: None,
        size_bytes: Some(2048),
        missing_reason: Some("skipped".into()),
        bytes: None,
    }];
    write_jsonl(dir.path(), &doc);

    let mut cfg = text_only_config(dir.path(), server.base_url());
    cfg.force = true;

    let mut issues = Vec::new();
    let report = {
        let mut progress = |event: ProgressEvent| {
            if let ProgressEvent::Issue {
                kind, item, reason, ..
            } = event
            {
                issues.push((kind, item, reason));
            }
        };
        run(&cfg, Some(&mut progress)).unwrap()
    };

    assert!(report.ok);
    assert_eq!(report.conversations_ok, 1);
    assert_eq!(report.conversations_failed, 0);
    assert_eq!(report.assets_uploaded, 0);
    assert_eq!(report.assets_skipped, 1);
    import.assert();
    assert!(
        issues.is_empty(),
        "a deliberate skip must not fill Import Errors, got {issues:?}"
    );
}

/// A pathless attachment with no stated reason is an exporter defect. Import it
/// as missing, but surface a skip row so the defect stays visible.
#[test]
fn reports_pathless_attachment_without_reason_as_no_path() {
    let server = MockServer::start();
    let _auth = server.mock(|when, then| {
        when.method(GET).path("/v1/session");
        then.status(200).json_body(json!({
            "account_id": 1,
            "username": "alice",
            "sources": ["sms-backup-restore"]
        }));
    });
    let _run = mock_import_run(&server, 7);
    let import = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/imports/7/batches")
            .body_includes(r#""missing_reason":"no_path""#)
            .body_includes("mystery.bin");
        then.status(200).json_body(json!({
            "messages": 1,
            "messages_appended": 1
        }));
    });

    let dir = tempdir().unwrap();
    let mut doc = sample_doc();
    doc.messages[0].attachments = vec![IrAttachment {
        path: None,
        original_name: Some("mystery.bin".into()),
        mime_type: Some("application/octet-stream".into()),
        digest_sha256: None,
        is_sticker: false,
        transcription: None,
        sticker_effect: None,
        size_bytes: None,
        missing_reason: None,
        bytes: None,
    }];
    write_jsonl(dir.path(), &doc);

    let mut cfg = text_only_config(dir.path(), server.base_url());
    cfg.force = true;

    let mut issues = Vec::new();
    let report = {
        let mut progress = |event: ProgressEvent| {
            if let ProgressEvent::Issue {
                kind, item, reason, ..
            } = event
            {
                issues.push((kind, item, reason));
            }
        };
        run(&cfg, Some(&mut progress)).unwrap()
    };

    assert!(report.ok);
    assert_eq!(report.conversations_ok, 1);
    assert_eq!(report.assets_skipped, 1);
    import.assert();
    assert!(
        issues.iter().any(|(kind, item, reason)| {
            kind == "skip" && item.contains("mystery.bin") && reason.contains("no file path")
        }),
        "expected skip issue for pathless attachment, got {issues:?}"
    );
}

/// A text-only push sends each message with its text and GUID but without
/// its attachments, uploads nothing, and journals the message's own GUID.
#[test]
fn a_push_that_skips_attachments_sends_text_and_uploads_nothing() {
    let server = MockServer::start();
    let _auth = mock_session(&server);
    let _run = mock_import_run(&server, 7);
    let assets = server.mock(|when, then| {
        when.path_prefix("/v1/assets");
        then.status(500);
    });
    let import = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/imports/7/batches")
            .body_includes("\"guid\":\"guid-1\"")
            .body_includes("hello there")
            .body_excludes("photo.txt");
        then.status(200).json_body(json!({
            "messages": 1,
            "messages_appended": 1,
            "conversations": 1
        }));
    });

    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("attachments")).unwrap();
    fs::write(dir.path().join("attachments/photo.txt"), b"photo bytes").unwrap();
    let mut doc = sample_doc();
    doc.messages[0].attachments = vec![ir_attachment(
        "attachments/photo.txt",
        hex::encode(Sha256::digest(b"photo bytes")),
    )];
    write_jsonl(dir.path(), &doc);
    let cfg = PushConfig {
        skip_attachments: true,
        ..text_only_config(dir.path(), server.base_url())
    };

    let report = run(&cfg, None).unwrap();

    assert!(report.ok, "{:?}", report.results);
    assert_eq!(import.calls(), 1);
    assert_eq!(assets.calls(), 0, "a text-only push uploads no file");
    assert_eq!(report.assets_uploaded, 0);
    assert_eq!(journaled_guids(dir.path()), vec!["guid-1".to_string()]);
}

/// Each import request carries one backup source, so conversations from two
/// sources go out in two requests even when both would fit in one.
#[test]
fn conversations_from_two_sources_go_out_in_separate_requests() {
    let server = MockServer::start();
    let _auth = mock_session(&server);
    let _run = mock_import_run(&server, 7);
    let sms = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/imports/7/batches")
            .body_includes("guid-1")
            .body_excludes("guid-2");
        then.status(200).json_body(json!({
            "messages": 1,
            "messages_appended": 1,
            "conversations": 1
        }));
    });
    let whatsapp = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/imports/7/batches")
            .body_includes("guid-2")
            .body_excludes("guid-1");
        then.status(200).json_body(json!({
            "messages": 1,
            "messages_appended": 1,
            "conversations": 1
        }));
    });

    let dir = tempdir().unwrap();
    write_jsonl(dir.path(), &sample_doc());
    let mut other = sample_doc_for("+15555550102", "guid-2");
    other.export.source = "whatsapp".into();
    write_jsonl(dir.path(), &other);

    let report = run(&text_only_config(dir.path(), server.base_url()), None).unwrap();

    assert!(report.ok, "{:?}", report.results);
    assert_eq!(sms.calls(), 1);
    assert_eq!(whatsapp.calls(), 1);
}

/// The journal rows of one kind (`file_ok`, `message_batch_ok`, …).
fn journal_events(dir: &Path, event: &str) -> Vec<serde_json::Value> {
    fs::read_to_string(dir.join(".import-state.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .filter(|row| row["event"] == event)
        .collect()
}

/// Every guid the journal records as imported, once per row it appears in.
fn journaled_guids(dir: &Path) -> Vec<String> {
    journal_events(dir, "message_batch_ok")
        .iter()
        .flat_map(|row| row["messages"].as_array().unwrap().clone())
        .map(|message| message["guid"].as_str().unwrap().to_string())
        .collect()
}

/// A mock server session that accepts the key.
fn mock_session(server: &MockServer) -> httpmock::Mock<'_> {
    server.mock(|when, then| {
        when.method(GET).path("/v1/session");
        then.status(200).json_body(json!({
            "account_id": 1,
            "username": "alice",
        }));
    })
}

/// A batch the server answers 503 once and 200 on the retry is counted once:
/// one journal entry for its file and message, and `attempted` equal to the
/// messages the run sent, not the requests it made.
///
/// Guards `with_retries` around the batch POST, which no other test drives
/// end to end (every other config sets `max_retries: 0`, except
/// `an_unreadable_2xx_answer_is_not_retried` and
/// `a_2xx_answer_whose_body_is_cut_off_is_not_retried`, which check that no
/// retry happens). A regression that counts each attempt, or journals the batch on
/// the failed try, fails here.
#[test]
fn a_batch_retried_after_a_503_is_counted_and_journaled_once() {
    let server = MockServer::start();
    let _auth = mock_session(&server);
    let _run = mock_import_run(&server, 7);
    let mut busy = server.mock(|when, then| {
        when.method(POST).path("/v1/imports/7/batches");
        then.status(503).json_body(json!({
            "type": "about:blank",
            "title": "Service unavailable",
            "status": 503,
            "detail": "the server is busy"
        }));
    });

    let dir = tempdir().unwrap();
    write_jsonl(dir.path(), &sample_doc());
    let cfg = PushConfig {
        max_retries: 2,
        ..text_only_config(dir.path(), server.base_url())
    };

    // The first retry waits at least 500 ms, so there is time to swap the
    // 503 for a 200 once the first attempt has landed.
    let (report, accepted_calls) = std::thread::scope(|scope| {
        let pusher = scope.spawn(|| run(&cfg, None).unwrap());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while busy.calls() == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "the batch was never posted"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        busy.delete();
        let accepted = server.mock(|when, then| {
            when.method(POST).path("/v1/imports/7/batches");
            then.status(200).json_body(json!({
                "messages": 1,
                "messages_appended": 1,
                "conversations": 1
            }));
        });
        let report = pusher.join().unwrap();
        (report, accepted.calls())
    });

    assert_eq!(accepted_calls, 1, "the retry must reach the server once");
    assert!(report.ok, "{:?}", report.results);
    assert_eq!(report.conversations_ok, 1);
    assert_eq!(report.messages_attempted, 1);
    assert_eq!(report.messages_inserted, 1);
    assert_eq!(report.messages_failed, 0);
    assert_eq!(
        report.messages_attempted,
        report.messages_inserted + report.messages_deduped + report.messages_failed
    );
    assert_eq!(journal_events(dir.path(), "file_ok").len(), 1);
    assert_eq!(journaled_guids(dir.path()), vec!["guid-1".to_string()]);
}

/// A cancel that arrives while the push is under way sends no further batch:
/// the report is not ok, and the journal leaves the unsent conversations for
/// the next push.
///
/// Guards the cancel check before each batch POST. Without it, the batch
/// already packed when the cancel arrived still goes out.
#[test]
fn a_cancelled_push_sends_no_further_batch_and_resumes_later() {
    let server = MockServer::start();
    let _auth = mock_session(&server);
    let _run = mock_import_run(&server, 7);
    let mut import = server.mock(|when, then| {
        when.method(POST).path("/v1/imports/7/batches");
        then.status(200).json_body(json!({
            "messages": 1,
            "messages_appended": 1,
            "conversations": 1
        }));
    });

    let dir = tempdir().unwrap();
    write_jsonl(dir.path(), &sample_doc());
    write_jsonl(dir.path(), &sample_doc_for("+15555550102", "guid-2"));
    write_jsonl(dir.path(), &sample_doc_for("+15555550103", "guid-3"));
    let cancel = Arc::new(AtomicBool::new(false));
    let cfg = PushConfig {
        batch_size: 1,
        prepare_ahead: 1,
        prepare_workers: 1,
        cancel: Some(cancel.clone()),
        ..text_only_config(dir.path(), server.base_url())
    };

    // Cancel as soon as the first conversation is on the server.
    let flag = cancel.clone();
    let mut on_progress = move |event: ProgressEvent| {
        if let ProgressEvent::FileDone { status, .. } = event
            && status == "ok"
        {
            flag.store(true, Ordering::SeqCst);
        }
    };
    let report = run(&cfg, Some(&mut on_progress)).unwrap();

    assert!(!report.ok, "a cancelled push is not ok");
    assert!(
        report.cancelled,
        "the report says the cancel flag stopped it"
    );
    assert_eq!(import.calls(), 1, "no batch is sent after the cancel");
    assert_eq!(report.conversations_ok, 1);
    assert_eq!(journal_events(dir.path(), "file_ok").len(), 1);
    assert_eq!(journaled_guids(dir.path()), vec!["guid-1".to_string()]);

    import.delete();
    let resumed_import = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/imports/7/batches")
            .body_excludes("guid-1");
        then.status(200).json_body(json!({
            "messages": 1,
            "messages_appended": 1,
            "conversations": 1
        }));
    });
    cancel.store(false, Ordering::SeqCst);
    let resumed = run(&cfg, None).unwrap();

    assert!(resumed.ok, "{:?}", resumed.results);
    assert_eq!(resumed.conversations_skipped, 1);
    assert_eq!(resumed.conversations_ok, 2);
    assert_eq!(resumed_import.calls(), 2);
}

/// Every conversation of a cancelled push is in one category of the report:
/// `ok + failed + skipped + cancelled = total`, with one result row per file.
///
/// Guards the report of a stopped Upload. Without a result for each file the
/// stop left unsent, the counts add up to less than the total and nothing in
/// the report names those files.
#[test]
fn a_cancelled_push_reports_every_conversation_in_one_category() {
    let server = MockServer::start();
    let _auth = mock_session(&server);
    let _run = mock_import_run(&server, 7);
    let _import = server.mock(|when, then| {
        when.method(POST).path("/v1/imports/7/batches");
        then.status(200).json_body(json!({
            "messages": 1,
            "messages_appended": 1,
            "conversations": 1
        }));
    });

    let dir = tempdir().unwrap();
    write_jsonl(dir.path(), &sample_doc());
    write_jsonl(dir.path(), &sample_doc_for("+15555550102", "guid-2"));
    write_jsonl(dir.path(), &sample_doc_for("+15555550103", "guid-3"));
    write_jsonl(dir.path(), &sample_doc_for("+15555550104", "guid-4"));
    let cancel = Arc::new(AtomicBool::new(false));
    let cfg = PushConfig {
        batch_size: 1,
        prepare_ahead: 1,
        prepare_workers: 1,
        cancel: Some(cancel.clone()),
        ..text_only_config(dir.path(), server.base_url())
    };

    let flag = cancel.clone();
    let mut on_progress = move |event: ProgressEvent| {
        if let ProgressEvent::FileDone { status, .. } = event
            && status == "ok"
        {
            flag.store(true, Ordering::SeqCst);
        }
    };
    let report = run(&cfg, Some(&mut on_progress)).unwrap();

    assert_eq!(report.conversations_total, 4);
    assert_eq!(report.conversations_ok, 1);
    assert_eq!(report.conversations_cancelled, 3);
    assert!(report.cancelled, "the cancel flag stopped the run");
    assert_eq!(
        report.conversations_ok
            + report.conversations_failed
            + report.conversations_skipped
            + report.conversations_cancelled,
        report.conversations_total
    );
    assert_eq!(report.results.len(), 4, "{:?}", report.results);
    let cancelled: Vec<&str> = report
        .results
        .iter()
        .filter(|row| row.status == "cancelled")
        .map(|row| row.file.as_str())
        .collect();
    assert_eq!(cancelled.len(), 3, "{:?}", report.results);
    assert!(
        cancelled.iter().all(|file| file.ends_with(".jsonl")),
        "each cancelled row names its file: {cancelled:?}"
    );
    assert_eq!(journal_events(dir.path(), "file_ok").len(), 1);
}

/// A conversation whose messages were partly queued when the stop came is
/// counted as cancelled, and the journal does not mark it done, so a resumed
/// push sends the rest of it.
///
/// Guards the `Abort` path in `consume_result`, which returns before the
/// file's result is written: without a result the file falls out of every
/// count, and journalling it as done would lose its unsent messages.
#[test]
fn a_conversation_cut_off_mid_way_by_a_cancel_is_counted_as_cancelled() {
    let server = MockServer::start();
    let _auth = mock_session(&server);
    let _run = mock_import_run(&server, 7);
    // The delay keeps the first batch in flight long enough to cancel while
    // the second message of the same conversation waits to be sent.
    let first_batch = server.mock(|when, then| {
        when.method(POST).path("/v1/imports/7/batches");
        then.status(200)
            .delay(std::time::Duration::from_millis(1_000))
            .json_body(json!({
                "messages": 1,
                "messages_appended": 1,
                "conversations": 1
            }));
    });

    let dir = tempdir().unwrap();
    let mut doc = sample_doc();
    let mut second = doc.messages[0].clone();
    second.guid = "guid-1b".into();
    let mut third = doc.messages[0].clone();
    third.guid = "guid-1c".into();
    doc.messages.extend([second, third]);
    write_jsonl(dir.path(), &doc);
    let cancel = Arc::new(AtomicBool::new(false));
    let cfg = PushConfig {
        batch_size: 1,
        prepare_ahead: 1,
        prepare_workers: 1,
        cancel: Some(cancel.clone()),
        ..text_only_config(dir.path(), server.base_url())
    };

    let report = std::thread::scope(|scope| {
        let pusher = scope.spawn(|| run(&cfg, None).unwrap());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while first_batch.calls() == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "the first batch was never posted"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        cancel.store(true, Ordering::SeqCst);
        pusher.join().unwrap()
    });

    assert_eq!(first_batch.calls(), 1, "no batch is sent after the cancel");
    assert_eq!(report.conversations_total, 1);
    assert_eq!(report.conversations_cancelled, 1, "{:?}", report.results);
    assert!(report.cancelled, "the cancel flag stopped the run");
    assert_eq!(report.results.len(), 1);
    assert_eq!(report.results[0].status, "cancelled");
    assert_eq!(
        report.results[0].messages, 1,
        "the row counts the message that reached the server"
    );
    assert_eq!(journaled_guids(dir.path()), vec!["guid-1".to_string()]);
    assert!(
        journal_events(dir.path(), "file_ok").is_empty(),
        "a conversation cut off mid way is not journalled as done"
    );
}

/// After one batch fails, a second push on the same folder sends only the
/// failed conversation's messages, and the journal then has every file ok.
///
/// Guards the resume path after a partial failure: a failed file wrongly
/// journaled as ok would never be sent again, and a lost journal entry for a
/// good file would send it twice.
#[test]
fn a_second_push_sends_only_the_conversation_whose_batch_failed() {
    let server = MockServer::start();
    let _auth = mock_session(&server);
    let _run = mock_import_run(&server, 7);
    let mut failing = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/imports/7/batches")
            .body_includes("guid-2");
        then.status(500).json_body(json!({
            "type": "about:blank",
            "title": "Internal server error",
            "status": 500,
            "detail": "intentional batch failure"
        }));
    });
    let mut others = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/imports/7/batches")
            .body_excludes("guid-2");
        then.status(200).json_body(json!({
            "messages": 1,
            "messages_appended": 1,
            "conversations": 1
        }));
    });

    let dir = tempdir().unwrap();
    write_jsonl(dir.path(), &sample_doc());
    write_jsonl(dir.path(), &sample_doc_for("+15555550102", "guid-2"));
    write_jsonl(dir.path(), &sample_doc_for("+15555550103", "guid-3"));
    let mut cfg = text_only_config(dir.path(), server.base_url());
    cfg.batch_size = 1;

    let first = run(&cfg, None).unwrap();
    assert!(!first.ok);
    assert!(!first.cancelled, "a failed batch is not a cancel");
    assert_eq!(first.conversations_ok, 2);
    assert_eq!(first.conversations_failed, 1);
    assert_eq!(failing.calls(), 1);
    assert_eq!(others.calls(), 2);
    assert_eq!(journal_events(dir.path(), "file_ok").len(), 2);
    assert!(!journaled_guids(dir.path()).contains(&"guid-2".to_string()));

    failing.delete();
    others.delete();
    let retried = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/imports/7/batches")
            .body_includes("guid-2");
        then.status(200).json_body(json!({
            "messages": 1,
            "messages_appended": 1,
            "conversations": 1
        }));
    });
    let resent = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/imports/7/batches")
            .body_excludes("guid-2");
        then.status(200).json_body(json!({
            "messages": 1,
            "messages_appended": 1,
            "conversations": 1
        }));
    });

    let second = run(&cfg, None).unwrap();

    assert!(second.ok, "{:?}", second.results);
    assert_eq!(second.conversations_ok, 1);
    assert_eq!(second.conversations_skipped, 2);
    assert_eq!(second.messages_attempted, 1);
    assert_eq!(retried.calls(), 1, "the failed conversation is sent again");
    assert_eq!(
        resent.calls(),
        0,
        "a journaled conversation is not sent again"
    );
    assert_eq!(journal_events(dir.path(), "file_ok").len(), 3);
    let mut guids = journaled_guids(dir.path());
    guids.sort();
    assert_eq!(guids, vec!["guid-1", "guid-2", "guid-3"]);
}

/// A 2xx means the server did the work. When its body cannot be read, the
/// batch is not posted again as if the request had failed in transit: the
/// conversation fails once with a sentence saying the answer was unreadable,
/// and the next push sends it again for the server to dedupe.
#[test]
fn an_unreadable_2xx_answer_is_not_retried() {
    let server = MockServer::start();
    let _auth = mock_session(&server);
    let _run = mock_import_run(&server, 7);
    let import = server.mock(|when, then| {
        when.method(POST).path("/v1/imports/7/batches");
        then.status(200).body("<html>not the server</html>");
    });

    let dir = tempdir().unwrap();
    write_jsonl(dir.path(), &sample_doc());
    let cfg = PushConfig {
        max_retries: 2,
        ..text_only_config(dir.path(), server.base_url())
    };

    let report = run(&cfg, None).unwrap();

    assert_eq!(import.calls(), 1, "a 2xx must not be posted again");
    assert!(!report.ok);
    assert_eq!(report.conversations_failed, 1);
    let error = report.results[0].error.as_deref().unwrap_or_default();
    assert!(
        error.contains("could not read the server's answer to import batch"),
        "{error}"
    );
    assert!(journal_events(dir.path(), "file_ok").is_empty());
}

/// A server that answers each request on its own connection and closes it.
/// The batch POST gets a `200 OK` whose headers promise a body the server
/// never sends, so the client fails while reading it; the other routes a
/// push calls get their usual answers. Returns the base URL and a count of
/// batch POSTs.
fn serve_a_200_that_drops_the_batch_body() -> (String, Arc<AtomicUsize>) {
    use std::io::{BufRead, BufReader, Read};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let batches = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&batches);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            if reader.read_line(&mut request_line).is_err() {
                continue;
            }
            let mut content_length = 0usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some((name, value)) = line.split_once(':')
                    && name.eq_ignore_ascii_case("content-length")
                {
                    content_length = value.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; content_length];
            let _ = reader.read_exact(&mut body);
            let answer = |status: &str, body: &str| {
                format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
            };
            let response = if request_line.starts_with("GET /v1/session ") {
                answer("200 OK", r#"{"account_id":1,"username":"alice"}"#)
            } else if request_line.starts_with("POST /v1/imports ") {
                answer("201 Created", r#"{"id":7}"#)
            } else if request_line.starts_with("POST /v1/imports/7/batches ") {
                counted.fetch_add(1, Ordering::SeqCst);
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 4096\r\nConnection: close\r\n\r\n{\"messages\":".to_string()
            } else {
                answer("404 Not Found", "{}")
            };
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    });
    (base_url, batches)
}

/// A `200 OK` whose body is cut off means the server committed the batch.
/// Sending it again would store every message without a GUID a second time,
/// so the batch is posted once and the conversation fails with a sentence
/// saying the answer could not be read (#1162).
#[test]
fn a_2xx_answer_whose_body_is_cut_off_is_not_retried() {
    let (base_url, batches) = serve_a_200_that_drops_the_batch_body();

    let dir = tempdir().unwrap();
    write_jsonl(dir.path(), &sample_doc());
    let cfg = PushConfig {
        max_retries: 2,
        ..text_only_config(dir.path(), base_url)
    };

    let report = run(&cfg, None).unwrap();

    assert_eq!(
        batches.load(Ordering::SeqCst),
        1,
        "a 2xx must not be posted again"
    );
    assert!(!report.ok);
    assert_eq!(report.conversations_failed, 1);
    let error = report.results[0].error.as_deref().unwrap_or_default();
    assert!(
        error.contains("could not read the server's answer to import batch"),
        "{error}"
    );
    assert!(journal_events(dir.path(), "file_ok").is_empty());
}

/// When a conversation's chunk would overflow the pending batch, the batch
/// goes first and the chunk starts the next one: every message still reaches
/// the server, and no request carries more than `batch_size` messages.
///
/// Guards the flush-then-add step in `queue_chunk`. A chunk dropped after
/// the forced flush never reaches the server, yet its conversation would be
/// journaled as done, so a later push would never send it.
#[test]
fn a_chunk_that_overflows_the_pending_batch_is_sent_in_the_next_one() {
    let server = MockServer::start();
    let _auth = mock_session(&server);
    let _run = mock_import_run(&server, 7);
    let first = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/imports/7/batches")
            .body_includes("guid-a1")
            .body_excludes("guid-b1")
            .body_excludes("guid-b2");
        then.status(200).json_body(json!({
            "messages": 1,
            "messages_appended": 1,
            "conversations": 1
        }));
    });
    let second = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/imports/7/batches")
            .body_includes("guid-b1")
            .body_includes("guid-b2")
            .body_excludes("guid-a1");
        then.status(200).json_body(json!({
            "messages": 2,
            "messages_appended": 2,
            "conversations": 1
        }));
    });

    let dir = tempdir().unwrap();
    write_jsonl(dir.path(), &sample_doc_for("+15555550101", "guid-a1"));
    let mut two_messages = sample_doc_for("+15555550102", "guid-b1");
    let mut reply = two_messages.messages[0].clone();
    reply.guid = "guid-b2".into();
    reply.timestamp_unix_ms += 1_000;
    two_messages.messages.push(reply);
    write_jsonl(dir.path(), &two_messages);
    let cfg = PushConfig {
        batch_size: 2,
        ..text_only_config(dir.path(), server.base_url())
    };

    let report = run(&cfg, None).unwrap();

    assert!(report.ok, "{:?}", report.results);
    assert_eq!(first.calls(), 1, "the one-message batch goes out alone");
    assert_eq!(second.calls(), 1, "the two-message chunk goes out next");
    assert_eq!(report.conversations_ok, 2);
    assert_eq!(report.messages_attempted, 3);
    assert_eq!(report.messages_inserted, 3);
    let mut guids = journaled_guids(dir.path());
    guids.sort();
    assert_eq!(guids, vec!["guid-a1", "guid-b1", "guid-b2"]);
}

/// The desktop app sets no journal path, so the journal a second push reads
/// is the one the first push wrote inside the export folder.
#[test]
fn a_push_with_no_journal_path_keeps_its_journal_in_the_export_folder() {
    let server = MockServer::start();
    let _auth = mock_session(&server);
    let _run = mock_import_run(&server, 7);
    let import = server.mock(|when, then| {
        when.method(POST).path("/v1/imports/7/batches");
        then.status(200).json_body(json!({
            "messages": 1,
            "messages_appended": 1,
            "conversations": 1
        }));
    });

    let dir = tempdir().unwrap();
    write_jsonl(dir.path(), &sample_doc());
    let mut cfg = text_only_config(dir.path(), server.base_url());
    cfg.journal_path = None;

    assert!(run(&cfg, None).unwrap().ok);
    assert!(dir.path().join(".import-state.jsonl").is_file());

    let second = run(&cfg, None).unwrap();
    assert!(second.ok);
    assert_eq!(second.conversations_skipped, 1);
    assert_eq!(import.calls(), 1, "the second push sends nothing");
}

/// A batch of 100 messages or more is sent while later conversations are
/// still being prepared. That early send succeeding must not end the push.
#[test]
fn a_push_goes_on_after_sending_a_large_batch_early() {
    let server = MockServer::start();
    let _auth = mock_session(&server);
    let _run = mock_import_run(&server, 7);
    let import = server.mock(|when, then| {
        when.method(POST).path("/v1/imports/7/batches");
        then.status(200).json_body(json!({
            "messages": 1,
            "messages_appended": 1,
            "conversations": 1
        }));
    });

    let dir = tempdir().unwrap();
    // The first file in name order holds 120 messages. One conversation is
    // prepared at a time, so the loop comes round with that batch pending
    // while two conversations are still to come.
    let mut large = sample_doc();
    let first = large.messages[0].clone();
    large.messages = (0..120)
        .map(|i| {
            let mut msg = first.clone();
            msg.guid = format!("large-{i}");
            msg.timestamp_unix_ms += i;
            msg
        })
        .collect();
    write_jsonl(dir.path(), &large);
    write_jsonl(dir.path(), &sample_doc_for("+15555550102", "guid-2"));
    write_jsonl(dir.path(), &sample_doc_for("+15555550103", "guid-3"));
    let mut cfg = text_only_config(dir.path(), server.base_url());
    cfg.batch_size = message_crate_push::DEFAULT_BATCH_SIZE;
    cfg.prepare_ahead = 1;

    let report = run(&cfg, None).unwrap();

    assert!(report.ok, "{:?}", report.results);
    assert_eq!(report.conversations_ok, 3);
    assert_eq!(report.messages_attempted, 122);
    assert_eq!(import.calls(), 2, "the large batch, then the rest");
}

/// The default size limit is for files no server would take. A photo of a
/// few MiB is far under it and must be uploaded, not skipped as too large.
#[test]
fn a_push_with_the_default_size_limit_uploads_a_photo_of_two_mebibytes() {
    let photo = vec![0x5a_u8; 2 * 1024 * 1024];
    let digest = hex::encode(Sha256::digest(&photo));

    let server = MockServer::start();
    let _auth = mock_session(&server);
    let _run = mock_import_run(&server, 7);
    let _head = server.mock(|when, then| {
        when.method("HEAD").path(format!("/v1/assets/{digest}"));
        then.status(404);
    });
    let put = server.mock(|when, then| {
        when.method(PUT).path(format!("/v1/assets/{digest}"));
        then.status(200)
            .json_body(json!({ "already_present": false }));
    });
    let _import = server.mock(|when, then| {
        when.method(POST).path("/v1/imports/7/batches");
        then.status(200).json_body(json!({
            "messages": 1,
            "messages_appended": 1
        }));
    });

    let dir = tempdir().unwrap();
    let attachment_dir = dir.path().join("attachments");
    fs::create_dir(&attachment_dir).unwrap();
    fs::write(attachment_dir.join("photo.jpg"), &photo).unwrap();
    let mut doc = sample_doc();
    doc.messages[0].attachments = vec![ir_attachment("attachments/photo.jpg", digest)];
    write_jsonl(dir.path(), &doc);

    let mut cfg = text_only_config(dir.path(), server.base_url());
    cfg.asset_max_bytes = message_crate_push::DEFAULT_ASSET_MAX_BYTES;
    let report = run(&cfg, None).unwrap();

    assert!(report.ok, "{:?}", report.results);
    assert_eq!(put.calls(), 1);
    assert_eq!(report.assets_uploaded, 1);
}
