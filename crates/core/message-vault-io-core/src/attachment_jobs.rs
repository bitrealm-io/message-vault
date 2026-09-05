//! Copy, convert, or skip attachment files after parse.

use crate::attachments::attachment_dest_name;
use crate::config::MediaConfig;
use crate::pipeline::ExportReport;
use crate::process::{CancelFlag, LogSink, emit_log};
use crate::progress::{ProgressEvent, ProgressSink, emit_progress};
use media::MediaMode;
use message_ir::{ConversationDocument, IrAttachment};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// File and byte counts emitted after each attachment job (and once for skip).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttachmentProgress {
    /// Jobs finished so far.
    pub done: usize,
    /// Job count from parse. Does not change.
    pub total: usize,
    /// Bytes written (or measured) so far.
    pub bytes_done: u64,
    /// Known or measured byte total. Grows when a file had no `size_hint`.
    pub bytes_total: u64,
}

/// One attachment to stage, pointing at the in-memory IR row.
pub struct AttachmentJob<'a> {
    /// Conversation attachment to fill after the write.
    pub attachment: &'a mut IrAttachment,
    /// Message timestamp in milliseconds (used for the dest date prefix).
    pub timestamp_unix_ms: i64,
    /// Size from the backup, if known.
    pub size_hint: Option<u64>,
}

/// Load, write, and optionally convert each attachment.
///
/// `load(i)` returns `Ok(None)` when the source is missing. `Ok(Some(bytes))`
/// is the file to stage. An `Err` from `load(i)` other than `"canceled"` is
/// caught here and treated the same as a missing source: the attachment gets
/// `missing_reason = "file_missing"` and the run continues rather than
/// aborting. Cancel is checked before each job.
///
/// # Errors
///
/// Returns `"canceled"` when the flag is set before a job starts, or when
/// `load(i)` itself returns `"canceled"`. Returns an I/O or convert error
/// string when the staging directory cannot be used.
pub fn run_attachment_jobs(
    jobs: &mut [AttachmentJob<'_>],
    attachments_dir: &Path,
    media: &MediaConfig,
    mut load: impl FnMut(usize) -> Result<Option<Vec<u8>>, String>,
    mut on_progress: impl FnMut(AttachmentProgress),
    log: Option<&LogSink>,
    cancel: Option<&AtomicBool>,
) -> Result<(), String> {
    let total = jobs.len();
    if total == 0 {
        on_progress(AttachmentProgress {
            done: 0,
            total: 0,
            bytes_done: 0,
            bytes_total: 0,
        });
        return Ok(());
    }
    if matches!(media.mode, MediaMode::Disabled) {
        for job in jobs.iter_mut() {
            job.attachment.missing_reason = Some("not_copied".into());
        }
        on_progress(AttachmentProgress {
            done: total,
            total,
            bytes_done: 0,
            bytes_total: 0,
        });
        return Ok(());
    }

    let mut bytes_total: u64 = jobs.iter().filter_map(|job| job.size_hint).sum();
    let mut bytes_done = 0_u64;

    fs::create_dir_all(attachments_dir)
        .map_err(|e| format!("create {}: {e}", attachments_dir.display()))?;

    for (i, job) in jobs.iter_mut().enumerate() {
        if cancel.is_some_and(|flag| flag.load(Ordering::SeqCst)) {
            return Err("canceled".into());
        }

        let loaded = match load(i) {
            Ok(loaded) => loaded,
            // A cancel raised inside the loader still stops the run.
            Err(err) if err == "canceled" => return Err(err),
            // One unreadable source is that attachment's problem, not the
            // run's. Fall through to the missing-file handling below.
            Err(_) => None,
        };
        let bytes = match loaded {
            Some(bytes) if !bytes.is_empty() => bytes,
            _ => {
                job.attachment.missing_reason = Some("file_missing".into());
                on_progress(AttachmentProgress {
                    done: i + 1,
                    total,
                    bytes_done,
                    bytes_total,
                });
                continue;
            }
        };

        if job.size_hint.is_none() {
            bytes_total += bytes.len() as u64;
        }

        persist_clone(job, attachments_dir, &bytes)?;
        bytes_done += bytes.len() as u64;
        on_progress(AttachmentProgress {
            done: i + 1,
            total,
            bytes_done,
            bytes_total,
        });
    }

    if matches!(media.mode, MediaMode::Convert | MediaMode::Compress) {
        if cancel.is_some_and(|flag| flag.load(Ordering::SeqCst)) {
            return Err("canceled".into());
        }
        apply_convert_or_compress(jobs, attachments_dir, media, log)?;
        on_progress(AttachmentProgress {
            done: total,
            total,
            bytes_done,
            bytes_total,
        });
    }

    Ok(())
}

/// Write queued attachment bytes after parse and before conversation files.
///
/// The shared non-queue staging step every exporter used to copy: assemble
/// one [`AttachmentJob`] per attachment across `documents` (in document
/// order), run [`run_attachment_jobs`] with the standard progress report
/// (a log line for people and an [`ProgressEvent::Attachments`] for the
/// progress bar), count staged files into `report.attachments_saved`, and
/// clear any in-memory `bytes` left on the attachments.
///
/// `load(i)` is the per-exporter payload hook: `i` is the flat attachment
/// index in document order. `Ok(None)` (or a non-cancel `Err`) marks that
/// attachment `file_missing` and the run continues.
///
/// Size hints for the progress totals come from each attachment's
/// `size_bytes` (falling back to in-memory `bytes` length when present);
/// path-backed exporters whose attachments carry no size get unhinted totals
/// that grow as files load.
///
/// # Errors
///
/// Returns `"canceled"` when the user cancels, or an I/O / convert error
/// string when the staging directory cannot be used.
// Every argument is one of the run's hooks or one of its inputs; folding
// them into a struct would only move the same eight names one level down.
#[allow(clippy::too_many_arguments)]
pub fn stage_conversation_attachments(
    documents: &mut [ConversationDocument],
    attachments_dir: &Path,
    media: &MediaConfig,
    load: impl FnMut(usize) -> Result<Option<Vec<u8>>, String>,
    log: Option<&LogSink>,
    progress: Option<&ProgressSink>,
    cancel: Option<&CancelFlag>,
    report: &mut ExportReport,
) -> Result<(), String> {
    let mut jobs = attachment_jobs(documents);
    run_attachment_jobs(
        &mut jobs,
        attachments_dir,
        media,
        load,
        report_attachment_progress(log, progress),
        log,
        cancel.map(|flag| flag.as_ref()),
    )?;

    for job in &jobs {
        if job.attachment.path.is_some() && job.attachment.digest_sha256.is_some() {
            report.attachments_saved += 1;
        }
    }
    drop(jobs);
    clear_attachment_bytes(documents);
    Ok(())
}

/// The size to report for an attachment before its bytes are read: the
/// record's own size, else the length of the bytes held in memory, else
/// unknown (the progress total grows once the file loads).
pub fn attachment_size_hint(att: &IrAttachment) -> Option<u64> {
    att.size_bytes
        .or_else(|| att.bytes.as_ref().map(|b| b.len() as u64))
}

/// One job per attachment across every document, in document order. The
/// position in the result is the flat attachment index a `load(i)` hook
/// receives.
pub fn attachment_jobs(documents: &mut [ConversationDocument]) -> Vec<AttachmentJob<'_>> {
    let mut jobs = Vec::new();
    for doc in documents.iter_mut() {
        for msg in &mut doc.messages {
            let ts = msg.timestamp_unix_ms;
            for att in &mut msg.attachments {
                let hint = attachment_size_hint(att);
                jobs.push(AttachmentJob {
                    attachment: att,
                    timestamp_unix_ms: ts,
                    size_hint: hint,
                });
            }
        }
    }
    jobs
}

/// The progress report every attachment run makes: a log line (files done
/// of total, bytes done of total) for people, and a typed
/// [`ProgressEvent::Attachments`] for the progress bar.
pub fn report_attachment_progress<'a>(
    log: Option<&'a LogSink>,
    progress: Option<&'a ProgressSink>,
) -> impl FnMut(AttachmentProgress) + 'a {
    move |counts| {
        emit_log(
            log,
            format!(
                "  attachments {}/{} {}/{}",
                counts.done, counts.total, counts.bytes_done, counts.bytes_total
            ),
        );
        emit_progress(progress, ProgressEvent::from(counts));
    }
}

/// Drop the bytes held in memory on every attachment, once they have been
/// written or are no longer wanted.
pub fn clear_attachment_bytes(documents: &mut [ConversationDocument]) {
    for doc in documents.iter_mut() {
        for msg in &mut doc.messages {
            for att in &mut msg.attachments {
                att.bytes = None;
            }
        }
    }
}

/// Monotonic counter distinguishing concurrent temp files.
///
/// The final name is content-addressed, so two workers staging identical
/// bytes produce the same `dest` — that is fine, the second rename is a
/// no-op overwrite of identical bytes — but they must not share a temp path
/// mid-write, or one worker's rename pulls the file out from under the
/// other's still-open write.
static CLONE_TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

fn next_clone_temp_name(name: &str) -> String {
    let seq = CLONE_TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{name}.{seq}.tmp")
}

fn persist_clone(
    job: &mut AttachmentJob<'_>,
    attachments_dir: &Path,
    bytes: &[u8],
) -> Result<(), String> {
    let digest_hex = hex_sha256(bytes);
    let ext = extension_from_name(job.attachment.original_name.as_deref());
    let secs = job.timestamp_unix_ms.div_euclid(1000);
    let name = attachment_dest_name(secs, &digest_hex, &ext);
    let dest = attachments_dir.join(&name);
    let tmp = attachments_dir.join(next_clone_temp_name(&name));
    fs::write(&tmp, bytes).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, &dest).map_err(|e| format!("rename {}: {e}", dest.display()))?;
    job.attachment.path = Some(format!("attachments/{name}"));
    job.attachment.digest_sha256 = Some(digest_hex);
    job.attachment.size_bytes = Some(bytes.len() as u64);
    job.attachment.missing_reason = None;
    Ok(())
}

fn apply_convert_or_compress(
    jobs: &mut [AttachmentJob<'_>],
    attachments_dir: &Path,
    media: &MediaConfig,
    log: Option<&LogSink>,
) -> Result<(), String> {
    let Some(output_dir) = attachments_dir.parent() else {
        return Err("attachments directory has no parent".into());
    };
    let files = media::collect_media_files(attachments_dir).map_err(|e| e.to_string())?;
    let mut emit = |line: &str| emit_log(log, line);
    let (report, remap) = media::process_attachment_files(
        output_dir,
        &files,
        media.mode,
        &media.compress,
        Some(&mut emit),
    )
    .map_err(|e| e.to_string())?;
    apply_remap_to_jobs(jobs, &remap, output_dir);
    for err in &report.errors {
        mark_convert_error(jobs, err);
    }
    Ok(())
}

fn apply_remap_to_jobs(
    jobs: &mut [AttachmentJob<'_>],
    remap: &std::collections::HashMap<String, String>,
    output_dir: &Path,
) {
    for job in jobs.iter_mut() {
        let Some(path) = job.attachment.path.as_mut() else {
            continue;
        };
        if let Some(new_rel) = remap.get(path.as_str()) {
            *path = new_rel.clone();
            if let Some(mime) = mime_for_rel(new_rel) {
                job.attachment.mime_type = Some(mime);
            }
            if refresh_digest_and_size(job.attachment, output_dir).is_err() {
                job.attachment.missing_reason = Some("file_missing".into());
            }
        }
    }
}

fn mark_convert_error(jobs: &mut [AttachmentJob<'_>], err: &str) {
    let Some((path, reason)) = err.split_once(": ") else {
        return;
    };
    for job in jobs.iter_mut() {
        let Some(rel) = job.attachment.path.as_deref() else {
            continue;
        };
        let native = rel.replace('/', std::path::MAIN_SEPARATOR_STR);
        if path.ends_with(rel) || path.ends_with(native.as_str()) {
            job.attachment.missing_reason = Some(format!("convert_failed: {reason}"));
        }
    }
}

/// MIME type inferred from a `attachments/…` relative path's extension.
///
/// Thin wrapper over [`media::mime_for_ext`] — the one shared
/// extension-to-mime table — kept because many pipeline callers hand paths
/// rather than extensions. `None` for unrecognized extensions.
pub fn mime_for_rel(rel: &str) -> Option<String> {
    let ext = Path::new(rel).extension().and_then(|e| e.to_str())?;
    media::mime_for_ext(ext).map(str::to_string)
}

fn refresh_digest_and_size(attachment: &mut IrAttachment, output_dir: &Path) -> Result<(), String> {
    let Some(rel) = attachment.path.as_deref() else {
        return Ok(());
    };
    let dest = output_dir.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    let bytes = fs::read(&dest).map_err(|e| format!("read {}: {e}", dest.display()))?;
    attachment.digest_sha256 = Some(hex_sha256(&bytes));
    attachment.size_bytes = Some(bytes.len() as u64);
    Ok(())
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn extension_from_name(original_name: Option<&str>) -> String {
    original_name
        .and_then(|name| Path::new(name).extension())
        .and_then(|ext| ext.to_str())
        .map(|ext| format!(".{ext}"))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
