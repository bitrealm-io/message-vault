//! `extract` and `cancel` commands.
//!
//! `extract` starts the selected exporter on a background thread and returns
//! immediately. Progress is sent back as Tauri events:
//! `extract:log` (one human-readable log line), `extract:progress` (one
//! typed [`ExtractProgressEvent`], mapped from the exporter's
//! `ProgressEvent`), `extract:finished` (a summary string or JSON object),
//! and `extract:error` ([`ExtractErrorEvent`]).
//!
//! The shared cancel flag lives in [`AppState`]. `cancel` sets it to true.
//! `extract` turns it off at the start of a job. The exporter checks it
//! between steps through `ExporterConfig.cancel`.

use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use media::{CompressOptions, MaxResolution};
use message_vault_io_core::{
    ApplePlatform, AttachmentMedia, Exporter, ExporterConfig, Form, LogSink, OutputFormat,
    ProgressSink, SourceConfig, WhatsappPlatform,
};

// Short names so the match in `run_exporter` stays easy to read.
use go_sms_pro_exporter::run as run_go_sms_pro;
use imazing_exporter::run as run_imazing;
use imessage_ir_exporter::run as run_imessage;
use openextract_exporter::run as run_openextract;
use sms_backup_plus_exporter::run as run_sms_plus;
use sms_backup_restore_exporter::run as run_sms_restore;
use whatsapp_exporter::run as run_whatsapp;

use super::events;
use super::events::{ExtractErrorEvent, ExtractProgressEvent};
use super::jobs::{reset_and_clone_cancel, spawn_job};
use super::last_log_line_or;
use crate::state::AppState;

/// Ask this process to stop the export that is currently running.
///
/// Sets the shared cancel flag. The exporter checks the flag between steps
/// and exits on its own. There is no hard kill.
///
/// # Errors
///
/// Returns an error if another thread panicked while holding the shared
/// state lock.
#[tauri::command]
pub async fn cancel(state: tauri::State<'_, Arc<Mutex<AppState>>>) -> Result<(), String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    state.cancel_flag.store(true, Ordering::SeqCst);
    Ok(())
}

/// How many conversation files and messages an extract wrote.
///
/// Each JSON Lines file (one JSON object per line) starts with a conversation
/// header. That header is not counted as a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct JsonlOutputCounts {
    files: usize,
    messages: usize,
}

/// True when `path` looks like a JSON Lines file (one JSON object per line).
fn is_json_lines_file(path: &Path) -> bool {
    let Some(extension) = path.extension() else {
        return false;
    };
    let Some(extension) = extension.to_str() else {
        return false;
    };
    extension == "jsonl"
}

/// Walk `root` and count JSON Lines conversation files and the messages in them.
///
/// The first non-empty line of each file is the conversation header, so it is
/// subtracted from the message total.
///
/// # Errors
///
/// Returns an error if a directory cannot be listed or a file cannot be opened.
fn count_jsonl_output(root: &Path) -> anyhow::Result<JsonlOutputCounts> {
    let mut counts = JsonlOutputCounts {
        files: 0,
        messages: 0,
    };
    let mut directories = vec![root.to_path_buf()];

    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                directories.push(entry.path());
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            if !is_json_lines_file(&entry.path()) {
                continue;
            }

            let mut reader = BufReader::new(File::open(entry.path())?);
            let mut line = String::new();
            let mut nonempty_lines = 0usize;
            while reader.read_line(&mut line)? != 0 {
                if !line.trim().is_empty() {
                    nonempty_lines = nonempty_lines.saturating_add(1);
                }
                line.clear();
            }
            if nonempty_lines > 0 {
                counts.files = counts.files.saturating_add(1);
                let message_lines = nonempty_lines.saturating_sub(1);
                counts.messages = counts.messages.saturating_add(message_lines);
            }
        }
    }

    Ok(counts)
}

/// User-facing parameters for the `extract` command (before defaults/parsing).
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractArgs {
    /// Backup source key, for example `imessage-ios` or `whatsapp-android`.
    pub source: String,
    /// Path to the phone backup (a folder, database file, or XML file).
    pub path: String,
    /// Folder the exporter writes conversation files into.
    pub output_dir: String,
    /// Password for encrypted backups, when the source needs one.
    pub backup_password: Option<String>,
    /// Attachment handling choice: `copy`, `convert`, `compress`, or `skip`.
    pub attachment_media: Option<String>,
    /// Video/image size cap for convert and compress: `720p`, `1080p`, or `4k`.
    pub media_max_resolution: Option<String>,
    /// Frame-rate cap for compressed video, for example `30`.
    pub media_max_fps: Option<String>,
    /// Smallest media file size that still counts as an attachment, for example `20M`.
    pub media_min_size: Option<String>,
    /// When true, replace names and phone numbers with fake ones.
    pub obfuscate: Option<bool>,
    /// Owner phone numbers for Android SMS exporters (SMS Backup & Restore).
    pub owner_phones: Option<Vec<String>>,
    /// Owner email addresses for SMS Backup+, whose archive is Gmail-backed
    /// and needs them to tell sent mail from received.
    pub owner_emails: Option<Vec<String>>,
    /// Alternate folder for Attachments and StickerCache (Mac and jailbreak).
    pub attachment_root: Option<String>,
    /// Path to an Apple AddressBook file (Mac and jailbreak).
    pub apple_contacts: Option<String>,
    /// WhatsApp decryption key or key-file path (Android crypt backups).
    pub whatsapp_key: Option<String>,
    /// Optional WhatsApp contacts database (`wa.db` / `ContactsV2.sqlite`).
    pub whatsapp_wa: Option<String>,
    /// Optional WhatsApp media folder.
    pub whatsapp_media: Option<String>,
    /// Optional explicit `msgstore.db` path.
    pub whatsapp_db: Option<String>,
    /// WhatsApp Business backup (iPhone only; Android stays false).
    pub whatsapp_business: Option<bool>,
    /// Continue an interrupted export in the same output folder: previous
    /// output is kept and conversations already written are skipped.
    pub resume: Option<bool>,
}

/// Ask this process to parse a phone backup and write conversation files.
///
/// Returns as soon as the background thread starts. Log lines, progress, and
/// the final summary are sent as `extract:log`, `extract:progress`,
/// `extract:finished`, and `extract:error`. Output is JSON Lines (one JSON
/// object per line) so the Import and Push screens can read it later.
///
/// # Errors
///
/// Returns an error if a form field is invalid, the source is unknown, or
/// another thread panicked while holding the shared state lock. Failures
/// during the export itself are sent as `extract:error`, not returned here.
#[tauri::command]
pub async fn extract(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    args: ExtractArgs,
) -> Result<(), String> {
    let options = ExtractOptions {
        backup_password: args.backup_password.unwrap_or_default(),
        attachment_media: parse_attachment_media(args.attachment_media.as_deref())?,
        media_max_resolution: parse_max_resolution(args.media_max_resolution.as_deref())?,
        media_max_fps: args.media_max_fps.unwrap_or_else(|| "30".into()),
        media_min_size: args.media_min_size.unwrap_or_else(|| "20M".into()),
        obfuscate: args.obfuscate.unwrap_or(false),
        // `Form` trims and drops empty values itself, so the raw strings can
        // pass through unchanged.
        owner_phones: args.owner_phones.unwrap_or_default(),
        owner_emails: args.owner_emails.unwrap_or_default(),
        attachment_root: args.attachment_root.unwrap_or_default(),
        apple_contacts: args.apple_contacts.unwrap_or_default(),
        whatsapp_key: args.whatsapp_key.unwrap_or_default(),
        whatsapp_wa: args.whatsapp_wa.unwrap_or_default(),
        whatsapp_media: args.whatsapp_media.unwrap_or_default(),
        whatsapp_db: args.whatsapp_db.unwrap_or_default(),
        whatsapp_business: args.whatsapp_business.unwrap_or(false),
    };

    let output_dir = args.output_dir;
    let mut config = build_exporter_config(&args.source, &args.path, &output_dir, &options)?;
    config.resume = args.resume.unwrap_or(false);

    let cancel = reset_and_clone_cancel(&state)?;

    let app_handle = app.clone();
    config.cancel = Some(cancel);
    // Two channels, two jobs: log lines are for the person reading the log
    // panel, progress events are for the bar. Nothing reads counts out of
    // the prose.
    let log_app = app_handle.clone();
    config.log = Some(LogSink::new(move |line: &str| {
        events::emit(&log_app, events::LOG, line.to_string());
    }));
    let progress_app = app_handle.clone();
    config.progress = Some(ProgressSink::new(move |event| {
        events::emit(
            &progress_app,
            events::PROGRESS,
            ExtractProgressEvent::from(event),
        );
    }));

    spawn_job(app, move || {
        let result = run_exporter(&config);

        match result {
            Ok(run_result) => {
                let summary = last_log_line_or(&run_result.messages, "Export complete.");
                for line in run_result.messages {
                    events::emit(&app_handle, events::LOG, line);
                }
                match count_jsonl_output(Path::new(&output_dir)) {
                    Ok(counts) => {
                        let payload = serde_json::json!({
                            "summary": summary,
                            "files_parsed": counts.files,
                            "messages_parsed": counts.messages,
                        });
                        events::emit(&app_handle, events::FINISHED, payload.to_string());
                    }
                    Err(err) => {
                        events::emit(&app_handle,
                            events::ERROR,
                            ExtractErrorEvent {
                                detail: format!(
                                    "count extracted JSON Lines records in {output_dir}: {err:#}"
                                ),
                                user_message: Some(
                                    "Extraction completed, but the generated message count could not be verified."
                                        .into(),
                                ),
                            },
                        );
                    }
                }
            }
            Err(err) => return Err(err),
        }
        Ok(())
    });

    Ok(())
}

/// Form fields from the Extract screen after defaults are filled in.
struct ExtractOptions {
    backup_password: String,
    attachment_media: AttachmentMedia,
    media_max_resolution: MaxResolution,
    media_max_fps: String,
    media_min_size: String,
    obfuscate: bool,
    owner_phones: Vec<String>,
    owner_emails: Vec<String>,
    attachment_root: String,
    apple_contacts: String,
    whatsapp_key: String,
    whatsapp_wa: String,
    whatsapp_media: String,
    whatsapp_db: String,
    whatsapp_business: bool,
}

/// Parse the attachment handling choice from the Extract form.
///
/// The UI says "copy" and "skip". The exporter config uses "clone" and
/// "disabled" for those same choices.
///
/// # Errors
///
/// Returns an error if the string is not copy, convert, compress, or skip.
pub(crate) fn parse_attachment_media(raw: Option<&str>) -> Result<AttachmentMedia, String> {
    let Some(raw) = raw.and_then(message_ir::trimmed) else {
        return Ok(AttachmentMedia::default());
    };
    let lowered = raw.to_ascii_lowercase();
    let key = match lowered.as_str() {
        "copy" => "clone",
        "skip" => "disabled",
        other => other,
    };
    AttachmentMedia::parse(key).ok_or_else(|| {
        format!("invalid attachment_media '{raw}' (expected copy, convert, compress, or skip)")
    })
}

/// Parse the max video/image size from the Extract form.
///
/// # Errors
///
/// Returns an error if the string is not 720p, 1080p, or 4k.
pub(crate) fn parse_max_resolution(raw: Option<&str>) -> Result<MaxResolution, String> {
    let Some(raw) = raw.and_then(message_ir::trimmed) else {
        return Ok(MaxResolution::default());
    };
    MaxResolution::parse(raw).ok_or_else(|| {
        format!("invalid media_max_resolution '{raw}' (expected 720p, 1080p, or 4k)")
    })
}

/// `AttachmentMedia` the exporter's `Form` is asked for.
///
/// Convert and Compress become Clone: the desktop stages originals, shows the
/// first gate, and runs the media pass itself, so the expensive work happens
/// after the user has approved it rather than before. Copy and Skip have no
/// media step and reach the exporter unchanged. Kept in `AttachmentMedia`'s
/// own domain because `Form::attachment_media` drives the exporter's media
/// mode, the upfront ffmpeg-availability check, and Apple `copy_method` —
/// none of which must see Convert or Compress, or the exporter would demand
/// ffmpeg (and stage a converted file) before the user has approved anything.
fn exporter_attachment_media(chosen: AttachmentMedia) -> AttachmentMedia {
    match chosen {
        AttachmentMedia::Convert | AttachmentMedia::Compress => AttachmentMedia::Clone,
        other => other,
    }
}

/// Build the `CompressOptions` a media pass will use, from the same
/// max-resolution/fps/min-size fields the Extract form parses.
///
/// `CompressOptions` only takes effect under [`media::MediaMode::Compress`],
/// so the real options are built only when `Compress` was chosen and
/// `CompressOptions::default()` is returned otherwise. Shared so the
/// desktop's own media pass (`commands::staging`) parses these fields the
/// same way Extract does, rather than re-deriving the parsing.
///
/// # Errors
///
/// Returns an error if `max_fps` is not a number or `min_size` cannot be
/// parsed as a byte size.
pub(crate) fn parse_compress_options(
    chosen: AttachmentMedia,
    max_resolution: MaxResolution,
    max_fps: &str,
    min_size: &str,
) -> Result<CompressOptions, String> {
    if !matches!(chosen, AttachmentMedia::Compress) {
        return Ok(CompressOptions::default());
    }
    let fps = max_fps
        .parse::<f32>()
        .map_err(|_| format!("invalid media_max_fps '{max_fps}'"))?;
    media::compress_options_from_cli(max_resolution, fps, min_size, true).map_err(|e| e.to_string())
}

/// Build the exporter config the background thread will run.
///
/// Every source maps its UI key to an [`Exporter`] variant, fills the shared
/// [`Form`], and goes through `Form::to_config` — so the Form builders in
/// io-core are the single source of truth for field mapping and validation.
/// Every path writes JSON Lines (one JSON object per line).
///
/// # Errors
///
/// Returns an error if the source is unknown, compress options are invalid,
/// or `Form::to_config` rejects the form (missing input path, missing owner
/// phones, bad date, …). Multiple validation problems are joined with `; `.
fn build_exporter_config(
    source: &str,
    path: &str,
    output_dir: &str,
    options: &ExtractOptions,
) -> Result<ExporterConfig, String> {
    // `Form`'s own compress validation only fires when `Form.attachment_media`
    // is `Compress` — and that field reads `Clone` for a real Convert/Compress
    // choice (see `exporter_attachment_media`'s docs), so it would otherwise
    // stay silent about a malformed `media_max_fps`/`media_min_size` until the
    // desktop's own media pass parses the same fields again at the approval
    // gate, hours later. Validate against the REAL chosen mode here so a bad
    // value still fails immediately; the parsed value itself is unused here —
    // the exporter's own media step is a no-op under Clone.
    parse_compress_options(
        options.attachment_media,
        options.media_max_resolution,
        &options.media_max_fps,
        &options.media_min_size,
    )?;

    let mut form = Form {
        output: output_dir.to_string(),
        // See `exporter_attachment_media`'s docs: the exporter is asked for
        // Clone whenever the user chose Convert or Compress, so it stages
        // originals and the desktop runs the media pass itself, after the
        // gate.
        attachment_media: exporter_attachment_media(options.attachment_media),
        media_max_resolution: options.media_max_resolution,
        media_max_fps: options.media_max_fps.clone(),
        media_min_size: options.media_min_size.clone(),
        obfuscate: options.obfuscate,
        // Import and Push read conversation files as JSON Lines (one JSON
        // object per line).
        output_format: OutputFormat::Jsonl,
        ..Default::default()
    };

    let exporter = match source {
        "imessage-ios" => {
            form.db_path = path.to_string();
            form.apple_platform = ApplePlatform::Ios;
            form.backup_password = options.backup_password.clone();
            Exporter::Imessage
        }
        "imessage-macos" | "imessage-jailbreak" => {
            form.db_path = path.to_string();
            form.apple_platform = ApplePlatform::MacOs;
            form.attachment_root = options.attachment_root.clone();
            form.apple_contacts = options.apple_contacts.clone();
            // The Extract screen only offers obfuscation for iOS backups.
            form.obfuscate = false;
            Exporter::Imessage
        }
        "sms-backup-restore" => {
            form.input = path.to_string();
            form.owner_phones = options.owner_phones.join("\n");
            Exporter::SmsBackupRestore
        }
        "go-sms-pro" => {
            form.input = path.to_string();
            form.owner_phones = options.owner_phones.join("\n");
            Exporter::GoSmsPro
        }
        "sms-backup-plus" => {
            form.input = path.to_string();
            form.owner_phones = options.owner_phones.join("\n");
            form.owner_emails = options.owner_emails.join("\n");
            Exporter::SmsBackupPlus
        }
        "openextract" => {
            form.input = path.to_string();
            Exporter::OpenExtract
        }
        "imazing" => {
            form.input = path.to_string();
            Exporter::Imazing
        }
        "whatsapp-android" => {
            form.input = path.to_string();
            form.whatsapp_platform = WhatsappPlatform::Android;
            form.whatsapp_key = options.whatsapp_key.clone();
            form.whatsapp_wa = options.whatsapp_wa.clone();
            form.whatsapp_media = options.whatsapp_media.clone();
            form.whatsapp_db = options.whatsapp_db.clone();
            Exporter::Whatsapp
        }
        "whatsapp-ios" => {
            form.input = path.to_string();
            form.whatsapp_platform = WhatsappPlatform::Ios;
            form.whatsapp_backup = path.to_string();
            form.whatsapp_wa = options.whatsapp_wa.clone();
            form.whatsapp_business = options.whatsapp_business;
            Exporter::Whatsapp
        }
        _ => return Err(format!("unsupported source '{source}'")),
    };

    form.to_config(exporter).map_err(|errors| errors.join("; "))
}

/// Call the exporter that matches `config.source`.
///
/// # Errors
///
/// Returns an error if the exporter fails, or if the source is format
/// conversion (that job uses the `format` command instead).
fn run_exporter(config: &ExporterConfig) -> anyhow::Result<message_vault_io_core::RunResult> {
    match &config.source {
        SourceConfig::GoSmsPro(_) => run_go_sms_pro(config),
        SourceConfig::SmsBackupRestore(_) => run_sms_restore(config),
        SourceConfig::SmsBackupPlus(_) => run_sms_plus(config),
        SourceConfig::OpenExtract(_) => run_openextract(config),
        SourceConfig::Imazing(_) => run_imazing(config),
        SourceConfig::Apple(_) => run_imessage(config),
        SourceConfig::Whatsapp(_) => run_whatsapp(config),
        SourceConfig::Format(_) => Err(anyhow::anyhow!("Format conversion not yet wired")),
    }
}

#[cfg(test)]
mod tests;
