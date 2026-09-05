//! The expensive per-conversation step: read one JSON Lines file, upload its
//! attachments, and cut its messages into import-sized chunks.
//!
//! Prepare work runs on a small pool of worker threads ([`PrepareQueue`]) so
//! disk reads and media uploads for the next few chats overlap with the
//! message-import HTTP request the main thread is waiting on. Everything a
//! worker needs is read-only and lives in [`PrepareContext`]; the only shared
//! mutable state is the journal behind a mutex.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::Scope;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use message_ir::{ConversationDocument, ConversationHeader, IrAttachment, IrMessage};
use message_ir_format::read_conversation_jsonl;
use message_vault_io_core::{check_cancel, parallel_for_each};

use crate::folder::{attachment_label, resolve_attachment, safe_rel};
use crate::http::{AssetPutResponse, AssetUpload};
use crate::journal::{JournalMessage, RunJournal};
use crate::progress::AttachmentSkip;
use crate::project::{self, AttachmentProjection};
use crate::report::{AssetTotals, UploadProfile, elapsed_ms};
use crate::run::{MAX_IMPORT_BODY_BYTES, Session, VaultPushConfig};

/// Journal state shared by every prepare worker and the import pipeline.
///
/// `assets_in_flight` stops two workers from uploading the same sha256 at once
/// when two chats share a file that is not in the journal yet.
#[derive(Debug)]
pub(crate) struct SharedJournal {
    pub journal: RunJournal,
    assets_in_flight: HashSet<String>,
}

impl SharedJournal {
    /// Wrap a run journal for sharing across threads.
    pub(crate) fn new(journal: RunJournal) -> Self {
        Self {
            journal,
            assets_in_flight: HashSet::new(),
        }
    }

    /// Try to reserve this sha256 for upload. Returns false if another worker
    /// already uploaded it or is uploading it (unless `force`).
    fn claim_asset(&mut self, digest: &str, force: bool) -> bool {
        if !force && (self.journal.has_asset(digest) || self.assets_in_flight.contains(digest)) {
            return false;
        }
        self.assets_in_flight.insert(digest.to_string());
        true
    }

    /// Give up a claim without marking the digest uploaded, so a retry (or
    /// another conversation sharing the file) is free to try again.
    fn release_asset(&mut self, digest: &str) {
        self.assets_in_flight.remove(digest);
    }

    /// Clear the claim and record the digest as present in the vault.
    fn asset_uploaded(&mut self, source: &str, digest: &str) -> Result<()> {
        self.assets_in_flight.remove(digest);
        self.journal.asset_ok(source, digest)
    }
}

/// Shared map: absolute file path → sha256 hex string.
///
/// The same attachment file can appear in many chats. Caching the hash means
/// that file is read and hashed only once per push run.
type DigestCache = Mutex<HashMap<PathBuf, String>>;

/// Read-only inputs every prepare worker shares for the whole run.
pub(crate) struct PrepareContext<'a> {
    pub input: &'a Path,
    pub cfg: &'a VaultPushConfig,
    pub session: &'a Session,
    pub journal: &'a Mutex<SharedJournal>,
    pub batch_size: usize,
    digests: DigestResolver,
    /// Set once any HEAD or PUT reports the vault already has an asset. From
    /// then on workers HEAD before PUT so a re-import sends no bodies.
    probe_existing: AtomicBool,
    /// Guards the single preflight HEAD so parallel chats do not race it.
    preflight_done: Mutex<bool>,
}

impl<'a> PrepareContext<'a> {
    /// Bundle the run's shared inputs for the prepare workers.
    pub(crate) fn new(
        input: &'a Path,
        cfg: &'a VaultPushConfig,
        session: &'a Session,
        journal: &'a Mutex<SharedJournal>,
        batch_size: usize,
    ) -> Self {
        Self {
            input,
            cfg,
            session,
            journal,
            batch_size,
            digests: DigestResolver {
                cache: Mutex::new(HashMap::new()),
                verify_digests: cfg.verify_digests,
                trust_export: cfg.trust_export,
            },
            probe_existing: AtomicBool::new(false),
            preflight_done: Mutex::new(false),
        }
    }

    /// Lock the shared journal (panics only if another thread panicked while holding it).
    pub(crate) fn lock_journal(&self) -> MutexGuard<'_, SharedJournal> {
        self.journal.lock().expect("journal mutex poisoned")
    }

    /// True when the journal says this conversation file already fully imported.
    pub(crate) fn already_imported(&self, name: &str) -> bool {
        self.cfg.mode == "append" && !self.cfg.force && self.lock_journal().journal.has_file(name)
    }
}

/// One piece of an import request: NDJSON body bytes plus the message ids in it.
pub(crate) struct ImportChunk {
    pub body: Vec<u8>,
    pub messages: Vec<JournalMessage>,
}

/// Output of preparing one conversation: uploaded media + message chunks ready to import.
pub(crate) struct PreparedFile {
    pub source: String,
    pub chunks: Vec<ImportChunk>,
    pub attachments: u64,
    pub profile: UploadProfile,
    pub total_started: Instant,
    pub assets: AssetTotals,
    pub log_lines: Vec<String>,
    pub attachment_skips: Vec<AttachmentSkip>,
}

impl PreparedFile {
    /// Messages queued for import across every chunk.
    pub(crate) fn message_count(&self) -> usize {
        self.chunks.iter().map(|chunk| chunk.messages.len()).sum()
    }
}

/// Read one conversation JSON Lines file, upload its attachments, split messages into import chunks.
///
/// Design choices:
/// - Collect **unique** attachment digests first, then upload each digest once
///   (a photo sent twice in the same chat should not be uploaded twice).
/// - Upload media **before** building message lines that reference those digests.
/// - Split messages into chunks sized for Cloudflare-safe import requests.
///
/// # Errors
///
/// Returns an error when the file cannot be read, an attachment path is unsafe,
/// hashing fails, or an upload fails.
pub(crate) fn prepare_file(
    ctx: &PrepareContext<'_>,
    path: &Path,
    name: &str,
) -> Result<PreparedFile> {
    let total_started = Instant::now();
    let read_started = Instant::now();
    let doc = read_conversation_jsonl(path)?;
    let mut profile = UploadProfile {
        read_ms: elapsed_ms(read_started),
        ..UploadProfile::default()
    };
    let header = ConversationHeader::from_document(&doc);
    let source = project::validate_header(&header)?;

    let scan_started = Instant::now();
    let scan = if ctx.cfg.skip_attachments {
        AttachmentScan::text_only(&doc.messages)
    } else {
        scan_attachments(ctx, name, &doc.messages)?
    };
    profile.attachment_scan_hash_ms = elapsed_ms(scan_started);
    profile.unique_assets = u64::try_from(scan.unique.len()).unwrap_or(u64::MAX);

    let mut log_lines: Vec<String> = scan.warnings.iter().map(|w| format!("WARN {w}")).collect();
    let mut assets = AssetTotals {
        skipped: scan.skipped,
        ..AssetTotals::default()
    };
    if !ctx.cfg.skip_attachments {
        let upload_started = Instant::now();
        let uploaded = upload_assets(ctx, name, &source, &scan.unique)?;
        profile.asset_upload_ms = elapsed_ms(upload_started);
        profile.asset_bytes = uploaded.bytes;
        assets.add(AssetTotals {
            uploaded: uploaded.uploaded,
            skipped: uploaded.skipped,
            bytes: uploaded.bytes,
        });
        log_lines.extend(uploaded.log_lines);
    }

    let chunks = build_import_chunks(ctx, name, &doc, &scan.projections)?;
    Ok(PreparedFile {
        source,
        chunks,
        attachments: scan.count,
        profile,
        total_started,
        assets,
        log_lines,
        attachment_skips: scan.skips,
    })
}

/// What the attachment pass learned about one conversation.
struct AttachmentScan {
    /// Per message: how each attachment maps onto the import line.
    projections: Vec<Vec<AttachmentProjection>>,
    /// sha256 → (relative path, mime). `BTreeMap` keeps a stable upload order.
    unique: BTreeMap<String, (String, Option<String>)>,
    /// Attachments seen, uploaded or not.
    count: u64,
    /// Attachments left out of the upload (no path, missing, too large).
    skipped: u64,
    /// Import Errors rows for skips that deserve one.
    skips: Vec<AttachmentSkip>,
    /// Digest warnings to write to the log.
    warnings: Vec<String>,
}

impl AttachmentScan {
    /// The scan for a text-only push: count every attachment as skipped,
    /// upload nothing, and reference nothing from the import lines.
    fn text_only(messages: &[IrMessage]) -> Self {
        let count = messages.iter().map(|m| m.attachments.len() as u64).sum();
        Self {
            projections: messages.iter().map(|_| Vec::new()).collect(),
            unique: BTreeMap::new(),
            count,
            skipped: count,
            skips: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

/// Walk every attachment, decide whether it can be uploaded, and fingerprint the ones that can.
///
/// # Errors
///
/// Returns an error for an unsafe attachment path, an unreadable file, or a
/// digest mismatch when `verify_digests` is on.
fn scan_attachments(
    ctx: &PrepareContext<'_>,
    name: &str,
    messages: &[IrMessage],
) -> Result<AttachmentScan> {
    let mut scan = AttachmentScan {
        projections: Vec::with_capacity(messages.len()),
        unique: BTreeMap::new(),
        count: 0,
        skipped: 0,
        skips: Vec::new(),
        warnings: Vec::new(),
    };
    for msg in messages {
        let mut projections = Vec::with_capacity(msg.attachments.len());
        for (index, att) in msg.attachments.iter().enumerate() {
            scan.count += 1;
            projections.push(scan_one_attachment(ctx, name, att, index, &mut scan)?);
        }
        scan.projections.push(projections);
    }
    Ok(scan)
}

/// Classify one attachment: missing from the export, missing on disk, too
/// large, or uploadable with a known digest.
///
/// # Errors
///
/// Returns an error for an unsafe path, an unreadable file, or a digest
/// mismatch when `verify_digests` is on.
fn scan_one_attachment(
    ctx: &PrepareContext<'_>,
    name: &str,
    att: &IrAttachment,
    index: usize,
    scan: &mut AttachmentScan,
) -> Result<AttachmentProjection> {
    let Some(rel) = att.path.as_deref().and_then(message_ir::trimmed) else {
        // No path means the bytes were never staged. "Do not copy" exports
        // look like this, and the reason the exporter set ("not_copied";
        // older exports say "skipped" or "embed_disabled") explains why.
        // Keep the metadata so the thread still shows the file was there.
        scan.skipped += 1;
        if att.missing_reason.is_none() {
            // An exporter dropped the path without saying why. That is a
            // defect, so it earns an Import Errors row; a deliberate skip
            // does not.
            scan.skips.push(AttachmentSkip {
                item: format!("{name}:{}", attachment_label(att, index)),
                reason: "attachment has no file path in the export".into(),
            });
        }
        return Ok(AttachmentProjection::Missing {
            index,
            reason: att.missing_reason.as_deref().unwrap_or("no_path").into(),
            size: att.size_bytes,
        });
    };
    safe_rel(rel)?;
    let Some(abs) = resolve_attachment(ctx.input, rel) else {
        scan.skipped += 1;
        scan.skips.push(AttachmentSkip {
            item: format!("{name}:{rel}"),
            reason: "attachment file not found on disk".into(),
        });
        return Ok(AttachmentProjection::Missing {
            index,
            reason: "file_missing".into(),
            size: att.size_bytes,
        });
    };
    let file_len = std::fs::metadata(&abs)
        .with_context(|| format!("{name}: stat attachment {rel}"))?
        .len();
    if file_len > ctx.cfg.asset_max_bytes {
        scan.skipped += 1;
        scan.skips.push(AttachmentSkip {
            item: format!("{name}:{rel}"),
            reason: format!(
                "attachment is {} bytes ({} MiB), over the configured asset max of {} MiB",
                file_len,
                file_len / message_ir::MIB,
                ctx.cfg.asset_max_bytes / message_ir::MIB
            ),
        });
        return Ok(AttachmentProjection::Missing {
            index,
            reason: "too_large".into(),
            size: Some(file_len),
        });
    }
    let claimed = att.digest_sha256.as_deref().and_then(message_ir::trimmed);
    let digest = ctx
        .digests
        .resolve(&abs, claimed, att.size_bytes, name, rel, &mut |warning| {
            scan.warnings.push(warning);
        })?;
    scan.unique
        .entry(digest.clone())
        .or_insert_with(|| (rel.to_string(), att.mime_type.clone()));
    Ok(AttachmentProjection::Digested {
        index,
        digest,
        size: file_len,
    })
}

/// Cut a conversation's messages into import chunks: each chunk is "header
/// line + many message lines" as NDJSON bytes, sized under the request limits.
///
/// Messages the journal already saw are left out unless `force` is set.
///
/// # Errors
///
/// Returns an error when a message cannot be encoded, a single message alone
/// exceeds the chunk limit, or the run is cancelled.
fn build_import_chunks(
    ctx: &PrepareContext<'_>,
    name: &str,
    doc: &ConversationDocument,
    projections: &[Vec<AttachmentProjection>],
) -> Result<Vec<ImportChunk>> {
    let mut builder = ChunkBuilder::new(project::document_header_line(doc)?, ctx.batch_size);
    for (i, msg) in doc.messages.iter().enumerate() {
        check_cancel(ctx.cfg.cancel.as_ref())?;
        let (line, guid) = if ctx.cfg.skip_attachments {
            project::message_line_without_attachments(msg, i)?
        } else {
            // Rewrite attachment fields to uploaded digests or missing placeholders.
            project::message_line(msg, &projections[i], i)?
        };
        if !ctx.cfg.force && ctx.lock_journal().journal.has_message(name, &guid) {
            // Already imported this message id on a previous successful push.
            continue;
        }
        // A single message larger than the chunk limit cannot be split further.
        if line.len() > MAX_IMPORT_BODY_BYTES {
            bail!(
                "{name}: message {guid} encodes to {} bytes alone, which exceeds the \
                 {} MiB import chunk limit — cannot upload through Cloudflare safely",
                line.len(),
                MAX_IMPORT_BODY_BYTES as u64 / message_ir::MIB
            );
        }
        builder.push(
            &line,
            JournalMessage {
                file: name.to_string(),
                guid,
            },
        );
    }
    Ok(builder.finish())
}

/// Accumulates message lines into [`ImportChunk`]s under a count and byte budget.
struct ChunkBuilder {
    header_line: Vec<u8>,
    max_messages: usize,
    chunks: Vec<ImportChunk>,
    body: Vec<u8>,
    messages: Vec<JournalMessage>,
}

impl ChunkBuilder {
    /// Start with an empty chunk that already holds the conversation header.
    fn new(header_line: Vec<u8>, max_messages: usize) -> Self {
        Self {
            body: header_line.clone(),
            header_line,
            max_messages,
            chunks: Vec::new(),
            messages: Vec::new(),
        }
    }

    /// Add one encoded message, starting a new chunk first when this one is full.
    fn push(&mut self, line: &[u8], message: JournalMessage) {
        let full = !self.messages.is_empty()
            && (self.messages.len() >= self.max_messages
                || self.body.len() + line.len() > MAX_IMPORT_BODY_BYTES);
        if full {
            self.seal();
        }
        self.body.extend_from_slice(line);
        self.messages.push(message);
    }

    /// Move the current chunk onto the finished list and start a fresh one.
    fn seal(&mut self) {
        self.chunks.push(ImportChunk {
            body: std::mem::replace(&mut self.body, self.header_line.clone()),
            messages: std::mem::take(&mut self.messages),
        });
    }

    /// Seal the last partial chunk and return every chunk in order.
    fn finish(mut self) -> Vec<ImportChunk> {
        if !self.messages.is_empty() {
            self.seal();
        }
        self.chunks
    }
}

/// Decides whether an attachment's sha256 comes from the export file, the
/// per-run cache, or a fresh hash of the bytes on disk.
struct DigestResolver {
    cache: DigestCache,
    verify_digests: bool,
    trust_export: bool,
}

impl DigestResolver {
    /// Resolve the SHA-256 fingerprint for an attachment file.
    ///
    /// The default is to hash every file from disk, compare against any JSON
    /// Lines claim, and warn on mismatch (using the actual disk hash). Two
    /// flags alter this:
    ///
    /// * `trust_export` — skip the hash when the JSON Lines `size_bytes`
    ///   matches the file size on disk (a cheap proxy for "file unchanged
    ///   since export").
    /// * `verify_digests` — hash from disk and **fail** on mismatch.
    ///
    /// The vault server is the final verifier on upload; a stale fingerprint
    /// is self-correcting (the server rejects mismatches).
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be hashed, or when
    /// `verify_digests` is on and the on-disk hash does not match the claim.
    fn resolve(
        &self,
        abs: &Path,
        claimed_raw: Option<&str>,
        claimed_size: Option<u64>,
        name: &str,
        rel: &str,
        warn: &mut dyn FnMut(String),
    ) -> Result<String> {
        // Fast path: another conversation already hashed this absolute path
        // during this run. Always trust the cache — it was computed from disk.
        if let Some(digest) = self.cached(abs) {
            return Ok(digest);
        }

        let claimed = claimed_raw.and_then(|raw| match normalize_digest_sha256(raw) {
            Ok(digest) => Some(digest),
            Err(e) => {
                warn(format!("{name}: bad digest_sha256 for {rel}: {e}"));
                None
            }
        });

        let disk_size = std::fs::metadata(abs)
            .with_context(|| format!("{name}: stat {rel}"))?
            .len();

        if self.trust_export
            && !self.verify_digests
            && let (Some(claimed_digest), Some(claimed_size)) = (claimed.as_deref(), claimed_size)
            && claimed_size == disk_size
        {
            self.remember(abs, claimed_digest);
            return Ok(claimed_digest.to_string());
        }

        let disk_digest =
            message_ir::file_sha256(abs).with_context(|| format!("{name}: hash {rel}"))?;
        if let Some(claimed_digest) = claimed.as_deref()
            && claimed_digest != disk_digest
        {
            let size_note = match claimed_size {
                Some(cs) if cs != disk_size => {
                    format!(", size changed from {cs} to {disk_size} bytes")
                }
                _ => String::new(),
            };
            let msg = format!(
                "{name}: sha256 mismatch for {rel}: \
                 claimed {claimed_digest}, got {disk_digest}{size_note}"
            );
            if self.verify_digests {
                bail!("{msg}");
            }
            warn(msg);
        }
        self.remember(abs, &disk_digest);
        Ok(disk_digest)
    }

    /// The digest another conversation already computed for this path, if any.
    fn cached(&self, abs: &Path) -> Option<String> {
        self.cache
            .lock()
            .expect("digest cache mutex poisoned")
            .get(abs)
            .cloned()
    }

    /// Store one file's sha256 so other conversations sharing the file skip hashing.
    fn remember(&self, abs: &Path, digest: &str) {
        self.cache
            .lock()
            .expect("digest cache mutex poisoned")
            .insert(abs.to_path_buf(), digest.to_string());
    }
}

/// Check that a SHA-256 fingerprint is exactly 64 hex digits; return lowercase form.
///
/// # Errors
///
/// Returns an error when the string is not 64 hexadecimal characters.
fn normalize_digest_sha256(digest: &str) -> Result<String> {
    let s = digest.trim().to_ascii_lowercase();
    if s.len() != 64 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("invalid sha256 digest (expected 64 hex digits)");
    }
    Ok(s)
}

/// One attachment a worker should HEAD/PUT.
struct AssetUploadJob {
    digest: String,
    path: PathBuf,
    mime: Option<String>,
}

/// Totals from uploading one conversation's unique attachments.
#[derive(Default)]
struct AssetUploadStats {
    bytes: u64,
    uploaded: u64,
    skipped: u64,
    log_lines: Vec<String>,
}

/// Upload each unique attachment for one conversation (several workers in parallel).
///
/// PUT first after one cheap HEAD of the first queued digest in this run.
/// If that HEAD reports `already_present`, later files HEAD and skip the body
/// (re-import). If it misses, this run PUTs until a response sets the flag.
///
/// # Errors
///
/// Returns an error when a file is missing or oversized, when HEAD/PUT fails
/// after retries, or when a worker panics.
fn upload_assets(
    ctx: &PrepareContext<'_>,
    name: &str,
    source: &str,
    unique: &BTreeMap<String, (String, Option<String>)>,
) -> Result<AssetUploadStats> {
    let mut stats = AssetUploadStats::default();
    let jobs = claim_upload_jobs(ctx, name, unique, &mut stats)?;
    let Some(first) = jobs.first() else {
        return Ok(stats);
    };
    preflight_existing_asset(ctx, source, &first.digest)?;

    // Work-stealing style: workers pull the next job index from a shared counter.
    let results = parallel_for_each(
        &jobs,
        ctx.cfg.asset_upload_workers,
        ctx.cfg.cancel.as_ref(),
        |job| upload_one_asset(ctx, source, job).map_err(|error| error.to_string()),
    );

    // Apply journal updates in a stable order after all workers finish.
    for (job, result) in jobs.iter().zip(results) {
        match result {
            Ok(response) => {
                ctx.lock_journal().asset_uploaded(source, &job.digest)?;
                let outcome = if response.already_present {
                    stats.skipped += 1;
                    "skip"
                } else {
                    stats.uploaded += 1;
                    "ok"
                };
                stats
                    .log_lines
                    .push(format!("asset {outcome} {}", job.digest));
            }
            Err(error) => {
                // Release every in-flight claim so a retry is not stuck forever.
                let mut guard = ctx.lock_journal();
                for job in &jobs {
                    guard.release_asset(&job.digest);
                }
                bail!("{name}: {error}");
            }
        }
    }
    Ok(stats)
}

/// Build the upload work list, claiming each digest in the shared journal and
/// skipping the ones another chat already uploaded or is uploading.
///
/// # Errors
///
/// Returns an error when a claimed file is missing, unreadable, or larger
/// than the configured asset limit. The claim is released before returning.
fn claim_upload_jobs(
    ctx: &PrepareContext<'_>,
    name: &str,
    unique: &BTreeMap<String, (String, Option<String>)>,
    stats: &mut AssetUploadStats,
) -> Result<Vec<AssetUploadJob>> {
    let mut jobs = Vec::with_capacity(unique.len());
    for (digest, (rel, mime)) in unique {
        check_cancel(ctx.cfg.cancel.as_ref())?;
        if !ctx.lock_journal().claim_asset(digest, ctx.cfg.force) {
            stats.skipped += 1;
            continue;
        }
        let (path, file_len) = match check_upload_file(ctx, name, rel) {
            Ok(checked) => checked,
            Err(error) => {
                ctx.lock_journal().release_asset(digest);
                return Err(error);
            }
        };
        stats.bytes = stats.bytes.saturating_add(file_len);
        jobs.push(AssetUploadJob {
            digest: digest.clone(),
            path,
            mime: mime.clone(),
        });
    }
    Ok(jobs)
}

/// Locate one attachment on disk and confirm it is under the size limit.
/// Returns the path and its size in bytes.
///
/// # Errors
///
/// Returns an error when the file is missing, cannot be stat'ed, or is too large.
fn check_upload_file(ctx: &PrepareContext<'_>, name: &str, rel: &str) -> Result<(PathBuf, u64)> {
    let Some(path) = resolve_attachment(ctx.input, rel) else {
        bail!("{name}: missing attachment {rel}");
    };
    let file_len = std::fs::metadata(&path)
        .with_context(|| format!("stat {}", path.display()))?
        .len();
    if file_len > ctx.cfg.asset_max_bytes {
        bail!(
            "{name}: attachment {rel} is {} bytes ({} MiB), over the configured \
             asset max of {} MiB. Raise vault [server] asset_max_bytes (and \
             vault-push --asset-max-bytes) or omit the file.",
            file_len,
            file_len / message_ir::MIB,
            ctx.cfg.asset_max_bytes / message_ir::MIB
        );
    }
    Ok((path, file_len))
}

/// One HEAD of the first queued digest for this run. If the vault already has
/// it, enable HEAD-skip so later files do not send PUT bodies.
///
/// Holding the preflight lock during that HEAD keeps parallel conversations
/// from PUTting duplicate bodies before the answer is known.
///
/// # Errors
///
/// Returns an error when the HEAD fails after retries.
fn preflight_existing_asset(ctx: &PrepareContext<'_>, source: &str, digest: &str) -> Result<()> {
    if ctx.probe_existing.load(Ordering::Relaxed) {
        return Ok(());
    }
    let mut done = ctx.preflight_done.lock().expect("preflight mutex poisoned");
    if ctx.probe_existing.load(Ordering::Relaxed) || *done {
        return Ok(());
    }
    *done = true;
    let session = ctx.session;
    let present =
        vault_http::with_retries(ctx.cfg.max_retries, || session.head_asset(source, digest))?;
    if present.is_some() {
        ctx.probe_existing.store(true, Ordering::Relaxed);
    }
    Ok(())
}

/// HEAD (when the vault is known to have assets already) then PUT one attachment, with retries.
///
/// # Errors
///
/// Returns the last HTTP error once retries are exhausted.
fn upload_one_asset(
    ctx: &PrepareContext<'_>,
    source: &str,
    job: &AssetUploadJob,
) -> Result<AssetPutResponse> {
    let session = ctx.session;
    vault_http::with_retries(ctx.cfg.max_retries, || {
        if ctx.probe_existing.load(Ordering::Relaxed)
            && let Some(existing) = session.head_asset(source, &job.digest)?
        {
            return Ok(existing);
        }
        let response = session.put_asset(&AssetUpload {
            source,
            sha256: &job.digest,
            file: &job.path,
            mime: job.mime.as_deref(),
            multipart_threshold: ctx.cfg.asset_multipart_threshold,
        })?;
        if response.already_present {
            ctx.probe_existing.store(true, Ordering::Relaxed);
        }
        Ok(response)
    })
}

/// One conversation handed to a prepare worker.
struct PrepareJob {
    idx: usize,
    path: PathBuf,
    name: String,
}

/// How one conversation came out of the prepare step.
pub(crate) enum PrepareOutcome {
    /// The journal says it already imported; nothing was read.
    Skipped,
    /// Media uploaded and chunks built; ready for the import pipeline.
    Prepared(PreparedFile),
    /// Reading, hashing, or uploading failed.
    Failed(String),
}

/// Result coming back from a prepare worker (may finish out of order).
pub(crate) struct PrepareResult {
    pub idx: usize,
    pub name: String,
    pub outcome: PrepareOutcome,
}

/// A bounded pool of prepare workers plus the reorder buffer that hands
/// results back in file order.
///
/// At most `capacity` jobs are waiting or running, so hundreds of chats are
/// not prepared (and held in memory) before the import loop catches up.
/// Workers may finish out of order; early results wait in `ready` until
/// their index is next, which keeps import order stable for the journal.
pub(crate) struct PrepareQueue {
    job_tx: SyncSender<Option<PrepareJob>>,
    result_rx: Receiver<PrepareResult>,
    workers: usize,
    capacity: usize,
    inflight: usize,
    ready: BTreeMap<usize, PrepareResult>,
}

impl PrepareQueue {
    /// Spawn `workers` threads inside `scope` that pull jobs until told to stop.
    pub(crate) fn start<'scope, 'env>(
        scope: &'scope Scope<'scope, 'env>,
        ctx: &'env PrepareContext<'env>,
        workers: usize,
        capacity: usize,
    ) -> Self {
        let (job_tx, job_rx) = mpsc::sync_channel::<Option<PrepareJob>>(capacity);
        let (result_tx, result_rx) = mpsc::channel::<PrepareResult>();
        let job_rx = Arc::new(Mutex::new(job_rx));
        for _ in 0..workers {
            let result_tx = result_tx.clone();
            let job_rx = Arc::clone(&job_rx);
            scope.spawn(move || {
                loop {
                    let job = {
                        let rx = job_rx.lock().expect("prepare job mutex poisoned");
                        rx.recv().unwrap_or(None)
                    };
                    let Some(job) = job else {
                        break;
                    };
                    let outcome = match prepare_file(ctx, &job.path, &job.name) {
                        Ok(prepared) => PrepareOutcome::Prepared(prepared),
                        Err(error) => PrepareOutcome::Failed(error.to_string()),
                    };
                    let _ = result_tx.send(PrepareResult {
                        idx: job.idx,
                        name: job.name,
                        outcome,
                    });
                }
            });
        }
        // Drop this clone so workers' sends finish cleanly when they exit.
        drop(result_tx);
        Self {
            job_tx,
            result_rx,
            workers,
            capacity,
            inflight: 0,
            ready: BTreeMap::new(),
        }
    }

    /// True while fewer than `capacity` jobs are waiting or running.
    pub(crate) fn has_capacity(&self) -> bool {
        self.inflight < self.capacity
    }

    /// True when no job is waiting or running.
    pub(crate) fn is_idle(&self) -> bool {
        self.inflight == 0
    }

    /// Hand one conversation to the workers.
    pub(crate) fn submit(&mut self, idx: usize, path: PathBuf, name: String) {
        self.job_tx
            .send(Some(PrepareJob { idx, path, name }))
            .expect("prepare workers alive");
        self.inflight += 1;
    }

    /// Record a conversation as skipped without sending it to a worker, so the
    /// consume side still advances through that index in order.
    pub(crate) fn mark_skipped(&mut self, idx: usize, name: String) {
        self.ready.insert(
            idx,
            PrepareResult {
                idx,
                name,
                outcome: PrepareOutcome::Skipped,
            },
        );
    }

    /// True when the result for `idx` is already buffered.
    pub(crate) fn is_ready(&self, idx: usize) -> bool {
        self.ready.contains_key(&idx)
    }

    /// Take the buffered result for `idx`, if it has arrived.
    pub(crate) fn take(&mut self, idx: usize) -> Option<PrepareResult> {
        self.ready.remove(&idx)
    }

    /// Block until one more worker result arrives and buffer it.
    ///
    /// # Errors
    ///
    /// Returns an error when every worker has exited (a worker panicked).
    pub(crate) fn wait_one(&mut self) -> Result<()> {
        let result = self
            .result_rx
            .recv()
            .context("prepare worker disconnected")?;
        self.inflight = self.inflight.saturating_sub(1);
        self.ready.insert(result.idx, result);
        Ok(())
    }

    /// Tell every worker to exit and collect whatever they were still finishing.
    ///
    /// Their asset stats still count even if the import loop aborted.
    pub(crate) fn shutdown(self) -> Vec<PrepareResult> {
        for _ in 0..self.workers {
            let _ = self.job_tx.send(None);
        }
        let mut leftovers: Vec<PrepareResult> = self.ready.into_values().collect();
        while let Ok(result) = self.result_rx.recv() {
            leftovers.push(result);
        }
        leftovers
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn resolver(trust_export: bool) -> DigestResolver {
        DigestResolver {
            cache: Mutex::new(HashMap::new()),
            verify_digests: false,
            trust_export,
        }
    }

    #[test]
    fn normalize_digest_sha256_accepts_hex() {
        let d = "A".repeat(64);
        assert_eq!(normalize_digest_sha256(&d).unwrap(), "a".repeat(64));
        assert!(normalize_digest_sha256("not-a-digest").is_err());
    }

    #[test]
    fn trust_export_skips_hash_when_size_matches() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pic.bin");
        std::fs::write(&path, b"hello").unwrap();
        let claimed = "a".repeat(64);
        let expected_disk = hex::encode(Sha256::digest(b"hello"));

        let mut warnings = Vec::new();
        let trusted = resolver(true)
            .resolve(
                &path,
                Some(&claimed),
                Some(5),
                "chat.jsonl",
                "attachments/pic.bin",
                &mut |m| warnings.push(m),
            )
            .unwrap();
        assert_eq!(
            trusted, claimed,
            "matching size_bytes must skip hashing and keep the export digest"
        );
        assert!(warnings.is_empty());

        let mut warnings = Vec::new();
        let disk = resolver(false)
            .resolve(
                &path,
                Some(&claimed),
                Some(5),
                "chat.jsonl",
                "attachments/pic.bin",
                &mut |m| warnings.push(m),
            )
            .unwrap();
        assert_eq!(disk, expected_disk);
        assert_eq!(warnings.len(), 1);

        let mut warnings = Vec::new();
        let size_mismatch = resolver(true)
            .resolve(
                &path,
                Some(&claimed),
                Some(4),
                "chat.jsonl",
                "attachments/pic.bin",
                &mut |m| warnings.push(m),
            )
            .unwrap();
        assert_eq!(
            size_mismatch, expected_disk,
            "trust_export must still hash when size_bytes does not match the file"
        );
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn chunk_builder_splits_on_count() {
        let header = b"{\"h\":1}\n".to_vec();
        let mut builder = ChunkBuilder::new(header.clone(), 2);
        for i in 0..5 {
            builder.push(
                b"{}\n",
                JournalMessage {
                    file: "c.jsonl".into(),
                    guid: format!("g{i}"),
                },
            );
        }
        let chunks = builder.finish();
        assert_eq!(chunks.len(), 3);
        assert!(chunks.iter().all(|c| c.body.starts_with(&header)));
        assert_eq!(chunks[2].messages.len(), 1);
    }
}
