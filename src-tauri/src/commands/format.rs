//! `format` command — convert an existing extract folder to another format.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use message_crate_core::{
    ExporterConfig, FormatConfig, LogSink, MediaConfig, OutputFormat, SourceConfig,
};

use super::events;
use super::jobs::{spawn_job, start_job};
use super::last_log_line_or;
use crate::state::AppState;

/// Ask this process to rewrite an extract folder in a different file format.
///
/// Returns as soon as the background thread starts. Log lines and the final
/// summary use the same `extract:log` / `extract:finished` / `extract:error`
/// events as Extract, so the UI can reuse one progress view.
///
/// # Errors
///
/// Returns an error if `output_format` is not one of json, jsonl, csv, eml,
/// mbox, xml, or sms-backup-plus, if another job is running, or if another
/// thread panicked while holding the shared state lock. Failures during
/// conversion are sent as `extract:error`.
#[tauri::command(async)]
pub fn format(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    input_dir: String,
    output_dir: String,
    output_format: String,
    run_started_ms: Option<i64>,
) -> Result<(), String> {
    let fmt = match output_format.as_str() {
        "json" => OutputFormat::Json,
        "jsonl" => OutputFormat::Jsonl,
        "csv" => OutputFormat::Csv,
        "eml" => OutputFormat::Eml,
        "mbox" => OutputFormat::Mbox,
        "xml" => OutputFormat::Xml,
        "sms-backup-plus" => OutputFormat::SmsBackupPlus,
        _ => return Err(format!("unsupported output format '{output_format}'")),
    };

    let job = start_job(&state, "a format conversion")?;
    let cancel = job.cancel_flag();

    let app_handle = app.clone();
    spawn_job(app, job, move || {
        let log_app = app_handle.clone();
        let config = ExporterConfig {
            inputs: vec![PathBuf::from(&input_dir)],
            output: PathBuf::from(&output_dir),
            timezone: None,
            obfuscate: Default::default(),
            media: MediaConfig::default(),
            cancel: Some(cancel),
            log: Some(LogSink::new(move |line: &str| {
                events::emit(&log_app, events::LOG, line.to_string());
            })),
            // Settings → Convert shows a log, not a progress bar.
            progress: None,
            output_format: fmt,
            resume: false,
            source: SourceConfig::Format(FormatConfig { run_started_ms }),
        };

        let run_result = message_reexport::run(&config)?;
        let summary = last_log_line_or(&run_result.messages, "Format conversion complete.");
        for line in run_result.messages {
            events::emit(&app_handle, events::LOG, line);
        }
        Ok(summary)
    });

    Ok(())
}
