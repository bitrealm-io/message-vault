//! Shared exporter forms, typed config, and background-job helpers for desktop apps.
//!
//! The desktop app and exporter command-line tools share this crate so every
//! backup type validates the same way before a job starts.

pub mod attachment_jobs;
pub mod attachments;
mod config;
mod exporters;
mod pipeline;
mod process;
mod progress;
#[cfg(feature = "testutil")]
pub mod testutil;

pub use attachment_jobs::{
    AttachmentJob, AttachmentProgress, attachment_jobs, attachment_size_hint, document_messages,
    mime_for_rel, report_attachment_progress, run_attachment_jobs, stage_conversation_attachments,
};
pub use attachments::{
    attachment_date_prefix, attachment_dest_name, copy_if_missing, digest_prefix, write_if_missing,
};
pub use config::{
    AppleConfig, ExporterConfig, FormatConfig, GoSmsProConfig, ImazingConfig, MediaConfig,
    ObfuscateConfig, OpenExtractConfig, OutputFormat, SmsBackupPlusConfig, SmsBackupRestoreConfig,
    SourceConfig, WhatsappConfig,
};
pub use exporters::{
    ApplePlatform, AttachmentMedia, CONVERT_COMPRESS_FFMPEG_REQUIRED, Exporter, Form,
    WhatsappPlatform, ensure_output_dir,
};
pub use pipeline::{
    ExportReport, RunResult, discover_files, export_meta, name_stem, parse_date_range,
    parse_date_range_tz, prepare_outputs, print_result, project_conversation,
    prune_and_finish_conversation,
};
pub use process::{
    CancelFlag, Cancelled, LogSink, check_cancel, emit_log, is_cancelled, parallel_for_each,
};
pub use progress::{ProgressEvent, ProgressSink, emit_progress};
