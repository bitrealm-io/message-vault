//! Staging: the resumable path that writes a run's conversations and
//! attachments into the Staging Directory.
//!
//! Parse finishes before anything is written; an exporter hands its
//! documents to [`ExportWriter`], which either drains the bounded write
//! queue (JSON Lines, the import path: a conversation file lands only after
//! everything it references, so an interrupted run resumes by skipping what
//! is already there) or stages attachments and writes through
//! `message_ir_format::FormatSink` for every other format. The transcode
//! pass ([`transcode_staged`]) converts or compresses staged attachments
//! afterwards as its own resumable pass, and [`summarize_staging`] reads a
//! staging folder back for the Staging Review.
//!
//! Formats live in `message-ir-format`; the run model in
//! `message-vault-io-core`. Why this is its own crate:
//! `docs/adr/0012-four-crates-in-the-export-pipeline.md`.

mod export_writer;
mod staging_summary;
mod transcode;
mod write_queue;

pub use export_writer::{ExportWriter, ExportWriterParts};
pub use staging_summary::{
    AttachmentForecast, StagingSummary, SummaryProgress, VerdictCounts, summarize_staging,
};
pub use transcode::{TranscodeOptions, TranscodeProgress, TranscodeReport, transcode_staged};
pub use write_queue::{
    AttachmentSource, ConversationUnit, UnitAttachment, WriteQueueOptions, WriteQueueReport,
    default_writer_count, drain_units, drain_write_queue, drain_write_queue_with_loader,
    load_attachment_source,
};
