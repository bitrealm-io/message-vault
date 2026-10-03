//! `extract` and `cancel` commands.
//!
//! `extract` starts the selected exporter on a background thread and returns
//! immediately. Progress is sent back as Tauri events:
//! `extract:log` (one human-readable log line), `extract:progress` (one
//! typed [`ExtractProgressEvent`], mapped from the exporter's
//! `ProgressEvent`), `extract:finished` (a summary string or JSON object),
//! and `extract:error` ([`ExtractErrorEvent`]).
//!
//! `cancel` sets the cancel flag of the job that is running (see
//! `commands::jobs`). The exporter checks it between steps through
//! `ExporterConfig.cancel`.

use std::path::Path;
use std::sync::{Arc, Mutex};

use media::{CompressOptions, MaxResolution};
use message_crate_core::{
    ApplePlatform, AttachmentMedia, Exporter, ExporterConfig, Form, LogSink, OutputFormat,
    ProgressSink, RunResult, SourceConfig, WhatsappPlatform,
};
use message_staging::TranscodeOptions;

// Short names so the match in `run_exporter` stays easy to read.
use go_sms_pro_exporter::run as run_go_sms_pro;
use imazing_exporter::run as run_imazing;
use imessage_ir_exporter::run as run_imessage;
use openextract_exporter::run as run_openextract;
use sms_backup_plus_exporter::run as run_sms_plus;
use sms_backup_restore_exporter::run as run_sms_restore;
use whatsapp_exporter::run as run_whatsapp;

use super::events;
use super::events::ExtractProgressEvent;
use super::jobs::{cancel_running_job, spawn_job, start_job};
use super::last_log_line_or;
use crate::staging_folders::StagingFolders;
use crate::state::AppState;

/// Ask this process to stop the job that is running. Does nothing when no
/// job runs.
///
/// Sets that job's cancel flag. The job checks the flag between steps and
/// exits on its own. There is no hard kill.
///
/// # Errors
///
/// Returns an error if another thread panicked while holding the shared
/// state lock.
#[tauri::command(async)]
pub fn cancel(state: tauri::State<'_, Arc<Mutex<AppState>>>) -> Result<(), String> {
    cancel_running_job(&state)
}

/// The `extract:finished` payload: the run's last log line as the summary,
/// and the conversation and message counts the exporter reported.
///
/// The counts come from the exporter rather than from reading the output
/// folder again, because that folder also holds the staged attachments, and
/// an attachment can be named `*.jsonl` and hold any bytes at all.
fn finished_payload(run_result: &RunResult) -> String {
    serde_json::json!({
        "summary": last_log_line_or(&run_result.messages, "Export complete."),
        "files_parsed": run_result.conversations,
        "messages_parsed": run_result.message_count,
    })
    .to_string()
}

/// User-facing parameters for the `extract` command (before defaults/parsing).
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractArgs {
    /// Backup source key, for example `imessage-ios` or `whatsapp-android`.
    pub source: String,
    /// Path to the phone backup (a folder, database file, or XML file).
    pub path: String,
    /// Staging folder `create_staging_dir` made, which the exporter writes
    /// conversation files into.
    pub output_dir: String,
    /// Password for encrypted backups, when the source needs one.
    pub backup_password: Option<String>,
    /// Attachment handling choice: `copy`, `convert`, `compress`, or `skip`.
    pub attachment_media: Option<String>,
    /// Long-edge cap for compressed video: `720p`, `1080p`, or `4k`.
    pub media_max_resolution: Option<String>,
    /// Frame-rate cap for compressed video, for example `30`.
    pub media_max_fps: Option<String>,
    /// Size below which a video is not compressed, for example `20M`.
    pub media_min_size: Option<String>,
    /// When true, replace names and phone numbers with fake ones.
    pub obfuscate: Option<bool>,
    /// Zone iMazing dates are read in: an IANA name (`America/New_York`) or a
    /// fixed offset (`UTC-05:00`). The screen sends the account's zone unless
    /// the person picked another in the advanced section.
    pub timezone: Option<String>,
    /// Owner phone numbers for the Android SMS sources (SMS Backup & Restore,
    /// GO SMS Pro, SMS Backup+) and both WhatsApp platforms.
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
    /// The server's attachment size limit, in bytes, as the app read it from
    /// `GET /v1/server` when the Import Run was created. Staging records it
    /// with the run's media settings, where the Staging Review's forecast
    /// and the Media stage read it.
    pub asset_max_bytes: u64,
}

/// Ask this process to parse a phone backup and write conversation files.
///
/// Returns as soon as the background thread starts. Log lines, progress, and
/// the final summary are sent as `extract:log`, `extract:progress`,
/// `extract:finished`, and `extract:error`. Output is JSON Lines (one JSON
/// object per line) so the Import screen's upload step can read it later.
///
/// This is where an Import Run's media settings are decided: the mode the
/// person chose and the compress fields are parsed here, before anything is
/// staged, and recorded in the staging folder once Staging finishes (see
/// [`run_staging`]). `summarize_staging` and `transcode_staging` read them
/// from there and are never given them again.
///
/// # Errors
///
/// Returns an error if `output_dir` is not a staging folder this app made, a
/// form field is invalid, the source is unknown, another job is running, or
/// another thread panicked while holding the shared state lock. Failures
/// during the export itself are sent as `extract:error`, not returned here.
#[tauri::command(async)]
pub fn extract(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    folders: tauri::State<'_, StagingFolders>,
    app: tauri::AppHandle,
    args: ExtractArgs,
) -> Result<(), String> {
    // The exporter cleans the folder it writes into, so it writes only into
    // one this app made for staging.
    let output_dir = folders.folder(&args.output_dir)?;
    let options = ExtractOptions {
        backup_password: args.backup_password.unwrap_or_default(),
        attachment_media: parse_attachment_media(args.attachment_media.as_deref())?,
        media_max_resolution: parse_max_resolution(args.media_max_resolution.as_deref())?,
        media_max_fps: args.media_max_fps.unwrap_or_else(|| "30".into()),
        media_min_size: args.media_min_size.unwrap_or_else(|| "20M".into()),
        obfuscate: args.obfuscate.unwrap_or(false),
        // `Form` trims and drops empty values itself, so the raw strings can
        // pass through unchanged.
        timezone: args.timezone.unwrap_or_default(),
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

    let media_settings = media_settings_for(&options, args.asset_max_bytes)?;
    let mut config = build_exporter_config(
        &args.source,
        &args.path,
        &output_dir.display().to_string(),
        &options,
    )?;
    config.resume = args.resume.unwrap_or(false);

    let job = start_job(&state, "an extract")?;

    let app_handle = app.clone();
    config.cancel = Some(job.cancel_flag());
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

    spawn_job(app, job, move || {
        let run_result = run_staging(&config, &output_dir, &media_settings)?;
        let payload = finished_payload(&run_result);
        for line in run_result.messages {
            events::emit(&app_handle, events::LOG, line);
        }
        // Before `extract:finished`, so the Import Run holds them when it
        // moves on.
        for issue in &run_result.issues {
            events::emit(
                &app_handle,
                events::ISSUE,
                events::ExtractIssueEvent::from(issue),
            );
        }
        Ok(payload)
    });

    Ok(())
}

/// Form fields from the Import screen after defaults are filled in.
struct ExtractOptions {
    backup_password: String,
    attachment_media: AttachmentMedia,
    media_max_resolution: MaxResolution,
    media_max_fps: String,
    media_min_size: String,
    obfuscate: bool,
    timezone: String,
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

/// Parse the attachment handling choice from the Import form.
///
/// The UI says "copy" and "skip". The exporter config uses "clone" and
/// "disabled" for those same choices.
///
/// # Errors
///
/// Returns an error if the string is not copy, convert, compress, or skip.
fn parse_attachment_media(raw: Option<&str>) -> Result<AttachmentMedia, String> {
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

/// Parse the compressed-video resolution cap from the Import form.
///
/// # Errors
///
/// Returns an error if the string is not 720p, 1080p, or 4k.
fn parse_max_resolution(raw: Option<&str>) -> Result<MaxResolution, String> {
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

/// Build the `CompressOptions` the Media stage will use, from the
/// max-resolution/fps/min-size fields the Import form sends.
///
/// `CompressOptions` only takes effect under [`media::MediaMode::Compress`],
/// so the real options are built only when `Compress` was chosen and
/// `CompressOptions::default()` is returned otherwise. The errors name the
/// form's fields, because the Import Run shows them to the person as they
/// are.
///
/// # Errors
///
/// Returns an error if `max_fps` is not a positive number or `min_size`
/// cannot be parsed as a byte size.
fn parse_compress_options(
    chosen: AttachmentMedia,
    max_resolution: MaxResolution,
    max_fps: &str,
    min_size: &str,
) -> Result<CompressOptions, String> {
    if !matches!(chosen, AttachmentMedia::Compress) {
        return Ok(CompressOptions::default());
    }
    let max_fps = max_fps.trim();
    let fps = max_fps
        .parse::<f32>()
        .ok()
        .filter(|fps| fps.is_finite() && *fps > 0.0)
        .ok_or_else(|| {
            if max_fps.is_empty() {
                "Max FPS is empty. It must be a number of frames per second, such as 30."
                    .to_string()
            } else {
                format!(
                    "Max FPS must be a number of frames per second above 0, such as 30, not '{max_fps}'."
                )
            }
        })?;
    media::compress_options_from_form(max_resolution, fps, min_size, true)
        .map_err(|e| format!("Minimum Video File Size is not a size: {e:#}"))
}

/// The media settings an Import Run works to, decided once when its Staging
/// starts: the mode the person chose, the compress options parsed from the
/// form's fields, and the server's attachment size limit.
///
/// A bad compress field fails here, before anything is staged, rather than
/// at the summary after hours of Staging.
///
/// # Errors
///
/// Returns an error if a compress field is invalid (see
/// [`parse_compress_options`]).
fn media_settings_for(
    options: &ExtractOptions,
    asset_max_bytes: u64,
) -> Result<TranscodeOptions, String> {
    Ok(TranscodeOptions {
        mode: options.attachment_media.media_mode(),
        compress: parse_compress_options(
            options.attachment_media,
            options.media_max_resolution,
            &options.media_max_fps,
            &options.media_min_size,
        )?,
        asset_max_bytes,
    })
}

/// Run the exporter into `output_dir`, then record the run's media settings
/// beside the export sentinel it wrote.
///
/// The record is written after the exporter and not before, because a fresh
/// export refuses a folder that already holds files it does not recognise.
/// A Staging that is interrupted leaves no record. Nothing reads one until
/// Staging finishes, and the resumed Staging writes it.
///
/// # Errors
///
/// Returns an error if the exporter fails or the record cannot be written.
fn run_staging(
    config: &ExporterConfig,
    output_dir: &Path,
    media_settings: &TranscodeOptions,
) -> anyhow::Result<RunResult> {
    let run_result = run_exporter(config)?;
    message_staging::write_media_settings(output_dir, media_settings)?;
    Ok(run_result)
}

/// Build the exporter config the background thread will run.
///
/// Every source maps its UI key to an [`Exporter`] variant, fills the shared
/// [`Form`], and goes through `Form::to_config` — so the Form builders in
/// message-crate-core are the single source of truth for field mapping and validation.
/// Every path writes JSON Lines (one JSON object per line).
///
/// # Errors
///
/// Returns an error if the source is unknown or `Form::to_config` rejects
/// the form (missing input path, missing owner phones, …). Multiple
/// validation problems are joined with `; `. The compress fields are not
/// checked here: `Form` sees Clone for a real Compress choice, and
/// [`media_settings_for`] checks them against the real one.
fn build_exporter_config(
    source: &str,
    path: &str,
    output_dir: &str,
    options: &ExtractOptions,
) -> Result<ExporterConfig, String> {
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
            form.backup_password.clone_from(&options.backup_password);
            Exporter::Imessage
        }
        "imessage-macos" | "imessage-jailbreak" => {
            form.db_path = path.to_string();
            form.apple_platform = ApplePlatform::MacOs;
            form.attachment_root.clone_from(&options.attachment_root);
            form.apple_contacts.clone_from(&options.apple_contacts);
            // The Import screen offers obfuscation for iPhone backups and the
            // Android SMS sources, not for Mac Messages or a jailbreak copy.
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
            // iMazing dates carry no zone, so the exporter reads them in this
            // one. Without it the exporter falls back to the machine's zone,
            // and the same folder gives different instants on different
            // machines.
            form.timezone.clone_from(&options.timezone);
            Exporter::Imazing
        }
        // Both WhatsApp platforms take the holder's number as an owner phone:
        // Android's only source, iPhone's fallback when the backup's
        // preferences carry no owner key.
        "whatsapp-android" => {
            form.input = path.to_string();
            form.owner_phones = options.owner_phones.join("\n");
            form.whatsapp_platform = WhatsappPlatform::Android;
            form.whatsapp_key.clone_from(&options.whatsapp_key);
            form.whatsapp_wa.clone_from(&options.whatsapp_wa);
            form.whatsapp_media.clone_from(&options.whatsapp_media);
            form.whatsapp_db.clone_from(&options.whatsapp_db);
            Exporter::Whatsapp
        }
        "whatsapp-ios" => {
            form.input = path.to_string();
            form.owner_phones = options.owner_phones.join("\n");
            form.whatsapp_platform = WhatsappPlatform::Ios;
            form.whatsapp_backup = path.to_string();
            form.backup_password.clone_from(&options.backup_password);
            form.whatsapp_wa.clone_from(&options.whatsapp_wa);
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
fn run_exporter(config: &ExporterConfig) -> anyhow::Result<RunResult> {
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
