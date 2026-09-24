//! Write conversations in one output format: one file per conversation
//! (JSON, JSON Lines, CSV, EML, MBOX), or one merged archive supplied by
//! the crate that owns that archive's format.

use crate::clean::clean_previous_ir_output;
use crate::export_transforms::apply_transforms;
use crate::write::write_format;
use anyhow::{Context, Result};
use message_ir::ConversationDocument;
use message_vault_io_core::{ExportReport, ExportTransforms, OutputFormat};
use std::fs;
use std::path::{Path, PathBuf};

/// A format that folds every conversation into one file with the attachment
/// bytes inside it. The crate that owns such a format implements this and
/// the caller that wants it hands it to [`FormatSink::with_archive`]; this
/// crate knows no archive format by name.
pub trait MergedArchive: std::fmt::Debug + Send {
    /// Write `documents` under `output_dir` as one file and return its path.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be written or an attachment
    /// it embeds cannot be read.
    fn write(&self, output_dir: &Path, documents: &[ConversationDocument]) -> Result<PathBuf>;
}

/// Writes conversations in the requested [`OutputFormat`], or through a
/// [`MergedArchive`] when the caller supplied one.
///
/// Documents are buffered until [`finish`](Self::finish), which applies
/// attachment media transforms and obfuscation, then projects all chats.
#[derive(Debug)]
pub struct FormatSink {
    output_dir: PathBuf,
    format: OutputFormat,
    transforms: ExportTransforms,
    archive: Option<Box<dyn MergedArchive>>,
    docs: Vec<ConversationDocument>,
}

impl FormatSink {
    /// Open a sink that buffers documents until [`finish`](Self::finish).
    ///
    /// # Errors
    ///
    /// Currently always succeeds. The `Result` matches the other constructors.
    pub fn open(
        output_dir: &Path,
        format: OutputFormat,
        transforms: ExportTransforms,
    ) -> Result<Self> {
        Ok(Self {
            output_dir: output_dir.to_path_buf(),
            format,
            transforms,
            archive: None,
            docs: Vec::new(),
        })
    }

    /// Write every buffered document through `archive` at
    /// [`finish`](Self::finish) instead of one file per conversation. The
    /// archive holds the attachment bytes, so the staged `attachments/` is
    /// removed once it is written.
    #[must_use]
    pub fn with_archive(mut self, archive: Box<dyn MergedArchive>) -> Self {
        self.archive = Some(archive);
        self
    }

    /// Prepare `output` for a fresh export, then open a sink into it.
    ///
    /// Creates the output directory, removes files from previous exports via
    /// [`clean_previous_ir_output`], creates `attachments/` when the
    /// transforms copy media, and returns the sink together with the
    /// attachments directory path (created or not).
    ///
    /// # Errors
    ///
    /// Returns an error when the directory cannot be created or previous
    /// export files cannot be removed.
    pub fn open_prepared(
        output: &Path,
        format: OutputFormat,
        transforms: ExportTransforms,
    ) -> Result<(Self, PathBuf)> {
        fs::create_dir_all(output).with_context(|| format!("create {}", output.display()))?;
        clean_previous_ir_output(output)?;
        let att_dir = output.join("attachments");
        if transforms.copies_attachments() {
            fs::create_dir_all(&att_dir)
                .with_context(|| format!("create {}", att_dir.display()))?;
        }
        let sink = Self::open(output, format, transforms)?;
        Ok((sink, att_dir))
    }

    /// Reopen `output` to continue an interrupted export.
    ///
    /// Unlike [`open_prepared`](Self::open_prepared), nothing is cleaned: the
    /// conversation files and staged attachments the interrupted run left
    /// behind are exactly the work a resumed run gets to skip. The directory
    /// must already be an export folder — it carries the sentinel — because
    /// resuming into anything else is a caller bug, not something to repair
    /// by cleaning.
    ///
    /// # Errors
    ///
    /// Returns an error when the directory or its sentinel is missing, or the
    /// attachments directory cannot be created.
    pub fn open_resume(
        output: &Path,
        format: OutputFormat,
        transforms: ExportTransforms,
    ) -> Result<(Self, PathBuf)> {
        if !output.join(crate::clean::EXPORT_SENTINEL).is_file() {
            anyhow::bail!(
                "cannot resume into {}: it is not a staging folder from a previous run",
                output.display()
            );
        }
        let att_dir = output.join("attachments");
        if transforms.copies_attachments() {
            fs::create_dir_all(&att_dir)
                .with_context(|| format!("create {}", att_dir.display()))?;
        }
        let sink = Self::open(output, format, transforms)?;
        Ok((sink, att_dir))
    }

    /// Output format this sink will write.
    pub fn format(&self) -> OutputFormat {
        self.format
    }

    /// Directory conversations are written into.
    pub fn output_dir(&self) -> &Path {
        &self.output_dir
    }

    /// Media and obfuscation settings applied at [`finish`](Self::finish).
    pub fn transforms(&self) -> &ExportTransforms {
        &self.transforms
    }

    /// Buffer one conversation until [`finish`](Self::finish).
    ///
    /// # Errors
    ///
    /// Currently always succeeds.
    pub fn write_document(&mut self, doc: ConversationDocument) -> Result<()> {
        self.docs.push(doc);
        Ok(())
    }

    /// Apply media and obfuscation transforms, then write all buffered documents.
    ///
    /// For EML, MBOX, and a merged archive, media is transformed then
    /// embedded. The staged `attachments/` directory is removed so the output
    /// folder holds only the archive.
    ///
    /// Folds the obfuscated-document count into `report`. Convert and
    /// compress ran in the staging step before documents reached the sink,
    /// so finish itself does no media work and reports none.
    ///
    /// # Errors
    ///
    /// Returns an error when a transform or a write fails, or the format is
    /// a merged archive and no [`MergedArchive`] was supplied.
    pub fn finish(mut self, report: &mut ExportReport) -> Result<()> {
        let embeds_media = self.format.is_mail_archive() || self.archive.is_some();
        let outcome = apply_transforms(&mut self.docs, &self.output_dir, &self.transforms)?;

        report.obfuscated_docs += outcome.obfuscated_docs as u64;

        if let Some(archive) = &self.archive {
            archive.write(&self.output_dir, &self.docs)?;
        } else {
            let mut docs: Vec<&mut ConversationDocument> = self.docs.iter_mut().collect();
            message_ir::give_each_document_its_own_file(&mut docs).map_err(anyhow::Error::msg)?;
            for doc in self.docs {
                write_format(&self.output_dir, self.format, doc)?;
            }
        }

        if embeds_media {
            remove_staged_attachments(&self.output_dir)?;
        }
        Ok(())
    }
}

/// Drop transform staging under `attachments/` after media has been embedded.
fn remove_staged_attachments(output_dir: &Path) -> Result<()> {
    let att_dir = output_dir.join("attachments");
    if att_dir.is_dir() {
        fs::remove_dir_all(&att_dir)?;
    }
    Ok(())
}

/// Write every document through the sink with the shared "Preparing N
/// conversation file(s)" log line and [`ProgressEvent::Prepare`] events,
/// bumping `report.conversations` per document, then finish the sink. The
/// shared tail of every exporter's non-queue arm.
pub fn write_documents_through_sink(
    documents: Vec<message_ir::ConversationDocument>,
    mut sink: FormatSink,
    log: Option<&message_vault_io_core::LogSink>,
    progress: Option<&message_vault_io_core::ProgressSink>,
    cancel: Option<&message_vault_io_core::CancelFlag>,
    report: &mut ExportReport,
) -> anyhow::Result<()> {
    use message_vault_io_core::{ProgressEvent, emit_log, emit_progress};
    let total = documents.len();
    emit_log(log, "");
    emit_log(log, format!("Preparing {total} conversation file(s)..."));
    emit_progress(progress, ProgressEvent::Prepare { done: 0, total });
    let mut written = 0usize;
    for doc in documents {
        message_vault_io_core::check_cancel(cancel)?;
        written += 1;
        sink.write_document(doc)?;
        report.conversations += 1;
        if written.is_multiple_of(100) || written == total {
            emit_log(log, format!("  preparing {written}/{total}"));
            emit_progress(
                progress,
                ProgressEvent::Prepare {
                    done: written,
                    total,
                },
            );
        }
    }
    sink.finish(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use media::MediaMode;
    use message_ir::IrAttachment;
    use std::fs;

    #[test]
    fn write_documents_through_sink_reports_prepare_progress() {
        use message_vault_io_core::{ProgressEvent, ProgressSink};
        use std::sync::{Arc, Mutex};

        let tmp = tempfile::tempdir().unwrap();
        let sink =
            FormatSink::open(tmp.path(), OutputFormat::Json, ExportTransforms::none()).unwrap();
        let seen = Arc::new(Mutex::new(Vec::<ProgressEvent>::new()));
        let sink_seen = Arc::clone(&seen);
        let progress = ProgressSink::new(move |event| sink_seen.lock().unwrap().push(event));
        let mut report = ExportReport::default();

        write_documents_through_sink(
            vec![message_ir::testutil::sample_document("hello")],
            sink,
            None,
            Some(&progress),
            None,
            &mut report,
        )
        .unwrap();

        assert_eq!(report.conversations, 1);
        assert_eq!(
            seen.lock().unwrap().as_slice(),
            [
                ProgressEvent::Prepare { done: 0, total: 1 },
                ProgressEvent::Prepare { done: 1, total: 1 },
            ]
        );
    }

    #[test]
    fn two_groups_with_one_title_are_both_written() {
        let tmp = tempfile::tempdir().unwrap();
        let mut sink =
            FormatSink::open(tmp.path(), OutputFormat::Json, ExportTransforms::none()).unwrap();
        for chat in ["chat1", "chat2"] {
            let mut doc = message_ir::testutil::sample_document(chat);
            doc.conversation.chat_identifier = chat.into();
            doc.conversation.conversation_type = message_ir::IrConversationType::Group;
            doc.conversation.group_title = Some("Family".into());
            sink.write_document(doc).unwrap();
        }

        sink.finish(&mut ExportReport::default()).unwrap();

        let written = fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
            .count();
        assert_eq!(written, 2, "neither group's file replaces the other's");
    }

    #[test]
    fn format_sink_csv_writes_per_conversation() {
        let tmp = tempfile::tempdir().unwrap();
        let mut sink =
            FormatSink::open(tmp.path(), OutputFormat::Csv, ExportTransforms::none()).unwrap();
        sink.write_document(message_ir::testutil::sample_document("hello"))
            .unwrap();
        sink.finish(&mut ExportReport::default()).unwrap();
        assert!(tmp.path().join("+15555550101.csv").is_file());
    }

    /// An archive the test owns: every document's chat id on one line each,
    /// which is enough to see that the sink handed over all of them at once
    /// and cleaned the staging folder afterwards.
    #[derive(Debug)]
    struct LineArchive;

    impl MergedArchive for LineArchive {
        fn write(&self, output_dir: &Path, documents: &[ConversationDocument]) -> Result<PathBuf> {
            let path = output_dir.join("all.txt");
            let body: Vec<String> = documents
                .iter()
                .map(|doc| doc.conversation.chat_identifier.clone())
                .collect();
            fs::write(&path, body.join("\n"))?;
            Ok(path)
        }
    }

    #[test]
    fn format_sink_writes_a_merged_archive_and_drops_the_staging_folder() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("attachments")).unwrap();
        let mut sink = FormatSink::open(tmp.path(), OutputFormat::Xml, ExportTransforms::none())
            .unwrap()
            .with_archive(Box::new(LineArchive));
        sink.write_document(message_ir::testutil::sample_document("one"))
            .unwrap();
        let mut doc2 = message_ir::testutil::sample_document("two");
        doc2.conversation.chat_identifier = "+15555550102".into();
        sink.write_document(doc2).unwrap();
        sink.finish(&mut ExportReport::default()).unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("all.txt")).unwrap(),
            "+15555550101\n+15555550102"
        );
        assert!(
            !tmp.path().join("attachments").exists(),
            "the archive holds the bytes, so the staging folder goes"
        );
    }

    #[test]
    fn a_merged_format_without_its_archive_is_refused_at_finish() {
        let tmp = tempfile::tempdir().unwrap();
        let mut sink =
            FormatSink::open(tmp.path(), OutputFormat::Xml, ExportTransforms::none()).unwrap();
        sink.write_document(message_ir::testutil::sample_document("one"))
            .unwrap();
        let err = sink
            .finish(&mut ExportReport::default())
            .unwrap_err()
            .to_string();
        assert!(err.contains("merged archive"), "{err}");
    }

    /// A mail archive holds the attachment bytes itself, and the sink then
    /// deletes the staged `attachments/`. The bytes must have been read from
    /// the staged file first, or the attachment is lost from both.
    #[test]
    fn mail_archives_embed_the_staged_bytes_and_drop_attachments_dir() {
        for format in [OutputFormat::Eml, OutputFormat::Mbox] {
            let tmp = tempfile::tempdir().unwrap();
            let att_dir = tmp.path().join("attachments");
            fs::create_dir_all(&att_dir).unwrap();
            let rel = "attachments/photo.jpg";
            fs::write(tmp.path().join(rel), b"\xff\xd8\xffjpeg-bytes").unwrap();

            let mut doc = message_ir::testutil::sample_document("with media");
            doc.messages[0].attachments = vec![IrAttachment {
                path: Some(rel.into()),
                original_name: Some("photo.jpg".into()),
                mime_type: Some("image/jpeg".into()),
                digest_sha256: None,
                is_sticker: false,
                transcription: None,
                sticker_effect: None,
                size_bytes: None,
                missing_reason: None,
                bytes: None,
            }];

            let transforms = ExportTransforms {
                media: MediaMode::Clone,
                ..ExportTransforms::none()
            };
            let mut sink = FormatSink::open(tmp.path(), format, transforms).unwrap();
            sink.write_document(doc).unwrap();
            sink.finish(&mut ExportReport::default()).unwrap();

            assert!(!att_dir.exists(), "{format:?}");
            let back = match format {
                OutputFormat::Eml => {
                    crate::read_conversation_eml_dir(&tmp.path().join("+15555550101"))
                }
                _ => crate::read_conversation_mbox(&tmp.path().join("+15555550101.mbox")),
            }
            .unwrap();
            let attachments = &back.messages[0].attachments;
            assert_eq!(attachments.len(), 1, "{format:?}");
            assert_eq!(
                attachments[0].bytes.as_deref(),
                Some(&b"\xff\xd8\xffjpeg-bytes"[..]),
                "{format:?}"
            );
        }
    }

    #[test]
    fn format_sink_csv_keeps_attachments_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let att_dir = tmp.path().join("attachments");
        fs::create_dir_all(&att_dir).unwrap();
        fs::write(att_dir.join("photo.jpg"), b"jpeg-bytes").unwrap();

        let mut sink =
            FormatSink::open(tmp.path(), OutputFormat::Csv, ExportTransforms::none()).unwrap();
        sink.write_document(message_ir::testutil::sample_document("hello"))
            .unwrap();
        sink.finish(&mut ExportReport::default()).unwrap();
        assert!(tmp.path().join("attachments/photo.jpg").is_file());
    }

    #[test]
    fn format_sink_obfuscate_rewrites_handles() {
        let tmp = tempfile::tempdir().unwrap();
        let transforms = ExportTransforms {
            obfuscate: true,
            obfuscate_seed: Some(
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
            ),
            ..ExportTransforms::none()
        };
        let mut sink = FormatSink::open(tmp.path(), OutputFormat::Csv, transforms).unwrap();
        // The fixture's `source` bag carries the raw sender address; obfuscation
        // must drop it, so the bag stays in and the assertion below covers it.
        let doc = message_ir::testutil::sample_document("secret");
        assert!(
            doc.messages[0].source.is_some(),
            "fixture should carry a vendor bag"
        );
        sink.write_document(doc).unwrap();
        let mut report = ExportReport::default();
        sink.finish(&mut report).unwrap();
        assert_eq!(report.obfuscated_docs, 1);
        let mut found = false;
        for entry in fs::read_dir(tmp.path()).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) == Some("csv") {
                let body = fs::read_to_string(&path).unwrap();
                assert!(!body.contains("+15555550101"));
                assert!(!body.contains("secret"));
                found = true;
            }
        }
        assert!(found, "expected obfuscated csv");
    }
    #[test]
    fn open_resume_keeps_previous_output_and_requires_the_sentinel() {
        let tmp = tempfile::tempdir().unwrap();
        // A directory that was never an export folder is refused: resuming
        // into one is a caller bug, not something to repair by cleaning.
        assert!(
            FormatSink::open_resume(tmp.path(), OutputFormat::Jsonl, ExportTransforms::none())
                .is_err()
        );

        let (_sink, att_dir) =
            FormatSink::open_prepared(tmp.path(), OutputFormat::Jsonl, ExportTransforms::none())
                .unwrap();
        std::fs::create_dir_all(&att_dir).unwrap();
        std::fs::write(tmp.path().join("keep.jsonl"), "x").unwrap();
        std::fs::write(att_dir.join("keep.jpg"), "y").unwrap();

        let (_sink, att_dir2) =
            FormatSink::open_resume(tmp.path(), OutputFormat::Jsonl, ExportTransforms::none())
                .unwrap();

        assert!(
            tmp.path().join("keep.jsonl").is_file(),
            "a resumed run keeps the conversations the interrupted one wrote"
        );
        assert!(
            att_dir2.join("keep.jpg").is_file(),
            "and the attachments they point at"
        );
    }
}
