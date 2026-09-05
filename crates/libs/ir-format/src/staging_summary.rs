//! Recompute what a staged folder holds, for the approval gates.
//!
//! Everything here is measured from the folder. The one estimate is what the
//! media step will do to a file's size, and it is labelled as an estimate all
//! the way to the screen.
//!
//! Decision 39: this is always recomputed from the folder, never read back
//! from a previously-written `summary_json` — the folder is the truth, and
//! that is what makes resuming at a gate work: reopening the session
//! recomputes rather than restoring.
//!
//! Contact matching is not done here — the vault answers which identifiers
//! it already knows, and this returns the distinct identifiers found on
//! disk for the caller to ask about.
//!
//! ## Aliasing
//!
//! Staged names are content-addressed, so two attachment records — in the
//! same document or different ones — can legitimately share one physical
//! file (see `transcode.rs`'s module docs for why). `attachments` counts
//! every reference, because that is what the documents actually contain, but
//! `attachment_bytes`, `verdict_counts`, and `forecasts` are per physical
//! file: the first reference to a given recorded path is measured and
//! classified, and every later reference at that same path is folded into
//! the `attachments` count alone. Mirrors `pending_in`'s dedup in
//! `transcode.rs`, which faces the identical fact about the folder.

use std::collections::{BTreeSet, HashSet};
use std::path::Path;

use anyhow::Result;
use media::{MediaMode, SizeVerdict, classify_probed, estimate_bytes, needs_probe, probe_media};

use crate::read_json::read_conversation_jsonl;
use crate::transcode::{COMMITTED_SUFFIX, TranscodeOptions, conversation_files};
use crate::util::safe_attachment_path;

/// How often [`summarize_staging`] reports progress, over attachments.
///
/// Matches the media crate's own cadence (its private `MEDIA_PROGRESS_EVERY`
/// is 100 too) so a summary pass and a media pass over the same folder feel
/// the same to whatever is watching progress.
const SUMMARY_PROGRESS_EVERY: usize = 100;

/// One attachment the user should see before approving.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentForecast {
    /// Relative path inside the staging folder.
    pub path: String,
    /// Name to show on the screen: the attachment's original name when the
    /// document recorded one, else the staged (content-addressed) file
    /// name. The person approving this knows the file as `IMG_4821.MOV`,
    /// not as whatever hash-derived name it was staged under.
    pub name: String,
    /// Bytes on disk now.
    pub size_bytes: u64,
    /// Bytes expected after the media step. Equal to `size_bytes` when there
    /// is no media step.
    pub estimate_bytes: u64,
    /// How it is expected to land against the limit.
    pub verdict: SizeVerdict,
}

/// How many attachments landed in each verdict.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerdictCounts {
    /// Under the limit now, and expected to stay under.
    pub fits_as_is: usize,
    /// Over the limit now, expected to come under after the media step.
    pub likely_fits: usize,
    /// Under the limit now, expected to cross it during the media step.
    pub may_grow: usize,
    /// Over the limit now, and expected to stay over.
    pub probably_too_big: usize,
    /// The media step does not handle this kind of file, so its size is fixed.
    pub cannot_process: usize,
}

impl VerdictCounts {
    /// Tally one more attachment's verdict.
    fn record(&mut self, verdict: SizeVerdict) {
        match verdict {
            SizeVerdict::FitsAsIs => self.fits_as_is += 1,
            SizeVerdict::LikelyFits => self.likely_fits += 1,
            SizeVerdict::MayGrow => self.may_grow += 1,
            SizeVerdict::ProbablyTooBig => self.probably_too_big += 1,
            SizeVerdict::CannotProcess => self.cannot_process += 1,
        }
    }
}

/// What a staged folder holds.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StagingSummary {
    /// Conversation files found in the folder.
    pub conversations: usize,
    /// Messages across every conversation.
    pub messages: u64,
    /// Distinct participant identifiers, sorted. The vault decides which of
    /// these it already knows.
    pub contact_identifiers: Vec<String>,
    /// Attachments referenced by the documents, including ones already marked
    /// missing and every reference to a shared, content-addressed file.
    pub attachments: usize,
    /// Bytes on disk under `attachments/` for the files that are actually
    /// there, counted once per physical file — see the module docs on
    /// aliasing.
    pub attachment_bytes: u64,
    /// How many physical files landed in each size verdict — see the module
    /// docs on aliasing.
    pub verdict_counts: VerdictCounts,
    /// One row per physical file whose verdict is not `fits_as_is` — see the
    /// module docs on aliasing.
    pub forecasts: Vec<AttachmentForecast>,
}

/// How far [`summarize_staging`] has got, reported over attachments.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryProgress {
    /// Attachments visited so far.
    pub done: usize,
    /// Attachments total.
    pub total: usize,
}

/// Recompute a staged folder's summary: exact conversation/message/attachment
/// counts plus a per-attachment size forecast, for the approval gates.
///
/// Walks the same `*.jsonl` list the media pass walks. For each attachment
/// already carrying a `missing_reason` — settled, whether or not its `path`
/// still points at a file on disk (a `convert_failed` original keeps its
/// path so a resume can retry it, but it has already been flagged and must
/// not be forecast again) — this counts it and stops there: no bytes, no
/// forecast row. The same is true for an attachment with no `path`, or whose
/// recorded file is not on disk.
///
/// Everything else reads its length from disk (never the document's stale
/// `size_bytes`). A recorded path whose stem already ends in `-mv` names a
/// derivative the media pass has committed and will never touch again
/// (`pending_in`'s own exclusion rule, in `transcode.rs`); it is classified
/// on that size alone, with no probe and `estimate_bytes` equal to
/// `size_bytes` — applying a mode's growth or shrink factor to it would
/// forecast a transcode that can never happen. Every other file is probed
/// when [`media::needs_probe`] says it is close enough to the limit to
/// matter and `options.mode` has a media step, then classified with
/// [`media::classify_probed`]. Under [`MediaMode::Clone`] and
/// [`MediaMode::Disabled`] there is no media step at all: probing is skipped
/// entirely — not an optimization, since probing would forecast work that
/// will never run — `estimate_bytes` equals `size_bytes`, and the file is
/// classified on its current size alone.
///
/// The probe is best-effort: a failed ffprobe call on one file means
/// classifying it with no probe in hand, never failing the summary — a gate
/// that cannot render because one file is unreadable is worse than a gate
/// with one rougher estimate.
///
/// Content-addressed staging means two attachment records can share one
/// physical file (see the module docs on aliasing); `attachments` counts
/// every reference, but a shared file's bytes, verdict, and forecast row are
/// each counted exactly once, on the first reference to its recorded path.
///
/// `on_progress` reports [`SummaryProgress`] over attachments — every
/// reference, aliased or not, since that is the count the caller sees
/// growing — at the same cadence the media crate uses (every 100) plus a
/// final call.
///
/// # Errors
///
/// Returns an error when the folder cannot be read or a conversation file
/// cannot be parsed.
pub fn summarize_staging(
    staging_dir: &Path,
    options: &TranscodeOptions,
    on_progress: &mut dyn FnMut(SummaryProgress),
) -> Result<StagingSummary> {
    let files = conversation_files(staging_dir)?;

    let mut summary = StagingSummary::default();
    let mut contacts = BTreeSet::new();
    // Gathered while walking the documents for their conversation/message/
    // contact counts, so the classification pass below can run over a flat
    // list with a known total up front, matching `on_progress`'s contract.
    let mut attachments: Vec<AttachmentRef> = Vec::new();

    for jsonl in &files {
        let doc = read_conversation_jsonl(jsonl)?;
        summary.conversations += 1;
        summary.messages += doc.messages.len() as u64;
        for participant in &doc.conversation.participants {
            if let Some(handle) = participant.handle.clone() {
                contacts.insert(handle);
            }
        }
        for msg in &doc.messages {
            for att in &msg.attachments {
                attachments.push(AttachmentRef {
                    path: att.path.clone(),
                    missing_reason: att.missing_reason.clone(),
                    original_name: att.original_name.clone(),
                });
            }
        }
    }
    summary.contact_identifiers = contacts.into_iter().collect();

    let total = attachments.len();
    on_progress(SummaryProgress { done: 0, total });

    let has_media_step = matches!(options.mode, MediaMode::Convert | MediaMode::Compress);
    // Recorded paths already measured and classified, so an aliased second
    // (or third, …) reference to the same physical file contributes to
    // `attachments` only — see the module docs on aliasing.
    let mut classified_paths: HashSet<String> = HashSet::new();
    let mut done = 0usize;
    for att in attachments {
        summary.attachments += 1;
        if att.missing_reason.is_none()
            && let Some(rel) = att.path.as_deref()
            && classified_paths.insert(rel.to_string())
        {
            classify_one(
                staging_dir,
                rel,
                att.original_name.as_deref(),
                options,
                has_media_step,
                &mut summary,
            );
        }
        done += 1;
        if done.is_multiple_of(SUMMARY_PROGRESS_EVERY) || done == total {
            on_progress(SummaryProgress { done, total });
        }
    }

    Ok(summary)
}

/// One attachment reference gathered from a document, stripped to the fields
/// [`summarize_staging`]'s classification pass needs.
struct AttachmentRef {
    path: Option<String>,
    missing_reason: Option<String>,
    original_name: Option<String>,
}

/// Measure and classify one physical file, folding its bytes and verdict
/// into `summary`.
///
/// Called only for a reference not already settled (no `missing_reason`) and
/// not already classified via an earlier reference to the same recorded
/// path — the caller (`summarize_staging`) filters both before calling.
/// `rel` unsafe, or its file missing from disk, contributes nothing —
/// silently, since a document recording a path with no bytes behind it is
/// exactly the "not there" case this whole function exists to skip.
fn classify_one(
    staging_dir: &Path,
    rel: &str,
    original_name: Option<&str>,
    options: &TranscodeOptions,
    has_media_step: bool,
    summary: &mut StagingSummary,
) {
    let Ok(abs) = safe_attachment_path(staging_dir, rel) else {
        return;
    };
    let Ok(meta) = std::fs::metadata(&abs) else {
        return;
    };
    let size_bytes = meta.len();
    summary.attachment_bytes += size_bytes;

    let stem = abs.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let committed = stem.ends_with(COMMITTED_SUFFIX);

    let (verdict, estimate) = if committed {
        // A committed derivative: `pending_in` (transcode.rs) excludes a
        // `-mv` stem from work unconditionally, so applying `options.mode`'s
        // growth or shrink factor here would forecast a transcode that will
        // never run. Judged on its own size, exactly as `pending_in` treats
        // it as done.
        let verdict = if size_bytes <= options.asset_max_bytes {
            SizeVerdict::FitsAsIs
        } else {
            SizeVerdict::ProbablyTooBig
        };
        (verdict, size_bytes)
    } else {
        let ext = abs
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string();
        let probe = if has_media_step && needs_probe(size_bytes, options.asset_max_bytes) {
            // Best-effort: an ffprobe failure classifies without a probe
            // rather than failing the whole summary.
            probe_media(&abs).ok()
        } else {
            None
        };
        let verdict = classify_probed(
            size_bytes,
            probe.as_ref(),
            &ext,
            options.mode,
            &options.compress,
            options.asset_max_bytes,
        );
        let estimate = if has_media_step {
            estimate_bytes(
                size_bytes,
                probe.as_ref(),
                &ext,
                options.mode,
                &options.compress,
            )
        } else {
            size_bytes
        };
        (verdict, estimate)
    };

    summary.verdict_counts.record(verdict);
    if verdict == SizeVerdict::FitsAsIs {
        return;
    }
    let name = original_name.map(ToString::to_string).unwrap_or_else(|| {
        abs.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(rel)
            .to_string()
    });
    summary.forecasts.push(AttachmentForecast {
        path: rel.to_string(),
        name,
        size_bytes,
        estimate_bytes: estimate,
        verdict,
    });
}

#[cfg(test)]
mod tests;
