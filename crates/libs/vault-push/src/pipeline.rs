//! The message-import side of a push: pack prepared chunks into HTTP batches,
//! send one batch at a time, and settle each conversation's outcome.
//!
//! [`ImportPipeline`] owns every piece of state the import loop mutates: the
//! batch being filled, the request in flight, the per-conversation trackers,
//! the result rows, and the message accounting. The main loop in
//! [`crate::run`] only decides *when* to queue and flush; the pipeline decides
//! *what that means* for the journal, the report, and the log.

use std::collections::BTreeSet;
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::Instant;

use anyhow::Result;
use message_vault_io_core::check_cancel;

use crate::http;
use crate::journal::{JournalMessage, RunJournal};
use crate::prepare::{ImportChunk, PreparedFile, SharedJournal};
use crate::progress::Reporter;
use crate::report::{
    FileResult, MessageAccounting, UploadProfile, elapsed_ms, format_profile_line,
};
use crate::run::{MAX_IMPORT_BODY_BYTES, Session, VaultPushConfig};

/// If the pending message batch is at least this many messages, start its HTTP
/// import now instead of waiting until the next chat is prepared.
///
/// Preparing the next chat may upload many attachments. Holding a large ready
/// batch until that finishes makes the UI look stuck and wastes time when the
/// network could already be importing.
const OVERLAP_FLUSH_MIN_MESSAGES: usize = 100;
/// Same idea as [`OVERLAP_FLUSH_MIN_MESSAGES`], but for batch body size in bytes.
const OVERLAP_FLUSH_MIN_BODY_BYTES: usize = 512 * 1024;

/// What the main loop should do after queueing one chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChunkStep {
    /// Keep queueing this conversation's chunks.
    Continue,
    /// A flush failed for this conversation; drop its remaining chunks but keep the run going.
    FileFailed,
    /// A flush failed and the run must stop.
    Abort,
}

/// Tracks one conversation from "prepared" until all its import chunks succeed or fail.
struct FileTracker {
    name: String,
    source: String,
    attachments: u64,
    profile: UploadProfile,
    total_started: Instant,
    outstanding_messages: usize,
    successful_messages: u64,
    /// All chunks were handed to the pipeline (imports may still be in flight).
    queue_complete: bool,
    failed: Option<String>,
    done: bool,
}

impl FileTracker {
    /// Start tracking a conversation whose chunks are about to be queued.
    fn new(name: &str, prepared: &PreparedFile) -> Self {
        Self {
            name: name.to_string(),
            source: prepared.source.clone(),
            attachments: prepared.attachments,
            profile: prepared.profile.clone(),
            total_started: prepared.total_started,
            outstanding_messages: prepared.message_count(),
            successful_messages: 0,
            queue_complete: false,
            failed: None,
            done: false,
        }
    }

    /// True once the conversation's final result can be written: it failed,
    /// or every queued message has been accepted.
    fn is_settled(&self) -> bool {
        !self.done
            && (self.failed.is_some() || (self.queue_complete && self.outstanding_messages == 0))
    }
}

/// One message id in an import batch, tied back to its conversation file index.
struct BatchMessage {
    file_index: usize,
    journal: JournalMessage,
}

/// Messages from one backup source packed into a single import HTTP body.
struct ImportBatch {
    source: String,
    body: Vec<u8>,
    messages: Vec<BatchMessage>,
    conversations: usize,
}

impl ImportBatch {
    /// Empty batch that will hold messages from one backup source.
    fn new(source: &str) -> Self {
        Self {
            source: source.to_string(),
            body: Vec::new(),
            messages: Vec::new(),
            conversations: 0,
        }
    }

    /// Append one prepared chunk onto this batch (body bytes + message ids).
    fn push(&mut self, file_index: usize, chunk: ImportChunk) {
        self.body.extend_from_slice(&chunk.body);
        self.messages
            .extend(chunk.messages.into_iter().map(|journal| BatchMessage {
                file_index,
                journal,
            }));
        self.conversations += 1;
    }

    /// True if adding `chunk` would exceed the message count or byte size limit.
    ///
    /// In that case the caller should send this batch first, then start a new one.
    fn would_overflow(
        &self,
        chunk: &ImportChunk,
        max_messages: usize,
        max_body_bytes: usize,
    ) -> bool {
        !self.messages.is_empty()
            && (self.messages.len() + chunk.messages.len() > max_messages
                || self.body.len() + chunk.body.len() > max_body_bytes)
    }

    /// True once the batch has reached either limit and should be sent.
    fn is_full(&self, max_messages: usize, max_body_bytes: usize) -> bool {
        self.messages.len() >= max_messages || self.body.len() >= max_body_bytes
    }

    /// True when the batch is big enough to be worth sending before the next
    /// conversation is prepared (see [`OVERLAP_FLUSH_MIN_MESSAGES`]).
    fn is_worth_overlapping(&self) -> bool {
        self.messages.len() >= OVERLAP_FLUSH_MIN_MESSAGES
            || self.body.len() >= OVERLAP_FLUSH_MIN_BODY_BYTES
    }

    /// Distinct conversation indexes with messages in this batch, in order.
    fn file_indexes(&self) -> BTreeSet<usize> {
        self.messages.iter().map(|m| m.file_index).collect()
    }
}

/// Result of one import HTTP request, including timing and the batch that was sent.
struct ImportHttpOutcome {
    batch: ImportBatch,
    mode: String,
    request_ms: u64,
    messages_per_second: f64,
    mebibytes_per_second: f64,
    body_bytes: usize,
    message_count: usize,
    response: Result<http::ImportResponse, String>,
}

/// Owns the import-side state of one push run.
pub(crate) struct ImportPipeline<'a> {
    cfg: &'a VaultPushConfig,
    session: &'a Session,
    journal: &'a Mutex<SharedJournal>,
    import_id: Option<i64>,
    batch_size: usize,
    /// Messages waiting to be sent.
    pending: Option<ImportBatch>,
    /// The HTTP import currently running on a background thread, if any.
    inflight: Option<JoinHandle<ImportHttpOutcome>>,
    /// First import in replace mode uses mode=replace; later ones use append.
    first_import: bool,
    /// One slot per conversation file, filled once its chunks are queued.
    trackers: Vec<Option<FileTracker>>,
    /// One slot per conversation file, filled as each one finishes or is skipped.
    results: Vec<Option<FileResult>>,
    accounting: MessageAccounting,
}

impl<'a> ImportPipeline<'a> {
    /// An empty pipeline for `total` conversation files.
    pub(crate) fn new(
        cfg: &'a VaultPushConfig,
        session: &'a Session,
        journal: &'a Mutex<SharedJournal>,
        import_id: Option<i64>,
        batch_size: usize,
        total: usize,
    ) -> Self {
        Self {
            cfg,
            session,
            journal,
            import_id,
            batch_size,
            pending: None,
            inflight: None,
            first_import: true,
            trackers: std::iter::repeat_with(|| None).take(total).collect(),
            results: vec![None; total],
            accounting: MessageAccounting::default(),
        }
    }

    /// Hand back the per-file results and message totals for the report.
    pub(crate) fn into_results(self) -> (Vec<FileResult>, MessageAccounting) {
        (
            self.results.into_iter().flatten().collect(),
            self.accounting,
        )
    }

    /// True while a batch is pending or an import request is in flight.
    pub(crate) fn has_work(&self) -> bool {
        self.pending.is_some() || self.inflight.is_some()
    }

    /// True when the pending batch is large enough to send now, without
    /// waiting for the next conversation to be prepared.
    pub(crate) fn pending_is_worth_overlapping(&self) -> bool {
        self.pending
            .as_ref()
            .is_some_and(ImportBatch::is_worth_overlapping)
    }

    /// True when the pending batch holds messages from a different backup
    /// source; one request only ever carries one source.
    pub(crate) fn pending_source_differs(&self, source: &str) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|batch| batch.source != source)
    }

    /// Record a conversation the journal already had, with no work done.
    pub(crate) fn record_skipped(&mut self, idx: usize, name: &str, out: &mut Reporter<'_, '_>) {
        self.results[idx] = Some(FileResult::skipped(name));
        out.file_done(name, "skipped");
        out.note_skipped();
    }

    /// Record a conversation that failed during prepare, before any chunk was queued.
    pub(crate) fn record_prepare_failure(
        &mut self,
        idx: usize,
        name: &str,
        error: &str,
        out: &mut Reporter<'_, '_>,
    ) {
        self.lock_journal()
            .journal
            .record_failure("", name, "file", error);
        out.note_failed(name, error, None);
        out.file_done(name, "failed");
        self.results[idx] = Some(FileResult::failed(name, error));
    }

    /// Start tracking a prepared conversation whose chunks are about to be queued.
    pub(crate) fn start_file(&mut self, idx: usize, name: &str, prepared: &PreparedFile) {
        self.trackers[idx] = Some(FileTracker::new(name, prepared));
    }

    /// Add one chunk to the pending batch, sending the batch first or after if
    /// the limits require it.
    ///
    /// # Errors
    ///
    /// Returns an error when a flush fails hard (journal write or worker panic).
    pub(crate) fn queue_chunk(
        &mut self,
        idx: usize,
        chunk: ImportChunk,
        out: &mut Reporter<'_, '_>,
    ) -> Result<ChunkStep> {
        let limits = (self.batch_size, MAX_IMPORT_BODY_BYTES);
        let must_flush_first = self
            .pending
            .as_ref()
            .is_some_and(|batch| batch.would_overflow(&chunk, limits.0, limits.1));
        if must_flush_first {
            let step = self.flush_for_file(idx, out)?;
            if step != ChunkStep::Continue {
                return Ok(step);
            }
        }
        let source = self.trackers[idx]
            .as_ref()
            .map(|t| t.source.clone())
            .unwrap_or_default();
        let batch = self
            .pending
            .get_or_insert_with(|| ImportBatch::new(&source));
        batch.push(idx, chunk);
        if batch.is_full(limits.0, limits.1) {
            return self.flush_for_file(idx, out);
        }
        Ok(ChunkStep::Continue)
    }

    /// Flush while queueing `idx`, then translate the outcome for that file.
    fn flush_for_file(&mut self, idx: usize, out: &mut Reporter<'_, '_>) -> Result<ChunkStep> {
        if !self.flush_and_continue(!self.cfg.continue_on_error, out)? {
            return Ok(ChunkStep::Abort);
        }
        let file_failed = self.trackers[idx]
            .as_ref()
            .is_some_and(|t| t.failed.is_some());
        Ok(if file_failed {
            ChunkStep::FileFailed
        } else {
            ChunkStep::Continue
        })
    }

    /// Note that every chunk for `idx` has been queued and write its result if
    /// nothing is still in flight for it.
    ///
    /// # Errors
    ///
    /// Returns an error when the journal cannot be updated.
    pub(crate) fn finish_queueing(&mut self, idx: usize, out: &mut Reporter<'_, '_>) -> Result<()> {
        if let Some(tracker) = self.trackers[idx].as_mut() {
            tracker.queue_complete = true;
        }
        let mut guard = self.lock_journal();
        self.finish_file_if_settled(idx, &mut guard.journal, out)
    }

    /// Flush, then say whether the run may keep going.
    ///
    /// A failed request stops the run when it was cancelled or when
    /// `continue_on_error` is off; otherwise the failure is recorded against
    /// the conversations in that batch and the run moves on.
    ///
    /// # Errors
    ///
    /// Returns an error when the journal cannot be updated or the import
    /// thread panicked.
    pub(crate) fn flush_and_continue(
        &mut self,
        wait: bool,
        out: &mut Reporter<'_, '_>,
    ) -> Result<bool> {
        let request_ok = self.flush(wait, out)?;
        Ok(request_ok || (!self.is_cancelled() && self.cfg.continue_on_error))
    }

    /// Finish the current in-flight import (if any), then start the pending batch (if any).
    ///
    /// `wait = false` means: start the HTTP request on a background thread and
    /// return so the caller can keep preparing more chats. That overlap is a
    /// major reason large imports feel faster than "upload everything, then
    /// import everything".
    ///
    /// `wait = true` means: block until this import finishes (used at end of
    /// run or when continuing after an error is not allowed).
    ///
    /// Returns `false` when a request failed.
    ///
    /// # Errors
    ///
    /// Returns an error when the journal cannot be updated or the import
    /// thread panicked.
    fn flush(&mut self, wait: bool, out: &mut Reporter<'_, '_>) -> Result<bool> {
        let mut ok = self.join_inflight(out)?;
        if !ok && !self.cfg.continue_on_error {
            self.pending = None;
            return Ok(false);
        }
        let Some(batch) = self.pending.take() else {
            return Ok(ok);
        };
        // Do not start a new import after cancel; leave pending unsent.
        if self.is_cancelled() {
            return Ok(false);
        }
        let mode = if self.cfg.mode == "replace" && self.first_import {
            "replace"
        } else {
            "append"
        };
        self.inflight = Some(self.spawn_import(batch, mode));
        if wait {
            ok = self.join_inflight(out)?;
        }
        Ok(ok)
    }

    /// Start one message-import HTTP request on a background thread and return immediately.
    ///
    /// Running the POST off the main thread lets prepare workers keep hashing
    /// and uploading attachments during the network wait. Only one import is
    /// in flight at a time.
    fn spawn_import(&self, batch: ImportBatch, mode: &str) -> JoinHandle<ImportHttpOutcome> {
        let session = self.session.clone();
        let max_retries = self.cfg.max_retries;
        let import_id = self.import_id;
        let mode = mode.to_string();
        std::thread::spawn(move || {
            let request_started = Instant::now();
            let body_bytes = batch.body.len();
            let message_count = batch.messages.len();
            let response = vault_http::with_retries(max_retries, || {
                session.post_import(&batch.source, &mode, import_id, batch.body.clone())
            })
            .map_err(|error| error.to_string());
            let request_ms = elapsed_ms(request_started);
            let seconds = request_started.elapsed().as_secs_f64().max(0.001);
            ImportHttpOutcome {
                batch,
                mode,
                request_ms,
                messages_per_second: message_count as f64 / seconds,
                mebibytes_per_second: body_bytes as f64 / message_ir::MIB as f64 / seconds,
                body_bytes,
                message_count,
                response,
            }
        })
    }

    /// Wait for the background import thread (if any) and apply its success or failure.
    ///
    /// Returns `true` when there was nothing in flight or the request succeeded.
    ///
    /// # Errors
    ///
    /// Returns an error when the worker thread panics or the journal cannot be updated.
    pub(crate) fn join_inflight(&mut self, out: &mut Reporter<'_, '_>) -> Result<bool> {
        let Some(handle) = self.inflight.take() else {
            return Ok(true);
        };
        let outcome = handle
            .join()
            .map_err(|_| anyhow::anyhow!("import worker panicked"))?;
        let mut guard = self.lock_journal();
        self.apply_outcome(outcome, &mut guard.journal, out)
    }

    /// Update journal + per-file trackers after one import HTTP request finishes.
    ///
    /// On success: record each message id so a later push can skip them.
    /// On failure: mark every conversation that contributed to this batch as failed.
    ///
    /// # Errors
    ///
    /// Returns an error when the journal cannot be updated.
    fn apply_outcome(
        &mut self,
        outcome: ImportHttpOutcome,
        journal: &mut RunJournal,
        out: &mut Reporter<'_, '_>,
    ) -> Result<bool> {
        let represented = outcome.batch.file_indexes();
        self.accounting.attempted = self
            .accounting
            .attempted
            .saturating_add(outcome.message_count as u64);
        self.charge_request_time(&represented, outcome.request_ms);
        let stats = format!(
            "source={} mode={} conversations={} messages={} bytes={} elapsed_ms={} \
             messages_per_second={:.1} mib_per_second={:.2}",
            outcome.batch.source,
            outcome.mode,
            outcome.batch.conversations,
            outcome.message_count,
            outcome.body_bytes,
            outcome.request_ms,
            outcome.messages_per_second,
            outcome.mebibytes_per_second,
        );

        let request_ok = match outcome.response {
            Ok(response) => {
                self.accounting.inserted = self
                    .accounting
                    .inserted
                    .saturating_add(response.messages_appended);
                self.accounting.deduped = self
                    .accounting
                    .deduped
                    .saturating_add(response.messages_deduped);
                self.first_import = false;
                let messages: Vec<JournalMessage> = outcome
                    .batch
                    .messages
                    .iter()
                    .map(|m| m.journal.clone())
                    .collect();
                journal.message_batch_ok(&outcome.batch.source, messages)?;
                for message in &outcome.batch.messages {
                    if let Some(tracker) = self.trackers[message.file_index].as_mut() {
                        tracker.outstanding_messages =
                            tracker.outstanding_messages.saturating_sub(1);
                        tracker.successful_messages = tracker.successful_messages.saturating_add(1);
                    }
                }
                out.log(&format!(
                    "IMPORT_REQUEST ok {stats} server_messages={}",
                    response.messages.max(response.messages_appended)
                ));
                true
            }
            Err(error) => {
                self.accounting.failed = self
                    .accounting
                    .failed
                    .saturating_add(outcome.message_count as u64);
                out.log(&format!("IMPORT_REQUEST fail {stats} error={error}"));
                for &index in &represented {
                    let Some(tracker) = self.trackers[index].as_mut() else {
                        continue;
                    };
                    if tracker.failed.is_none() {
                        tracker.failed = Some(error.clone());
                        journal.record_failure(
                            &outcome.batch.source,
                            &tracker.name,
                            "import",
                            &error,
                        );
                    }
                }
                false
            }
        };
        for index in represented {
            self.finish_file_if_settled(index, journal, out)?;
        }
        Ok(request_ok)
    }

    /// Split one request's duration across the conversations it carried.
    ///
    /// Charging the full request time to each one would overcount; the first
    /// conversation absorbs the remainder.
    fn charge_request_time(&mut self, represented: &BTreeSet<usize>, request_ms: u64) {
        let conversation_count = represented.len().max(1) as u64;
        let share_ms = request_ms / conversation_count;
        let remainder_ms = request_ms % conversation_count;
        for (position, &index) in represented.iter().enumerate() {
            if let Some(tracker) = self.trackers[index].as_mut() {
                let add = share_ms + if position == 0 { remainder_ms } else { 0 };
                tracker.profile.message_import_ms =
                    tracker.profile.message_import_ms.saturating_add(add);
            }
        }
    }

    /// If this conversation has no remaining import chunks (or already failed), write its result.
    ///
    /// A chat can be "queue complete" (all chunks handed to the pipeline)
    /// while some HTTP imports are still in flight. The file is marked done
    /// only when the last outstanding message count hits zero, or when a hard
    /// failure was recorded.
    ///
    /// # Errors
    ///
    /// Returns an error when the journal cannot be updated.
    fn finish_file_if_settled(
        &mut self,
        idx: usize,
        journal: &mut RunJournal,
        out: &mut Reporter<'_, '_>,
    ) -> Result<()> {
        let Some(tracker) = self.trackers[idx].as_mut() else {
            return Ok(());
        };
        if !tracker.is_settled() {
            return Ok(());
        }
        tracker.done = true;
        tracker.profile.total_ms = elapsed_ms(tracker.total_started);
        let name = tracker.name.clone();
        let profile = tracker.profile.clone();
        let attachments = tracker.attachments;

        let result = if let Some(error) = tracker.failed.clone() {
            out.note_failed(&name, &error, Some(&profile));
            FileResult {
                file: name.clone(),
                status: "failed".into(),
                error: Some(error),
                messages: 0,
                attachments,
                profile: Some(profile),
            }
        } else {
            let messages = tracker.successful_messages;
            journal.file_ok(&tracker.source, &name)?;
            // Keep quiet per-file detail in the on-disk log only.
            out.log(&format!(
                "ok {name} msgs={messages} attachments={attachments}"
            ));
            out.log(&format_profile_line(&name, &profile));
            out.note_ok(messages, &profile);
            FileResult {
                file: name.clone(),
                status: "ok".into(),
                error: None,
                messages,
                attachments,
                profile: Some(profile),
            }
        };
        out.file_done(&name, &result.status);
        self.results[idx] = Some(result);
        Ok(())
    }

    /// True once the caller asked the run to stop.
    fn is_cancelled(&self) -> bool {
        check_cancel(self.cfg.cancel.as_ref()).is_err()
    }

    /// Lock the shared journal (panics only if another thread panicked while holding it).
    fn lock_journal(&self) -> std::sync::MutexGuard<'a, SharedJournal> {
        self.journal.lock().expect("journal mutex poisoned")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::NO_MESSAGE_COUNT_LIMIT;

    fn chunk(body_bytes: usize, messages: usize) -> ImportChunk {
        ImportChunk {
            body: vec![b'x'; body_bytes],
            messages: (0..messages)
                .map(|index| JournalMessage {
                    file: "conversation.jsonl".into(),
                    guid: format!("guid-{index}"),
                })
                .collect(),
        }
    }

    #[test]
    fn import_body_limit_is_64_mib() {
        assert_eq!(MAX_IMPORT_BODY_BYTES, 64 * 1024 * 1024);
    }

    #[test]
    fn import_batch_flushes_for_message_or_byte_limit() {
        let mut batch = ImportBatch::new("imessage");
        batch.push(0, chunk(40, 2));

        assert!(batch.would_overflow(&chunk(10, 2), 3, 100));
        assert!(batch.would_overflow(&chunk(70, 1), 10, 100));
        assert!(!batch.would_overflow(&chunk(10, 1), 3, 100));
    }

    #[test]
    fn import_batch_does_not_flush_on_count_when_unlimited() {
        let mut batch = ImportBatch::new("imessage");
        batch.push(0, chunk(40, 2));
        assert!(
            !batch.would_overflow(&chunk(10, 50), NO_MESSAGE_COUNT_LIMIT, 1000),
            "desktop size-only flush must not split on message count"
        );
        assert!(batch.would_overflow(&chunk(70, 1), NO_MESSAGE_COUNT_LIMIT, 100));
    }
}
