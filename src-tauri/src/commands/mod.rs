//! Commands the Vite UI calls through Tauri's `invoke`.
//!
//! Each `#[tauri::command]` function runs in this desktop process, not in the
//! WebView. That is how the UI starts exporters, finds ffmpeg, and reads the
//! user's home directory. Native file dialogs come from the dialog plugin
//! registered in `main.rs`.
//!
//! Long jobs return as soon as the background thread starts. Progress, log
//! lines, issues, and errors are sent as Tauri events (`extract:log`,
//! `extract:progress`, `extract:issue`, `extract:finished`,
//! `extract:error`).

pub mod events;
pub mod extract;
pub mod ffmpeg;
pub mod format;
pub mod jobs;
pub mod local_server;
pub mod paths;
pub mod pull;
pub mod push;
pub mod staging;

/// Last log line from a job, or `fallback` when the job wrote none.
pub(crate) fn last_log_line_or(messages: &[String], fallback: &str) -> String {
    match messages.last() {
        Some(line) => line.clone(),
        None => fallback.to_string(),
    }
}
