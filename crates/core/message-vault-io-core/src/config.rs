//! Shared typed export configuration for CLI, library, and GUI.
//!
//! [`ExporterConfig`] holds options common to (nearly) every exporter.
//! Exporter-specific knobs live in [`SourceConfig`].

use std::fmt;
use std::path::{Path, PathBuf};

use media::{CompressOptions, MediaMode};

use crate::exporters::{ApplePlatform, Exporter, WhatsappPlatform};
use crate::process::{CancelFlag, LogSink, emit_log};
use crate::progress::{ProgressEvent, ProgressSink, emit_progress};

/// Output packaging projected from the common message.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OutputFormat {
    /// Per-conversation CSV.
    Csv,
    /// Per-conversation folder of `.eml` files (see <https://bitrealm.io/vault/developer/formats/mail-archive/>).
    Eml,
    /// Per-conversation `.mbox` (mboxrd) mailbox file.
    Mbox,
    /// Per-conversation common message JSON (default; see <https://bitrealm.io/vault/developer/architecture/common-message/>).
    #[default]
    Json,
    /// Per-conversation common message as JSON Lines (header + one message per line).
    Jsonl,
    /// Single SMS Backup & Restore XML backup (`smses.xml`).
    Xml,
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Csv => "CSV (per conversation)",
            Self::Eml => "EML archive (mail folders)",
            Self::Mbox => "MBOX (per conversation)",
            Self::Json => "JSON (common message)",
            Self::Jsonl => "JSONL (common message lines)",
            Self::Xml => "XML (SMS Backup & Restore)",
        })
    }
}

impl OutputFormat {
    /// Short format id used on the CLI (`json`, `jsonl`, `csv`, …).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Eml => "eml",
            Self::Mbox => "mbox",
            Self::Json => "json",
            Self::Jsonl => "jsonl",
            Self::Xml => "xml",
        }
    }

    /// Parse a format id. `ndjson` is accepted as JSON Lines; `sbr`/`smses` as XML.
    ///
    /// # Errors
    ///
    /// Returns an error string when `s` is not a known format.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "csv" => Ok(Self::Csv),
            "eml" => Ok(Self::Eml),
            "mbox" => Ok(Self::Mbox),
            "json" => Ok(Self::Json),
            "jsonl" | "ndjson" => Ok(Self::Jsonl),
            "xml" | "sbr" | "smses" => Ok(Self::Xml),
            other => Err(format!(
                "unknown output format '{other}' (expected csv, eml, mbox, json, jsonl, or xml)"
            )),
        }
    }

    /// True for mail-archive packaging (EML folders or MBOX files).
    pub fn is_mail_archive(self) -> bool {
        matches!(self, Self::Eml | Self::Mbox)
    }

    /// True when export writes a single SyncTech `smses.xml` (the `FormatSink` XML path).
    pub fn is_sbr_xml(self) -> bool {
        matches!(self, Self::Xml)
    }
}

/// Shared export inputs. Source-specific fields are in [`Self::source`].
#[derive(Debug, Clone)]
pub struct ExporterConfig {
    /// Input paths (usually one). SMS Backup+ CLI may pass several; WhatsApp may leave empty.
    pub inputs: Vec<PathBuf>,
    /// Output directory the export is written to (packaging plus `attachments/`).
    pub output: PathBuf,
    /// Optional fixed UTC offset for naive timestamps, e.g. `UTC-05:00`.
    /// When `None`, dates are interpreted in host-local time.
    pub timezone: Option<String>,
    /// Fake-name rewrite settings; `None`-equivalent when disabled.
    pub obfuscate: ObfuscateConfig,
    /// Attachment handling for `FormatSink` (none / copy / convert / compress).
    pub media: MediaConfig,
    /// Shared cancel flag for in-process jobs; CLI runs leave it unset.
    pub cancel: Option<CancelFlag>,
    /// Human-readable mid-run notes and warnings. `None` → stderr; the
    /// desktop app sets a sink and shows the lines in its log panel.
    pub log: Option<LogSink>,
    /// Typed progress events (stage, done, total, bytes). `None` reports
    /// nothing; the desktop app sets a sink and drives its progress bar
    /// from the events. Log lines are never read for counts.
    pub progress: Option<ProgressSink>,
    /// Packaging format (`csv` / `eml` / `mbox` / `json` / `jsonl` / `xml`).
    pub output_format: OutputFormat,
    /// Continue an interrupted export in the same output directory: previous
    /// output is kept, and conversations already written are skipped. Only
    /// the desktop's import resume sets this; CLI runs leave it false.
    pub resume: bool,
    /// Exporter-specific options; exactly one variant is set per run.
    pub source: SourceConfig,
}

impl ExporterConfig {
    /// Send a progress or warning line to the log sink, or to stderr if none is set.
    pub fn emit_log(&self, line: impl AsRef<str>) {
        emit_log(self.log.as_ref(), line);
    }

    /// Send a typed progress event to the progress sink, if one is set.
    pub fn emit_progress(&self, event: ProgressEvent) {
        emit_progress(self.progress.as_ref(), event);
    }

    /// First input path, if any.
    pub fn primary_input(&self) -> Option<&Path> {
        self.inputs.first().map(PathBuf::as_path)
    }

    /// Require a single primary input (most exporters).
    ///
    /// # Errors
    ///
    /// Returns an error when no input is set, or when more than one path is set.
    pub fn require_input(&self) -> Result<&Path, String> {
        match self.inputs.as_slice() {
            [path] => Ok(path.as_path()),
            [] => Err("input is required".into()),
            _ => Err("expected a single input path".into()),
        }
    }

    /// True when fake-name rewrite is on, or a seed was supplied.
    pub fn obfuscate_active(&self) -> bool {
        self.obfuscate.enabled || self.obfuscate.seed.is_some()
    }
}

#[derive(Debug, Clone, Default)]
/// Fake-name rewrite: on/off plus an optional hex seed for repeatable output.
pub struct ObfuscateConfig {
    /// Whether fake-name rewrite is enabled.
    pub enabled: bool,
    /// Optional hex seed for repeatable obfuscation.
    pub seed: Option<String>,
}

#[derive(Debug, Clone)]
/// How attachments are copied, converted, or compressed when writing output.
pub struct MediaConfig {
    /// Attachment handling mode (none / copy / convert / compress).
    pub mode: MediaMode,
    /// Compress-mode options (long-edge cap, max fps, min size).
    pub compress: CompressOptions,
}

impl Default for MediaConfig {
    fn default() -> Self {
        Self {
            mode: MediaMode::Clone,
            compress: CompressOptions::default(),
        }
    }
}

/// Exporter-specific options. Exactly one variant is set per run.
#[derive(Debug, Clone)]
pub enum SourceConfig {
    /// GO SMS Pro backup source.
    GoSmsPro(GoSmsProConfig),
    /// SMS Backup & Restore XML backup source.
    SmsBackupRestore(SmsBackupRestoreConfig),
    /// SMS Backup+ archive source.
    SmsBackupPlus(SmsBackupPlusConfig),
    /// OpenExtract backup source.
    OpenExtract(OpenExtractConfig),
    /// iMazing backup source.
    Imazing(ImazingConfig),
    /// iMessage / iPhone backup source.
    Apple(AppleConfig),
    /// WhatsApp backup source.
    Whatsapp(WhatsappConfig),
    /// Existing Message Vault output → another IR format (`message-reexporter`).
    Format(FormatConfig),
}

impl SourceConfig {
    /// Backup type for this source, or `None` for the Format-tab converter.
    pub fn exporter(&self) -> Option<Exporter> {
        match self {
            Self::GoSmsPro(_) => Some(Exporter::GoSmsPro),
            Self::SmsBackupRestore(_) => Some(Exporter::SmsBackupRestore),
            Self::SmsBackupPlus(_) => Some(Exporter::SmsBackupPlus),
            Self::OpenExtract(_) => Some(Exporter::OpenExtract),
            Self::Imazing(_) => Some(Exporter::Imazing),
            Self::Apple(_) => Some(Exporter::Imessage),
            Self::Whatsapp(_) => Some(Exporter::Whatsapp),
            Self::Format(_) => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
/// Empty marker: convert an existing export folder to another output format.
pub struct FormatConfig {}

#[derive(Debug, Clone)]
/// GO SMS Pro extras: owner phone numbers used to mark outgoing messages.
pub struct GoSmsProConfig {
    /// Owner phone numbers used to mark outgoing messages.
    pub owner_phones: Vec<String>,
}

#[derive(Debug, Clone)]
/// SMS Backup & Restore extras: owner phone numbers used to mark outgoing messages.
pub struct SmsBackupRestoreConfig {
    /// Owner phone numbers used to mark outgoing messages.
    pub owner_phones: Vec<String>,
}

#[derive(Debug, Clone)]
/// SMS Backup+ extras: owner phones/emails and log flags.
pub struct SmsBackupPlusConfig {
    /// Owner phone numbers used to mark outgoing messages.
    pub owner_phones: Vec<String>,
    /// Owner email addresses used to mark outgoing messages.
    pub owner_emails: Vec<String>,
    /// Whether to emit verbose log lines.
    pub verbose: bool,
    /// Whether to print the end-of-run summary.
    pub include_summary: bool,
}

#[derive(Debug, Clone, Default)]
/// OpenExtract has no extra fields beyond the shared [`ExporterConfig`].
pub struct OpenExtractConfig {}

#[derive(Debug, Clone, Default)]
/// iMazing has no extra fields beyond the shared [`ExporterConfig`] (timezone lives there).
pub struct ImazingConfig {}

#[derive(Debug, Clone)]
/// iMessage / iPhone backup extras: platform, copy method, address book, password.
pub struct AppleConfig {
    /// iPhone vs Mac backup layout; `None` means auto-detect.
    pub platform: Option<ApplePlatform>,
    /// Custom attachment root (macOS backups).
    pub attachment_root: Option<String>,
    /// `disabled`, `clone`, `basic`, or `full`.
    pub copy_method: String,
    /// macOS AddressBook path.
    pub apple_contacts: Option<PathBuf>,
    /// Apple backup decryption password (never written to `export.ini`).
    pub backup_password: Option<String>,
    /// Use the destination caller id as the outgoing From display name.
    pub use_caller_id: bool,
}

impl Default for AppleConfig {
    fn default() -> Self {
        Self {
            platform: None,
            attachment_root: None,
            copy_method: "clone".into(),
            apple_contacts: None,
            backup_password: None,
            use_caller_id: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
/// WhatsApp extras: Android vs iOS, key, backup folder, and optional media/db paths.
pub struct WhatsappConfig {
    /// Android vs iOS backup layout.
    pub platform: Option<WhatsappPlatform>,
    /// Optional path to an existing `result.json` to convert (skips wtsexporter).
    pub json: Option<PathBuf>,
    /// WhatsApp backup decryption key (never written to `export.ini`).
    pub key: Option<String>,
    /// Encrypted backup or iOS backup path.
    pub backup: Option<PathBuf>,
    /// Contacts database (`wa.db` / `ContactsV2.sqlite`) path.
    pub wa: Option<PathBuf>,
    /// WhatsApp media folder path.
    pub media: Option<PathBuf>,
    /// Explicit `msgstore.db` path.
    pub db: Option<PathBuf>,
    /// Whether the backup is a WhatsApp Business backup.
    pub business: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn config_with_inputs(inputs: Vec<PathBuf>) -> ExporterConfig {
        ExporterConfig {
            inputs,
            output: PathBuf::from("out"),
            timezone: None,
            obfuscate: ObfuscateConfig::default(),
            media: MediaConfig::default(),
            cancel: None,
            log: None,
            progress: None,
            output_format: OutputFormat::Json,
            resume: false,
            source: SourceConfig::Format(FormatConfig {}),
        }
    }

    #[test]
    fn every_output_format_displays_its_label() {
        assert_eq!(OutputFormat::Csv.to_string(), "CSV (per conversation)");
        assert_eq!(OutputFormat::Eml.to_string(), "EML archive (mail folders)");
        assert_eq!(OutputFormat::Mbox.to_string(), "MBOX (per conversation)");
        assert_eq!(OutputFormat::Json.to_string(), "JSON (common message)");
        assert_eq!(
            OutputFormat::Jsonl.to_string(),
            "JSONL (common message lines)"
        );
        assert_eq!(OutputFormat::Xml.to_string(), "XML (SMS Backup & Restore)");
    }

    #[test]
    fn parse_accepts_every_format_id_and_its_aliases() {
        assert_eq!(OutputFormat::parse("csv"), Ok(OutputFormat::Csv));
        assert_eq!(OutputFormat::parse("eml"), Ok(OutputFormat::Eml));
        assert_eq!(OutputFormat::parse("mbox"), Ok(OutputFormat::Mbox));
        assert_eq!(OutputFormat::parse("json"), Ok(OutputFormat::Json));
        assert_eq!(OutputFormat::parse("jsonl"), Ok(OutputFormat::Jsonl));
        assert_eq!(OutputFormat::parse("ndjson"), Ok(OutputFormat::Jsonl));
        assert_eq!(OutputFormat::parse("xml"), Ok(OutputFormat::Xml));
        assert_eq!(OutputFormat::parse("sbr"), Ok(OutputFormat::Xml));
        assert_eq!(OutputFormat::parse("smses"), Ok(OutputFormat::Xml));
    }

    #[test]
    fn parse_ignores_case_and_surrounding_whitespace() {
        assert_eq!(OutputFormat::parse("  JSONL\n"), Ok(OutputFormat::Jsonl));
        assert_eq!(OutputFormat::parse("Csv"), Ok(OutputFormat::Csv));
    }

    #[test]
    fn parse_refuses_an_unknown_format_and_lists_the_known_ones() {
        assert_eq!(
            OutputFormat::parse("pdf"),
            Err(
                "unknown output format 'pdf' (expected csv, eml, mbox, json, jsonl, or xml)"
                    .to_string()
            )
        );
    }

    #[test]
    fn emit_log_hands_each_line_to_the_log_sink() {
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink_seen = Arc::clone(&seen);
        let mut config = config_with_inputs(Vec::new());
        config.log = Some(LogSink::new(move |line| {
            sink_seen.lock().unwrap().push(line.to_string());
        }));

        config.emit_log("first");
        config.emit_log(String::from("second"));

        assert_eq!(
            *seen.lock().unwrap(),
            vec!["first".to_string(), "second".to_string()]
        );
    }

    #[test]
    fn emit_log_without_a_sink_falls_through_to_stderr() {
        // No sink set: the line goes to stderr and nothing panics.
        config_with_inputs(Vec::new()).emit_log("unsunk line");
    }

    #[test]
    fn emit_progress_hands_each_event_to_the_progress_sink() {
        let seen = Arc::new(Mutex::new(Vec::<ProgressEvent>::new()));
        let sink_seen = Arc::clone(&seen);
        let mut config = config_with_inputs(Vec::new());
        config.progress = Some(ProgressSink::new(move |event| {
            sink_seen.lock().unwrap().push(event);
        }));

        config.emit_progress(ProgressEvent::Parse { done: 1, total: 4 });
        config.emit_progress(ProgressEvent::Prepare { done: 2, total: 2 });

        assert_eq!(
            *seen.lock().unwrap(),
            vec![
                ProgressEvent::Parse { done: 1, total: 4 },
                ProgressEvent::Prepare { done: 2, total: 2 },
            ]
        );
    }

    #[test]
    fn emit_progress_without_a_sink_is_a_no_op() {
        config_with_inputs(Vec::new()).emit_progress(ProgressEvent::Parse { done: 1, total: 1 });
    }

    #[test]
    fn require_input_returns_the_single_input_path() {
        let config = config_with_inputs(vec![PathBuf::from("backup/chat.db")]);
        assert_eq!(config.require_input(), Ok(Path::new("backup/chat.db")));
    }

    #[test]
    fn require_input_refuses_a_config_with_no_input() {
        let config = config_with_inputs(Vec::new());
        assert_eq!(config.require_input(), Err("input is required".to_string()));
    }

    #[test]
    fn require_input_refuses_more_than_one_input() {
        let config = config_with_inputs(vec![PathBuf::from("a.xml"), PathBuf::from("b.xml")]);
        assert_eq!(
            config.require_input(),
            Err("expected a single input path".to_string())
        );
    }

    #[test]
    fn every_source_names_its_exporter_and_the_format_converter_names_none() {
        assert_eq!(
            SourceConfig::GoSmsPro(GoSmsProConfig {
                owner_phones: Vec::new()
            })
            .exporter(),
            Some(Exporter::GoSmsPro)
        );
        assert_eq!(
            SourceConfig::SmsBackupRestore(SmsBackupRestoreConfig {
                owner_phones: Vec::new()
            })
            .exporter(),
            Some(Exporter::SmsBackupRestore)
        );
        assert_eq!(
            SourceConfig::SmsBackupPlus(SmsBackupPlusConfig {
                owner_phones: Vec::new(),
                owner_emails: Vec::new(),
                verbose: false,
                include_summary: false,
            })
            .exporter(),
            Some(Exporter::SmsBackupPlus)
        );
        assert_eq!(
            SourceConfig::OpenExtract(OpenExtractConfig {}).exporter(),
            Some(Exporter::OpenExtract)
        );
        assert_eq!(
            SourceConfig::Imazing(ImazingConfig {}).exporter(),
            Some(Exporter::Imazing)
        );
        assert_eq!(
            SourceConfig::Apple(AppleConfig::default()).exporter(),
            Some(Exporter::Imessage)
        );
        assert_eq!(
            SourceConfig::Whatsapp(WhatsappConfig::default()).exporter(),
            Some(Exporter::Whatsapp)
        );
        assert_eq!(SourceConfig::Format(FormatConfig {}).exporter(), None);
    }
}
