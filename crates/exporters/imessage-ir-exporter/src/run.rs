//! Library entry: [`ExporterConfig`] to the shared conversation structure, then
//! the chosen output format.
//!
//! Everything a person can get wrong about the source is checked here, in
//! this process, before the `imessage-reader` program is started: the
//! sentences below are the ones the Import screen shows. What only the
//! database can tell (a wrong password, a backup with no Messages in it)
//! comes back from the program as its own error sentence.

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use imessage_reader_protocol::{ExportRequest, Platform, Request, Source};
use message_ir_format::ExportTransforms;
use message_vault_io_core::{
    AppleConfig, ApplePlatform, CancelFlag, ExporterConfig, LogSink, OutputFormat, ProgressEvent,
    ProgressSink, RunResult, SourceConfig, emit_progress,
};

use crate::{backup::ios_backup_encrypted_flag, convert, helper::Helper};

/// User-facing copy when a custom attachment folder is missing.
pub(crate) const ATTACHMENT_FOLDER_MISSING: &str = "Attachment folder does not exist.";
/// User-facing copy when a supplied Apple Contacts file is missing.
pub(crate) const APPLE_CONTACTS_MISSING: &str = "Apple Contacts file does not exist.";
/// User-facing copy when the macOS Messages database file is missing.
pub(crate) const MESSAGES_DATABASE_MISSING: &str = "Messages database does not exist.";
/// User-facing copy when the folder is not an iPhone backup (or Messages is missing).
pub(crate) const NOT_AN_IPHONE_BACKUP: &str =
    "This folder is not an iPhone backup, or Messages is missing from it.";

/// Where an iPhone backup keeps the Messages database: the SHA-1 of its
/// domain and path, under a folder named by its first two characters.
const MESSAGES_DB_IN_IOS_BACKUP: &str = "3d/3d0d7e5fb2ce288813306e4d4636395e047a3d28";

/// Where the Messages database lives on a Mac.
fn default_macos_db_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join("Library/Messages/chat.db")
}

/// Whether to resolve attachment bytes for embedding (`.eml` / `.mbox`) or
/// persisting under `attachments/` (CSV / JSON).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttachmentEmbed {
    /// Resolve and embed media bytes (macOS path or iOS decrypt).
    Embed,
    /// Skip media bytes (empty attachment parts still possible via other fields).
    Disabled,
}

/// Map `AppleConfig.copy_method` to attachment handling.
///
/// `clone` copies files, `basic` embeds thumbnails, and `full` embeds
/// originals — all three resolve bytes through the same embed path in this
/// exporter. `disabled` skips media bytes entirely.
fn attachment_embed_from_copy_method(copy_method: &str) -> Result<AttachmentEmbed> {
    match copy_method.trim().to_ascii_lowercase().as_str() {
        "disabled" => Ok(AttachmentEmbed::Disabled),
        "clone" | "basic" | "full" => Ok(AttachmentEmbed::Embed),
        other => bail!(
            "{other} is not a valid attachment mode! Must be one of <clone, basic, full, disabled>"
        ),
    }
}

/// Everything one export run needs, checked and ready to send.
#[derive(Debug)]
pub(crate) struct ExportOptions {
    /// The request for the `imessage-reader` program, minus the scratch
    /// folder, which is created when the run starts.
    pub request: ExportRequest,
    pub export_path: PathBuf,
    pub attachment_embed: AttachmentEmbed,
    /// Media / obfuscate transforms applied by [`message_ir_format::FormatSink`].
    pub transforms: ExportTransforms,
    /// CSV, EML, MBOX, JSON, or JSON Lines (one JSON object per line).
    pub output_format: OutputFormat,
    /// Human-readable mid-run notes and warnings (desktop sink or stderr).
    pub log: Option<LogSink>,
    /// Typed progress events for the desktop's progress bar.
    pub progress: Option<ProgressSink>,
    /// Cooperative cancel flag, checked between events and before every write.
    pub cancel: Option<CancelFlag>,
    /// Continue an interrupted export: keep previous output and skip the
    /// conversations already written.
    pub resume: bool,
}

impl ExportOptions {
    /// Write one log line when a log sink is configured.
    pub fn emit_log(&self, line: impl AsRef<str>) {
        message_vault_io_core::emit_log(self.log.as_ref(), line);
    }

    /// Send one typed progress event when a progress sink is configured.
    pub fn emit_progress(&self, event: ProgressEvent) {
        emit_progress(self.progress.as_ref(), event);
    }

    /// The shared cancel check.
    pub fn check_cancel(&self) -> Result<()> {
        message_vault_io_core::check_cancel(self.cancel.as_ref()).map_err(|e| anyhow!(e))
    }
}

/// Build options from [`ExporterConfig`], start the `imessage-reader`
/// program on the Messages database, and write the export.
///
/// # Errors
///
/// Returns an error when the source is not Apple Messages, the program
/// cannot be found or the database cannot be opened, conversion fails, media
/// processing fails for every candidate file, or the user cancels.
pub fn run(config: &ExporterConfig) -> Result<RunResult> {
    let mut options = options_from_export_config(config)?;
    options.check_cancel()?;
    let format = options.output_format;

    // The program writes decrypted files here and this run deletes the
    // folder when it ends, whichever way it ends.
    let scratch = tempfile::Builder::new()
        .prefix("imessage-reader-")
        .tempdir()?;
    options.request.scratch_dir = Some(scratch.path().to_path_buf());

    let mut helper = Helper::spawn(
        &Request::Export(options.request.clone()),
        options.log.clone(),
        options.progress.clone(),
    )?;
    let sink = convert::export(&mut helper, &options)?;
    helper.finish()?;
    drop(scratch);
    options.check_cancel()?;

    if !sink.media.errors.is_empty() && sink.media.processed == 0 && config.media.mode.needs_tools()
    {
        bail!("media processing failed for all candidate files");
    }

    let mut messages = sink.log_lines();
    messages.push(format!(
        "Wrote {} export under {}",
        format.as_str(),
        config.output.display()
    ));
    Ok(RunResult { messages })
}

/// Translate the shared exporter config into this exporter's options, rejecting non-Apple sources.
fn options_from_export_config(config: &ExporterConfig) -> Result<ExportOptions> {
    let SourceConfig::Apple(source) = &config.source else {
        bail!("imessage-ir-exporter requires SourceConfig::Apple");
    };

    let db_path = match config.primary_input() {
        Some(path) if !path.as_os_str().is_empty() => path.to_path_buf(),
        _ => default_macos_db_path(),
    };
    let platform = platform_for(source, &db_path)?;

    if source.backup_password.is_some() && platform != Platform::Ios {
        bail!("backup password is enabled; it can only be used with iOS backups.");
    }
    check_macos_only_path(
        config,
        platform,
        source.attachment_root.as_deref().map(Path::new),
        ATTACHMENT_FOLDER_MISSING,
        "Option attachment-root is enabled, but the platform is iOS, so the root will have no effect!",
    )?;
    check_macos_only_path(
        config,
        platform,
        source.apple_contacts.as_deref(),
        APPLE_CONTACTS_MISSING,
        "Option contacts path is enabled, but the platform is iOS, so the path will have no effect!",
    )?;
    check_db_path(platform, &db_path)?;

    let attachment_embed = attachment_embed_from_copy_method(&source.copy_method)?;

    // Create the output directory; prior IR artifacts are removed in `convert`
    // via ExportWriter::open.
    std::fs::create_dir_all(&config.output)?;

    Ok(ExportOptions {
        request: ExportRequest {
            source: Source {
                db_path,
                platform,
                backup_password: source.backup_password.clone(),
            },
            attachment_root: source.attachment_root.clone(),
            contacts_path: source.apple_contacts.clone(),
            use_caller_id: source.use_caller_id,
            scratch_dir: None,
        },
        export_path: config.output.clone(),
        attachment_embed,
        transforms: ExportTransforms::from_config(config),
        output_format: config.output_format,
        log: config.log.clone(),
        progress: config.progress.clone(),
        cancel: config.cancel.clone(),
        resume: config.resume,
    })
}

/// The platform the source names, or the one the backup's layout shows.
fn platform_for(source: &AppleConfig, db_path: &Path) -> Result<Platform> {
    match source.platform {
        Some(ApplePlatform::MacOs) => Ok(Platform::MacOs),
        Some(ApplePlatform::Ios) => Ok(Platform::Ios),
        Some(ApplePlatform::Auto) | None => detect_platform(db_path),
    }
}

/// Tell a backup folder from a database file by layout: a folder holding the
/// Messages database at its hashed path is an iPhone backup, a file is a
/// Mac `chat.db`. Anything else is treated as a Mac path so the missing
/// database is what gets reported.
fn detect_platform(db_path: &Path) -> Result<Platform> {
    if db_path.ends_with(MESSAGES_DB_IN_IOS_BACKUP) {
        bail!(
            "{} is the Messages database inside an iPhone backup; choose the backup folder itself.",
            db_path.display()
        );
    }
    if db_path.join(MESSAGES_DB_IN_IOS_BACKUP).exists() {
        return Ok(Platform::Ios);
    }
    Ok(Platform::MacOs)
}

/// A path option that only a macOS export reads: refused when it points
/// nowhere, and noted in the log as having no effect on an iOS backup.
fn check_macos_only_path(
    config: &ExporterConfig,
    platform: Platform,
    path: Option<&Path>,
    missing: &str,
    ignored_on_ios: &str,
) -> Result<()> {
    let Some(path) = path else {
        return Ok(());
    };
    if !path.exists() {
        bail!("{missing}");
    }
    if platform == Platform::Ios {
        config.emit_log(ignored_on_ios);
    }
    Ok(())
}

/// The backup must be laid out as the platform expects: a messages
/// database file on macOS; on iOS a backup folder with its manifest and,
/// when the backup is not encrypted, the database at its hashed path.
fn check_db_path(platform: Platform, db_path: &Path) -> Result<()> {
    match platform {
        Platform::MacOs => {
            if !db_path.is_file() {
                bail!("{MESSAGES_DATABASE_MISSING}");
            }
        }
        Platform::Ios => {
            let manifest = db_path.join("Manifest.plist");
            if !db_path.is_dir() || !manifest.is_file() {
                bail!("{NOT_AN_IPHONE_BACKUP}");
            }
            if ios_backup_encrypted_flag(db_path) == Some(false)
                && !db_path.join(MESSAGES_DB_IN_IOS_BACKUP).is_file()
            {
                bail!("{NOT_AN_IPHONE_BACKUP}");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use message_vault_io_core::{AppleConfig, MediaConfig, OutputFormat};
    use std::{fs, path::Path};

    fn apple_cfg(input: &Path, apple: AppleConfig) -> ExporterConfig {
        ExporterConfig {
            inputs: vec![input.to_path_buf()],
            output: input.with_extension("export_out"),
            time_zone: None,
            obfuscate: Default::default(),
            media: MediaConfig::default(),
            cancel: None,
            log: None,
            progress: None,
            output_format: OutputFormat::Jsonl,
            resume: false,
            source: SourceConfig::Apple(apple),
        }
    }

    #[test]
    fn missing_chat_db_uses_locked_copy() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("chat.db");
        let err = options_from_export_config(&apple_cfg(
            &missing,
            AppleConfig {
                platform: Some(ApplePlatform::MacOs),
                ..AppleConfig::default()
            },
        ))
        .unwrap_err();
        assert_eq!(err.to_string(), MESSAGES_DATABASE_MISSING);
    }

    #[test]
    fn missing_attachment_folder_uses_locked_copy() {
        let dir = tempfile::tempdir().unwrap();
        let chat = dir.path().join("chat.db");
        fs::write(&chat, b"sqlite").unwrap();
        let err = options_from_export_config(&apple_cfg(
            &chat,
            AppleConfig {
                platform: Some(ApplePlatform::MacOs),
                attachment_root: Some(dir.path().join("no-such-root").display().to_string()),
                ..AppleConfig::default()
            },
        ))
        .unwrap_err();
        assert_eq!(err.to_string(), ATTACHMENT_FOLDER_MISSING);
    }

    #[test]
    fn missing_apple_contacts_uses_locked_copy() {
        let dir = tempfile::tempdir().unwrap();
        let chat = dir.path().join("chat.db");
        fs::write(&chat, b"sqlite").unwrap();
        let err = options_from_export_config(&apple_cfg(
            &chat,
            AppleConfig {
                platform: Some(ApplePlatform::MacOs),
                apple_contacts: Some(dir.path().join("no-such.abcddb")),
                ..AppleConfig::default()
            },
        ))
        .unwrap_err();
        assert_eq!(err.to_string(), APPLE_CONTACTS_MISSING);
    }

    #[test]
    fn empty_folder_is_not_an_iphone_backup() {
        let dir = tempfile::tempdir().unwrap();
        let err = options_from_export_config(&apple_cfg(
            dir.path(),
            AppleConfig {
                platform: Some(ApplePlatform::Ios),
                ..AppleConfig::default()
            },
        ))
        .unwrap_err();
        assert_eq!(err.to_string(), NOT_AN_IPHONE_BACKUP);
    }

    #[test]
    fn unencrypted_backup_missing_messages_uses_locked_copy() {
        let dir = tempfile::tempdir().unwrap();
        // Manifest.plist present, IsEncrypted false, hashed sms.db missing.
        fs::write(
            dir.path().join("Manifest.plist"),
            br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>IsEncrypted</key><false/></dict></plist>"#,
        )
        .unwrap();
        let err = options_from_export_config(&apple_cfg(
            dir.path(),
            AppleConfig {
                platform: Some(ApplePlatform::Ios),
                ..AppleConfig::default()
            },
        ))
        .unwrap_err();
        assert_eq!(err.to_string(), NOT_AN_IPHONE_BACKUP);
    }

    #[test]
    fn auto_detects_a_backup_folder_by_its_hashed_database() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(detect_platform(dir.path()).unwrap(), Platform::MacOs);

        let hashed = dir.path().join(MESSAGES_DB_IN_IOS_BACKUP);
        fs::create_dir_all(hashed.parent().unwrap()).unwrap();
        fs::write(&hashed, b"sqlite").unwrap();
        assert_eq!(detect_platform(dir.path()).unwrap(), Platform::Ios);

        let err = detect_platform(&hashed).unwrap_err();
        assert!(
            err.to_string().contains("choose the backup folder"),
            "{err}"
        );
    }

    #[test]
    fn options_carry_the_request_the_program_receives() {
        let dir = tempfile::tempdir().unwrap();
        let chat = dir.path().join("chat.db");
        fs::write(&chat, b"sqlite").unwrap();
        let options = options_from_export_config(&apple_cfg(
            &chat,
            AppleConfig {
                platform: None,
                copy_method: "disabled".into(),
                use_caller_id: false,
                ..AppleConfig::default()
            },
        ))
        .unwrap();
        assert_eq!(options.request.source.platform, Platform::MacOs);
        assert_eq!(options.request.source.db_path, chat);
        assert!(!options.request.use_caller_id);
        assert!(options.request.scratch_dir.is_none());
        assert_eq!(options.attachment_embed, AttachmentEmbed::Disabled);
        assert!(options.export_path.ends_with("chat.export_out"));
    }
}
