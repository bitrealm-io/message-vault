//! Read SMS Backup & Restore XML into the shared conversation structure, then
//! write the chosen output format via [`ExportWriter`].

use anyhow::Result;
use message_ir_format::{
    AttachmentSource, ExportTransforms, ExportWriter, FormatSinkResult, SbrArchive, SbrReadOptions,
    SbrReadReport, read_sbr_documents,
};
use message_vault_io_core::{CancelFlag, ExportReport, OutputFormat};
use std::path::Path;

/// Map the ir-format read report onto the shared [`ExportReport`] shape,
/// moving reader-specific counters into `extra`.
fn to_core_report(report: SbrReadReport) -> ExportReport {
    let mut out = ExportReport {
        conversations: report.conversations,
        sent: report.sent,
        received: report.received,
        attachments_saved: report.attachments_saved,
        skipped_invalid_date: report.skipped_invalid_date,
        skipped_out_of_range: report.skipped_out_of_range,
        errors: report.errors,
        ..ExportReport::default()
    };
    out.extra.insert("sms_seen".into(), report.sms_seen);
    out.extra.insert("mms_seen".into(), report.mms_seen);
    out.extra.insert(
        "skipped_unknown_address".into(),
        report.skipped_unknown_address,
    );
    out.extra
        .insert("skipped_unknown_type".into(), report.skipped_unknown_type);
    out.extra.insert(
        "skipped_draft_or_outbox".into(),
        report.skipped_draft_or_outbox,
    );
    out.extra.insert(
        "skipped_empty_participants".into(),
        report.skipped_empty_participants,
    );
    out.extra.insert(
        "skipped_bad_attachment".into(),
        report.skipped_bad_attachment,
    );
    out
}

/// Inputs for [`convert_export`].
pub(crate) struct ConvertExportArgs<'a> {
    pub input: &'a Path,
    pub output_dir: &'a Path,
    pub owner_phones: &'a [String],
    pub transforms: ExportTransforms,
    pub output_format: OutputFormat,
    pub cancel: Option<&'a CancelFlag>,
    /// Continue an interrupted export: keep previous output and skip the
    /// conversations already written.
    pub resume: bool,
}

/// Convert SMS Backup & Restore XML into the shared conversation structure,
/// then write the chosen output format.
///
/// # Errors
///
/// Returns an error when the XML cannot be read, a conversation cannot be
/// written, or the user cancels.
pub(crate) fn convert_export(
    args: ConvertExportArgs<'_>,
) -> Result<(ExportReport, FormatSinkResult)> {
    // The read options still need the compress settings after `transforms`
    // moves into the writer.
    let compress = args.transforms.compress.clone();
    let mut writer = ExportWriter::open(
        args.output_dir,
        args.output_format,
        args.transforms,
        args.resume,
    )?;
    if args.output_format == OutputFormat::Xml {
        // This crate owns the backup format, so a round trip back to
        // `smses.xml` goes through its own archive writer.
        writer = writer.with_archive(Box::new(SbrArchive));
    }
    let (documents, report) = read_sbr_documents(
        args.input,
        SbrReadOptions {
            owner_phones: args.owner_phones,
            attachments_dir: Some(writer.attachments_dir()),
            copy_attachments: writer.copies_attachments(),
            // The bytes ride into the shared write tail, which stages them
            // itself (a conversation at a time on the queue arm).
            stage_attachments: false,
            media: writer.media_mode(),
            compress,
            log: writer.log(),
            progress: writer.progress(),
            cancel: args.cancel,
        },
    )?;

    // The reader already counted conversations (and staged nothing, so its
    // attachments_saved is zero); zero the conversation counter so the shared
    // write tail's fold counts only the documents it actually writes.
    let mut core = to_core_report(report);
    core.conversations = 0;
    let sink_result = writer.finish(
        documents,
        &mut AttachmentSource::take_bytes,
        args.cancel,
        &mut core,
    )?;
    Ok((core, sink_result))
}
