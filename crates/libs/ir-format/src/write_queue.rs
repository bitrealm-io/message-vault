//! Drain a queue of conversations onto disk, one conversation at a time.
//!
//! Parse finishes before anything is written: every exporter buffers its
//! documents and collects attachment sources in one pass, then hands the
//! result here as a queue of [`ConversationUnit`]s. A worker writes a unit's
//! attachments first and its conversation file last, so a conversation file
//! on disk means everything it references is on disk too. That invariant is
//! what makes an interrupted write resumable: a resumed run skips any unit
//! whose conversation file it already finds.
//!
//! Writers never transcode. Convert and compress stage the originals here and
//! run afterwards as their own resumable pass.
//!
//! Only non-obfuscated JSONL exports are routed here. Obfuscation is stateful
//! across documents and the other formats merge or embed at finish, so those
//! keep the `FormatSink` path.

use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use anyhow::{Context, Result};
use media::{CompressOptions, MediaMode};
use message_ir::{ConversationDocument, IrAttachment};
use message_vault_io_core::{
    AttachmentJob, CancelFlag, LogSink, MediaConfig, OutputFormat, ProgressEvent, ProgressSink,
    attachment_size_hint, emit_log, emit_progress, run_attachment_jobs,
};

use crate::transcode::{TranscodeOptions, transcode_staged};
use crate::write::write_format;

/// Where a unit's attachment bytes come from at write time.
#[derive(Debug, Default)]
pub enum AttachmentSource {
    /// Read this file when the attachment is written. Worker-safe: a plain
    /// `fs::read`, no shared handle.
    Path(PathBuf),
    /// Bytes the exporter already holds (SBR blobs, handwriting SVG).
    Bytes(Vec<u8>),
    /// Nothing to read; the attachment becomes `file_missing` under a mode
    /// that copies files.
    #[default]
    Missing,
}

impl AttachmentSource {
    /// The attachment's in-memory bytes as its source, taken out of the
    /// record, with the size to report; `Missing` when the exporter held
    /// none. The `source_for` hook of every exporter whose attachments
    /// arrive as bytes rather than files.
    pub fn take_bytes(att: &mut IrAttachment) -> (Self, Option<u64>) {
        let hint = attachment_size_hint(att);
        match att.bytes.take() {
            Some(bytes) => (Self::Bytes(bytes), hint),
            None => (Self::Missing, hint),
        }
    }
}

/// One attachment of a unit, pinned to its place in the document.
#[derive(Debug)]
pub struct UnitAttachment {
    /// Index into `doc.messages`.
    pub message_index: usize,
    /// Index into that message's `attachments`.
    pub attachment_index: usize,
    /// Where the bytes come from.
    pub source: AttachmentSource,
    /// Message timestamp, which dates the staged filename.
    pub timestamp_unix_ms: i64,
    /// Size from the backup when known; byte totals grow as unhinted files load.
    pub size_hint: Option<u64>,
}

/// One conversation and everything it references: the queue's unit of work.
#[derive(Debug)]
pub struct ConversationUnit {
    /// The conversation to write.
    pub doc: ConversationDocument,
    /// Its attachments, in message order.
    pub attachments: Vec<UnitAttachment>,
}

impl ConversationUnit {
    /// Pair every attachment in `doc` with a source and a size hint.
    ///
    /// The closure sees each attachment in message order and receives it as
    /// `&mut`, so an exporter carrying bytes on the document can move them
    /// out with `att.bytes.take()` instead of copying them.
    pub fn from_doc(
        mut doc: ConversationDocument,
        mut source_for: impl FnMut(
            usize,
            &mut message_ir::IrAttachment,
        ) -> (AttachmentSource, Option<u64>),
    ) -> Self {
        let mut attachments = Vec::new();
        let mut flat = 0usize;
        for (message_index, msg) in doc.messages.iter_mut().enumerate() {
            let timestamp_unix_ms = msg.timestamp_unix_ms;
            for (attachment_index, att) in msg.attachments.iter_mut().enumerate() {
                let (source, size_hint) = source_for(flat, att);
                attachments.push(UnitAttachment {
                    message_index,
                    attachment_index,
                    source,
                    timestamp_unix_ms,
                    size_hint,
                });
                flat += 1;
            }
        }
        Self { doc, attachments }
    }
}

/// How a drain stages files.
#[derive(Debug, Clone)]
pub struct WriteQueueOptions {
    /// The mode the user asked for. Convert and compress stage originals here
    /// and transcode afterwards — writers do not transcode.
    pub media: MediaMode,
    /// Compress settings, used by the post-pass.
    pub compress: CompressOptions,
    /// Skip units whose conversation file is already on disk.
    pub resume: bool,
    /// 0 picks a count from the machine. The sequential drain ignores it.
    pub writer_count: usize,
}

/// What a drain did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WriteQueueReport {
    /// Conversation files written by this run.
    pub conversations_written: usize,
    /// Conversation files a resumed run found already written.
    pub conversations_skipped: usize,
    /// Attachment records staged with a path and a digest, duplicates included.
    pub attachments_saved: usize,
    /// Filled by the convert/compress post-pass; default otherwise.
    pub media: media::MediaReport,
}

/// Read one attachment source.
///
/// `Bytes` are moved out of the source rather than copied — every source is
/// loaded at most once, so taking them is safe and spares a full copy of the
/// payload. `Missing` reads as an absent file, which the staging step turns
/// into `file_missing`.
///
/// # Errors
///
/// Returns the read error when a `Path` source cannot be read.
pub fn load_attachment_source(source: &mut AttachmentSource) -> Result<Option<Vec<u8>>, String> {
    match source {
        AttachmentSource::Path(path) => fs::read(&*path)
            .map(Some)
            .map_err(|e| format!("read {}: {e}", path.display())),
        AttachmentSource::Bytes(bytes) => Ok(Some(std::mem::take(bytes))),
        AttachmentSource::Missing => Ok(None),
    }
}

/// What one attachment added to the drain's totals.
///
/// Deltas, not running counts: a parallel drain folds them into shared
/// atomics, and a sequential one adds them to plain locals. Either way the
/// per-unit body does not need to know the global picture.
struct UnitProgress {
    done: usize,
    bytes_done: u64,
    bytes_total: u64,
}

/// What one unit did. Byte and file counts travel through the progress
/// callback instead, so both drains can fold them their own way.
struct UnitOutcome {
    written: bool,
    attachments_saved: usize,
}

/// Loads one attachment's bytes by source; `Ok(None)` marks it missing.
pub type AttachmentLoader<'a> =
    dyn FnMut(&mut AttachmentSource) -> Result<Option<Vec<u8>>, String> + 'a;

/// Drain `units` with a caller-supplied loader.
///
/// Exporters whose attachment loader cannot cross threads — an encrypted iOS
/// backup holds a SQLite connection that is not `Sync` — use this and get one
/// writer. Everyone else wants `drain_write_queue`.
///
/// # Errors
///
/// Returns the first unit error, which stops the drain. A cancel surfaces as
/// `"canceled"`.
pub fn drain_write_queue_with_loader(
    output_dir: &Path,
    units: Vec<ConversationUnit>,
    options: &WriteQueueOptions,
    load: &mut AttachmentLoader<'_>,
    log: Option<&LogSink>,
    progress: Option<&ProgressSink>,
    cancel: Option<&CancelFlag>,
) -> Result<WriteQueueReport> {
    check_headroom(output_dir, &units)?;
    let attachments_dir = output_dir.join("attachments");
    let mut report = WriteQueueReport::default();

    let unit_count = units.len();
    let total: usize = units.iter().map(|u| u.attachments.len()).sum();
    let bytes_total_base: u64 = units
        .iter()
        .flat_map(|u| u.attachments.iter())
        .filter_map(|a| a.size_hint)
        .sum();

    announce_start(log, progress, unit_count);

    let done = Cell::new(0usize);
    let bytes_done = Cell::new(0u64);
    let bytes_total = Cell::new(bytes_total_base);
    let report_progress = |p: UnitProgress| {
        done.set(done.get() + p.done);
        bytes_done.set(bytes_done.get() + p.bytes_done);
        bytes_total.set(bytes_total.get() + p.bytes_total);
        report_attachments(
            log,
            progress,
            done.get(),
            total,
            bytes_done.get(),
            bytes_total.get(),
        );
    };

    for unit in units {
        let outcome = write_one_unit(
            output_dir,
            &attachments_dir,
            unit,
            options,
            load,
            &report_progress,
            cancel,
        )?;
        report.attachments_saved += outcome.attachments_saved;
        if outcome.written {
            report.conversations_written += 1;
        } else {
            report.conversations_skipped += 1;
        }
        emit_progress(
            progress,
            ProgressEvent::Prepare {
                done: report.conversations_written + report.conversations_skipped,
                total: unit_count,
            },
        );
    }

    report.media = run_media_post_pass(output_dir, options, log, progress, cancel)?;
    announce_finish(log, &report, options.resume);
    Ok(report)
}

/// Writers scale with the machine: writing is IO and hashing, and past a
/// handful of threads the disk, not the CPU, sets the pace.
pub fn default_writer_count() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, 8)
}

/// Drain `units` through the write queue, fold the written/skipped counts
/// into `report`, and return the `FormatSinkResult` the sink path would
/// have produced. The shared tail of every exporter's queue arm.
pub fn drain_units(
    output_dir: &Path,
    units: Vec<ConversationUnit>,
    options: &WriteQueueOptions,
    log: Option<&LogSink>,
    progress: Option<&ProgressSink>,
    cancel: Option<&CancelFlag>,
    report: &mut message_vault_io_core::ExportReport,
) -> Result<crate::FormatSinkResult> {
    let queue_report = drain_write_queue(output_dir, units, options, log, progress, cancel)?;
    report.conversations +=
        (queue_report.conversations_written + queue_report.conversations_skipped) as u64;
    report.attachments_saved += queue_report.attachments_saved as u64;
    Ok(crate::FormatSinkResult {
        xml_path: None,
        media: queue_report.media,
        obfuscated_docs: 0,
    })
}

/// Drain `units` across a pool of writer threads.
///
/// Each worker pops the next conversation, stages its attachments, and writes
/// its conversation file. The first error stops the pool and is what the
/// caller sees.
///
/// # Errors
///
/// Returns the first unit error, or the headroom error when the staging disk
/// cannot hold what the backup needs. A cancel surfaces as `"canceled"`.
pub fn drain_write_queue(
    output_dir: &Path,
    units: Vec<ConversationUnit>,
    options: &WriteQueueOptions,
    log: Option<&LogSink>,
    progress: Option<&ProgressSink>,
    cancel: Option<&CancelFlag>,
) -> Result<WriteQueueReport> {
    check_headroom(output_dir, &units)?;

    let attachments_dir = output_dir.join("attachments");
    // Idempotent, but doing it once here keeps every worker's first write
    // from racing the same create.
    fs::create_dir_all(&attachments_dir)
        .with_context(|| format!("create {}", attachments_dir.display()))?;

    let unit_count = units.len();
    let total: usize = units.iter().map(|u| u.attachments.len()).sum();
    let bytes_total_base: u64 = units
        .iter()
        .flat_map(|u| u.attachments.iter())
        .filter_map(|a| a.size_hint)
        .sum();

    announce_start(log, progress, unit_count);

    let done = AtomicUsize::new(0);
    let bytes_done = AtomicU64::new(0);
    let bytes_total = AtomicU64::new(bytes_total_base);
    let attachments_saved = AtomicUsize::new(0);
    let written = AtomicUsize::new(0);
    let skipped = AtomicUsize::new(0);
    let units_done = AtomicUsize::new(0);
    let abort = AtomicBool::new(false);
    let first_error: Mutex<Option<String>> = Mutex::new(None);
    let queue: Mutex<VecDeque<ConversationUnit>> = Mutex::new(VecDeque::from(units));

    let report_progress = |p: UnitProgress| {
        let d = done.fetch_add(p.done, Ordering::Relaxed) + p.done;
        let bd = bytes_done.fetch_add(p.bytes_done, Ordering::Relaxed) + p.bytes_done;
        let bt = bytes_total.fetch_add(p.bytes_total, Ordering::Relaxed) + p.bytes_total;
        report_attachments(log, progress, d, total, bd, bt);
    };

    let writer_count = if options.writer_count == 0 {
        default_writer_count()
    } else {
        options.writer_count
    }
    .min(unit_count.max(1));

    std::thread::scope(|scope| {
        for _ in 0..writer_count {
            scope.spawn(|| {
                loop {
                    if abort.load(Ordering::SeqCst) {
                        return;
                    }
                    let Some(unit) = queue.lock().expect("write queue lock").pop_front() else {
                        return;
                    };
                    let mut load = |source: &mut AttachmentSource| {
                        // Name the file before the failure turns into a chip:
                        // otherwise a systemic problem (a revoked permission, a
                        // failing disk) reads as a run's worth of unexplained
                        // missing attachments.
                        let named = match source {
                            AttachmentSource::Path(path) => Some(path.display().to_string()),
                            _ => None,
                        };
                        load_attachment_source(source).map_err(|e| {
                            if let Some(path) = named {
                                emit_log(
                                    log,
                                    format!("warning: attachment {path} could not be read: {e}"),
                                );
                            }
                            e
                        })
                    };
                    match write_one_unit(
                        output_dir,
                        &attachments_dir,
                        unit,
                        options,
                        &mut load,
                        &report_progress,
                        cancel,
                    ) {
                        Ok(outcome) => {
                            attachments_saved
                                .fetch_add(outcome.attachments_saved, Ordering::Relaxed);
                            if outcome.written {
                                written.fetch_add(1, Ordering::Relaxed);
                            } else {
                                skipped.fetch_add(1, Ordering::Relaxed);
                            }
                            let finished = units_done.fetch_add(1, Ordering::Relaxed) + 1;
                            emit_progress(
                                progress,
                                ProgressEvent::Prepare {
                                    done: finished,
                                    total: unit_count,
                                },
                            );
                        }
                        Err(err) => {
                            let mut slot = first_error.lock().expect("write queue error slot");
                            if slot.is_none() {
                                *slot = Some(format!("{err:#}"));
                            }
                            abort.store(true, Ordering::SeqCst);
                            return;
                        }
                    }
                }
            });
        }
    });

    if let Some(msg) = first_error.into_inner().expect("write queue error slot") {
        anyhow::bail!(msg);
    }

    let mut report = WriteQueueReport {
        conversations_written: written.load(Ordering::Relaxed),
        conversations_skipped: skipped.load(Ordering::Relaxed),
        attachments_saved: attachments_saved.load(Ordering::Relaxed),
        media: media::MediaReport::default(),
    };
    report.media = run_media_post_pass(output_dir, options, log, progress, cancel)?;
    announce_finish(log, &report, options.resume);
    Ok(report)
}

/// Convert or compress the staged originals, once every writer is done.
///
/// Writers stage originals and nothing else, so this is where convert and
/// compress actually happen. Running it as its own pass buys the CLI what the
/// desktop already had: per-file commits, so an interruption keeps every
/// derivative already finished, and progress worth printing.
fn run_media_post_pass(
    output_dir: &Path,
    options: &WriteQueueOptions,
    log: Option<&LogSink>,
    progress: Option<&ProgressSink>,
    cancel: Option<&CancelFlag>,
) -> Result<media::MediaReport> {
    if !matches!(options.media, MediaMode::Convert | MediaMode::Compress) {
        return Ok(media::MediaReport::default());
    }

    let transcode_options = TranscodeOptions {
        mode: options.media,
        compress: options.compress.clone(),
        // No vault limit applies to a local export, so nothing here is
        // written off as too large. The desktop's own media pass, which does
        // enforce the real limit, never reaches this code: it stages with
        // Clone and converts on its own.
        asset_max_bytes: u64::MAX,
    };
    // The desktop never runs this branch (it stages with Clone and converts
    // on its own after the gate); the events are for any other consumer.
    let report = transcode_staged(output_dir, &transcode_options, cancel, &mut |p| {
        emit_log(log, format!("  media {}/{}", p.done, p.total));
        emit_progress(
            progress,
            ProgressEvent::Media {
                done: p.done,
                total: p.total,
            },
        );
    })?;

    let mut media = media::MediaReport {
        processed: report.converted,
        skipped: report.skipped + report.repointed,
        bytes_before: report.bytes_before,
        bytes_after: report.bytes_after,
        errors: Vec::new(),
    };
    if report.failed > 0 {
        // The per-file reasons are already on the attachments themselves.
        media.errors.push(format!(
            "{} file(s) could not be converted; their conversation entries say why",
            report.failed
        ));
    }
    emit_log(
        log,
        format!(
            "Attachment {} done: converted={} skipped={} size {} → {}",
            options.media,
            media.processed,
            media.skipped,
            media::format_bytes(media.bytes_before),
            media::format_bytes(media.bytes_after)
        ),
    );
    Ok(media)
}

/// Slack above the measured need, for the derivative a convert holds in
/// flight and for whatever else shares the disk.
const DISK_HEADROOM_SLACK: u64 = 64 * 1024 * 1024;

/// Refuse a drain the staging disk plainly cannot hold.
///
/// `needed` counts the originals the units name. Peak usage is those plus one
/// in-flight derivative, since the media pass commits per file, so the sum
/// plus a fixed slack is the honest requirement.
fn check_headroom(output_dir: &Path, units: &[ConversationUnit]) -> Result<()> {
    // Summed before any resume skip: over-asking on a resumed run is the
    // conservative direction, and such a run usually has most of those bytes
    // on disk already.
    let needed: u64 = units
        .iter()
        .flat_map(|u| u.attachments.iter())
        .filter_map(|a| a.size_hint)
        .sum();
    // A filesystem that cannot answer must not block an export.
    let Ok(available) = fs2::available_space(output_dir) else {
        return Ok(());
    };
    match headroom_shortfall(needed, available) {
        Some(message) => anyhow::bail!(message),
        None => Ok(()),
    }
}

/// `None` when `available` covers `needed` plus slack; otherwise what to say.
fn headroom_shortfall(needed: u64, available: u64) -> Option<String> {
    let required = needed.saturating_add(DISK_HEADROOM_SLACK);
    if available >= required {
        return None;
    }
    Some(format!(
        "Not enough space on the staging disk: this backup needs about {}, and {} is free.",
        media::format_bytes(required),
        media::format_bytes(available)
    ))
}

/// Say that the write queue is starting on `units` conversations: a log
/// line for people and a zero-of-`units` prepare event for the bar.
fn announce_start(log: Option<&LogSink>, progress: Option<&ProgressSink>, units: usize) {
    emit_log(log, "");
    emit_log(log, format!("Preparing {units} conversation file(s)..."));
    emit_progress(
        progress,
        ProgressEvent::Prepare {
            done: 0,
            total: units,
        },
    );
}

/// Report the queue's running attachment totals: a log line for people and
/// an [`ProgressEvent::Attachments`] for the bar.
fn report_attachments(
    log: Option<&LogSink>,
    progress: Option<&ProgressSink>,
    done: usize,
    total: usize,
    bytes_done: u64,
    bytes_total: u64,
) {
    emit_log(
        log,
        format!("  attachments {done}/{total} {bytes_done}/{bytes_total}"),
    );
    emit_progress(
        progress,
        ProgressEvent::Attachments {
            done,
            total,
            bytes_done,
            bytes_total,
        },
    );
}

/// Log the write queue's totals, noting resumed work.
fn announce_finish(log: Option<&LogSink>, report: &WriteQueueReport, resume: bool) {
    emit_log(
        log,
        format!(
            "Prepared {} conversation file(s)",
            report.conversations_written
        ),
    );
    if resume && report.conversations_skipped > 0 {
        emit_log(
            log,
            format!(
                "Skipped {} already staged conversation(s)",
                report.conversations_skipped
            ),
        );
    }
}

/// Stage one conversation's attachments, then write the conversation file.
///
/// The order is the engine's whole contract: the conversation file lands last,
/// so its presence on disk vouches for everything it points at.
fn write_one_unit(
    output_dir: &Path,
    attachments_dir: &Path,
    unit: ConversationUnit,
    options: &WriteQueueOptions,
    load: &mut AttachmentLoader<'_>,
    on_progress: &dyn Fn(UnitProgress),
    cancel: Option<&CancelFlag>,
) -> Result<UnitOutcome> {
    if cancel.is_some_and(|f| f.load(Ordering::SeqCst)) {
        anyhow::bail!("canceled");
    }

    let ConversationUnit {
        mut doc,
        attachments,
    } = unit;
    let attachment_count = attachments.len();
    let hint_sum: u64 = attachments.iter().filter_map(|a| a.size_hint).sum();

    let path = output_dir.join(format!("{}.jsonl", doc.filename_stem()));
    if options.resume && path.is_file() {
        // Already written by an earlier run, attachments and all. Count its
        // attachments as done — progress describes the whole import, not just
        // this run's share of it — and load nothing.
        on_progress(UnitProgress {
            done: attachment_count,
            bytes_done: 0,
            bytes_total: 0,
        });
        return Ok(UnitOutcome {
            written: false,
            attachments_saved: 0,
        });
    }

    // Writers copy originals; convert and compress run later as their own pass.
    let stage_mode = match options.media {
        MediaMode::Disabled => MediaMode::Disabled,
        _ => MediaMode::Clone,
    };

    let timestamps: Vec<i64> = doc.messages.iter().map(|m| m.timestamp_unix_ms).collect();
    let mut slots: HashMap<(usize, usize), UnitAttachment> = attachments
        .into_iter()
        .map(|a| ((a.message_index, a.attachment_index), a))
        .collect();

    let mut sources: Vec<AttachmentSource> = Vec::with_capacity(attachment_count);
    let mut jobs: Vec<AttachmentJob<'_>> = Vec::new();
    for (message_index, msg) in doc.messages.iter_mut().enumerate() {
        let fallback_ts = timestamps.get(message_index).copied().unwrap_or(0);
        for (attachment_index, att) in msg.attachments.iter_mut().enumerate() {
            let slot = slots.remove(&(message_index, attachment_index));
            let (source, size_hint, timestamp_unix_ms) = match slot {
                Some(a) => (a.source, a.size_hint, a.timestamp_unix_ms),
                None => (AttachmentSource::Missing, None, fallback_ts),
            };
            sources.push(source);
            jobs.push(AttachmentJob {
                attachment: att,
                timestamp_unix_ms,
                size_hint,
            });
        }
    }

    let mut unit_bytes_done = 0_u64;
    let mut unit_bytes_extra = 0_u64;
    let mut reported_done = 0_usize;
    {
        let sources = &mut sources;
        run_attachment_jobs(
            &mut jobs,
            attachments_dir,
            &MediaConfig {
                mode: stage_mode,
                compress: options.compress.clone(),
            },
            |i| match sources.get_mut(i) {
                Some(source) => load(source),
                None => Ok(None),
            },
            |p| {
                // run_attachment_jobs reports this unit's running totals; the
                // drain wants what each attachment added.
                let extra = p.bytes_total.saturating_sub(hint_sum);
                on_progress(UnitProgress {
                    done: p.done.saturating_sub(reported_done),
                    bytes_done: p.bytes_done.saturating_sub(unit_bytes_done),
                    bytes_total: extra.saturating_sub(unit_bytes_extra),
                });
                reported_done = p.done;
                unit_bytes_done = p.bytes_done;
                unit_bytes_extra = extra;
            },
            None,
            cancel.map(|flag| flag.as_ref()),
        )
        .map_err(anyhow::Error::msg)?;
    }

    let attachments_saved = jobs
        .iter()
        .filter(|j| j.attachment.path.is_some() && j.attachment.digest_sha256.is_some())
        .count();
    drop(jobs);

    crate::export_transforms::clear_attachments_when_disabled(&mut doc, options.media);
    for msg in &mut doc.messages {
        for att in &mut msg.attachments {
            att.bytes = None;
        }
    }

    write_format(output_dir, OutputFormat::Jsonl, doc)
        .with_context(|| format!("write {}", path.display()))?;

    Ok(UnitOutcome {
        written: true,
        attachments_saved,
    })
}

#[cfg(test)]
mod tests;
