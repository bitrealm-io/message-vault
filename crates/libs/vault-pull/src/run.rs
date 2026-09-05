//! Page exported messages, download attachments, and write JSON Lines folders.
//!
//! JSON Lines means one JSON object per line. Message Vault is the HTTP server
//! that stores imported messages.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use message_ir_format::write_export_sentinel;
use message_vault_io_core::{CancelFlag, check_cancel, parallel_for_each};
use serde::Serialize;
use vault_http::{auth_check as authenticate, with_retries};

use crate::http::{ExportMessagesArgs, HttpSession};
use crate::project::{build_document, conversation_key, to_ir_message};
use vault_api_types::Message;

/// Page size for GET /v1/export/messages; the vault's maximum.
pub const DEFAULT_PAGE_LIMIT: usize = 500;
/// The largest page the vault will hand back for GET /v1/export/messages.
pub const MAX_PAGE_LIMIT: usize = 500;
/// Default number of parallel asset download workers.
pub const DEFAULT_ASSET_DOWNLOAD_WORKERS: usize = 8;
/// Extra tries for transient HTTP failures, matching the vault-push default.
const MAX_RETRIES: u32 = 3;

/// Settings for one download run (output folder, URL, search, flags).
#[derive(Debug, Clone)]
pub struct VaultPullConfig {
    /// Folder the JSON Lines files and attachments are written into.
    pub out_dir: PathBuf,
    /// Vault base URL, e.g. `http://127.0.0.1:8080`.
    pub base_url: String,
    /// Account username, recorded in the journal and progress events.
    pub username: String,
    /// API token or session token for the vault.
    pub key: String,
    /// A query in the vault's search language (may be empty).
    pub query: String,
    /// Write messages only; download no attachments.
    pub skip_attachments: bool,
    /// Messages per `GET /v1/export/messages` page, clamped to
    /// `1..=MAX_PAGE_LIMIT`.
    pub page_limit: usize,
    /// Checked between pages and downloads; set it to stop the run early.
    pub cancel: Option<CancelFlag>,
    /// Number of parallel asset download workers (default 8).
    pub asset_download_workers: usize,
}

/// Final summary of a download (conversations, messages, attachment counts).
#[derive(Debug, Clone, Serialize)]
pub struct PullReport {
    /// Always `true`: a run that fails returns an error instead of a report.
    pub ok: bool,
    /// Account id the key resolved to.
    pub account: String,
    /// The query the run asked the vault for.
    pub query: String,
    /// Conversations written.
    pub conversations: u64,
    /// Messages written.
    pub messages: u64,
    /// Attachments fetched this run.
    pub attachments_downloaded: u64,
    /// Attachments already on disk according to the journal.
    pub attachments_skipped: u64,
    /// The output folder, as given.
    pub out_dir: String,
}

/// Live progress sent to the CLI or desktop app during a query or download.
#[derive(Debug, Clone)]
pub enum ProgressEvent {
    /// One line for the log panel.
    Log(String),
    /// The key was accepted; the run knows whose vault it is reading.
    Auth {
        /// Account id the key resolved to.
        account_id: String,
        /// Username the vault reports for that account, else the account id.
        username: String,
    },
    /// One page of messages arrived.
    Page {
        /// Messages on this page.
        messages: usize,
        /// Messages fetched so far, this page included.
        total_so_far: u64,
    },
    /// The run finished; the report is final.
    Done(PullReport),
}

/// Callback type for live progress (CLI stderr, desktop log panel, tests).
pub type ProgressFn<'a> = dyn FnMut(ProgressEvent) + 'a;

/// Send one event to the caller's progress callback when it supplied one.
fn emit(on_progress: &mut Option<&mut ProgressFn<'_>>, event: ProgressEvent) {
    if let Some(callback) = on_progress.as_mut() {
        callback(event);
    }
}

/// Where the next page starts, or `None` when the walk is over: the vault said
/// this was the last of `total`, or it sent nothing (a stale total must not spin).
fn next_offset(offset: usize, fetched: usize, total: u64) -> Option<usize> {
    if fetched == 0 {
        return None;
    }
    let next = offset + fetched;
    (u64::try_from(next).unwrap_or(u64::MAX) < total).then_some(next)
}

/// Create the output folder and its `attachments/` child, and mark the folder
/// as a Message Vault export.
///
/// The sentinel names this folder as one an export wrote. The desktop app
/// refuses to clean or transcode a folder without it
/// (`resolve_staging_child` in `src-tauri/src/commands/staging.rs`), which is
/// what stands between a path bug and a recursive delete somewhere else on
/// disk. A pulled folder that skipped the sentinel could not be used as
/// export staging.
///
/// # Errors
///
/// Returns an error when the folder, its `attachments/` child, or the
/// sentinel cannot be written.
fn prepare_out_dir(out_dir: &Path, skip_attachments: bool) -> Result<()> {
    fs::create_dir_all(out_dir).with_context(|| format!("create {}", out_dir.display()))?;
    if !skip_attachments {
        let attachments_dir = out_dir.join("attachments");
        fs::create_dir_all(&attachments_dir)
            .with_context(|| format!("create {}", attachments_dir.display()))?;
    }
    write_export_sentinel(out_dir)
        .with_context(|| format!("mark {} as an export folder", out_dir.display()))
}

/// Download matching messages into `cfg.out_dir` as JSON Lines plus attachments.
///
/// JSON Lines means one JSON object per line. A local journal
/// (`.vault-pull-state.jsonl`) records which files were already downloaded so a
/// later run can skip them.
///
/// # Errors
///
/// Returns an error when the key or output folder is missing, login fails, a
/// page or download fails, or a conversation file cannot be written.
pub fn run(
    cfg: &VaultPullConfig,
    mut on_progress: Option<&mut ProgressFn<'_>>,
) -> Result<PullReport> {
    if cfg.key.trim().is_empty() {
        bail!("vault key is required");
    }
    if cfg.out_dir.as_os_str().is_empty() {
        bail!("output directory is required");
    }
    let pull = Pull::login(cfg, &mut on_progress)?;
    prepare_out_dir(&cfg.out_dir, cfg.skip_attachments)?;
    if pull.journal.backup_complete {
        emit(
            &mut on_progress,
            ProgressEvent::Log(
                "Previous backup completed successfully. Running to check for new messages…".into(),
            ),
        );
    }

    let fetched = pull.fetch_all_messages(&mut on_progress)?;
    let assets = if cfg.skip_attachments {
        AssetCounts::default()
    } else {
        pull.download_assets(&fetched.assets, &mut on_progress)?
    };
    let conversations = pull.write_conversations(fetched.by_conv)?;
    pull.finish_journal(
        conversations,
        fetched.total_messages,
        &assets,
        fetched.assets,
    );

    let report = PullReport {
        ok: true,
        account: pull.account,
        query: pull.query,
        conversations,
        messages: fetched.total_messages,
        attachments_downloaded: assets.downloaded,
        attachments_skipped: assets.skipped,
        out_dir: cfg.out_dir.display().to_string(),
    };
    emit(
        &mut on_progress,
        ProgressEvent::Log(format!(
            "Wrote {} conversation(s), {} message(s) → {}",
            report.conversations, report.messages, report.out_dir
        )),
    );
    emit(&mut on_progress, ProgressEvent::Done(report.clone()));
    Ok(report)
}

/// Everything the vault handed back while paging: messages grouped by
/// conversation, the attachments they reference, and the running count.
struct Fetched {
    /// Conversation key → (first message as the metadata seed, converted messages).
    by_conv: BTreeMap<String, (Message, Vec<message_ir::IrMessage>)>,
    /// sha256 → (source, relative path under the output folder).
    assets: HashMap<String, (String, String)>,
    total_messages: u64,
}

/// Attachment outcome counts for the report.
#[derive(Debug, Default, Clone, Copy)]
struct AssetCounts {
    downloaded: u64,
    skipped: u64,
}

/// One authenticated download run: the connection, the account it resolved
/// to, and the local journal of files already on disk.
struct Pull<'a> {
    cfg: &'a VaultPullConfig,
    session: HttpSession,
    account: String,
    username: String,
    /// The search query with surrounding whitespace removed.
    query: String,
    journal_path: PathBuf,
    journal: crate::journal::PullJournalState,
}

impl<'a> Pull<'a> {
    /// Check the key, announce the account and query, and load the journal.
    ///
    /// # Errors
    ///
    /// Returns an error when login fails or the journal cannot be read.
    fn login(cfg: &'a VaultPullConfig, out: &mut Option<&mut ProgressFn<'_>>) -> Result<Self> {
        let auth =
            authenticate(&cfg.base_url, &cfg.key).map_err(|e| anyhow::anyhow!("{}", e.detail()))?;
        let account = auth.account_id.clone();
        let username = auth.username.unwrap_or_else(|| account.clone());
        emit(
            out,
            ProgressEvent::Auth {
                account_id: account.clone(),
                username: username.clone(),
            },
        );
        emit(
            out,
            ProgressEvent::Log(format!("Authenticated as {username} ({account})")),
        );
        let query = cfg.query.trim().to_string();
        emit(
            out,
            ProgressEvent::Log(if query.is_empty() {
                "Backup query: (all messages)".into()
            } else {
                format!("Backup query: {query}")
            }),
        );
        // Load the local skip log so a later run does not re-download files already on disk.
        let journal_path = crate::journal::journal_path(&cfg.out_dir);
        let journal = crate::journal::load(&journal_path, &cfg.base_url, &username)?;
        Ok(Self {
            cfg,
            session: HttpSession::new()?,
            account,
            username,
            query,
            journal_path,
            journal,
        })
    }

    /// Page through every matching message, grouping by conversation and
    /// noting which attachments they reference.
    ///
    /// # Errors
    ///
    /// Returns an error when a page fails after retries, a message cannot be
    /// converted, or the run is cancelled.
    fn fetch_all_messages(&self, out: &mut Option<&mut ProgressFn<'_>>) -> Result<Fetched> {
        let cfg = self.cfg;
        let mut fetched = Fetched {
            by_conv: BTreeMap::new(),
            assets: HashMap::new(),
            total_messages: 0,
        };
        let mut offset = 0usize;
        loop {
            check_cancel(cfg.cancel.as_ref())?;
            let page = with_retries(MAX_RETRIES, || {
                crate::http::export_messages(
                    &self.session,
                    ExportMessagesArgs {
                        base_url: &cfg.base_url,
                        key: &cfg.key,
                        q: &self.query,
                        limit: cfg.page_limit.clamp(1, MAX_PAGE_LIMIT),
                        offset,
                        account: &self.account,
                    },
                )
            })?;
            let count = page.items.len();
            fetched.total_messages += count as u64;
            emit(
                out,
                ProgressEvent::Page {
                    messages: count,
                    total_so_far: fetched.total_messages,
                },
            );
            for msg in page.items {
                if !cfg.skip_attachments {
                    note_asset_refs(&msg, &mut fetched.assets);
                }
                let ir = to_ir_message(&msg, cfg.skip_attachments)?;
                fetched
                    .by_conv
                    .entry(conversation_key(&msg))
                    // Keep first message as seed for conversation metadata.
                    .or_insert_with(|| (msg.clone(), Vec::new()))
                    .1
                    .push(ir);
            }
            match next_offset(offset, count, page.total) {
                Some(next) => offset = next,
                None => break,
            }
        }
        Ok(fetched)
    }

    /// Download every referenced attachment the journal does not already
    /// have, and journal each one that is now on disk so a resume skips it.
    ///
    /// # Errors
    ///
    /// Returns an error when a download fails after retries or the run is cancelled.
    fn download_assets(
        &self,
        assets: &HashMap<String, (String, String)>,
        out: &mut Option<&mut ProgressFn<'_>>,
    ) -> Result<AssetCounts> {
        let cfg = self.cfg;
        let to_download = assets_needing_download(assets, &self.journal.assets, &cfg.out_dir);
        let skipped_by_journal = assets.len() as u64 - to_download.len() as u64;
        if to_download.is_empty() {
            return Ok(AssetCounts {
                downloaded: 0,
                skipped: skipped_by_journal,
            });
        }
        emit(
            out,
            ProgressEvent::Log(format!(
                "Downloading {} unique asset(s) with {} worker(s) ({} skipped from journal)…",
                to_download.len(),
                cfg.asset_download_workers,
                skipped_by_journal
            )),
        );
        let stats = download_assets_parallel(DownloadAssetsParallelArgs {
            session: &self.session,
            base_url: &cfg.base_url,
            key: &cfg.key,
            account: &self.account,
            assets: &to_download,
            out_dir: &cfg.out_dir,
            workers: cfg.asset_download_workers,
            cancel: cfg.cancel.as_ref(),
        })?;
        let counts = AssetCounts {
            downloaded: stats.downloaded,
            skipped: stats.skipped + skipped_by_journal,
        };
        for sha in to_download.keys() {
            if !self.journal.assets.contains(sha) {
                let event = crate::journal::PullJournalEvent::AssetOk {
                    url: cfg.base_url.clone(),
                    username: self.username.clone(),
                    sha256: sha.clone(),
                    path: String::new(),
                    size_bytes: 0,
                };
                let _ = crate::journal::append(&self.journal_path, &event);
            }
        }
        emit(
            out,
            ProgressEvent::Log(format!(
                "Assets: {} downloaded, {} skipped ({} total bytes)",
                counts.downloaded,
                counts.skipped,
                media::format_bytes(stats.bytes)
            )),
        );
        Ok(counts)
    }

    /// Write one JSON Lines file per conversation and return how many.
    ///
    /// # Errors
    ///
    /// Returns an error when a file cannot be written.
    fn write_conversations(
        &self,
        by_conv: BTreeMap<String, (Message, Vec<message_ir::IrMessage>)>,
    ) -> Result<u64> {
        let mut conversations = 0u64;
        for (_key, (seed, messages)) in by_conv {
            let mut doc = build_document(&seed.source, &seed, messages);
            // Disambiguate same chat across sources.
            if !doc.export.source.trim().is_empty() {
                doc.packaging_stem_suffix =
                    Some(format!("__{}", sanitize_source_suffix(&doc.export.source)));
            }
            message_ir_format::write_conversation_jsonl(&self.cfg.out_dir, &doc)?;
            conversations += 1;
        }
        Ok(conversations)
    }

    /// Record that this download finished, then rewrite the journal in its
    /// shortest form. Every asset this run saw is on disk: it was downloaded
    /// above, or an earlier run had already fetched it. Best effort: a journal
    /// write failure does not fail a run whose files are already written.
    fn finish_journal(
        &self,
        conversations: u64,
        messages: u64,
        assets: &AssetCounts,
        seen_assets: HashMap<String, (String, String)>,
    ) {
        let event = crate::journal::PullJournalEvent::BackupComplete {
            url: self.cfg.base_url.clone(),
            username: self.username.clone(),
            conversations,
            messages,
            assets: assets.downloaded + assets.skipped,
        };
        let _ = crate::journal::append(&self.journal_path, &event);
        let mut recorded_assets = self.journal.assets.clone();
        recorded_assets.extend(seen_assets.into_keys());
        let final_state = crate::journal::PullJournalState {
            assets: recorded_assets,
            backup_complete: true,
        };
        let _ = crate::journal::compact(
            &self.journal_path,
            &self.cfg.base_url,
            &self.username,
            &final_state,
        );
    }
}

/// Remember where each attachment a message references should land on disk.
///
/// The first message to mention a sha256 decides the source and path; the
/// vault stores one blob per fingerprint, so later mentions are the same file.
fn note_asset_refs(msg: &Message, assets: &mut HashMap<String, (String, String)>) {
    for att in &msg.attachments {
        let Some(sha) = att.sha256.as_deref().and_then(message_ir::trimmed) else {
            continue;
        };
        let rel = att
            .path
            .as_deref()
            .and_then(message_ir::trimmed)
            .map(|p| p.trim_start_matches('/').to_string())
            .unwrap_or_else(|| format!("attachments/{sha}"));
        assets
            .entry(sha.to_string())
            .or_insert_with(|| (msg.source.clone(), rel));
    }
}

struct AssetDownloadJob {
    sha256: String,
    source: String,
    dest: PathBuf,
}

#[derive(Default)]
struct AssetDownloadStats {
    bytes: u64,
    downloaded: u64,
    skipped: u64,
}

/// Attachments whose SHA-256 fingerprint is not already on disk from a prior run.
///
/// SHA-256 is a short hex fingerprint of the file bytes. The journal lists
/// fingerprints already downloaded; those files are skipped when they still exist.
fn assets_needing_download(
    assets: &HashMap<String, (String, String)>,
    journal_assets: &HashSet<String>,
    out_dir: &Path,
) -> HashMap<String, (String, String)> {
    let mut to_download = HashMap::new();
    for (sha, entry) in assets {
        let (_source, rel) = entry;
        if journal_assets.contains(sha) && out_dir.join(rel).is_file() {
            continue;
        }
        to_download.insert(sha.clone(), entry.clone());
    }
    to_download
}

/// Download unique attachments in parallel using work-stealing workers.
///
/// Same pattern as vault-push `upload_assets`: jobs are collected, then
/// [`parallel_for_each`] runs them on `asset_download_workers` threads.
/// Files already on disk are skipped (counted as `skipped`); each download
/// retries transient HTTP failures like push does.
///
/// # Errors
///
/// Returns an error when a download fails after retries, a dest path cannot be
/// created, or cancel is requested.
struct DownloadAssetsParallelArgs<'a> {
    session: &'a crate::http::HttpSession,
    base_url: &'a str,
    key: &'a str,
    account: &'a str,
    assets: &'a HashMap<String, (String, String)>, // sha256 -> (source, rel_path)
    out_dir: &'a Path,
    workers: usize,
    cancel: Option<&'a CancelFlag>,
}

/// Download the given assets with a worker pool, skipping any already on disk. Returns the counts and bytes.
fn download_assets_parallel(args: DownloadAssetsParallelArgs<'_>) -> Result<AssetDownloadStats> {
    let DownloadAssetsParallelArgs {
        session,
        base_url,
        key,
        account,
        assets,
        out_dir,
        workers,
        cancel,
    } = args;
    let mut jobs: Vec<AssetDownloadJob> = Vec::with_capacity(assets.len());
    let mut stats = AssetDownloadStats::default();

    for (sha256, (source, rel)) in assets {
        let dest = out_dir.join(rel);
        if dest.is_file() {
            let meta = fs::metadata(&dest).with_context(|| format!("stat {}", dest.display()))?;
            stats.bytes = stats.bytes.saturating_add(meta.len());
            stats.skipped += 1;
            continue;
        }
        jobs.push(AssetDownloadJob {
            sha256: sha256.clone(),
            source: source.clone(),
            dest,
        });
    }

    let results = parallel_for_each(&jobs, workers, cancel, |job| {
        with_retries(MAX_RETRIES, || {
            crate::http::download_asset(
                session,
                base_url,
                key,
                account,
                &job.source,
                &job.sha256,
                &job.dest,
            )?;
            let meta = fs::metadata(&job.dest)
                .with_context(|| format!("stat after download {}", job.dest.display()))?;
            Ok(meta.len())
        })
        .map_err(|e| e.to_string())
    });

    for result in results {
        match result {
            Ok(bytes) => {
                stats.bytes = stats.bytes.saturating_add(bytes);
                stats.downloaded += 1;
            }
            Err(error) => {
                bail!("asset download failed: {error}");
            }
        }
    }
    Ok(stats)
}

/// Write one conversation as a JSON Lines file (header, then one message per line).
///
/// The stem comes from [`ConversationDocument::filename_stem`], which appends
/// [`ConversationDocument::packaging_stem_suffix`] (the sanitized source, set
/// by the caller). The write is atomic: ir-format writes a `.tmp` sibling and
/// renames it, so a crash never leaves a truncated conversation file.
///
/// # Errors
///
/// Returns an error when the file cannot be created, serialized, or renamed.
/// Keep letters, digits, `-`, and `_`; replace every other character with `_`.
fn sanitize_source_suffix(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    for c in source.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    out
}

#[cfg(test)]
mod paging_tests {
    use super::next_offset;

    #[test]
    fn paging_stops_at_the_total_or_on_an_empty_page() {
        assert_eq!(next_offset(0, 500, 1200), Some(500));
        assert_eq!(next_offset(1000, 200, 1200), None);
        assert_eq!(
            next_offset(1000, 0, 1200),
            None,
            "an empty page ends the walk even under total"
        );
    }
}

#[cfg(test)]
mod out_dir_tests {
    use super::*;
    use message_ir_format::EXPORT_SENTINEL;

    #[test]
    fn marks_the_folder_as_an_export() {
        // Without the sentinel the desktop app's staging guard refuses to
        // clean a pulled folder, so an export that staged into one would
        // leave the staging folder behind.
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("pulled");

        prepare_out_dir(&out, false).unwrap();

        assert!(out.join(EXPORT_SENTINEL).is_file());
    }

    #[test]
    fn creates_attachments_unless_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let with = dir.path().join("with");
        let without = dir.path().join("without");

        prepare_out_dir(&with, false).unwrap();
        prepare_out_dir(&without, true).unwrap();

        assert!(with.join("attachments").is_dir());
        assert!(!without.join("attachments").exists());
    }

    #[test]
    fn runs_again_over_a_folder_it_already_prepared() {
        // A second pull into the same folder is the resume path.
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("pulled");

        prepare_out_dir(&out, false).unwrap();
        prepare_out_dir(&out, false).unwrap();

        assert!(out.join(EXPORT_SENTINEL).is_file());
    }
}
