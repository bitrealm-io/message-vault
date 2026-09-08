//! Upload a folder of conversation files into Message Vault.
//!
//! # What this module does
//!
//! An export folder has one `.jsonl` file per conversation, plus an
//! `attachments/` folder of media files. A push:
//!
//! 1. Logs in to the vault with the API key.
//! 2. For each conversation file, finds attachments, uploads any the vault
//!    does not already have, then sends the messages in batches.
//! 3. Remembers progress in a journal file so a later run can skip work that
//!    already succeeded.
//!
//! # Why it is built this way (upload performance)
//!
//! - **Attachments first, then messages.** Messages point at attachments by a
//!   content fingerprint (sha256). The vault must already have that file, or
//!   the import would fail. Media is uploaded before message text is sent.
//! - **Fingerprint = sha256.** Same bytes always produce the same hex string.
//!   The vault stores one copy per fingerprint, so the same photo shared in
//!   many chats is uploaded once.
//! - **Prepare ahead.** Reading a chat and uploading its media can take a long
//!   time. While the main loop waits on a message-import HTTP request, other
//!   threads can already prepare the next few conversations
//!   ([`crate::prepare`]). That hides disk and upload work behind network wait.
//! - **Several attachment uploads at once.** Small files are slow if sent one
//!   after another (network round trips dominate). Workers upload several at
//!   the same time.
//! - **One message-import request at a time.** Imports update shared vault
//!   state; running many imports in parallel is harder to reason about and
//!   can confuse the journal ([`crate::pipeline`]). Attachments stay parallel;
//!   message batches do not.
//! - **Size limits on each request.** Cloudflare (and similar proxies) reject
//!   huge single uploads. Message batches are split, and large attachments use
//!   multipart, so a big chat or video does not hit that wall.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;
use vault_api_types::ImportMode;

use anyhow::{Context, Result, bail};
use message_vault_io_core::{CancelFlag, check_cancel};

use crate::AuthInfo;
use crate::folder::{detect_source, file_label, input_folder, list_jsonl_files};
use crate::http::{HttpSession, ImportOutcome};
use crate::journal::{self, RunJournal};
use crate::pipeline::{ChunkStep, ImportPipeline};
use crate::prepare::{
    PrepareContext, PrepareOutcome, PrepareQueue, PrepareResult, PreparedFile, SharedJournal,
};
use crate::progress::{ProgressEvent, ProgressFn, Reporter};
use crate::report::{
    AssetTotals, PushReport, count_file_results, elapsed_ms, format_push_summary, now_stamp,
    outcome_status,
};

/// How many messages to pack into one import HTTP request when size is not the limit.
pub const DEFAULT_BATCH_SIZE: usize = 1_000;
/// Soft max size of one import request body (about 64 MiB).
///
/// Kept under Cloudflare's ~100 MiB upload cap so a large group chat is
/// split into several requests instead of one giant one that gets rejected.
pub const MAX_IMPORT_BODY_BYTES: usize = 64 * 1024 * 1024;
/// Sentinel for "do not flush import batches on message count; size only".
///
/// Desktop import uses this so SMS-style short messages pack until
/// [`MAX_IMPORT_BODY_BYTES`] instead of stopping at a small count.
pub const NO_MESSAGE_COUNT_LIMIT: usize = usize::MAX;
/// Max size for uploading an attachment in a single HTTP PUT.
///
/// Bigger files use multipart upload (many smaller pieces), which proxies
/// accept more reliably than one huge body.
pub const MAX_PROXY_BODY_BYTES: usize = 90 * 1024 * 1024;
/// Refuse attachments larger than this (must match the vault server setting).
pub const DEFAULT_ASSET_MAX_BYTES: u64 = 512 * 1024 * 1024;
/// How many attachment uploads may run at the same time.
pub const DEFAULT_ASSET_UPLOAD_WORKERS: usize = 8;
/// How many conversations may be prepared (read + upload media) ahead of the
/// import loop. Higher uses more memory/disk bandwidth; lower leaves the CPU
/// idle while waiting on the network.
pub const DEFAULT_PREPARE_AHEAD: usize = 3;
/// Worker threads that prepare conversations for that prepare-ahead queue.
pub const DEFAULT_PREPARE_WORKERS: usize = 2;

/// Settings for one full push run (paths, URL, flags, limits).
#[derive(Debug, Clone)]
pub struct VaultPushConfig {
    /// A folder of JSON Lines conversation files, or one such file.
    pub input: PathBuf,
    /// Vault base URL, e.g. `http://127.0.0.1:8080`.
    pub base_url: String,
    /// Account username, recorded in the report and progress events.
    pub username: String,
    /// API token or session token for the vault.
    pub key: String,
    /// `Append` adds to existing data; `Replace` clears then imports (with force).
    pub mode: ImportMode,
    /// If true, keep going after one conversation fails. If false, stop early.
    pub continue_on_error: bool,
    /// If true, ignore the journal and upload/import everything again.
    pub force: bool,
    /// Text-only import: do not upload or attach media.
    pub skip_attachments: bool,
    /// If true, always re-hash files and fail when the export's claimed sha256
    /// does not match the bytes on disk.
    ///
    /// If false (default), trust a SHA-256 fingerprint already written in the
    /// JSON Lines file when it is present. That skips a slow full-file hash for
    /// every attachment. Files with an empty digest are still hashed. A path
    /// cache avoids hashing the same file twice when several chats share it.
    pub verify_digests: bool,
    /// If true, skip re-hashing attachments when the JSON Lines `size_bytes` matches
    /// the file size on disk. Default remains full verification of every file.
    pub trust_export: bool,
    /// Extra tries per HTTP request after a transient failure.
    pub max_retries: u32,
    /// Messages per import request; at least 1.
    pub batch_size: usize,
    /// Max parallel attachment uploads. Message imports stay one-at-a-time.
    pub asset_upload_workers: usize,
    /// Conversations to prepare (read + upload media) ahead of the import loop.
    pub prepare_ahead: usize,
    /// Worker threads that prepare conversations for that prepare-ahead queue.
    pub prepare_workers: usize,
    /// Files larger than this use multipart upload instead of one PUT.
    pub asset_multipart_threshold: usize,
    /// Hard max attachment size this run will attempt to upload.
    pub asset_max_bytes: u64,
    /// Where to write the report JSON; `None` puts it beside the input.
    pub report_path: Option<PathBuf>,
    /// Where to write the run log; `None` puts it beside the input.
    pub log_path: Option<PathBuf>,
    /// Where the journal lives; `None` puts it beside the input.
    pub journal_path: Option<PathBuf>,
    /// Checked between files and uploads; set it to stop the run early.
    pub cancel: Option<CancelFlag>,
    /// Existing Import Run to post into when the caller already created one.
    pub import_id: Option<i64>,
}

/// Check the API key against the vault without importing any messages.
///
/// # Errors
///
/// Returns [`crate::AuthError`] when the URL is invalid, the host is unreachable,
/// or the key is rejected.
pub fn authenticate(base_url: &str, key: &str) -> std::result::Result<AuthInfo, crate::AuthError> {
    vault_http::auth_check(base_url, key)
}

/// The authenticated connection one push run uses for every request.
#[derive(Clone)]
pub(crate) struct Session {
    pub http: HttpSession,
    /// Base URL with any trailing slash removed.
    pub url: String,
    /// The API key every request carries.
    pub key: String,
    /// The account the API key resolved to (server-reported name, or the id).
    pub username: String,
    pub auth: AuthInfo,
}

/// Where this run reads from and writes its journal, report, and log.
struct RunPaths {
    input: PathBuf,
    report: PathBuf,
    log: PathBuf,
    journal: PathBuf,
}

impl RunPaths {
    /// Resolve the export folder and the three side files, honouring overrides in `cfg`.
    ///
    /// # Errors
    ///
    /// Returns an error when the input folder does not exist.
    fn resolve(cfg: &VaultPushConfig) -> Result<Self> {
        let input = input_folder(&cfg.input)?;
        Ok(Self {
            report: cfg
                .report_path
                .clone()
                .unwrap_or_else(|| input.join(journal::REPORT_NAME)),
            log: cfg
                .log_path
                .clone()
                .unwrap_or_else(|| input.join(journal::LOG_NAME)),
            journal: cfg
                .journal_path
                .clone()
                .unwrap_or_else(|| journal::journal_path(&input)),
            input,
        })
    }
}

/// Push every `.jsonl` conversation under `cfg.input`.
///
/// High-level flow:
/// 1. Log in, open the journal, and list conversation files.
/// 2. Start a few **prepare workers** ([`PrepareQueue`]). Each worker reads one
///    chat file, uploads its attachments, and builds message chunks.
/// 3. The main thread **consumes prepare results in file order**, packs
///    message chunks into import batches, and sends those batches over HTTP
///    ([`ImportPipeline`]). An import can start while prepare workers keep
///    working on later chats.
/// 4. Write the report and close the import session.
///
/// # Errors
///
/// Returns an error when setup fails, a worker disconnects, or the report cannot
/// be written. Per-conversation failures are recorded in the report when
/// `continue_on_error` is true.
pub fn run(cfg: &VaultPushConfig, progress: Option<&mut ProgressFn<'_>>) -> Result<PushReport> {
    let run_started = Instant::now();
    let started_at = now_stamp();
    let paths = RunPaths::resolve(cfg)?;
    let mut out = Reporter::open(&paths.log, progress)?;

    check_cancel(cfg.cancel.as_ref())?;
    let session = login(cfg, &mut out)?;
    let journal = RunJournal::open(
        paths.journal.clone(),
        &session.url,
        &session.username,
        cfg.force || cfg.mode == ImportMode::Replace,
    )?;
    let files = list_jsonl_files(&paths.input, &[&paths.journal, &paths.report, &paths.log])?;
    if files.is_empty() {
        bail!(
            "no .jsonl files under {} (export with JSONL in the Export tab first)",
            paths.input.display()
        );
    }
    out.expect_files(files.len());
    let import_id = start_import_run(cfg, &session, &paths.input, &mut out)?;
    let batch_size = cfg.batch_size.max(1);

    let shared = Mutex::new(SharedJournal::new(journal));
    let ctx = PrepareContext::new(&paths.input, cfg, &session, &shared, batch_size);
    let mut pipeline =
        ImportPipeline::new(cfg, &session, &shared, import_id, batch_size, files.len());
    let mut assets = AssetTotals::default();

    let aborted = drive(&ctx, &files, &mut pipeline, &mut assets, &mut out)?;
    let aborted = settle(cfg, &mut pipeline, aborted, &mut out)?;
    out.flush_file_counter();

    let (results, accounting) = pipeline.into_results();
    let journal = shared.into_inner().expect("journal mutex poisoned").journal;
    let counted = count_file_results(&results);
    // Only shrink/rewrite the journal after a clean run so a failed run can retry.
    if counted.failed == 0 && !aborted {
        let _ = journal.compact();
    }
    let report = PushReport {
        ok: counted.failed == 0 && !aborted,
        account: session.auth.account_id,
        username: session.username.clone(),
        mode: cfg.mode,
        started_at,
        finished_at: now_stamp(),
        elapsed_ms: elapsed_ms(run_started),
        conversations_total: files.len() as u64,
        conversations_ok: counted.ok,
        conversations_failed: counted.failed,
        conversations_skipped: counted.skipped,
        messages_attempted: accounting.attempted,
        messages_inserted: accounting.inserted,
        messages_deduped: accounting.deduped,
        messages_failed: accounting.failed,
        messages: counted.messages,
        assets_uploaded: assets.uploaded,
        assets_skipped: assets.skipped,
        assets_bytes: assets.bytes,
        results,
    };
    write_report(&paths.report, &report)?;
    if cfg.import_id.is_none() {
        complete_import_session(
            &session,
            import_id,
            &report,
            counted.attachments,
            aborted,
            &mut out,
        );
    }
    out.log("");
    out.log(&format_push_summary(&report));
    out.conversation_issues(&report.results);
    out.event(ProgressEvent::Log(String::new()));
    out.event(ProgressEvent::Finished(report.clone()));
    Ok(report)
}

/// Check the API key and pick the account name the rest of the run uses.
///
/// The API key decides which account this run uses. The username the server
/// returns wins; the account id is the fallback when that is empty.
///
/// # Errors
///
/// Returns an error when the HTTP client cannot be built or the key is rejected.
fn login(cfg: &VaultPushConfig, out: &mut Reporter<'_, '_>) -> Result<Session> {
    let url = cfg.base_url.trim_end_matches('/').to_string();
    let http = HttpSession::new()?;
    let auth = http.auth_check(&url, &cfg.key)?;
    let username = auth
        .username
        .as_deref()
        .and_then(message_ir::trimmed)
        .map_or_else(|| auth.account_id.to_string(), str::to_string);
    out.log(&format!(
        "authenticated username={username} account={}",
        auth.account_id
    ));
    out.event(ProgressEvent::Auth {
        account_id: auth.account_id,
        username: username.clone(),
    });
    out.event(ProgressEvent::Log(format!("Authenticated as {username}")));
    if cfg.skip_attachments {
        out.show_as(
            "skip_attachments=true (text-only import)",
            "Skipping attachments (text-only import)".into(),
        );
    }
    Ok(Session {
        http,
        url,
        key: cfg.key.clone(),
        username,
        auth,
    })
}

/// Create the Import Run every batch is posted into, or reuse the one the
/// caller already created. There is no run without one: a vault that refuses
/// to start it ends the push here, before any file is read.
///
/// # Errors
///
/// Returns the vault's refusal, which includes an account that already has a
/// running Import Run.
fn start_import_run(
    cfg: &VaultPushConfig,
    session: &Session,
    input: &Path,
    out: &mut Reporter<'_, '_>,
) -> Result<i64> {
    let source = detect_source(input)
        .ok()
        .flatten()
        .unwrap_or_else(|| "unknown".to_string());
    if let Some(import_id) = cfg.import_id {
        out.show_as(
            &format!("using provided Import Run id={import_id}"),
            format!("Reusing Import Run {import_id} ({source})"),
        );
        return Ok(import_id);
    }
    let id = session
        .start_import(&source, cfg.mode, Some("vault-push"))
        .context("start the Import Run on the vault")?;
    out.show_as(
        &format!("Import Run id={id} source={source}"),
        format!("Recording Import Run {id} ({source})"),
    );
    Ok(id)
}

/// Run the prepare workers and the import loop until every file is consumed
/// or the run aborts. Returns `true` when it aborted.
///
/// # Errors
///
/// Returns an error when a worker disconnects or the journal cannot be updated.
fn drive(
    ctx: &PrepareContext<'_>,
    files: &[PathBuf],
    pipeline: &mut ImportPipeline<'_>,
    assets: &mut AssetTotals,
    out: &mut Reporter<'_, '_>,
) -> Result<bool> {
    let total = files.len();
    let prepare_ahead = ctx.cfg.prepare_ahead.max(1);
    let prepare_workers = ctx.cfg.prepare_workers.max(1).min(prepare_ahead);

    std::thread::scope(|scope| -> Result<bool> {
        let mut queue = PrepareQueue::start(scope, ctx, prepare_workers, prepare_ahead);
        let mut aborted = false;
        let mut next_submit = 0usize;
        let mut next_consume = 0usize;

        while next_consume < total {
            // Cancel must still join the in-flight import and write a report.
            if check_cancel(ctx.cfg.cancel.as_ref()).is_err() {
                aborted = true;
                break;
            }
            // A large ready batch goes out now (without waiting) so prepare
            // workers keep the pipeline full.
            if pipeline.pending_is_worth_overlapping()
                && !pipeline.flush_and_continue(false, out)?
            {
                aborted = true;
                break;
            }
            while next_submit < total && queue.has_capacity() {
                submit_file(
                    ctx,
                    next_submit,
                    &files[next_submit],
                    total,
                    &mut queue,
                    pipeline,
                    out,
                );
                next_submit += 1;
            }
            if !queue.is_ready(next_consume) {
                if queue.is_idle() && next_submit >= total {
                    break;
                }
                queue.wait_one()?;
            }
            // Process every consecutive ready index starting at `next_consume`.
            while let Some(result) = queue.take(next_consume) {
                if consume_result(ctx, result, pipeline, assets, out)? {
                    next_consume += 1;
                } else {
                    aborted = true;
                    break;
                }
            }
            if aborted {
                break;
            }
        }

        for leftover in queue.shutdown() {
            if let PrepareOutcome::Prepared(prepared) = leftover.outcome {
                absorb_prepared(&prepared, assets, out);
            }
        }
        Ok(aborted)
    })
}

/// Queue one conversation for prepare, or record it as skipped when the
/// journal says the whole file already imported.
fn submit_file(
    ctx: &PrepareContext<'_>,
    idx: usize,
    path: &Path,
    total: usize,
    queue: &mut PrepareQueue,
    pipeline: &mut ImportPipeline<'_>,
    out: &mut Reporter<'_, '_>,
) {
    let name = file_label(path);
    out.file_start(idx + 1, total, &name);
    if ctx.already_imported(&name) {
        pipeline.record_skipped(idx, &name, out);
        queue.mark_skipped(idx, name);
    } else {
        queue.submit(idx, path.to_path_buf(), name);
    }
}

/// Feed one prepare result into the import pipeline. Returns `false` when the
/// run must abort.
///
/// # Errors
///
/// Returns an error when the journal cannot be updated or the import thread panicked.
fn consume_result(
    ctx: &PrepareContext<'_>,
    result: PrepareResult,
    pipeline: &mut ImportPipeline<'_>,
    assets: &mut AssetTotals,
    out: &mut Reporter<'_, '_>,
) -> Result<bool> {
    let continue_on_error = ctx.cfg.continue_on_error;
    match result.outcome {
        PrepareOutcome::Skipped => Ok(true),
        PrepareOutcome::Failed(error) => {
            // Let the in-flight import land before stopping so the journal is consistent.
            if !continue_on_error
                && pipeline.has_work()
                && !pipeline.flush_and_continue(true, out)?
            {
                return Ok(false);
            }
            pipeline.record_prepare_failure(result.idx, &result.name, &error, out);
            Ok(continue_on_error)
        }
        PrepareOutcome::Prepared(prepared) => {
            absorb_prepared(&prepared, assets, out);
            if pipeline.pending_source_differs(&prepared.source)
                && !pipeline.flush_and_continue(!continue_on_error, out)?
            {
                return Ok(false);
            }
            pipeline.start_file(result.idx, &result.name, &prepared);
            for chunk in prepared.chunks {
                match pipeline.queue_chunk(result.idx, chunk, out)? {
                    ChunkStep::Continue => {}
                    ChunkStep::FileFailed => break,
                    ChunkStep::Abort => return Ok(false),
                }
            }
            pipeline.finish_queueing(result.idx, out)?;
            Ok(true)
        }
    }
}

/// Fold one prepared conversation's asset totals and log output into the run.
fn absorb_prepared(prepared: &PreparedFile, assets: &mut AssetTotals, out: &mut Reporter<'_, '_>) {
    assets.add(prepared.assets);
    for line in &prepared.log_lines {
        out.log(line);
    }
    out.attachment_skips(&prepared.attachment_skips);
}

/// End of run: send any leftover batch and wait for the last import. An
/// aborted run still waits for the in-flight import so the journal stays
/// consistent. Returns the final aborted flag.
///
/// # Errors
///
/// Returns an error when the journal cannot be updated or the import thread panicked.
fn settle(
    cfg: &VaultPushConfig,
    pipeline: &mut ImportPipeline<'_>,
    aborted: bool,
    out: &mut Reporter<'_, '_>,
) -> Result<bool> {
    let mut aborted = aborted || check_cancel(cfg.cancel.as_ref()).is_err();
    if !aborted && !pipeline.flush_and_continue(true, out)? {
        aborted = true;
    }
    if aborted {
        let _ = pipeline.join_inflight(out);
    }
    Ok(aborted)
}

/// Write the report JSON next to the export.
///
/// # Errors
///
/// Returns an error when the folder cannot be created or the file cannot be written.
fn write_report(path: &Path, report: &PushReport) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(report).context("serialize report")?,
    )
    .with_context(|| format!("write report {}", path.display()))
}

/// Tell the vault how the import session ended. Best effort: a failure here
/// is logged, not returned, because the data is already in the vault.
fn complete_import_session(
    session: &Session,
    import_id: i64,
    report: &PushReport,
    attachment_count: u64,
    aborted: bool,
    out: &mut Reporter<'_, '_>,
) {
    let completed = session.complete_import(
        import_id,
        &ImportOutcome {
            ok: report.ok,
            status: outcome_status(report, aborted),
            message_count: report.messages,
            attachment_count,
            bytes_uploaded: report.assets_bytes,
        },
    );
    match completed {
        Ok(()) => out.log(&format!("vault import session {import_id} completed")),
        Err(error) => out.log(&format!(
            "warning: could not complete vault import session {import_id}: {error}"
        )),
    }
}
