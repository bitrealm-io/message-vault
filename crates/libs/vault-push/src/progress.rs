//! Everything a push says while it runs: the on-disk log, the live progress
//! callback, and the "files N/M" batching that keeps big imports readable.
//!
//! [`Reporter`] is the one object the rest of the crate talks to. It owns the
//! log file, the optional progress callback, and the [`ProgressBatcher`], so
//! callers never juggle three separate borrows to say one thing.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result};

use crate::report::{FileResult, PushReport, UploadProfile, elapsed_ms, format_profile_line};

/// Events the GUI/CLI can show while a push is running.
#[derive(Debug, Clone)]
pub enum ProgressEvent {
    /// One line for the log panel.
    Log(String),
    /// The key was accepted; the run knows which account it imports into.
    Auth {
        /// Account id the key resolved to.
        account_id: i64,
        /// Username the vault reports for that account, else the account id.
        username: String,
    },
    /// Work on one conversation file began.
    FileStart {
        /// Position of this file in the run, starting at 1.
        index: usize,
        /// Files in the run.
        total: usize,
        /// File name relative to the input folder.
        file: String,
    },
    /// Work on one conversation file ended.
    FileDone {
        /// File name relative to the input folder.
        file: String,
        /// `ok`, `failed`, or `skipped`.
        status: String,
    },
    /// Structured skip/error for Import Errors (e.g. oversized attachment).
    Issue {
        /// `skip` when the item was left out, `error` when it failed.
        kind: String,
        /// Pipeline step that raised it, e.g. `upload`.
        step: String,
        /// What was affected: an attachment path or a conversation file.
        item: String,
        /// Why, in one sentence.
        reason: String,
    },
    /// The run finished; the report is final.
    Finished(PushReport),
}

/// Callback type for live progress (GUI log panel, CLI stderr, tests).
pub type ProgressFn<'a> = dyn FnMut(ProgressEvent) + Send + 'a;

/// How many finished conversations are grouped into one "files N/M …" log line.
/// Printing every single chat would flood the log on a big import.
const PROGRESS_BATCH_SIZE: usize = 10;

/// One attachment omitted from upload but kept as metadata on the message.
#[derive(Debug, Clone)]
pub(crate) struct AttachmentSkip {
    pub item: String,
    pub reason: String,
}

/// Append-only log file next to the export (also mirrored to progress callbacks).
struct LogWriter {
    file: File,
}

impl LogWriter {
    /// Create or open the log file, making parent folders if needed.
    fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("open log {}", path.display()))?;
        Ok(Self { file })
    }

    /// Write one line and flush so a crash still leaves the last message on disk.
    fn line(&mut self, msg: &str) {
        let _ = writeln!(self.file, "{msg}");
        let _ = self.file.flush();
    }
}

/// Collects successes and writes one progress line every [`PROGRESS_BATCH_SIZE`] files.
struct ProgressBatcher {
    total: usize,
    done: usize,
    chunk_conversations: u64,
    chunk_messages: u64,
    chunk_bytes: u64,
    chunk_import_ms: u64,
    /// Wall clock for the current progress chunk (first note until the line is written).
    chunk_started: Option<Instant>,
    chunk_count: usize,
}

impl ProgressBatcher {
    /// Start a batcher that writes a line when a chunk of successes is full.
    fn new(total: usize) -> Self {
        Self {
            total,
            done: 0,
            chunk_conversations: 0,
            chunk_messages: 0,
            chunk_bytes: 0,
            chunk_import_ms: 0,
            chunk_started: None,
            chunk_count: 0,
        }
    }

    /// Start the chunk wall clock on the first success or skip in this window.
    fn begin_chunk_if_needed(&mut self) {
        if self.chunk_started.is_none() {
            self.chunk_started = Some(Instant::now());
        }
    }

    /// Record one successful conversation. Returns a log line when the batch is full.
    fn note_ok(&mut self, messages: u64, profile: &UploadProfile) -> Option<String> {
        self.begin_chunk_if_needed();
        self.done = self.done.saturating_add(1);
        self.chunk_count = self.chunk_count.saturating_add(1);
        self.chunk_conversations = self.chunk_conversations.saturating_add(1);
        self.chunk_messages = self.chunk_messages.saturating_add(messages);
        self.chunk_bytes = self.chunk_bytes.saturating_add(profile.asset_bytes);
        self.chunk_import_ms = self
            .chunk_import_ms
            .saturating_add(profile.message_import_ms);
        self.line_if_full()
    }

    /// Record a conversation skipped because the journal says it already imported.
    fn note_skipped(&mut self) -> Option<String> {
        self.begin_chunk_if_needed();
        self.done = self.done.saturating_add(1);
        self.chunk_count = self.chunk_count.saturating_add(1);
        self.chunk_conversations = self.chunk_conversations.saturating_add(1);
        self.line_if_full()
    }

    /// Count a failure toward "done" without adding it to the success chunk totals.
    fn note_failed(&mut self) {
        self.done = self.done.saturating_add(1);
    }

    /// The chunk line when this window is full or the run is complete.
    fn line_if_full(&mut self) -> Option<String> {
        (self.chunk_count >= PROGRESS_BATCH_SIZE || self.done >= self.total)
            .then(|| self.take_chunk_line())
    }

    /// Write any leftover partial batch at the end of the run.
    fn flush_remainder(&mut self) -> Option<String> {
        (self.chunk_count > 0).then(|| self.take_chunk_line())
    }

    /// Format the current chunk line, then zero the counters for the next chunk.
    fn take_chunk_line(&mut self) -> String {
        // Wall time for this progress window — not the sum of per-file clocks
        // (those overlap when prepares run ahead of imports).
        let wall_ms = self.chunk_started.map_or(0, elapsed_ms);
        let line = format!(
            "files {}/{} - conversations={} messages={} transfer size={}, import time={}, total time={}",
            self.done,
            self.total,
            self.chunk_conversations,
            self.chunk_messages,
            media::format_bytes(self.chunk_bytes),
            format_ms_seconds(self.chunk_import_ms),
            format_ms_seconds(wall_ms),
        );
        self.chunk_conversations = 0;
        self.chunk_messages = 0;
        self.chunk_bytes = 0;
        self.chunk_import_ms = 0;
        self.chunk_started = None;
        self.chunk_count = 0;
        line
    }
}

/// Format a millisecond count as seconds with one decimal place.
fn format_ms_seconds(ms: u64) -> String {
    format!("{:.1}s", ms as f64 / 1000.0)
}

/// The single outlet for everything a push run says.
///
/// Quiet per-file detail goes to the log file only (`log`). Anything a person
/// watching the desktop app should see goes to both the log and the progress
/// callback (`show`). Structured events (`event`) go to the callback only.
pub(crate) struct Reporter<'p, 'f> {
    log: LogWriter,
    progress: Option<&'p mut ProgressFn<'f>>,
    batcher: ProgressBatcher,
}

impl<'p, 'f> Reporter<'p, 'f> {
    /// Open the log file. The "files N/M" counter starts at zero until
    /// [`Reporter::expect_files`] says how many conversations the run has.
    ///
    /// # Errors
    ///
    /// Returns an error when the log file cannot be created.
    pub(crate) fn open(log_path: &Path, progress: Option<&'p mut ProgressFn<'f>>) -> Result<Self> {
        Ok(Self {
            log: LogWriter::open(log_path)?,
            progress,
            batcher: ProgressBatcher::new(0),
        })
    }

    /// Tell the "files N/M" counter how many conversations this run covers.
    pub(crate) fn expect_files(&mut self, total: usize) {
        self.batcher = ProgressBatcher::new(total);
    }

    /// Write to the log file only.
    pub(crate) fn log(&mut self, line: &str) {
        self.log.line(line);
    }

    /// Write to the log file and mirror the same text to the progress callback.
    pub(crate) fn show(&mut self, line: String) {
        self.log.line(&line);
        self.event(ProgressEvent::Log(line));
    }

    /// Write one wording to the log file and a friendlier one to the callback.
    pub(crate) fn show_as(&mut self, log_line: &str, shown: String) {
        self.log.line(log_line);
        self.event(ProgressEvent::Log(shown));
    }

    /// Send a structured event to the progress callback, if there is one.
    pub(crate) fn event(&mut self, event: ProgressEvent) {
        if let Some(cb) = self.progress.as_mut() {
            cb(event);
        }
    }

    /// Announce that a conversation is starting.
    pub(crate) fn file_start(&mut self, index: usize, total: usize, file: &str) {
        self.event(ProgressEvent::FileStart {
            index,
            total,
            file: file.to_string(),
        });
    }

    /// Announce a conversation's final status (`ok`, `failed`, or `skipped`).
    pub(crate) fn file_done(&mut self, file: &str, status: &str) {
        self.event(ProgressEvent::FileDone {
            file: file.to_string(),
            status: status.to_string(),
        });
    }

    /// Record a successful conversation in the "files N/M" counter.
    pub(crate) fn note_ok(&mut self, messages: u64, profile: &UploadProfile) {
        if let Some(line) = self.batcher.note_ok(messages, profile) {
            self.show(line);
        }
    }

    /// Record a skipped conversation in the "files N/M" counter.
    pub(crate) fn note_skipped(&mut self) {
        if let Some(line) = self.batcher.note_skipped() {
            self.show(line);
        }
    }

    /// Record a failed conversation: flush the pending "files N/M" success line
    /// first so failure text is not mixed into it, then log the failure and,
    /// when known, its PROFILE timings so slow failures stay diagnosable.
    pub(crate) fn note_failed(&mut self, name: &str, error: &str, profile: Option<&UploadProfile>) {
        self.flush_file_counter();
        self.batcher.note_failed();
        self.show(format!("fail {name}: {error}"));
        if let Some(profile) = profile {
            self.show(format_profile_line(name, profile));
        }
    }

    /// Write any partial "files N/M" line (end of run, or before an error line).
    pub(crate) fn flush_file_counter(&mut self) {
        if let Some(line) = self.batcher.flush_remainder() {
            self.show(line);
        }
    }

    /// Write Import Errors skip rows for attachments that were not uploaded.
    pub(crate) fn attachment_skips(&mut self, skips: &[AttachmentSkip]) {
        for skip in skips {
            self.show(format!("skip {}: {}", skip.item, skip.reason));
            self.event(ProgressEvent::Issue {
                kind: "skip".into(),
                step: "upload".into(),
                item: skip.item.clone(),
                reason: skip.reason.clone(),
            });
        }
    }

    /// Send one Import Errors row per conversation that failed or was
    /// skipped, so any consumer can list them without reading the report.
    /// The log already carries the `fail` line for each failure, so this
    /// goes to the callback only.
    pub(crate) fn conversation_issues(&mut self, results: &[FileResult]) {
        for result in results {
            let (kind, fallback) = match result.status.as_str() {
                "failed" => ("error", "upload failed"),
                "skipped" => ("skip", "already imported or skipped"),
                _ => continue,
            };
            self.event(ProgressEvent::Issue {
                kind: kind.into(),
                step: "upload".into(),
                item: result.file.clone(),
                reason: result.error.clone().unwrap_or_else(|| fallback.to_string()),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_batcher_emits_every_ten_and_on_completion() {
        let mut batcher = ProgressBatcher::new(25);
        let profile = UploadProfile {
            message_import_ms: 3_300,
            total_ms: 5_500,
            asset_bytes: 700_000,
            ..UploadProfile::default()
        };
        let mut lines = Vec::new();
        for _ in 0..9 {
            assert!(batcher.note_ok(2, &profile).is_none());
        }
        let tenth = batcher.note_ok(2, &profile).unwrap();
        assert!(tenth.starts_with("files 10/25 - "));
        assert!(tenth.contains("conversations=10"));
        assert!(tenth.contains("messages=20"));
        assert!(tenth.contains("transfer size=7.0 MB"));
        assert!(tenth.contains("import time=33.0s"));
        // total time is wall-clock for the progress window, not sum of profile.total_ms.
        assert!(tenth.contains("total time="));
        assert!(!tenth.contains("total time=55.0s"));
        assert!(!tenth.contains("bytes="));
        assert!(!tenth.contains("import_ms="));
        assert!(!tenth.contains("total_ms="));
        lines.push(tenth);
        for _ in 0..15 {
            if let Some(line) = batcher.note_ok(1, &profile) {
                lines.push(line);
            }
        }
        assert_eq!(lines.len(), 3);
        assert!(lines[1].starts_with("files 20/25 - "));
        assert!(lines[1].contains("conversations=10"));
        assert!(lines[2].starts_with("files 25/25 - "));
        assert!(lines[2].contains("conversations=5"));
    }

    #[test]
    fn reporter_mirrors_shown_lines_to_callback_and_log() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("push.log");
        let mut seen = Vec::new();
        {
            let mut cb = |event: ProgressEvent| seen.push(event);
            let mut reporter = Reporter::open(&log_path, Some(&mut cb)).unwrap();
            reporter.log("quiet");
            reporter.show("loud".into());
            reporter.show_as("logged", "shown".into());
        }
        let text = fs::read_to_string(&log_path).unwrap();
        assert_eq!(text, "quiet\nloud\nlogged\n");
        let shown: Vec<String> = seen
            .into_iter()
            .map(|event| match event {
                ProgressEvent::Log(line) => line,
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(shown, ["loud", "shown"]);
    }

    #[test]
    fn conversation_issues_cover_failed_and_skipped_files_only() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("push.log");
        let mut seen = Vec::new();
        {
            let mut cb = |event: ProgressEvent| seen.push(event);
            let mut reporter = Reporter::open(&log_path, Some(&mut cb)).unwrap();
            reporter.conversation_issues(&[
                FileResult {
                    file: "ok.jsonl".into(),
                    status: "ok".into(),
                    error: None,
                    messages: 3,
                    attachments: 0,
                    profile: None,
                },
                FileResult::failed("bad.jsonl", "attachment exceeds limit"),
                FileResult::skipped("done.jsonl"),
                FileResult {
                    file: "silent.jsonl".into(),
                    status: "failed".into(),
                    error: None,
                    messages: 0,
                    attachments: 0,
                    profile: None,
                },
            ]);
        }
        let rows: Vec<(String, String, String)> = seen
            .into_iter()
            .map(|event| match event {
                ProgressEvent::Issue {
                    kind, item, reason, ..
                } => (kind, item, reason),
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(
            rows,
            [
                (
                    "error".to_string(),
                    "bad.jsonl".to_string(),
                    "attachment exceeds limit".to_string()
                ),
                (
                    "skip".to_string(),
                    "done.jsonl".to_string(),
                    "already imported or skipped".to_string()
                ),
                (
                    "error".to_string(),
                    "silent.jsonl".to_string(),
                    "upload failed".to_string()
                ),
            ]
        );
        assert_eq!(
            fs::read_to_string(&log_path).unwrap(),
            "",
            "conversation issues are callback-only; the log already has the fail lines"
        );
    }
}
