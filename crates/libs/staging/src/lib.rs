//! Staging: the resumable path that writes a run's conversations and
//! attachments into the Staging Directory.
//!
//! Parse finishes before anything is written; an exporter hands its
//! documents to [`ExportWriter`], which either drains the bounded write
//! queue (JSON Lines, the import path: a conversation file lands only after
//! everything it references, so an interrupted run resumes by skipping what
//! is already there) or stages attachments and writes through
//! `message_ir_format::FormatSink` for every other format. An exporter whose
//! attachments arrive as bytes writes each payload to the writer's
//! [`AttachmentSpool`] as it parses, so parse never holds the backup's
//! attachments in memory, and the write reads them back a file at a time.
//! The transcode pass ([`transcode_staged`]) converts or compresses staged
//! attachments afterwards as its own resumable pass, and
//! [`summarize_staging`] reads a staging folder back for the Staging Review.
//! Both work to the media settings Staging recorded in the folder
//! ([`write_media_settings`], [`read_media_settings`]).
//!
//! Formats live in `message-ir-format`; the run model in
//! `message-crate-core`. Why this is its own crate:
//! `docs/adr/0012-four-crates-in-the-export-pipeline.md`.

mod export_writer;
mod media_settings;
mod spool;
mod staging_summary;
mod transcode;
mod write_queue;

pub use export_writer::{ExportWriter, ExportWriterParts};
pub use media_settings::{MEDIA_SETTINGS_FILE, read_media_settings, write_media_settings};
pub use spool::AttachmentSpool;
pub use staging_summary::{AttachmentForecast, StagingSummary, SummaryProgress, summarize_staging};
pub use transcode::{TranscodeOptions, TranscodeProgress, TranscodeReport, transcode_staged};
pub use write_queue::{
    AttachmentSource, ConversationUnit, UnitAttachment, WriteQueueOptions, WriteQueueReport,
    default_writer_count, drain_units, drain_write_queue, drain_write_queue_with_loader,
    load_attachment_source,
};
