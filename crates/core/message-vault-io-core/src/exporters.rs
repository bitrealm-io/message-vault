//! Backup-type forms, dropdown labels, and validation used by the desktop app.
//!
//! [`Form`] is the GUI field set. [`Form::to_config`] turns it into a typed
//! [`ExporterConfig`] after checking required paths and options.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use media::{MaxResolution, MediaMode};

use crate::config::{
    AppleConfig, ExporterConfig, GoSmsProConfig, ImazingConfig, MediaConfig, ObfuscateConfig,
    OpenExtractConfig, OutputFormat, SmsBackupPlusConfig, SmsBackupRestoreConfig, SourceConfig,
    WhatsappConfig,
};

/// iMessage Import / CLI copy when Convert or Compress is selected and ffmpeg is missing.
pub const CONVERT_COMPRESS_FFMPEG_REQUIRED: &str = "Convert and Compress need ffmpeg and ffprobe. Put them on PATH, or in the desktop app set the ffmpeg directory in Settings → System.";

/// Which backup type the user selected (iMessage, WhatsApp, SMS Backup & Restore, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Exporter {
    /// GO SMS Pro backup type.
    GoSmsPro,
    /// iMazing backup type.
    Imazing,
    #[default]
    /// iMessage / iPhone backup type.
    Imessage,
    /// OpenExtract backup type.
    OpenExtract,
    /// SMS Backup & Restore backup type.
    SmsBackupRestore,
    /// SMS Backup+ backup type.
    SmsBackupPlus,
    /// WhatsApp backup type.
    Whatsapp,
}

/// Android vs iOS for the WhatsApp / wtsexporter bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WhatsappPlatform {
    #[default]
    /// Android WhatsApp backup layout.
    Android,
    /// iOS WhatsApp backup layout.
    Ios,
}

impl fmt::Display for WhatsappPlatform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Android => "Android",
            Self::Ios => "iOS",
        })
    }
}

/// Create `path` and parents if missing.
///
/// # Errors
///
/// Returns an error string when the directory cannot be created.
pub fn ensure_output_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|error| {
        format!(
            "Could not create output directory {}: {error}",
            path.display()
        )
    })
}

/// How attachments are copied or converted when writing output files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AttachmentMedia {
    #[default]
    /// Copy attachments without re-encoding.
    Clone,
    /// Re-encode attachments to a standard format.
    Convert,
    /// Re-encode and compress videos (720p/1080p/4k).
    Compress,
    /// Do not copy attachments.
    Disabled,
}

impl fmt::Display for AttachmentMedia {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Clone => "Copy",
            Self::Convert => "Convert",
            Self::Compress => "Convert & compress",
            Self::Disabled => "Do not copy",
        })
    }
}

impl AttachmentMedia {
    /// The `media::MediaMode` this GUI choice maps to (the same mode the
    /// `--media-mode` CLI flag selects).
    pub fn media_mode(self) -> MediaMode {
        match self {
            Self::Clone => MediaMode::Clone,
            Self::Convert => MediaMode::Convert,
            Self::Compress => MediaMode::Compress,
            Self::Disabled => MediaMode::Disabled,
        }
    }

    /// True when convert or compress is selected (ffmpeg must be on PATH).
    pub fn needs_ffmpeg(self) -> bool {
        matches!(self, Self::Convert | Self::Compress)
    }

    /// Parse a media-mode wire string (`clone`, `convert`, `compress`,
    /// `disabled`), or `None` if unknown.
    pub fn parse(s: &str) -> Option<Self> {
        MediaMode::parse(s).map(|mode| match mode {
            MediaMode::Clone => Self::Clone,
            MediaMode::Convert => Self::Convert,
            MediaMode::Compress => Self::Compress,
            MediaMode::Disabled => Self::Disabled,
        })
    }
}

/// iPhone vs Mac backup layout for iMessage / iMazing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ApplePlatform {
    #[default]
    /// Auto-detect the backup layout.
    Auto,
    /// macOS backup layout.
    MacOs,
    /// iOS backup layout.
    Ios,
}

impl fmt::Display for ApplePlatform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Auto => "Auto-detect",
            Self::MacOs => "macOS",
            Self::Ios => "iOS backup",
        })
    }
}

/// GUI field set for one backup type, plus shared output and media options.
#[derive(Debug, Clone)]
pub struct Form {
    /// Primary input path (source backup file or directory).
    pub input: String,
    /// Output directory for the export.
    pub output: String,
    /// Comma-separated owner phone numbers (marks outgoing messages).
    pub owner_phones: String,
    /// Comma-separated owner email addresses (marks outgoing messages).
    pub owner_emails: String,
    /// IANA time zone name (e.g. `America/New_York`) for naive timestamps.
    ///
    /// Text, because a form field is what someone typed; it is parsed once in
    /// [`Form::to_config`] and a bad name joins the other validation errors.
    pub time_zone: String,
    /// Whether to rewrite output with stable fake identities.
    pub obfuscate: bool,
    /// Optional hex seed for reproducible obfuscation.
    pub obfuscate_seed: String,
    /// Whether the advanced section of the GUI form is shown.
    pub advanced: bool,
    /// iMessage chat database path (Apple sources).
    pub db_path: String,
    /// Apple backup attachment root directory.
    pub attachment_root: String,
    /// macOS AddressBook path (Apple sources).
    pub apple_contacts: String,
    /// Apple backup decryption password (never written to `export.ini`).
    pub backup_password: String,
    /// Packaging format projected from the common message (`json` default).
    pub output_format: OutputFormat,
    /// Attachment handling choice for the export.
    pub attachment_media: AttachmentMedia,
    /// Compress-only long-edge cap (720p/1080p/4k).
    pub media_max_resolution: MaxResolution,
    /// Compress-only max frame rate.
    pub media_max_fps: String,
    /// Compress-only minimum video size (e.g. `20M`).
    pub media_min_size: String,
    /// Compress-only: skip already-efficient HEVC videos.
    pub media_skip_efficient: bool,
    /// iPhone vs Mac backup layout.
    pub apple_platform: ApplePlatform,
    /// Android vs iOS WhatsApp layout.
    pub whatsapp_platform: WhatsappPlatform,
    /// WhatsApp backup encryption key (never written to `export.ini`).
    pub whatsapp_key: String,
    /// WhatsApp backup file path.
    pub whatsapp_backup: String,
    /// WhatsApp Web session/wa path.
    pub whatsapp_wa: String,
    /// WhatsApp media folder path.
    pub whatsapp_media: String,
    /// WhatsApp message database path.
    pub whatsapp_db: String,
    /// Whether the backup is a WhatsApp Business backup.
    pub whatsapp_business: bool,
}

impl Default for Form {
    fn default() -> Self {
        Self {
            input: String::new(),
            output: String::new(),
            owner_phones: String::new(),
            owner_emails: String::new(),
            time_zone: String::new(),
            obfuscate: false,
            obfuscate_seed: String::new(),
            advanced: false,
            db_path: String::new(),
            attachment_root: String::new(),
            apple_contacts: String::new(),
            backup_password: String::new(),
            output_format: OutputFormat::default(),
            attachment_media: AttachmentMedia::default(),
            media_max_resolution: MaxResolution::default(),
            media_max_fps: "30".into(),
            media_min_size: "20M".into(),
            media_skip_efficient: true,
            apple_platform: ApplePlatform::default(),
            whatsapp_platform: WhatsappPlatform::default(),
            whatsapp_key: String::new(),
            whatsapp_backup: String::new(),
            whatsapp_wa: String::new(),
            whatsapp_media: String::new(),
            whatsapp_db: String::new(),
            whatsapp_business: false,
        }
    }
}

impl Form {
    /// Validate the form and build a typed [`ExporterConfig`] for `exporter`.
    ///
    /// # Errors
    ///
    /// Returns one string per validation problem (missing path, bad seed, …).
    pub fn to_config(&self, exporter: Exporter) -> Result<ExporterConfig, Vec<String>> {
        let mut errors = Vec::new();
        let obfuscate = self.validate_obfuscate(&mut errors);

        let config = match exporter {
            Exporter::Imessage => self.to_imessage_config(obfuscate, &mut errors),
            Exporter::Whatsapp => self.to_whatsapp_config(obfuscate, &mut errors),
            Exporter::Imazing => self.to_imazing_config(obfuscate, &mut errors),
            Exporter::OpenExtract => self.to_openextract_config(obfuscate, &mut errors),
            Exporter::GoSmsPro => self.to_go_sms_pro_config(obfuscate, &mut errors),
            Exporter::SmsBackupRestore => self.to_sms_restore_config(obfuscate, &mut errors),
            Exporter::SmsBackupPlus => self.to_sms_plus_config(obfuscate, &mut errors),
        };

        if errors.is_empty() {
            Ok(config)
        } else {
            Err(errors)
        }
    }

    /// Build an iMessage config, pushing path and ffmpeg problems onto `errors`.
    fn to_imessage_config(
        &self,
        obfuscate: ObfuscateConfig,
        errors: &mut Vec<String>,
    ) -> ExporterConfig {
        required_text(&self.output, "Output directory", errors);
        let obfuscate_active = self.obfuscate || !self.obfuscate_seed.trim().is_empty();
        if !obfuscate_active && self.attachment_media.needs_ffmpeg() && !media::ffmpeg_available() {
            errors.push(CONVERT_COMPRESS_FFMPEG_REQUIRED.into());
        }
        let media = self.media_config_for(
            matches!(self.attachment_media, AttachmentMedia::Compress),
            errors,
        );
        let copy_method = match self.attachment_media {
            AttachmentMedia::Disabled => "disabled".into(),
            _ => "clone".into(),
        };
        let platform = match self.apple_platform {
            ApplePlatform::Auto => None,
            other => Some(other),
        };
        let inputs = message_ir::trimmed(&self.db_path)
            .map(|p| vec![PathBuf::from(p)])
            .unwrap_or_default();
        ExporterConfig {
            inputs,
            output: PathBuf::from(self.output.trim()),
            time_zone: None,
            obfuscate,
            media,
            cancel: None,
            log: None,
            progress: None,
            output_format: self.output_format,
            resume: false,
            source: SourceConfig::Apple(AppleConfig {
                platform,
                attachment_root: message_ir::nonempty(&self.attachment_root),
                copy_method,
                apple_contacts: non_empty_path(&self.apple_contacts),
                backup_password: message_ir::nonempty(&self.backup_password),
                use_caller_id: true,
            }),
        }
    }

    /// Build a WhatsApp config, pushing path and key problems onto `errors`.
    ///
    /// `input` is optional here: when set it becomes the backup search root
    /// (and an allowed media root) for the wtsexporter bridge, which falls
    /// back to its working directory when no input is given.
    fn to_whatsapp_config(
        &self,
        obfuscate: ObfuscateConfig,
        errors: &mut Vec<String>,
    ) -> ExporterConfig {
        let inputs = if self.input.trim().is_empty() {
            Vec::new()
        } else {
            require_single_existing_path(&self.input, "Input", errors)
                .into_iter()
                .collect()
        };
        required_text(&self.output, "Output", errors);
        if self.whatsapp_platform == WhatsappPlatform::Ios && self.whatsapp_backup.trim().is_empty()
        {
            errors.push("Backup path is required for iOS.".into());
        }
        let media = self.validate_media(errors);
        ExporterConfig {
            inputs,
            output: PathBuf::from(self.output.trim()),
            time_zone: None,
            obfuscate,
            media,
            cancel: None,
            log: None,
            progress: None,
            output_format: self.output_format,
            resume: false,
            source: SourceConfig::Whatsapp(WhatsappConfig {
                platform: Some(self.whatsapp_platform),
                json: None,
                key: message_ir::nonempty(&self.whatsapp_key),
                backup: non_empty_path(&self.whatsapp_backup),
                wa: non_empty_path(&self.whatsapp_wa),
                media: non_empty_path(&self.whatsapp_media),
                db: non_empty_path(&self.whatsapp_db),
                business: self.whatsapp_business,
            }),
        }
    }

    /// Build an iMazing config, pushing path and time-zone problems onto `errors`.
    fn to_imazing_config(
        &self,
        obfuscate: ObfuscateConfig,
        errors: &mut Vec<String>,
    ) -> ExporterConfig {
        let input = require_single_existing_path(&self.input, "Input", errors);
        required_text(&self.output, "Output", errors);
        let media = self.validate_media(errors);
        let time_zone = self.parse_time_zone(errors, false);
        ExporterConfig {
            inputs: input.into_iter().collect(),
            output: PathBuf::from(self.output.trim()),
            time_zone,
            obfuscate,
            media,
            cancel: None,
            log: None,
            progress: None,
            output_format: self.output_format,
            resume: false,
            source: SourceConfig::Imazing(ImazingConfig {}),
        }
    }

    /// Build an OpenExtract config, pushing path problems onto `errors`.
    fn to_openextract_config(
        &self,
        obfuscate: ObfuscateConfig,
        errors: &mut Vec<String>,
    ) -> ExporterConfig {
        let input = require_single_existing_path(&self.input, "Input", errors);
        required_text(&self.output, "Output", errors);
        let media = self.validate_media(errors);
        ExporterConfig {
            inputs: input.into_iter().collect(),
            output: PathBuf::from(self.output.trim()),
            time_zone: None,
            obfuscate,
            media,
            cancel: None,
            log: None,
            progress: None,
            output_format: self.output_format,
            resume: false,
            source: SourceConfig::OpenExtract(OpenExtractConfig {}),
        }
    }

    /// Build a GO SMS Pro config from the shared Android fields.
    fn to_go_sms_pro_config(
        &self,
        obfuscate: ObfuscateConfig,
        errors: &mut Vec<String>,
    ) -> ExporterConfig {
        let (inputs, media, owner_phones) = self.android_common(errors);
        ExporterConfig {
            inputs,
            output: PathBuf::from(self.output.trim()),
            time_zone: None,
            obfuscate,
            media,
            cancel: None,
            log: None,
            progress: None,
            output_format: self.output_format,
            resume: false,
            source: SourceConfig::GoSmsPro(GoSmsProConfig { owner_phones }),
        }
    }

    /// Build an SMS Backup & Restore config from the shared Android fields.
    fn to_sms_restore_config(
        &self,
        obfuscate: ObfuscateConfig,
        errors: &mut Vec<String>,
    ) -> ExporterConfig {
        let (inputs, media, owner_phones) = self.android_common(errors);
        ExporterConfig {
            inputs,
            output: PathBuf::from(self.output.trim()),
            time_zone: None,
            obfuscate,
            media,
            cancel: None,
            log: None,
            progress: None,
            output_format: self.output_format,
            resume: false,
            source: SourceConfig::SmsBackupRestore(SmsBackupRestoreConfig { owner_phones }),
        }
    }

    /// Build an SMS Backup+ config, including owner emails and name mapping.
    fn to_sms_plus_config(
        &self,
        obfuscate: ObfuscateConfig,
        errors: &mut Vec<String>,
    ) -> ExporterConfig {
        let (inputs, media, owner_phones) = self.android_common(errors);
        let owner_emails: Vec<String> = values(&self.owner_emails)
            .into_iter()
            .map(str::to_string)
            .collect();
        if owner_emails.is_empty() {
            errors.push("At least one email address is required.".into());
        }
        // Archive transcripts carry a wall clock and no offset, so this
        // exporter cannot resolve a time without a zone and must not guess one.
        let time_zone = self.parse_time_zone(errors, true);
        ExporterConfig {
            inputs,
            output: PathBuf::from(self.output.trim()),
            time_zone,
            obfuscate,
            media,
            cancel: None,
            log: None,
            progress: None,
            output_format: self.output_format,
            resume: false,
            source: SourceConfig::SmsBackupPlus(SmsBackupPlusConfig {
                owner_phones,
                owner_emails,
                verbose: true,
                include_summary: true,
            }),
        }
    }

    /// Shared Android backup fields: input path, owner phones, media.
    fn android_common(&self, errors: &mut Vec<String>) -> (Vec<PathBuf>, MediaConfig, Vec<String>) {
        let input = require_single_existing_path(&self.input, "Input", errors);
        required_text(&self.output, "Output", errors);
        let mut owner_phones = Vec::new();
        for phone in values(&self.owner_phones) {
            owner_phones.push(phone.to_string());
        }
        if owner_phones.is_empty() {
            errors.push("At least one phone number is required.".into());
        }
        let media = self.validate_media(errors);
        (input.into_iter().collect(), media, owner_phones)
    }

    /// Media options for Android exporters (always validate compress settings).
    /// The named time zone, pushing a problem onto `errors` when it cannot be
    /// used.
    ///
    /// Parsed once here rather than inside each exporter, so a bad name is one
    /// message beside the other form errors instead of a failure per file.
    /// `required` says whether an empty field is itself an error: an exporter
    /// reading wall-clock times has no sane default to fall back on.
    fn parse_time_zone(&self, errors: &mut Vec<String>, required: bool) -> Option<chrono_tz::Tz> {
        let name = self.time_zone.trim();
        if name.is_empty() {
            if required {
                errors.push("A time zone is required.".into());
            }
            return None;
        }
        match name.parse::<chrono_tz::Tz>() {
            Ok(zone) => Some(zone),
            Err(_) => {
                errors.push(format!("Unknown time zone: {name}."));
                None
            }
        }
    }

    fn validate_media(&self, errors: &mut Vec<String>) -> MediaConfig {
        let mode = self.attachment_media.media_mode();
        let obfuscate_active = self.obfuscate || !self.obfuscate_seed.trim().is_empty();
        // Obfuscate skips copy/convert, so ffmpeg is not required.
        if !obfuscate_active && mode.needs_tools() && !media::ffmpeg_available() {
            errors.push(
                "Convert/Compress require ffmpeg and ffprobe in lib/ (or beside the program), in MESSAGE_VAULT_IO_BIN, or on PATH.".into(),
            );
        }
        self.media_config_for(matches!(mode, MediaMode::Compress), errors)
    }

    /// Fake-name rewrite flag and optional hex seed.
    fn validate_obfuscate(&self, errors: &mut Vec<String>) -> ObfuscateConfig {
        let seed = validate_obfuscate_seed(&self.obfuscate_seed, errors);
        ObfuscateConfig {
            enabled: self.obfuscate || seed.is_some(),
            seed,
        }
    }

    /// Attachment copy/convert/compress options; optionally check compress fields.
    fn media_config_for(&self, validate_compress: bool, errors: &mut Vec<String>) -> MediaConfig {
        let mode = self.attachment_media.media_mode();
        let compress = if validate_compress || matches!(mode, MediaMode::Compress) {
            match self.compress_options() {
                Ok(options) => options,
                Err(error) => {
                    errors.push(error);
                    media::CompressOptions::default()
                }
            }
        } else {
            media::CompressOptions::default()
        };
        MediaConfig { mode, compress }
    }

    /// Compress options for GUI iMessage post-process (after exporter exits).
    ///
    /// # Errors
    ///
    /// Returns an error when fps or min-size cannot be parsed.
    pub fn compress_options(&self) -> Result<media::CompressOptions, String> {
        let fps = self.media_max_fps.trim();
        if fps.is_empty() {
            return Err("Max fps is required for Compress.".into());
        }
        let fps: f32 = fps
            .parse()
            .map_err(|_| "Max fps must be a number.".to_string())?;
        let min_size = self.media_min_size.trim();
        if min_size.is_empty() {
            return Err("Min size is required for Compress.".into());
        }
        media::compress_options_from_cli(
            self.media_max_resolution,
            fps,
            min_size,
            self.media_skip_efficient,
        )
        .map_err(|e| format!("{e:#}"))
    }
}

/// Require one existing file or directory; push a message onto `errors` if missing.
fn require_single_existing_path(
    value: &str,
    label: &str,
    errors: &mut Vec<String>,
) -> Option<PathBuf> {
    let paths = lines(value);
    if paths.is_empty() {
        errors.push(format!("{label} is required."));
        return None;
    }
    if paths.len() > 1 {
        errors.push(format!("{label} must be a single file or folder."));
        return None;
    }
    let path = paths[0];
    if !Path::new(path).exists() {
        errors.push(format!("{label} path does not exist: {path}"));
    }
    Some(PathBuf::from(path))
}

/// Push `"{label} is required."` when `value` is blank.
fn required_text(value: &str, label: &str, errors: &mut Vec<String>) {
    if value.trim().is_empty() {
        errors.push(format!("{label} is required."));
    }
}

/// Return a trimmed hex seed, or push an error when it is not a valid seed.
fn validate_obfuscate_seed(seed: &str, errors: &mut Vec<String>) -> Option<String> {
    let seed = seed.trim();
    if seed.is_empty() {
        return None;
    }
    match obfuscate::check_seed_hex(seed) {
        Ok(()) => Some(seed.to_string()),
        Err(message) => {
            errors.push(message);
            None
        }
    }
}

/// Non-empty trimmed lines from a multiline text field.
fn lines(value: &str) -> Vec<&str> {
    value.lines().filter_map(message_ir::trimmed).collect()
}

/// Non-empty tokens split on commas or whitespace.
fn values(value: &str) -> Vec<&str> {
    value
        .split(['\n', ',', ';'])
        .filter_map(message_ir::trimmed)
        .collect()
}

/// `Some(path)` when the string is not blank.
fn non_empty_path(value: &str) -> Option<PathBuf> {
    message_ir::trimmed(value).map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imazing_passes_obfuscate() {
        let form = Form {
            input: std::env::current_dir().unwrap().display().to_string(),
            output: "out".into(),
            obfuscate: true,
            ..Form::default()
        };
        let config = form.to_config(Exporter::Imazing).unwrap();
        assert!(config.obfuscate.enabled);
        assert!(matches!(config.source, SourceConfig::Imazing(_)));
    }

    #[test]
    fn seed_must_be_valid_hex() {
        let form = Form {
            input: std::env::current_dir().unwrap().display().to_string(),
            output: "out".into(),
            obfuscate_seed: "bad".into(),
            ..Form::default()
        };
        assert!(form.to_config(Exporter::OpenExtract).is_err());
        let form = Form {
            input: std::env::current_dir().unwrap().display().to_string(),
            output: "out".into(),
            obfuscate_seed: "01234567".into(),
            ..Form::default()
        };
        assert!(form.to_config(Exporter::OpenExtract).is_err());
        let form = Form {
            input: std::env::current_dir().unwrap().display().to_string(),
            output: "out".into(),
            obfuscate_seed: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .into(),
            ..Form::default()
        };
        let config = form.to_config(Exporter::OpenExtract).unwrap();
        assert!(config.obfuscate.enabled);
        assert_eq!(
            config.obfuscate.seed.as_deref(),
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
        );
    }

    #[test]
    fn plus_verbose_and_owner_fields() {
        let cwd = std::env::current_dir().unwrap().display().to_string();
        let form = Form {
            input: cwd,
            output: "out".into(),
            owner_phones: "+15555550100\n+15555550101".into(),
            owner_emails: "me@example.com".into(),
            time_zone: "America/New_York".into(),
            ..Form::default()
        };
        let config = form.to_config(Exporter::SmsBackupPlus).unwrap();
        assert_eq!(config.inputs.len(), 1);
        let SourceConfig::SmsBackupPlus(plus) = config.source else {
            panic!("expected SmsBackupPlus");
        };
        assert_eq!(plus.owner_phones.len(), 2);
        assert!(plus.verbose);
        assert!(plus.include_summary);
    }

    #[test]
    fn plus_rejects_multiple_inputs() {
        let cwd = std::env::current_dir().unwrap().display().to_string();
        let form = Form {
            input: format!("{cwd}\n{cwd}"),
            output: "out".into(),
            owner_phones: "+15555550100".into(),
            owner_emails: "me@example.com".into(),
            ..Form::default()
        };
        let err = form.to_config(Exporter::SmsBackupPlus).unwrap_err();
        assert!(err.iter().any(|e| e.contains("single file or folder")));
    }

    #[test]
    fn imessage_requires_output_and_uses_caller_id() {
        let form = Form {
            output: String::new(),
            ..Form::default()
        };
        assert!(form.to_config(Exporter::Imessage).is_err());

        let form = Form {
            output: "out".into(),
            ..Form::default()
        };
        let config = form.to_config(Exporter::Imessage).unwrap();
        assert_eq!(config.output, PathBuf::from("out"));
        let SourceConfig::Apple(apple) = config.source else {
            panic!("expected Apple");
        };
        assert!(apple.use_caller_id);
        assert_eq!(apple.copy_method, "clone");
    }

    #[test]
    fn sbr_passes_output_format() {
        let form = Form {
            input: std::env::current_dir().unwrap().display().to_string(),
            output: "out".into(),
            owner_phones: "+15555550100".into(),
            output_format: OutputFormat::Eml,
            ..Form::default()
        };
        let config = form.to_config(Exporter::SmsBackupRestore).unwrap();
        assert_eq!(config.output_format, OutputFormat::Eml);

        let go = form.to_config(Exporter::GoSmsPro).unwrap();
        assert_eq!(go.output_format, OutputFormat::Eml);
    }

    #[test]
    fn imessage_passes_output_format() {
        let form = Form {
            output: "out".into(),
            output_format: OutputFormat::Eml,
            ..Form::default()
        };
        let config = form.to_config(Exporter::Imessage).unwrap();
        assert_eq!(config.output_format, OutputFormat::Eml);
    }

    #[test]
    fn android_passes_media_mode() {
        let form = Form {
            input: std::env::current_dir().unwrap().display().to_string(),
            output: "out".into(),
            owner_phones: "+15555550100".into(),
            attachment_media: AttachmentMedia::Clone,
            ..Form::default()
        };
        let config = form.to_config(Exporter::GoSmsPro).unwrap();
        assert_eq!(config.media.mode, MediaMode::Clone);

        let form = Form {
            input: std::env::current_dir().unwrap().display().to_string(),
            output: "out".into(),
            owner_phones: "+15555550100".into(),
            attachment_media: AttachmentMedia::Disabled,
            ..Form::default()
        };
        let config = form.to_config(Exporter::GoSmsPro).unwrap();
        assert_eq!(config.media.mode, MediaMode::Disabled);
    }

    #[test]
    fn openextract_builds_its_own_source() {
        let form = Form {
            input: std::env::current_dir().unwrap().display().to_string(),
            output: "out".into(),
            ..Form::default()
        };
        let config = form.to_config(Exporter::OpenExtract).unwrap();
        assert!(matches!(config.source, SourceConfig::OpenExtract(_)));
    }

    #[test]
    fn imazing_passes_a_named_zone_to_config() {
        let form = Form {
            input: std::env::current_dir().unwrap().display().to_string(),
            output: "out".into(),
            time_zone: "America/New_York".into(),
            ..Form::default()
        };
        let config = form.to_config(Exporter::Imazing).unwrap();
        let SourceConfig::Imazing(_) = &config.source else {
            panic!("expected Imazing");
        };
        assert_eq!(config.time_zone, Some(chrono_tz::America::New_York));
    }

    /// The form takes an IANA zone name. A fixed offset cannot express a zone
    /// that observes daylight saving, which is what a multi-year backup needs.
    #[test]
    fn a_fixed_utc_offset_is_not_a_time_zone() {
        let form = Form {
            input: std::env::current_dir().unwrap().display().to_string(),
            output: "out".into(),
            time_zone: "UTC-05:00".into(),
            ..Form::default()
        };
        let errors = form.to_config(Exporter::Imazing).unwrap_err();
        assert!(
            errors.iter().any(|e| e.contains("Unknown time zone")),
            "{errors:?}"
        );
    }

    #[test]
    fn sms_backup_plus_requires_a_time_zone() {
        // Archive transcripts are wall-clock with no offset, so there is no
        // sane default: the exporter must be told, not left to guess.
        let form = sms_plus_form("");
        let errors = form.to_config(Exporter::SmsBackupPlus).unwrap_err();
        assert!(
            errors.iter().any(|e| e == "A time zone is required."),
            "{errors:?}"
        );
    }

    #[test]
    fn sms_backup_plus_rejects_an_unknown_time_zone() {
        let form = sms_plus_form("Not/AZone");
        let errors = form.to_config(Exporter::SmsBackupPlus).unwrap_err();
        assert!(
            errors.iter().any(|e| e == "Unknown time zone: Not/AZone."),
            "{errors:?}"
        );
    }

    #[test]
    fn sms_backup_plus_passes_its_zone_through() {
        let form = sms_plus_form("America/New_York");
        let config = form.to_config(Exporter::SmsBackupPlus).unwrap();
        assert_eq!(config.time_zone, Some(chrono_tz::America::New_York));
    }

    /// An otherwise valid SMS Backup+ form carrying `time_zone`.
    fn sms_plus_form(time_zone: &str) -> Form {
        Form {
            input: std::env::current_dir().unwrap().display().to_string(),
            output: "out".into(),
            owner_phones: "+15555550100".into(),
            owner_emails: "owner@example.com".into(),
            time_zone: time_zone.into(),
            ..Form::default()
        }
    }

    #[test]
    fn whatsapp_passes_platform_and_media() {
        let form = Form {
            output: "out".into(),
            whatsapp_platform: WhatsappPlatform::Android,
            whatsapp_key: "abc123".into(),
            whatsapp_backup: "/tmp/backup".into(),
            whatsapp_media: "/tmp/media".into(),
            whatsapp_business: true,
            attachment_media: AttachmentMedia::Clone,
            ..Form::default()
        };
        let config = form.to_config(Exporter::Whatsapp).unwrap();
        assert!(config.inputs.is_empty());
        assert_eq!(config.media.mode, MediaMode::Clone);
        let SourceConfig::Whatsapp(wa) = config.source else {
            panic!("expected Whatsapp");
        };
        assert_eq!(wa.platform, Some(WhatsappPlatform::Android));
        assert_eq!(wa.key.as_deref(), Some("abc123"));
        assert_eq!(wa.backup, Some(PathBuf::from("/tmp/backup")));
        assert_eq!(wa.media, Some(PathBuf::from("/tmp/media")));
        assert!(wa.business);

        let ios = Form {
            output: "out".into(),
            whatsapp_platform: WhatsappPlatform::Ios,
            whatsapp_backup: "/tmp/ios-backup".into(),
            ..Form::default()
        };
        let ios_config = ios.to_config(Exporter::Whatsapp).unwrap();
        let SourceConfig::Whatsapp(wa) = ios_config.source else {
            panic!("expected Whatsapp");
        };
        assert_eq!(wa.platform, Some(WhatsappPlatform::Ios));
        assert_eq!(wa.backup, Some(PathBuf::from("/tmp/ios-backup")));
        assert!(wa.key.is_none());

        let ios_missing = Form {
            output: "out".into(),
            whatsapp_platform: WhatsappPlatform::Ios,
            ..Form::default()
        };
        let err = ios_missing.to_config(Exporter::Whatsapp).unwrap_err();
        assert!(
            err.iter()
                .any(|e| e.contains("Backup path is required for iOS"))
        );
    }

    #[test]
    fn whatsapp_forwards_existing_input_as_search_root() {
        let dir = tempfile::tempdir().unwrap();
        let form = Form {
            input: dir.path().display().to_string(),
            output: "out".into(),
            ..Form::default()
        };
        let config = form.to_config(Exporter::Whatsapp).unwrap();
        assert_eq!(config.inputs, vec![dir.path().to_path_buf()]);

        let missing = Form {
            input: "/does/not/exist-whatsapp-input".into(),
            output: "out".into(),
            ..Form::default()
        };
        let err = missing.to_config(Exporter::Whatsapp).unwrap_err();
        assert!(err.iter().any(|e| e.contains("does not exist")), "{err:?}");
    }

    #[test]
    fn ensure_output_dir_creates_missing_path() {
        let out = std::env::temp_dir().join(format!(
            "message-vault-io-core-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        ensure_output_dir(&out).unwrap();
        assert!(out.is_dir());
        let _ = fs::remove_dir_all(&out);
    }

    struct RestoreToolsDir;

    impl Drop for RestoreToolsDir {
        fn drop(&mut self) {
            media::set_tools_dir(None);
        }
    }

    #[test]
    fn imessage_convert_without_ffmpeg_uses_locked_copy() {
        let dir = tempfile::tempdir().unwrap();
        let _restore = RestoreToolsDir;
        media::set_tools_dir(Some(dir.path().to_path_buf()));
        let form = Form {
            output: "out".into(),
            attachment_media: AttachmentMedia::Convert,
            ..Form::default()
        };
        let err = form.to_config(Exporter::Imessage).unwrap_err();
        assert!(
            err.iter().any(|e| e == CONVERT_COMPRESS_FFMPEG_REQUIRED),
            "{err:?}"
        );
    }
}
