//! `pull` command — download messages from a Message Vault server.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use vault_pull::{
    DEFAULT_ASSET_DOWNLOAD_WORKERS, DEFAULT_PAGE_LIMIT, ProgressEvent, VaultPullConfig,
    run as run_pull,
};

use super::events;
use super::jobs::{reset_and_clone_cancel, spawn_job};
use crate::state::AppState;

/// User-facing parameters for the `pull` command.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullArgs {
    /// Base URL of the vault server, for example `http://127.0.0.1:8080`.
    pub base_url: String,
    /// Vault account name.
    pub username: String,
    /// API token or account password for the vault.
    pub key: String,
    /// Folder the pulled conversation files are written into.
    pub out_dir: String,
    /// Vault search query selecting which conversations to pull.
    pub query: String,
    /// When true, skip attachments and download messages only.
    pub skip_attachments: bool,
}

/// Ask this process to download conversations from a vault server.
///
/// Returns as soon as the background thread starts. Log lines and the final
/// summary use the same `extract:log` / `extract:finished` / `extract:error`
/// events as Extract.
///
/// # Errors
///
/// Returns an error if another thread panicked while holding the shared
/// state lock. Failures during the download are sent as `extract:error`.
#[tauri::command]
pub async fn pull(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    args: PullArgs,
) -> Result<(), String> {
    let cancel = reset_and_clone_cancel(&state)?;

    let app_handle = app.clone();
    spawn_job(app, move || {
        let cfg = VaultPullConfig {
            out_dir: PathBuf::from(&args.out_dir),
            base_url: args.base_url,
            username: args.username,
            key: args.key,
            query: args.query,
            skip_attachments: args.skip_attachments,
            page_limit: DEFAULT_PAGE_LIMIT,
            cancel: Some(cancel),
            asset_download_workers: DEFAULT_ASSET_DOWNLOAD_WORKERS,
        };

        let mut progress = |event: ProgressEvent| match event {
            ProgressEvent::Log(line) => {
                events::emit(&app_handle, events::LOG, line);
            }
            ProgressEvent::Auth { .. } => {}
            ProgressEvent::Page {
                messages,
                total_so_far,
            } => {
                events::emit(
                    &app_handle,
                    events::LOG,
                    format!("Fetched {messages} message(s) ({total_so_far} total)"),
                );
            }
            ProgressEvent::Done(_) => {}
        };

        match run_pull(&cfg, Some(&mut progress)) {
            Ok(report) => {
                let summary = format!(
                    "Pull complete: {} messages, {} conversations",
                    report.messages, report.conversations,
                );
                events::emit(&app_handle, events::FINISHED, summary);
            }
            Err(err) => return Err(err),
        }
        Ok(())
    });

    Ok(())
}
