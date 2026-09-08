//! Report types written at the end of a push, plus the small formatting
//! helpers that turn them into log text.
//!
//! [`PushReport`] is the JSON file left next to the export and the payload of
//! [`crate::ProgressEvent::Finished`]. Everything here is plain data with no
//! I/O so the desktop app and tests can build and inspect reports directly.

use std::time::{Instant, SystemTime, UNIX_EPOCH};
use vault_api_types::ImportMode;

use serde::{Deserialize, Serialize};

/// Per-conversation outcome written into the final report JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileResult {
    /// File name relative to the input folder.
    pub file: String,
    /// `ok`, `failed`, or `skipped`.
    pub status: String,
    /// The failure, when `status` is `failed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Messages sent to the vault from this file.
    pub messages: u64,
    /// Attachments uploaded for this file.
    pub attachments: u64,
    /// Timings and sizes, when profiling was on for the run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<UploadProfile>,
}

impl FileResult {
    /// Result row for a conversation the journal says was already imported.
    pub(crate) fn skipped(file: &str) -> Self {
        Self {
            file: file.to_string(),
            status: "skipped".into(),
            error: None,
            messages: 0,
            attachments: 0,
            profile: None,
        }
    }

    /// Result row for a conversation that failed before any message was queued.
    pub(crate) fn failed(file: &str, error: &str) -> Self {
        Self {
            file: file.to_string(),
            status: "failed".into(),
            error: Some(error.to_string()),
            messages: 0,
            attachments: 0,
            profile: None,
        }
    }
}

/// Timing and size stats for one conversation (used for PROFILE log lines).
///
/// These numbers help answer "why was this chat slow?" — reading JSON Lines,
/// hashing/scanning attachments, uploading media, or importing messages.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UploadProfile {
    /// Reading and parsing the JSON Lines file.
    pub read_ms: u64,
    /// Finding attachments on disk and hashing them.
    pub attachment_scan_hash_ms: u64,
    /// Uploading attachment bytes.
    pub asset_upload_ms: u64,
    /// Posting the message batches.
    pub message_import_ms: u64,
    /// The whole file, start to finish.
    pub total_ms: u64,
    /// Distinct attachments by fingerprint.
    pub unique_assets: u64,
    /// Bytes across those distinct attachments.
    pub asset_bytes: u64,
}

/// Final summary of a whole push (also written to disk as the report file).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushReport {
    /// `true` when no conversation failed and the run was not cancelled.
    pub ok: bool,
    /// Account id the key resolved to.
    pub account: i64,
    /// Username the vault reports for that account, else the account id.
    pub username: String,
    /// `append` or `replace`.
    pub mode: ImportMode,
    /// Unix seconds when the run started, as a string.
    pub started_at: String,
    /// Unix seconds when the run finished, as a string.
    pub finished_at: String,
    /// Wall-clock time from start of auth through the last import.
    pub elapsed_ms: u64,
    /// Conversation files the run found.
    pub conversations_total: u64,
    /// Files imported without error.
    pub conversations_ok: u64,
    /// Files that failed.
    pub conversations_failed: u64,
    /// Files the journal said were already imported.
    pub conversations_skipped: u64,
    /// Messages placed in HTTP import request bodies.
    #[serde(default)]
    pub messages_attempted: u64,
    /// Messages the server inserted as new rows.
    #[serde(default)]
    pub messages_inserted: u64,
    /// Attempted messages the server reported as already present.
    #[serde(default)]
    pub messages_deduped: u64,
    /// Messages in HTTP requests that failed after all retries.
    #[serde(default)]
    pub messages_failed: u64,
    /// Legacy successful-request count. Equal to attempted minus failed.
    pub messages: u64,
    /// Attachments whose bytes went up this run.
    pub assets_uploaded: u64,
    /// Attachments the vault already had, by fingerprint.
    pub assets_skipped: u64,
    /// Bytes uploaded.
    pub assets_bytes: u64,
    /// One row per conversation file.
    pub results: Vec<FileResult>,
}

/// Running totals of messages attempted, inserted, already present, and failed.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct MessageAccounting {
    pub attempted: u64,
    pub inserted: u64,
    pub deduped: u64,
    pub failed: u64,
}

/// Running totals of attachment uploads across every prepared conversation.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct AssetTotals {
    pub uploaded: u64,
    pub skipped: u64,
    pub bytes: u64,
}

impl AssetTotals {
    /// Add one conversation's upload counts onto the run totals.
    pub(crate) fn add(&mut self, other: AssetTotals) {
        self.uploaded = self.uploaded.saturating_add(other.uploaded);
        self.skipped = self.skipped.saturating_add(other.skipped);
        self.bytes = self.bytes.saturating_add(other.bytes);
    }
}

/// Totals derived from per-conversation [`FileResult`] rows.
#[derive(Debug, Default)]
pub(crate) struct FileResultCounts {
    pub ok: u64,
    pub failed: u64,
    pub skipped: u64,
    pub messages: u64,
    pub attachments: u64,
}

/// Count ok / failed / skipped conversations and sum messages and attachments.
pub(crate) fn count_file_results(results: &[FileResult]) -> FileResultCounts {
    let mut counted = FileResultCounts::default();
    for result in results {
        match result.status.as_str() {
            "ok" => {
                counted.ok += 1;
                counted.messages += result.messages;
                counted.attachments += result.attachments;
            }
            "failed" => counted.failed += 1,
            "skipped" => counted.skipped += 1,
            _ => {}
        }
    }
    counted
}

/// Unix time in seconds as a string (for report timestamps).
pub(crate) fn now_stamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or_else(|_| "0".into(), |d| d.as_secs().to_string())
}

/// Milliseconds since `started` (for PROFILE timing fields).
pub(crate) fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Turn a millisecond count into a short human string like `34m12s` or `1h02m03s`.
pub fn format_duration_ms(ms: u64) -> String {
    let total_secs = ms / 1000;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    if hours > 0 {
        format!("{hours}h{minutes:02}m{seconds:02}s")
    } else if minutes > 0 {
        format!("{minutes}m{seconds:02}s")
    } else if total_secs > 0 || ms == 0 {
        format!("{seconds}s")
    } else {
        format!("{ms}ms")
    }
}

/// Three-way session status for `/v1/imports/{id}/complete` (import-session
/// spec, decisions 21–22). `failed` has a zero floor: aborted, or nothing
/// landed at all. A skip-only re-push is a no-op, not a failure. Item-level
/// failures beside successes are `completed_with_issues`.
pub fn outcome_status(report: &PushReport, aborted: bool) -> &'static str {
    let nothing_landed = report.conversations_total > 0
        && report.conversations_ok == 0
        && report.conversations_skipped == 0;
    if aborted || nothing_landed {
        return "failed";
    }
    if report.conversations_failed > 0 || report.messages_failed > 0 {
        return "completed_with_issues";
    }
    "completed"
}

/// Build the multi-line "Import success / completed with errors" blurb for the log.
pub fn format_push_summary(report: &PushReport) -> String {
    let status = if report.ok {
        "success"
    } else {
        "completed with errors"
    };
    format!(
        "==== Summary ====\n\
Import {status}\n\
Conversations: {} ok, {} failed, {} skipped ({} total)\n\
Messages: {}\n\
Message accounting: {} attempted = {} new + {} deduped + {} failed\n\
Assets: {} uploaded, {} skipped\n\
Elapsed: {} ({} ms)",
        report.conversations_ok,
        report.conversations_failed,
        report.conversations_skipped,
        report.conversations_total,
        report.messages,
        report.messages_attempted,
        report.messages_inserted,
        report.messages_deduped,
        report.messages_failed,
        report.assets_uploaded,
        report.assets_skipped,
        format_duration_ms(report.elapsed_ms),
        report.elapsed_ms,
    )
}

/// One PROFILE line with per-phase timings for a conversation.
pub(crate) fn format_profile_line(name: &str, profile: &UploadProfile) -> String {
    format!(
        "PROFILE {name} read_ms={} attachment_scan_hash_ms={} asset_upload_ms={} \
         message_import_ms={} total_ms={} unique_assets={} asset_bytes={}",
        profile.read_ms,
        profile.attachment_scan_hash_ms,
        profile.asset_upload_ms,
        profile.message_import_ms,
        profile.total_ms,
        profile.unique_assets,
        profile.asset_bytes
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_duration_ms_humanizes() {
        assert_eq!(format_duration_ms(0), "0s");
        assert_eq!(format_duration_ms(999), "999ms");
        assert_eq!(format_duration_ms(12_000), "12s");
        assert_eq!(format_duration_ms(2_052_000), "34m12s");
        assert_eq!(format_duration_ms(3_723_000), "1h02m03s");
    }

    fn sample_report() -> PushReport {
        PushReport {
            ok: true,
            account: 1,
            username: "user".into(),
            mode: ImportMode::Append,
            started_at: "2026-08-29T00:00:00Z".into(),
            finished_at: "2026-08-29T00:01:00Z".into(),
            elapsed_ms: 60_000,
            conversations_total: 10,
            conversations_ok: 10,
            conversations_failed: 0,
            conversations_skipped: 0,
            messages_attempted: 100,
            messages_inserted: 90,
            messages_deduped: 10,
            messages_failed: 0,
            messages: 100,
            assets_uploaded: 5,
            assets_skipped: 0,
            assets_bytes: 1_000,
            results: Vec::new(),
        }
    }

    #[test]
    fn format_push_summary_is_multiline() {
        let report = PushReport {
            elapsed_ms: 12_000,
            conversations_ok: 8,
            conversations_failed: 1,
            conversations_skipped: 1,
            assets_uploaded: 4,
            assets_skipped: 2,
            assets_bytes: 1_048_576,
            ..sample_report()
        };
        let summary = format_push_summary(&report);
        assert!(summary.contains("==== Summary ===="));
        assert!(summary.contains("Import success"));
        assert!(summary.contains("Conversations: 8 ok, 1 failed, 1 skipped (10 total)"));
        assert!(summary.contains("Messages: 100"));
        assert!(
            summary.contains("Message accounting: 100 attempted = 90 new + 10 deduped + 0 failed")
        );
        assert!(summary.contains("Assets: 4 uploaded, 2 skipped"));
        assert!(summary.contains("Elapsed: 12s (12000 ms)"));
        assert!(
            !summary.lines().any(|l| l.starts_with(' ')),
            "summary lines must not be indented"
        );
    }

    #[test]
    fn outcome_status_matches_the_spec_verdicts() {
        // Clean run.
        assert_eq!(outcome_status(&sample_report(), false), "completed");

        // Aborted is failed regardless of counts.
        assert_eq!(outcome_status(&sample_report(), true), "failed");

        // Nothing landed at all: the zero floor.
        let mut nothing = sample_report();
        nothing.ok = false;
        nothing.conversations_ok = 0;
        nothing.conversations_failed = 10;
        assert_eq!(outcome_status(&nothing, false), "failed");

        // A skip-only re-push is a no-op, not a failure.
        let mut skips = sample_report();
        skips.conversations_ok = 0;
        skips.conversations_skipped = 10;
        assert_eq!(outcome_status(&skips, false), "completed");

        // Item-level failures beside successes.
        let mut partial = sample_report();
        partial.ok = false;
        partial.conversations_ok = 8;
        partial.conversations_failed = 2;
        assert_eq!(outcome_status(&partial, false), "completed_with_issues");

        // Message failures inside ok conversations.
        let mut msgs = sample_report();
        msgs.messages_failed = 3;
        assert_eq!(outcome_status(&msgs, false), "completed_with_issues");
    }

    #[test]
    fn count_file_results_sums_only_ok_rows() {
        let results = vec![
            FileResult {
                file: "a.jsonl".into(),
                status: "ok".into(),
                error: None,
                messages: 5,
                attachments: 2,
                profile: None,
            },
            FileResult::failed("b.jsonl", "boom"),
            FileResult::skipped("c.jsonl"),
        ];
        let counted = count_file_results(&results);
        assert_eq!((counted.ok, counted.failed, counted.skipped), (1, 1, 1));
        assert_eq!((counted.messages, counted.attachments), (5, 2));
    }
}
