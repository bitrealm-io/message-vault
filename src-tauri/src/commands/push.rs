//! `push` command — upload an extract folder to a Message Crate server.

use message_crate_push::ImportMode;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use message_crate_push::{ProgressEvent, PushConfig, run as run_push};

use super::events;
use super::events::ExtractProgressEvent;
use super::jobs::{spawn_job, start_job};
use crate::state::AppState;

/// Convert a report count to the `usize` the progress event uses.
fn as_usize(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

/// Progress bar update and finished JSON payload after a push completes.
fn finished_push_events(
    report: &message_crate_push::PushReport,
) -> (ExtractProgressEvent, serde_json::Value) {
    let progress = ExtractProgressEvent {
        step: "upload".into(),
        done: as_usize(report.conversations_total),
        total: as_usize(report.conversations_total),
        bytes_done: None,
        bytes_total: None,
        status: None,
    };
    let summary = serde_json::json!({
        "summary": format!(
            "Push complete: {} new, {} deduped, {} failed of {} attempted; {}/{} conversations ok; {} assets uploaded",
            report.messages_inserted,
            report.messages_deduped,
            report.messages_failed,
            report.messages_attempted,
            report.conversations_ok,
            report.conversations_total,
            report.assets_uploaded
        ),
        "ok": report.ok,
        "messages": report.messages,
        "messages_attempted": report.messages_attempted,
        "messages_inserted": report.messages_inserted,
        "messages_deduped": report.messages_deduped,
        "messages_failed": report.messages_failed,
        "assets_uploaded": report.assets_uploaded,
        "assets_bytes": report.assets_bytes,
        "conversations_ok": report.conversations_ok,
        "conversations_total": report.conversations_total,
        "conversations_failed": report.conversations_failed,
        "conversations_skipped": report.conversations_skipped,
        "results": report.results,
    });
    (progress, summary)
}

/// User-facing parameters for the `push` command.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushArgs {
    /// Base URL of the server, for example `http://127.0.0.1:8080`.
    pub base_url: String,
    /// Account name.
    pub username: String,
    /// Bearer token for the server: an API token, or the logged-in Session's
    /// token. Never a password.
    pub key: String,
    /// Folder of conversation files to upload.
    pub input_dir: String,
    /// Import mode. `append` adds to existing data (safe to re-run);
    /// `replace` deletes existing messages for this source, then imports.
    pub mode: ImportMode,
    /// When true, import messages without uploading attachments.
    pub skip_attachments: bool,
    /// When true, trust export metadata: skip re-hashing attachments when
    /// size_bytes matches the file size on disk. Without this flag every
    /// attachment is re-hashed.
    pub trust_export: bool,
    /// Import id of an earlier import to resume, when set.
    pub import_id: Option<i64>,
    /// The server's attachment size limit, in bytes: Upload leaves out a
    /// larger file as too large. The app reads it from `GET /v1/server`
    /// before Staging and stores it with the Import Run, so this is the
    /// number the Staging Review forecast against.
    pub asset_max_bytes: u64,
}

/// Ask this process to upload extracted conversations to a server.
///
/// Returns as soon as the background thread starts. Upload progress uses the
/// same `extract:*` events as Extract so the UI can reuse one progress view.
///
/// # Errors
///
/// Returns an error if another job is running, or if another thread panicked
/// while holding the shared state lock. Failures during the upload are sent
/// as `extract:error`.
#[tauri::command(async)]
pub fn push(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    args: PushArgs,
) -> Result<(), String> {
    let job = start_job(&state, "an upload")?;
    let cancel = job.cancel_flag();
    let app_handle = app.clone();
    spawn_job(app, job, move || {
        let mut cfg = push_config(args);
        cfg.cancel = Some(cancel);
        let mut progress = |event: ProgressEvent| forward_push_event(&app_handle, event);
        let report = run_push(&cfg, Some(&mut progress))?;
        Ok(finished_push_events(&report).1.to_string())
    });
    Ok(())
}

/// The push settings the desktop app uses. They differ from the
/// `message_crate_push::DEFAULT_*` constants because desktop imports are many
/// small files over a local network; each number says why.
fn push_config(args: PushArgs) -> PushConfig {
    PushConfig {
        input: PathBuf::from(&args.input_dir),
        base_url: args.base_url,
        username: args.username,
        key: args.key,
        mode: args.mode,
        // A resumed Upload skips what the journal in the staging folder
        // already recorded as sent.
        force: false,
        skip_attachments: args.skip_attachments,
        trust_export: args.trust_export,
        verify_digests: false,
        max_retries: 3,
        // Pack until message_crate_push::MAX_IMPORT_BODY_BYTES (64 MiB); do not stop at a message count.
        batch_size: message_crate_push::NO_MESSAGE_COUNT_LIMIT,
        // Above DEFAULT_ASSET_UPLOAD_WORKERS (8): desktop imports are often many small files.
        asset_upload_workers: 16,
        // Above DEFAULT_PREPARE_AHEAD (3): hide more hashing behind in-flight imports.
        prepare_ahead: 8,
        // Above DEFAULT_PREPARE_WORKERS (2): more of the prepare-ahead queue runs at once.
        prepare_workers: 4,
        // Below message_crate_push::MAX_PROXY_BODY_BYTES (90 MiB):
        // desktop uploads switch to multipart sooner so a large attachment
        // moves in small parts instead of one long PUT.
        asset_multipart_threshold: 5 * 1024 * 1024,
        // Per-file attachment cap, the server's own. JSONL import batches use
        // MAX_IMPORT_BODY_BYTES.
        asset_max_bytes: args.asset_max_bytes,
        report_path: None,
        log_path: None,
        // The journal stays in the staging folder, beside the files it tracks.
        journal_path: None,
        cancel: None,
        import_id: args.import_id,
    }
}

/// Relay one push progress event to the window as `extract:*` events.
fn forward_push_event(app: &tauri::AppHandle, event: ProgressEvent) {
    match event {
        ProgressEvent::Log(line) => {
            events::emit(app, events::LOG, line);
        }
        ProgressEvent::Auth { .. } => {}
        ProgressEvent::FileStart { index, total, file } => {
            events::emit(app, events::LOG, format!("Starting: {file}"));
            events::emit(
                app,
                events::PROGRESS,
                ExtractProgressEvent {
                    step: "upload".into(),
                    done: index.saturating_sub(1),
                    total,
                    bytes_done: None,
                    bytes_total: None,
                    status: None,
                },
            );
        }
        ProgressEvent::FileDone { file, status } => {
            events::emit(app, events::LOG, format!("Done: {file} ({status})"));
        }
        ProgressEvent::Issue {
            kind,
            step,
            item,
            reason,
        } => {
            events::emit(
                app,
                events::ISSUE,
                serde_json::json!({
                    "kind": kind,
                    "step": step,
                    "item": item,
                    "reason": reason,
                }),
            );
        }
        ProgressEvent::Finished(report) => {
            // The finished event goes out once the job has ended (`spawn_job`).
            let (progress, _summary) = finished_push_events(&report);
            events::emit(app, events::PROGRESS, progress);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use message_crate_push::{FileResult, PushReport};
    use message_ir::{
        ConversationMeta, ConversationStats, ExportMeta, IrConversationType, IrDirection,
        IrMessage, IrMessageKind, IrParticipant, IrService, SCHEMA_VERSION,
    };
    use serde_json::json;

    /// Upload holds a file to the limit the app read from the server, not to
    /// a number of the desktop app's own.
    #[test]
    fn upload_uses_the_attachment_size_limit_it_is_given() {
        let args: PushArgs = serde_json::from_value(json!({
            "baseUrl": "http://127.0.0.1:8080",
            "username": "",
            "key": "token",
            "inputDir": "/tmp/staging-root/staging-run",
            "mode": "append",
            "skipAttachments": false,
            "trustExport": true,
            "importId": 7,
            "assetMaxBytes": 123_456_789,
        }))
        .unwrap();
        assert_eq!(push_config(args).asset_max_bytes, 123_456_789);
    }

    /// The limit has no default in the desktop app: a call that leaves it out
    /// is refused, because the app has no number of its own to fall back on.
    #[test]
    fn upload_without_a_limit_is_refused() {
        let args = serde_json::from_value::<PushArgs>(json!({
            "baseUrl": "http://127.0.0.1:8080",
            "username": "",
            "key": "token",
            "inputDir": "/tmp/staging-root/staging-run",
            "mode": "append",
            "skipAttachments": false,
            "trustExport": true,
            "importId": 7,
        }));
        assert!(args.unwrap_err().to_string().contains("assetMaxBytes"));
    }

    /// A resumed Upload runs over the staging folder the interrupted Upload
    /// left, journal included. Sending the journaled messages again would
    /// only make the server count each one as a Duplicate.
    #[test]
    fn a_resumed_upload_does_not_send_what_the_journal_recorded() {
        let server = MockServer::start();
        let _session = server.mock(|when, then| {
            when.method(GET).path("/v1/session");
            then.status(200).json_body(json!({
                "account_id": 1,
                "username": "alice",
            }));
        });
        let batches = server.mock(|when, then| {
            when.method(POST).path("/v1/imports/7/batches");
            then.status(200).json_body(json!({
                "messages": 1,
                "messages_appended": 1,
                "conversations": 1
            }));
        });

        let staging = tempfile::tempdir().unwrap();
        let header = json!({
            "schema_version": SCHEMA_VERSION,
            "export": ExportMeta {
                source: "sms-backup-restore".into(),
                tool: "SMS Backup & Restore".into(),
                tool_version: "10.26.003".into(),
                owner_handle: Some("+15555550100".into()),
                owner_display_name: Some("Me".into()),
            },
            "conversation": ConversationMeta {
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
        });
        let message = json!(IrMessage {
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
        });
        std::fs::write(
            staging.path().join("sam.jsonl"),
            format!("{header}\n{message}\n"),
        )
        .unwrap();

        // What the Import screen sends for an Upload, first time and resumed.
        let upload = || {
            let args: PushArgs = serde_json::from_value(json!({
                "baseUrl": server.base_url(),
                "username": "",
                "key": "mc_test",
                "inputDir": staging.path(),
                "mode": "append",
                "skipAttachments": false,
                "trustExport": true,
                "importId": 7,
                "assetMaxBytes": 512 * 1024 * 1024,
            }))
            .unwrap();
            run_push(&push_config(args), None).unwrap()
        };

        let first = upload();
        assert!(first.ok, "{:?}", first.results);
        assert_eq!(batches.calls(), 1);

        let resumed = upload();
        assert!(resumed.ok, "{:?}", resumed.results);
        assert_eq!(resumed.messages_attempted, 0);
        assert_eq!(batches.calls(), 1, "the resumed Upload sends no batch");
    }

    #[test]
    fn finished_push_event_reports_complete_upload_and_totals() {
        let report = PushReport {
            ok: true,
            account: 1,
            username: "user".into(),
            mode: ImportMode::Append,
            started_at: "2026-08-11T00:00:00Z".into(),
            finished_at: "2026-08-11T00:00:01Z".into(),
            elapsed_ms: 1_000,
            conversations_total: 3,
            conversations_ok: 2,
            conversations_failed: 0,
            conversations_skipped: 1,
            messages_attempted: 45,
            messages_inserted: 42,
            messages_deduped: 2,
            messages_failed: 1,
            messages: 42,
            assets_uploaded: 4,
            assets_skipped: 1,
            assets_bytes: 12_345,
            results: vec![FileResult {
                file: "failed.jsonl".into(),
                status: "failed".into(),
                error: Some("attachment exceeds limit".into()),
                messages: 0,
                attachments: 0,
                profile: None,
            }],
        };

        let (progress, summary) = finished_push_events(&report);

        assert_eq!(progress.step, "upload");
        assert_eq!(progress.done, 3);
        assert_eq!(progress.total, 3);
        assert_eq!(summary["messages"], 42);
        assert_eq!(summary["messages_attempted"], 45);
        assert_eq!(summary["messages_inserted"], 42);
        assert_eq!(summary["messages_deduped"], 2);
        assert_eq!(summary["messages_failed"], 1);
        assert_eq!(summary["conversations_failed"], 0);
        assert_eq!(summary["conversations_skipped"], 1);
        assert_eq!(summary["results"][0]["error"], "attachment exceeds limit");
        assert_eq!(
            summary["summary"],
            "Push complete: 42 new, 2 deduped, 1 failed of 45 attempted; 2/3 conversations ok; 4 assets uploaded"
        );
        assert_eq!(summary["assets_bytes"], 12_345);
        assert_eq!(summary["conversations_ok"], 2);
        assert_eq!(summary["conversations_total"], 3);
    }
}
