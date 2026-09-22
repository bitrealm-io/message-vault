//! The shared write tail every exporter used to copy: sink opening, the
//! queue-or-sink decision, and both drain arms.

use crate::export_transforms::ExportTransforms;
use crate::format_sink::{FormatSink, FormatSinkResult, write_documents_through_sink};
use crate::write_queue::{
    AttachmentSource, ConversationUnit, WriteQueueOptions, drain_units, load_attachment_source,
};
use anyhow::Result;
use media::{CompressOptions, MediaMode};
use message_ir::{ConversationDocument, IrAttachment};
use message_vault_io_core::{CancelFlag, ExportReport, LogSink, OutputFormat, ProgressSink};
use std::path::{Path, PathBuf};

/// Owns the write tail of an exporter run: output preparation, the
/// queue-or-sink decision, attachment staging, and the format write.
///
/// [`open`](Self::open) runs before parse (it cleans or resumes the output
/// directory, so the exporter can query [`copies_attachments`](Self::copies_attachments),
/// [`use_queue`](Self::use_queue), and [`attachments_dir`](Self::attachments_dir)
/// while collecting messages); [`finish`](Self::finish) takes the collected
/// documents and writes everything. An exporter whose drain cannot go
/// through [`finish`](Self::finish) (iMessage's encrypted-backup loader is
/// not `Sync`) takes [`into_parts`](Self::into_parts) instead and keeps its
/// own tail.
#[derive(Debug)]
pub struct ExportWriter {
    output_dir: PathBuf,
    sink: FormatSink,
    attachments_dir: PathBuf,
    media_mode: MediaMode,
    compress: CompressOptions,
    log: Option<LogSink>,
    progress: Option<ProgressSink>,
    resume: bool,
    use_queue: bool,
    copy_attachments: bool,
}

/// The opened sink and the decisions [`ExportWriter::open`] made, for an
/// exporter that writes its own tail instead of calling
/// [`ExportWriter::finish`].
#[derive(Debug)]
pub struct ExportWriterParts {
    /// The opened (cleaned or resumed) format sink.
    pub sink: FormatSink,
    /// Directory attachment files are staged into (`<output>/attachments`).
    pub attachments_dir: PathBuf,
    /// Whether the run should drain the write queue instead of the sink.
    pub use_queue: bool,
    /// Media mode ([`MediaMode::Disabled`] when attachments are not copied).
    pub media_mode: MediaMode,
    /// Whether this run stages attachment bytes.
    pub copy_attachments: bool,
    /// Compression options from the transforms.
    pub compress: CompressOptions,
    /// Log sink captured from the transforms.
    pub log: Option<LogSink>,
    /// Progress sink captured from the transforms.
    pub progress: Option<ProgressSink>,
}

impl ExportWriter {
    /// Prepare `output_dir` and open the sink for one export run.
    ///
    /// When `resume` is set the previous run's output is kept and its
    /// conversations are skipped ([`FormatSink::open_resume`]); otherwise the
    /// directory is cleaned first ([`FormatSink::open_prepared`]). When the
    /// transforms do not copy attachments, media handling is forced to
    /// [`MediaMode::Disabled`].
    ///
    /// # Errors
    ///
    /// Returns an error when the output directory cannot be prepared, or a
    /// resumed run points at a directory that is not a staging folder.
    pub fn open(
        output_dir: &Path,
        format: OutputFormat,
        transforms: ExportTransforms,
        resume: bool,
    ) -> Result<Self> {
        let copy_attachments = transforms.copies_attachments();
        let media_mode = if copy_attachments {
            transforms.media
        } else {
            MediaMode::Disabled
        };
        let compress = transforms.compress.clone();
        let log = transforms.log.clone();
        let progress = transforms.progress.clone();
        // The queue path is for the import, which is JSONL and never
        // obfuscated. Obfuscation is stateful across documents and the other
        // formats merge or embed at finish, so those keep the sink path.
        let use_queue = format == OutputFormat::Jsonl && !transforms.obfuscate;
        let (sink, attachments_dir) = if resume {
            FormatSink::open_resume(output_dir, format, transforms)
        } else {
            FormatSink::open_prepared(output_dir, format, transforms)
        }?;
        Ok(Self {
            output_dir: output_dir.to_path_buf(),
            sink,
            attachments_dir,
            media_mode,
            compress,
            log,
            progress,
            resume,
            use_queue,
            copy_attachments,
        })
    }

    /// Directory attachment files are staged into (`<output>/attachments`).
    pub fn attachments_dir(&self) -> &Path {
        &self.attachments_dir
    }

    /// Whether this run stages attachment bytes (false when obfuscating or
    /// media is disabled). Exporters check this while parsing to decide
    /// whether to keep payloads around.
    pub fn copies_attachments(&self) -> bool {
        self.copy_attachments
    }

    /// Whether [`finish`](Self::finish) will drain the write queue instead of
    /// the buffered sink.
    pub fn use_queue(&self) -> bool {
        self.use_queue
    }

    /// Media mode this run stages with ([`MediaMode::Disabled`] when the
    /// transforms do not copy attachments).
    pub fn media_mode(&self) -> MediaMode {
        self.media_mode
    }

    /// Log sink captured from the transforms.
    pub fn log(&self) -> Option<&LogSink> {
        self.log.as_ref()
    }

    /// Progress sink captured from the transforms.
    pub fn progress(&self) -> Option<&ProgressSink> {
        self.progress.as_ref()
    }

    /// Give up the writer and take the opened sink plus the decisions
    /// [`open`](Self::open) made, for a custom drain (see
    /// [`ExportWriterParts`]).
    pub fn into_parts(self) -> ExportWriterParts {
        ExportWriterParts {
            sink: self.sink,
            attachments_dir: self.attachments_dir,
            use_queue: self.use_queue,
            media_mode: self.media_mode,
            copy_attachments: self.copy_attachments,
            compress: self.compress,
            log: self.log,
            progress: self.progress,
        }
    }

    /// Write the collected documents: the queue arm drains
    /// [`ConversationUnit`]s (attachments before each conversation file, so
    /// an interrupted run can resume); the sink arm stages attachments and
    /// writes every document through the buffered sink.
    ///
    /// `source_for` is the per-exporter attachment hook. It is called once
    /// per attachment, in document order, and returns where that
    /// attachment's bytes come from plus a size hint for the progress
    /// totals. Exporters carrying bytes on the document move them out with
    /// `att.bytes.take()`; path-backed exporters return
    /// [`AttachmentSource::Path`] from their own source list.
    ///
    /// Folds conversation and attachment counts into `report`.
    ///
    /// # Errors
    ///
    /// Returns an error when a write fails, the staging disk is too small,
    /// or the user cancels.
    pub fn finish(
        self,
        documents: Vec<ConversationDocument>,
        source_for: &mut dyn FnMut(&mut IrAttachment) -> (AttachmentSource, Option<u64>),
        cancel: Option<&CancelFlag>,
        report: &mut ExportReport,
    ) -> Result<FormatSinkResult> {
        if self.use_queue {
            let units: Vec<ConversationUnit> = documents
                .into_iter()
                .map(|doc| ConversationUnit::from_doc(doc, |_, att| source_for(att)))
                .collect();
            let options = WriteQueueOptions {
                media: self.media_mode,
                compress: self.compress.clone(),
                resume: self.resume,
                writer_count: 0,
            };
            return drain_units(
                &self.output_dir,
                units,
                &options,
                self.log.as_ref(),
                self.progress.as_ref(),
                cancel,
                report,
            );
        }

        let mut documents = documents;
        // Gather sources in flat document order; staging loads by that index.
        let mut sources: Vec<AttachmentSource> = Vec::new();
        for doc in &mut documents {
            for msg in &mut doc.messages {
                for att in &mut msg.attachments {
                    let (source, _hint) = source_for(att);
                    sources.push(source);
                }
            }
        }
        report.attachments_saved += message_vault_io_core::stage_conversation_attachments(
            message_vault_io_core::document_messages(&mut documents),
            &self.attachments_dir,
            &message_vault_io_core::MediaConfig {
                mode: self.media_mode,
                compress: self.compress.clone(),
            },
            |i| match sources.get_mut(i) {
                Some(source) => load_attachment_source(source),
                None => Ok(None),
            },
            self.log.as_ref(),
            self.progress.as_ref(),
            cancel,
        )
        .map_err(anyhow::Error::msg)?;

        write_documents_through_sink(
            documents,
            self.sink,
            self.log.as_ref(),
            self.progress.as_ref(),
            cancel,
            report,
        )
    }
}
