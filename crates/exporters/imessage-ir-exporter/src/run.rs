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
use message_vault_io_core::{
    AppleConfig, ApplePlatform, CancelFlag, ExportTransforms, ExporterConfig, LogSink,
    OutputFormat, ProgressEvent, ProgressSink, RunResult, SourceConfig, emit_progress,
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
        other => bail!("unknown attachment mode {other:?}; use clone, basic, full, or disabled"),
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
    run_with(config, Helper::spawn)
}

/// [`run`], starting the program with `spawn`. Tests pass a fake program.
fn run_with(
    config: &ExporterConfig,
    spawn: impl FnOnce(&Request, Option<LogSink>, Option<ProgressSink>) -> Result<Helper>,
) -> Result<RunResult> {
    let mut options = options_from_export_config(config)?;
    options.check_cancel()?;

    // The program writes decrypted files here and this run deletes the
    // folder when it ends, whichever way it ends.
    let scratch = tempfile::Builder::new()
        .prefix("imessage-reader-")
        .tempdir()?;
    options.request.scratch_dir = Some(scratch.path().to_path_buf());

    let mut helper = spawn(
        &Request::Export(options.request.clone()),
        options.log.clone(),
        options.progress.clone(),
    )?;
    let report = convert::export(&mut helper, &options)?;
    helper.finish()?;
    drop(scratch);
    options.check_cancel()?;

    message_vault_io_core::finish_run(config, &report, config.media.mode.needs_tools())
}

/// Translate the shared exporter config into this exporter's options, rejecting non-Apple sources.
fn options_from_export_config(config: &ExporterConfig) -> Result<ExportOptions> {
    let SourceConfig::Apple(source) = &config.source else {
        bail!("imessage-ir-exporter requires SourceConfig::Apple");
    };

    let db_path = db_path_for(config.primary_input());
    let platform = platform_for(source, &db_path)?;

    if source.backup_password.is_some() && platform != Platform::Ios {
        bail!("backup password is enabled; it can only be used with iOS backups.");
    }
    check_macos_only_path(
        config,
        platform,
        source.attachment_root.as_deref().map(Path::new),
        ATTACHMENT_FOLDER_MISSING,
        "An attachment folder was given, but iPhone backups keep attachments inside the backup, so it will be ignored.",
    )?;
    check_macos_only_path(
        config,
        platform,
        source.apple_contacts.as_deref(),
        APPLE_CONTACTS_MISSING,
        "An Apple Contacts file was given, but names for an iPhone backup come from the backup itself, so it will be ignored.",
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

/// The input the person chose, or this Mac's own Messages database when
/// they left it empty.
fn db_path_for(input: Option<&Path>) -> PathBuf {
    match input {
        Some(path) if !path.as_os_str().is_empty() => path.to_path_buf(),
        _ => default_macos_db_path(),
    }
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
            timezone: None,
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

    /// A backup password only unlocks an iPhone backup, so a Mac `chat.db`
    /// with one is refused rather than exported with the password ignored.
    #[test]
    fn a_backup_password_is_refused_for_a_mac_chat_db() {
        let dir = tempfile::tempdir().unwrap();
        let chat = dir.path().join("chat.db");
        fs::write(&chat, b"sqlite").unwrap();
        let err = options_from_export_config(&apple_cfg(
            &chat,
            AppleConfig {
                platform: Some(ApplePlatform::MacOs),
                backup_password: Some("secret".into()),
                ..AppleConfig::default()
            },
        ))
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("it can only be used with iOS backups"),
            "{err}"
        );
    }

    /// An input left empty means this Mac's own Messages database.
    #[test]
    fn an_empty_input_is_the_macs_own_database() {
        let chat = Path::new("/backups/chat.db");
        assert_eq!(db_path_for(Some(chat)), chat);
        assert_eq!(db_path_for(Some(Path::new(""))), default_macos_db_path());
        assert_eq!(db_path_for(None), default_macos_db_path());
        assert!(default_macos_db_path().ends_with("Library/Messages/chat.db"));
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

    #[cfg(unix)]
    #[test]
    fn rows_the_program_skipped_are_counted_in_the_run_result() {
        use crate::helper::tests::{fake_helper, source_line, spawn_fake};
        use imessage_reader_protocol::PROTOCOL_VERSION;

        let dir = tempfile::tempdir().unwrap();
        let chat = dir.path().join("chat.db");
        fs::write(&chat, b"sqlite").unwrap();
        let body = format!(
            "{}\necho '{{\"event\":\"export_done\",\"messages_seen\":5,\"failures\":2}}'",
            source_line(PROTOCOL_VERSION)
        );
        let program = fake_helper(dir.path(), &body);
        let config = apple_cfg(
            &chat,
            AppleConfig {
                platform: Some(ApplePlatform::MacOs),
                ..AppleConfig::default()
            },
        );

        let result = run_with(&config, |request, _, _| Ok(spawn_fake(&program, request))).unwrap();
        assert!(
            result
                .messages
                .iter()
                .any(|line| line == &format!("  {}: 2", convert::SKIPPED_UNREADABLE_MESSAGE)),
            "{:#?}",
            result.messages
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_run_with_no_skipped_rows_says_nothing_about_them() {
        use crate::helper::tests::{fake_helper, source_line, spawn_fake};
        use imessage_reader_protocol::PROTOCOL_VERSION;

        let dir = tempfile::tempdir().unwrap();
        let chat = dir.path().join("chat.db");
        fs::write(&chat, b"sqlite").unwrap();
        let body = format!(
            "{}\necho '{{\"event\":\"export_done\",\"messages_seen\":5,\"failures\":0}}'",
            source_line(PROTOCOL_VERSION)
        );
        let program = fake_helper(dir.path(), &body);
        let config = apple_cfg(
            &chat,
            AppleConfig {
                platform: Some(ApplePlatform::MacOs),
                ..AppleConfig::default()
            },
        );

        let result = run_with(&config, |request, _, _| Ok(spawn_fake(&program, request))).unwrap();
        assert!(
            !result
                .messages
                .iter()
                .any(|line| line.contains(convert::SKIPPED_UNREADABLE_MESSAGE)),
            "{:#?}",
            result.messages
        );
    }

    /// A handwriting message's SVG comes from the program as text, not a
    /// file. A JSON export writes it under `attachments/`, and an attachment
    /// with no file says it is missing.
    #[cfg(unix)]
    #[test]
    fn inline_and_missing_attachments_reach_a_file_backed_export() {
        use crate::helper::tests::{fake_helper, source_line, spawn_fake};
        use imessage_reader_protocol::{
            Attachment, AttachmentSource, Conversation, Event, Message, PROTOCOL_VERSION,
        };
        use message_ir_format::read_conversation_json;

        const SVG: &str = "<svg></svg>";
        let attachment = |source| Attachment {
            original_name: None,
            mime_type: Some("image/svg+xml".into()),
            is_sticker: false,
            transcription: None,
            sticker_effect: None,
            source,
        };
        let events = [
            Event::Conversation(Conversation {
                chat_identifier: "+15555550122".into(),
                conversation_type: "individual".into(),
                group_title: None,
                participants: Vec::new(),
            }),
            Event::Message(Box::new(Message {
                chat_identifier: "+15555550122".into(),
                guid: "g1".into(),
                timestamp_unix_ms: 1_609_459_200_000,
                outgoing: false,
                service: "iMessage".into(),
                message_kind: "imessage".into(),
                sender_handle: Some("+15555550122".into()),
                sender_display_name: None,
                subject: None,
                text: String::new(),
                owner_handle: "+15555550100".into(),
                owner_display_name: None,
                imessage: None,
                attachments: vec![
                    attachment(AttachmentSource::Inline { text: SVG.into() }),
                    attachment(AttachmentSource::Missing),
                ],
            })),
            Event::ExportDone {
                messages_seen: 1,
                failures: 0,
            },
        ];
        let mut body = source_line(PROTOCOL_VERSION);
        for event in &events {
            body.push_str(&format!(
                "\necho '{}'",
                serde_json::to_string(event).unwrap()
            ));
        }

        let dir = tempfile::tempdir().unwrap();
        let chat = dir.path().join("chat.db");
        fs::write(&chat, b"sqlite").unwrap();
        let program = fake_helper(dir.path(), &body);
        let config = ExporterConfig {
            output_format: OutputFormat::Json,
            ..apple_cfg(
                &chat,
                AppleConfig {
                    platform: Some(ApplePlatform::MacOs),
                    ..AppleConfig::default()
                },
            )
        };
        let result = run_with(&config, |request, _, _| Ok(spawn_fake(&program, request))).unwrap();
        assert!(
            result.messages.iter().any(|l| l == "  saved 1 attachments"),
            "{:#?}",
            result.messages
        );

        let doc = read_conversation_json(&config.output.join("+15555550122.json")).unwrap();
        let attachments = &doc.messages[0].attachments;
        let path = attachments[0].path.as_deref().expect("the SVG was staged");
        assert_eq!(fs::read_to_string(config.output.join(path)).unwrap(), SVG);
        assert_eq!(attachments[1].path, None);
        assert_eq!(
            attachments[1].missing_reason.as_deref(),
            Some("file_missing")
        );
    }
}
