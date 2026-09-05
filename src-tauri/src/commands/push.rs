//! `push` command — upload an extract folder to a Message Vault server.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use vault_push::{ProgressEvent, VaultPushConfig, run as run_push};

use super::events;
use super::events::ExtractProgressEvent;
use super::jobs::{reset_and_clone_cancel, spawn_job};
use crate::state::AppState;

/// Largest attachment the desktop app will upload.
///
/// The vault's own `asset_max_bytes` defaults higher and is not exposed to
/// clients, so this is the number the app can actually promise. The size
/// forecast at the first gate predicts against this same constant — a forecast
/// against a different limit than the upload uses would be worse than none.
pub const ASSET_MAX_BYTES: u64 = 50 * 1024 * 1024;

/// Convert a report count to the `usize` the progress event uses.
fn as_usize(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

/// Progress bar update and finished JSON payload after a push completes.
fn finished_push_events(
    report: &vault_push::PushReport,
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
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushArgs {
    /// Base URL of the vault server, for example `http://127.0.0.1:8080`.
    pub base_url: String,
    /// Vault account name.
    pub username: String,
    /// API token or account password for the vault.
    pub key: String,
    /// Folder of conversation files to upload.
    pub input_dir: String,
    /// Import mode. `append` adds to existing data (safe to re-run);
    /// `replace` deletes existing messages for this source, then imports.
    pub mode: String,
    /// When true, ignore the journal and re-upload assets and re-import
    /// messages.
    pub force: bool,
    /// When true, continue after a failed conversation.
    pub continue_on_error: bool,
    /// When true, import messages without uploading attachments.
    pub skip_attachments: bool,
    /// When true, trust export metadata: skip re-hashing attachments when
    /// size_bytes matches the file size on disk. Without this flag every
    /// attachment is re-hashed.
    pub trust_export: bool,
    /// Import id of an earlier import to resume, when set.
    pub import_id: Option<i64>,
}

/// Ask this process to upload extracted conversations to a vault server.
///
/// Returns as soon as the background thread starts. Upload progress uses the
/// same `extract:*` events as Extract so the UI can reuse one progress view.
///
/// # Errors
///
/// Returns an error if another thread panicked while holding the shared
/// state lock. Failures during the upload are sent as `extract:error`.
#[tauri::command]
pub async fn push(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    args: PushArgs,
) -> Result<(), String> {
    let cancel = reset_and_clone_cancel(&state)?;
    let app_handle = app.clone();
    spawn_job(app, move || {
        let mut cfg = push_config(args);
        cfg.cancel = Some(cancel);
        let mut progress = |event: ProgressEvent| forward_push_event(&app_handle, event);
        run_push(&cfg, Some(&mut progress)).map(|_report| ())
    });
    Ok(())
}

/// The push settings the desktop app uses. They differ from the command-line
/// defaults because desktop imports are many small files over a local
/// network; each number says why.
fn push_config(args: PushArgs) -> VaultPushConfig {
    VaultPushConfig {
        input: PathBuf::from(&args.input_dir),
        base_url: args.base_url,
        username: args.username,
        key: args.key,
        mode: args.mode,
        continue_on_error: args.continue_on_error,
        force: args.force,
        skip_attachments: args.skip_attachments,
        trust_export: args.trust_export,
        verify_digests: false,
        max_retries: 3,
        // Pack until vault_push::MAX_IMPORT_BODY_BYTES (64 MiB); do not stop at a message count.
        batch_size: vault_push::NO_MESSAGE_COUNT_LIMIT,
        // Above the CLI default (8): desktop imports are often many small files.
        asset_upload_workers: 16,
        // Above the CLI default (3): hide more hashing behind in-flight imports.
        prepare_ahead: 8,
        // Above the CLI default (2): more of the prepare-ahead queue runs at once.
        prepare_workers: 4,
        // Below the CLI default (vault_push::MAX_PROXY_BODY_BYTES, 90 MiB):
        // desktop uploads switch to multipart sooner so a large attachment
        // moves in small parts instead of one long PUT.
        asset_multipart_threshold: 5 * 1024 * 1024,
        // Per-file attachment cap. JSONL import batches use MAX_IMPORT_BODY_BYTES.
        asset_max_bytes: ASSET_MAX_BYTES,
        report_path: None,
        log_path: None,
        // Relies on one preflight HEAD per run instead of a persisted journal.
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
            let (progress, summary) = finished_push_events(&report);
            events::emit(app, events::PROGRESS, progress);
            events::emit(app, events::FINISHED, summary.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vault_push::{FileResult, PushReport};

    #[test]
    fn finished_push_event_reports_complete_upload_and_totals() {
        let report = PushReport {
            ok: true,
            account: "account".into(),
            username: "user".into(),
            mode: "append".into(),
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
