//! Import message-ir JSONL into the vault.
//!
//! The pipeline runs in three stages: `staging` parses JSONL files and writes
//! staging rows, `promote` copies staging rows into the production tables, and
//! `contact_name` links handles to vault contacts and merges display names.
//! The HTTP handlers for `POST /v1/import` and the `/v1/imports` session
//! routes live at the end of this module.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sqlx::AnyConnection;
use sqlx::Connection;
use tempfile::TempDir;

use crate::extract::{Json, Path as AxumPath, Query};
use axum::extract::{Request, State};
use axum::http::HeaderMap;
use tokio::sync::Mutex;

use crate::assets::AssetStats;
use crate::config::{PathsConfig, validate_source_id};
use crate::db::contacts;
use crate::db::dialect;
use crate::db::engine;
use crate::db::schema;
use crate::db::vault_imports::{self, CompleteImportArgs};
use media::MediaMode;

pub mod contact_name;
pub mod failure;
pub mod promote;
pub mod staging;

pub use failure::ImportFailure;
pub use staging::is_orphaned_export;

use staging::StagingInserts;

use crate::dedupe;
use crate::import::{self};
use crate::server::{
    ApiError, AppState, ImportAccess, content_type_base, is_jsonl_content_type,
    resolve_import_account, stream_body_to_file,
};

/// What happens to a source's messages that were imported before.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportMode {
    /// Wipe the source's existing messages before importing.
    Replace,
    /// Keep existing messages and add only new ones.
    Append,
}

impl ImportMode {
    /// Parse `replace` or `append`.
    ///
    /// # Errors
    ///
    /// Returns an error when `s` is not one of those values.
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "replace" => Ok(Self::Replace),
            "append" => Ok(Self::Append),
            other => bail!("invalid import mode '{other}' (expected replace or append)"),
        }
    }

    /// Canonical flag value (`replace` or `append`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Replace => "replace",
            Self::Append => "append",
        }
    }
}

/// Full import settings: paths, mode, media handling, and contact naming.
#[derive(Debug, Clone)]
pub struct ImportOptions<'a> {
    /// Content-addressed asset store when [`Self::source_from_jsonl`] is false.
    pub assets_dir: &'a Path,
    /// Root for resolving relative attachment paths in JSONL.
    pub asset_root: &'a Path,
    /// Optional address book to load: VCF or vCard CSV export.
    pub contacts: Option<&'a Path>,
    /// Reload the address book even when contacts already exist.
    pub overwrite_contacts: bool,
    /// Import mode: replace or append.
    pub mode: ImportMode,
    /// Fixed source id (HTTP / `--source` override). Ignored when `source_from_jsonl`.
    pub source: &'a str,
    /// Vault account the import writes into.
    pub account_id: &'a str,
    /// Fill missing `content_key` values during promote (needed before cross-source dedupe).
    pub fill_content_keys: bool,
    /// Optional vault import session id (messages stamped on promote).
    pub import_id: Option<i64>,
    /// When true, stamp `messages.source` from each conversation's IR `export.source`.
    pub source_from_jsonl: bool,
    /// Required when `source_from_jsonl` to resolve per-source asset dirs.
    pub paths: Option<&'a PathsConfig>,
    /// Attachment handling mode: copy, none, convert, compress.
    pub media: MediaMode,
    /// When `source_from_jsonl` + Replace: wipe these sources before import.
    pub wipe_sources: Option<Vec<String>>,
}

/// Path/mode fields for [`ImportOptions::fixed`].
#[derive(Debug, Clone, Copy)]
pub struct FixedImportArgs<'a> {
    /// Content-addressed asset store directory.
    pub assets_dir: &'a Path,
    /// Root for resolving relative attachment paths in JSONL.
    pub asset_root: &'a Path,
    /// Optional address book to load: VCF or vCard CSV export.
    pub contacts: Option<&'a Path>,
    /// Reload the address book even when contacts already exist.
    pub overwrite_contacts: bool,
    /// Import mode: replace or append.
    pub mode: ImportMode,
    /// Fixed source id applied to every conversation.
    pub source: &'a str,
    /// Vault account the import writes into.
    pub account_id: &'a str,
    /// Fill missing `content_key` values during promote.
    pub fill_content_keys: bool,
    /// Optional vault import session id (messages stamped on promote).
    pub import_id: Option<i64>,
}

impl<'a> ImportOptions<'a> {
    /// HTTP / tests / reset-demo: fixed source + assets dir, copy media.
    pub fn fixed(args: FixedImportArgs<'a>) -> Self {
        Self {
            assets_dir: args.assets_dir,
            asset_root: args.asset_root,
            contacts: args.contacts,
            overwrite_contacts: args.overwrite_contacts,
            mode: args.mode,
            source: args.source,
            account_id: args.account_id,
            fill_content_keys: args.fill_content_keys,
            import_id: args.import_id,
            source_from_jsonl: false,
            paths: None,
            media: MediaMode::Clone,
            wipe_sources: None,
        }
    }
}

/// Counters for one import run (staging and promote results).
#[derive(Debug, Default, Clone, Serialize, utoipa::ToSchema)]
pub struct ImportStats {
    /// Conversations imported.
    pub conversations: u64,
    /// Participant rows imported.
    pub participants: u64,
    /// Messages imported.
    pub messages: u64,
    /// Attachment records (message–media links) imported.
    pub attachments: u64,
    /// Tapback reactions imported.
    pub tapbacks: u64,
    /// JSONL files imported.
    pub files: u64,
    /// Unique media files written to the asset store.
    pub assets_copied: u64,
    /// Media files already present under the same fingerprint, skipped.
    pub assets_deduped: u64,
    /// Attachment files referenced but not found on disk.
    pub assets_missing: u64,
    /// Contacts loaded from the address book.
    pub contacts: u64,
    /// Contact–handle links created.
    pub contact_handles: u64,
    /// Contacts the import created for participants nothing else owned.
    pub contacts_created: u64,
    /// True when the address book was not loaded (already present or no file).
    pub contacts_skipped: bool,
    /// Messages hidden as duplicates within this import.
    pub messages_deduped: u64,
    /// Messages added by an append-mode import.
    pub messages_appended: u64,
    /// Import mode string (`replace` or `append`).
    pub mode: String,
    /// Flagged phone handles (ambiguous; review note set) inserted by this import.
    pub phones_needing_review: u64,
}

impl ImportStats {
    /// Add one staged file's counts onto the running import totals.
    fn merge_file(&mut self, other: &ImportStats) {
        self.conversations += other.conversations;
        self.participants += other.participants;
        self.messages += other.messages;
        self.attachments += other.attachments;
        self.tapbacks += other.tapbacks;
        self.messages_deduped += other.messages_deduped;
        self.phones_needing_review += other.phones_needing_review;
    }

    /// Add a whole import run's counts onto a running total, files and assets
    /// included; the demo reset imports three sources in turn.
    pub fn add_run(&mut self, other: &ImportStats) {
        self.merge_file(other);
        self.files += other.files;
        self.assets_copied += other.assets_copied;
        self.assets_deduped += other.assets_deduped;
        self.assets_missing += other.assets_missing;
        self.messages_appended += other.messages_appended;
    }
}

/// Arguments for [`import_export`].
#[derive(Debug, Clone, Copy)]
pub struct ImportExportArgs<'a> {
    /// Folder of `*.jsonl` conversation files to import.
    pub export_dir: &'a Path,
    /// Database to import into.
    pub db: engine::DbTarget<'a>,
    /// Content-addressed asset store directory.
    pub assets_dir: &'a Path,
    /// Optional address book to load: VCF or vCard CSV export.
    pub contacts: Option<&'a Path>,
    /// Reload the address book even when contacts already exist.
    pub overwrite_contacts: bool,
    /// Import mode: replace or append.
    pub mode: ImportMode,
    /// Fixed source id applied to every conversation.
    pub source: &'a str,
    /// Vault account the import writes into.
    pub account_id: &'a str,
}

/// Import every JSON Lines file (`*.jsonl`, one JSON object per line) under
/// `args.export_dir` (CLI staging path — the temporary import area).
///
/// # Errors
///
/// Returns an error when the export directory is missing, a file cannot be
/// parsed, or a database write fails.
pub async fn import_export(args: &ImportExportArgs<'_>) -> Result<ImportStats> {
    if !args.export_dir.is_dir() {
        bail!(
            "export directory does not exist: {}",
            args.export_dir.display()
        );
    }

    let paths = crate::import_cli::list_jsonl_files(args.export_dir)?;

    let pool = args.db.open().await?;
    let mut conn = pool.acquire().await?;
    schema::ensure_vault_schema(&mut conn).await?;
    crate::db::account_profile::ensure_account_row(&mut conn, args.account_id).await?;

    let session = OwnedSession::start(
        &mut conn,
        args.account_id,
        args.source,
        args.mode,
        "message-vault-server",
    )
    .await?;
    let result = import_jsonl_files_on_conn(
        &mut conn,
        &paths,
        &ImportOptions::fixed(FixedImportArgs {
            assets_dir: args.assets_dir,
            asset_root: args.export_dir,
            contacts: args.contacts,
            overwrite_contacts: args.overwrite_contacts,
            mode: args.mode,
            source: args.source,
            account_id: args.account_id,
            fill_content_keys: true,
            import_id: Some(session.id),
        }),
        ImportSchemaMode::AssumeReady,
    )
    .await;
    session.finish(&mut conn, &result).await;
    result
}

/// An import session this process opened for one run, as opposed to one a
/// client (vault-push) owns and closes itself. Whoever starts one must
/// finish it whatever the import does, so the Settings import table never
/// shows a run stuck in progress.
pub(crate) struct OwnedSession<'a> {
    account_id: &'a str,
    /// The `vault_imports` row id; the import stamps it on every message.
    pub id: i64,
}

impl<'a> OwnedSession<'a> {
    /// Record a session at the parse stage. Nothing client-side (staging
    /// folder, device, form) is known for a run started here.
    ///
    /// # Errors
    ///
    /// Returns an error when the account already has a live session or the
    /// row cannot be inserted.
    pub(crate) async fn start(
        conn: &mut AnyConnection,
        account_id: &'a str,
        source: &str,
        mode: ImportMode,
        tool: &str,
    ) -> std::result::Result<Self, vault_imports::StartImportError> {
        let id = vault_imports::start_import(
            conn,
            &vault_imports::StartImportArgs::new(account_id, source, mode.as_str(), Some(tool)),
        )
        .await?;
        Ok(Self { account_id, id })
    }

    /// Mark the session succeeded with the run's counts, or failed. Not
    /// being able to record the outcome is a warning on stderr, never an
    /// error: the import's own result is what the caller returns.
    pub(crate) async fn finish(self, conn: &mut AnyConnection, result: &Result<ImportStats>) {
        let outcome = match result {
            Ok(stats) => CompleteImportArgs::succeeded(stats.messages, stats.attachments),
            Err(_) => CompleteImportArgs::failed(),
        };
        // The import itself is done either way; a failure to record that is
        // worth a log line, not an error the caller would have to unwind.
        if let Err(error) =
            vault_imports::complete_import(conn, self.account_id, self.id, &outcome).await
        {
            eprintln!("warning: complete_import({}) failed: {error}", self.id);
        }
    }
}

/// Whether import should run DDL/schema ensure on the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportSchemaMode {
    /// CLI / one-shot: ensure vault + messages schema.
    Ensure,
    /// HTTP serve hot path: schema already ensured on the warm connection.
    AssumeReady,
}

/// Test helper: open a configured database and run one import.
///
/// Production paths use [`import_jsonl_files_on_conn`] on their own
/// connection (HTTP serve) or [`import_export`] (CLI directory import).
#[cfg(test)]
pub(crate) async fn import_jsonl_files(
    db_path: &Path,
    paths: &[PathBuf],
    opts: &ImportOptions<'_>,
) -> Result<ImportStats> {
    validate_import_options(opts)?;

    if let Some(parent) = db_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let pool = engine::open_pool_for_path(db_path)
        .await
        .with_context(|| format!("failed to open database {}", db_path.display()))?;
    let mut conn = pool.acquire().await?;
    println!("  sql:      opened {}", db_path.display());
    let _ = io::stdout().flush();
    import_jsonl_files_on_conn(&mut conn, paths, opts, ImportSchemaMode::Ensure).await
}

fn validate_import_options(opts: &ImportOptions<'_>) -> Result<()> {
    if opts.source_from_jsonl {
        if opts.paths.is_none() {
            bail!("source_from_jsonl requires config paths for per-source assets");
        }
    } else if opts.source.trim().is_empty() {
        bail!("import source id must not be empty");
    }
    Ok(())
}

/// Import onto an existing connection (warm serve path or tests).
///
/// # Errors
///
/// Returns an error when options are invalid or staging / promote fails.
pub async fn import_jsonl_files_on_conn(
    conn: &mut AnyConnection,
    paths: &[PathBuf],
    opts: &ImportOptions<'_>,
    schema_mode: ImportSchemaMode,
) -> Result<ImportStats> {
    validate_import_options(opts)?;
    if !opts.source_from_jsonl {
        fs::create_dir_all(opts.assets_dir)
            .with_context(|| format!("failed to create {}", opts.assets_dir.display()))?;
    }
    if schema_mode == ImportSchemaMode::Ensure {
        schema::ensure_vault_schema(conn).await?;
    }
    crate::db::account_profile::ensure_account_row(conn, opts.account_id).await?;

    let contact_stats = load_contacts_step(conn, opts).await?;
    if schema_mode == ImportSchemaMode::Ensure {
        say("  sql:      ensuring schema + resetting staging for account…");
    } else {
        say("  sql:      resetting staging for account…");
    }
    schema::reset_staging_for_account(conn, opts.account_id).await?;
    let wipe_sources = sources_to_wipe(opts)?;
    say(&format!(
        "  import:   {} JSONL file{}",
        paths.len(),
        if paths.len() == 1 { "" } else { "s" }
    ));
    if opts.mode == ImportMode::Replace {
        say(&format!(
            "  import:   will wipe source(s) '{}' after staging succeeds",
            wipe_sources.join(", ")
        ));
    }

    let mut stats = ImportStats {
        contacts: contact_stats.contacts,
        contact_handles: contact_stats.phones,
        contacts_skipped: contact_stats.skipped,
        phones_needing_review: contact_stats.phones_needing_review,
        mode: opts.mode.as_str().to_string(),
        ..Default::default()
    };
    let started = Instant::now();
    let asset_stats = stage_all_files(conn, paths, opts, &mut stats, started).await?;

    say(&format!(
        "  import:   promoting staging → production ({:.0}s so far)…",
        started.elapsed().as_secs_f64()
    ));
    promote_step(conn, opts, &wipe_sources, &mut stats).await?;
    schema::reset_staging_for_account(conn, opts.account_id).await?;

    stats.assets_copied = asset_stats.copied;
    stats.assets_deduped = asset_stats.deduped;
    stats.assets_missing = asset_stats.missing;
    say(&format!(
        "  import:   finished in {:.1}s  files={} msgs={} attachments={} assets_copied={}",
        started.elapsed().as_secs_f64(),
        stats.files,
        stats.messages,
        stats.attachments,
        stats.assets_copied
    ));
    Ok(stats)
}

/// Print one progress line and flush, so a long import shows movement even
/// when stdout is a pipe.
fn say(line: &str) {
    println!("{line}");
    let _ = io::stdout().flush();
}

/// Load the address book named by `--contacts`, if any, and report what happened.
///
/// # Errors
///
/// Returns an error when the address book cannot be read or written.
async fn load_contacts_step(
    conn: &mut AnyConnection,
    opts: &ImportOptions<'_>,
) -> Result<contacts::ContactLoadStats> {
    match opts.contacts {
        Some(path) => say(&format!(
            "  sql:      loading contacts from {}…",
            path.display()
        )),
        None => say("  sql:      contacts load skipped (no --contacts address book)"),
    }
    let contact_stats = contacts::load_contacts_if_needed(
        conn,
        opts.contacts,
        opts.overwrite_contacts,
        opts.account_id,
    )
    .await?;
    if contact_stats.skipped {
        say("  sql:      contacts skipped (already loaded or no address book)");
    } else {
        say(&format!(
            "  sql:      contacts={} phones={}",
            contact_stats.contacts, contact_stats.phones
        ));
    }
    Ok(contact_stats)
}

/// The source ids a replace-mode import wipes once staging succeeds: the
/// ones found in the files, or the single `--source` override. Append mode
/// wipes nothing.
///
/// # Errors
///
/// Returns an error when a source id is invalid.
fn sources_to_wipe(opts: &ImportOptions<'_>) -> Result<Vec<String>> {
    let wipe_sources = match (opts.mode, opts.source_from_jsonl) {
        (ImportMode::Replace, true) => opts.wipe_sources.clone().unwrap_or_default(),
        (ImportMode::Replace, false) => vec![opts.source.to_string()],
        (ImportMode::Append, _) => Vec::new(),
    };
    for source in &wipe_sources {
        validate_source_id(source)?;
    }
    Ok(wipe_sources)
}

/// How many files to stage per transaction before committing and starting a
/// new one, so a long import is not one giant transaction.
const STAGING_COMMIT_EVERY: usize = 50;

/// Stage every file into the staging tables, committing every
/// [`STAGING_COMMIT_EVERY`] files, and print progress along the way.
///
/// # Errors
///
/// Returns an error when a file cannot be read, a row cannot be written, or
/// a transaction cannot be committed.
async fn stage_all_files(
    conn: &mut AnyConnection,
    paths: &[PathBuf],
    opts: &ImportOptions<'_>,
    stats: &mut ImportStats,
    started: Instant,
) -> Result<AssetStats> {
    let total_files = paths.len();
    let progress_every = if total_files <= 20 {
        1usize
    } else {
        (total_files / 40).max(10)
    };
    let media_work = TempDir::new().context("temp dir for import-time media rewrite")?;
    let mut asset_stats = AssetStats::default();

    // Staging writes need the write lock up front on SQLite (IMMEDIATE) so
    // two imports for different accounts cannot race into SQLITE_BUSY at the
    // first INSERT; Postgres has no statement-level equivalent and uses a
    // plain BEGIN.
    let engine = dialect::engine_of(conn);
    let mut tx = conn
        .begin_with(dialect::begin_immediate_sql(engine))
        .await?;
    let mut stmts = StagingInserts::new(opts.account_id, opts.import_id);

    for (idx, path) in paths.iter().enumerate() {
        let file_stats = staging::import_file_to_staging(
            &mut tx,
            &mut stmts,
            opts,
            path,
            &mut asset_stats,
            media_work.path(),
        )
        .await?;
        stats.merge_file(&file_stats);
        stats.files += 1;

        let n = idx + 1;
        if n == 1 || n == total_files || n % progress_every == 0 {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("?");
            say(&format!(
                "  import:   [{n}/{total_files}] {name}  msgs={} attachments={} assets_copied={} missing={}  ({:.0}s)",
                stats.messages,
                stats.attachments,
                asset_stats.copied,
                asset_stats.missing,
                started.elapsed().as_secs_f64()
            ));
        }
        if n % STAGING_COMMIT_EVERY == 0 && n < total_files {
            tx.commit().await?;
            tx = conn
                .begin_with(dialect::begin_immediate_sql(engine))
                .await?;
        }
    }
    drop(stmts);
    tx.commit().await?;
    Ok(asset_stats)
}

/// Move staged rows into production and fold the promote counts into `stats`.
///
/// In append mode the promote step is the only place the final row counts
/// are known, so they replace the staging counts.
///
/// # Errors
///
/// Returns an error when the promote transaction fails.
async fn promote_step(
    conn: &mut AnyConnection,
    opts: &ImportOptions<'_>,
    wipe_sources: &[String],
    stats: &mut ImportStats,
) -> Result<()> {
    let promote_stats = promote::promote_append(
        conn,
        opts.mode,
        opts.account_id,
        opts.fill_content_keys,
        wipe_sources,
    )
    .await?;
    stats.messages_deduped += promote_stats.messages_deduped;
    stats.messages_appended = promote_stats.messages_appended;
    if opts.mode == ImportMode::Append {
        stats.conversations = promote_stats.conversations;
        stats.participants = promote_stats.participants;
        stats.messages = promote_stats.messages;
        stats.attachments = promote_stats.attachments;
        stats.tapbacks = promote_stats.tapbacks;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub(crate) struct ImportQuery {
    /// Source slug the import registers its data under. Required; checked in
    /// the handler so a missing value is the JSON 400 every other failure is.
    #[serde(default)]
    source: String,
    /// Username or UUID. Optional; when set must match the Bearer token's account.
    #[serde(default)]
    account: Option<String>,
    #[serde(default = "default_import_mode")]
    mode: String,
    /// Run cross-source soft-dedupe after import.
    #[serde(default)]
    dedupe: bool,
    /// Optional vault import session id from POST /v1/imports.
    #[serde(default)]
    import_id: Option<i64>,
}

fn default_import_mode() -> String {
    "append".to_string()
}

/// Import result: stats plus optional dedupe counts.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct ImportResponse {
    source: String,
    account: String,
    #[serde(flatten)]
    stats: ImportStats,
    #[serde(skip_serializing_if = "Option::is_none")]
    dedupe: Option<DedupeResponse>,
}

/// Cross-source dedupe outcome.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct DedupeResponse {
    keys_filled: u64,
    exact_groups: u64,
    exact_flagged: u64,
    near_flagged: u64,
}

/// Source, mode, tool, and optional account for a new import session.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct CreateImportBody {
    pub(crate) source: String,
    #[serde(default = "default_import_mode")]
    pub(crate) mode: String,
    #[serde(default)]
    pub(crate) tool: Option<String>,
    #[serde(default)]
    pub(crate) account: Option<String>,
    /// Stage the session opens at. Defaults to `parse`.
    #[serde(default)]
    pub(crate) stage: Option<String>,
    /// Absolute staging path on the client that owns this session.
    #[serde(default)]
    pub(crate) staging_dir: Option<String>,
    /// Which install is creating the session.
    #[serde(default)]
    pub(crate) device_id: Option<String>,
    /// Import form snapshot, stored so the screen can be restored.
    ///
    /// Credentials are stripped before storage: a `backupPassword` or
    /// `whatsappKey` posted here is dropped rather than persisted.
    #[serde(default)]
    pub(crate) form: Option<serde_json::Value>,
    /// Source path, size, mtime, and message count.
    #[serde(default)]
    pub(crate) source_fingerprint: Option<serde_json::Value>,
    /// Addresses the backup's device sent from, when the client read them.
    #[serde(default)]
    pub(crate) source_identities: Option<serde_json::Value>,
}

/// The new import session id.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct CreateImportResponse {
    pub(crate) id: i64,
}

/// Final stats and issues for a finished import session.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct CompleteImportBody {
    #[serde(default = "default_true")]
    pub(crate) ok: bool,
    /// Explicit session outcome; overrides `ok` when present.
    /// One of `completed`, `completed_with_issues`, `failed`.
    #[serde(default)]
    pub(crate) status: Option<String>,
    #[serde(default)]
    pub(crate) message_count: Option<i64>,
    #[serde(default)]
    pub(crate) attachment_count: Option<i64>,
    #[serde(default)]
    pub(crate) bytes_uploaded: Option<i64>,
    #[serde(default)]
    pub(crate) duration_ms: Option<i64>,
    #[serde(default)]
    pub(crate) parse_ms: Option<i64>,
    #[serde(default)]
    pub(crate) attachments_ms: Option<i64>,
    #[serde(default)]
    pub(crate) prepare_ms: Option<i64>,
    #[serde(default)]
    pub(crate) upload_ms: Option<i64>,
    #[serde(default)]
    pub(crate) summary: Option<serde_json::Value>,
    #[serde(default)]
    pub(crate) issues: Vec<CompleteImportIssueBody>,
}

fn default_true() -> bool {
    true
}

/// One parse/convert/upload issue from the import.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct CompleteImportIssueBody {
    pub(crate) kind: String,
    pub(crate) step: String,
    pub(crate) item: String,
    pub(crate) reason: String,
}

fn validate_complete_import_issues(issues: &[CompleteImportIssueBody]) -> Result<(), ApiError> {
    for issue in issues {
        match issue.kind.as_str() {
            "error" | "skip" => {}
            other => {
                return Err(ApiError::BadRequest(format!(
                    "invalid import issue kind '{other}'; expected 'error' or 'skip'"
                )));
            }
        }
    }
    Ok(())
}

fn validate_import_status(status: Option<&str>) -> Result<(), ApiError> {
    match status {
        None | Some("completed" | "completed_with_issues" | "failed") => Ok(()),
        Some(other) => Err(ApiError::BadRequest(format!(
            "invalid import status '{other}'; expected 'completed', 'completed_with_issues', or 'failed'"
        ))),
    }
}

/// Keys a stored form snapshot must never carry.
///
/// The invariant: `vault_imports.form_json` is a durable record read back to
/// restore the Import screen, and a secret typed once for one run must not
/// outlive it there. The desktop client already drops these before posting,
/// but the client is the wrong place to enforce it -- an older build, a
/// script, or a client not written yet would break the rule silently. The
/// snapshot is flat, so removing them at the top level is the whole job.
const FORM_CREDENTIAL_KEYS: [&str; 2] = ["backupPassword", "whatsappKey"];

/// Remove [`FORM_CREDENTIAL_KEYS`] from a form snapshot before it is stored.
fn strip_form_credentials(form: &serde_json::Value) -> serde_json::Value {
    let mut stripped = form.clone();
    if let Some(fields) = stripped.as_object_mut() {
        for key in FORM_CREDENTIAL_KEYS {
            fields.remove(key);
        }
    }
    stripped
}

/// Serialize an optional JSON body field for storage as TEXT.
fn optional_json_string(
    value: Option<&serde_json::Value>,
    field: &str,
) -> Result<Option<String>, ApiError> {
    match value {
        None => Ok(None),
        Some(v) => serde_json::to_string(v)
            .map(Some)
            .map_err(|e| ApiError::Internal(format!("serialize {field}: {e}"))),
    }
}

/// Stored session status after completion.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct CompleteImportResponse {
    id: i64,
    pub(crate) status: String,
    pub(crate) message_count: i64,
    pub(crate) attachment_count: i64,
    pub(crate) bytes_uploaded: i64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListImportsQuery {
    #[serde(default)]
    account: Option<String>,
}

/// Past import sessions.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct ImportsListResponse {
    items: Vec<crate::db::vault_imports::ImportSummary>,
}

/// One stored import issue.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct ImportDetailIssueResponse {
    pub(crate) kind: String,
    pub(crate) step: String,
    item: String,
    reason: String,
}

/// Full import session record.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct ImportDetailResponse {
    pub(crate) id: i64,
    source: String,
    tool: Option<String>,
    mode: String,
    status: String,
    started_at: String,
    finished_at: Option<String>,
    message_count: i64,
    attachment_count: i64,
    bytes_uploaded: i64,
    pub(crate) duration_ms: Option<i64>,
    pub(crate) parse_ms: Option<i64>,
    pub(crate) attachments_ms: Option<i64>,
    pub(crate) prepare_ms: Option<i64>,
    pub(crate) upload_ms: Option<i64>,
    pub(crate) summary: serde_json::Value,
    pub(crate) issues: Vec<ImportDetailIssueResponse>,
}

/// List past import sessions for the account with their stats.
#[utoipa::path(
    get,
    path = "/v1/imports",
    tag = "Import",
    security(("bearer" = [])),
    params(("account" = Option<String>, Query)),
    responses(
        (status = 200, body = ImportsListResponse),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody)
    )
)]
pub(crate) async fn imports_list_handler(
    State(state): State<AppState>,
    ImportAccess(auth): ImportAccess,
    Query(query): Query<ListImportsQuery>,
) -> Result<Json<ImportsListResponse>, ApiError> {
    let account = resolve_import_account(&auth, query.account.as_deref(), &state.db).await?;

    let mut conn = state.db.acquire().await?;
    let items = crate::db::vault_imports::list_imports(&mut conn, &account).await?;

    Ok(Json(ImportsListResponse { items }))
}

/// Status, timings, and issues for one import session.
#[utoipa::path(
    get,
    path = "/v1/imports/{id}",
    tag = "Import",
    security(("bearer" = [])),
    params(("id" = i64, Path, description = "Import session id")),
    responses(
        (status = 200, body = ImportDetailResponse),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody),
        (status = 404, body = crate::server::ErrorBody)
    )
)]
pub(crate) async fn imports_get_handler(
    State(state): State<AppState>,
    ImportAccess(auth): ImportAccess,
    AxumPath(import_id): AxumPath<i64>,
) -> Result<Json<ImportDetailResponse>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let detail =
        crate::db::vault_imports::get_import_detail(&mut conn, &auth.account_id, import_id)
            .await
            .map_err(ApiError::from)?;

    Ok(Json(import_detail_response(detail)))
}

/// Start an import session and return its id. Finish the session at
/// POST /v1/imports/{id}/complete.
#[utoipa::path(
    post,
    path = "/v1/imports",
    tag = "Import",
    security(("bearer" = [])),
    request_body = CreateImportBody,
    responses(
        (status = 200, body = CreateImportResponse),
        (status = 400, body = crate::server::ErrorBody),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody),
        (
            status = 409,
            body = crate::server::ErrorBody,
            description = "The account already has an active import session"
        )
    )
)]
pub(crate) async fn imports_create_handler(
    State(state): State<AppState>,
    ImportAccess(auth): ImportAccess,
    Json(body): Json<CreateImportBody>,
) -> Result<Json<CreateImportResponse>, ApiError> {
    if body.source.trim().is_empty() {
        return Err(ApiError::BadRequest("body field source is required".into()));
    }
    validate_source_id(&body.source).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    ImportMode::parse(&body.mode).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let account = resolve_import_account(&auth, body.account.as_deref(), &state.db).await?;
    let stage = match body.stage.as_deref() {
        None => crate::db::vault_imports::ImportStage::Parse,
        Some(raw) => crate::db::vault_imports::ImportStage::parse(raw).ok_or_else(|| {
            ApiError::BadRequest(format!(
                "invalid import stage '{raw}'; expected one of parse, write, awaiting_gate_1, transcode, awaiting_gate_2, pushing"
            ))
        })?,
    };
    // Credentials never reach the row, whoever the client is.
    let form = body.form.as_ref().map(strip_form_credentials);
    let form_json = optional_json_string(form.as_ref(), "form")?;
    let fingerprint_json =
        optional_json_string(body.source_fingerprint.as_ref(), "source_fingerprint")?;
    let identities_json =
        optional_json_string(body.source_identities.as_ref(), "source_identities")?;

    let mut conn = state.db.acquire().await?;
    crate::db::account_profile::ensure_account_row(&mut conn, &account).await?;
    let args = crate::db::vault_imports::StartImportArgs {
        account_id: &account,
        source: &body.source,
        mode: &body.mode,
        tool: body.tool.as_deref(),
        stage,
        staging_dir: body.staging_dir.as_deref(),
        device_id: body.device_id.as_deref(),
        form_json: form_json.as_deref(),
        source_fingerprint: fingerprint_json.as_deref(),
        source_identities: identities_json.as_deref(),
    };
    let id = crate::db::vault_imports::start_import(&mut conn, &args).await?;

    Ok(Json(CreateImportResponse { id }))
}

/// Record the outcome of an import session started with POST /v1/imports.
#[utoipa::path(
    post,
    path = "/v1/imports/{id}/complete",
    tag = "Import",
    security(("bearer" = [])),
    params(("id" = i64, Path, description = "Import session id")),
    request_body = CompleteImportBody,
    responses(
        (status = 200, body = CompleteImportResponse),
        (status = 400, body = crate::server::ErrorBody),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody),
        (status = 404, body = crate::server::ErrorBody)
    )
)]
pub(crate) async fn imports_complete_handler(
    State(state): State<AppState>,
    ImportAccess(auth): ImportAccess,
    AxumPath(import_id): AxumPath<i64>,
    Json(body): Json<CompleteImportBody>,
) -> Result<Json<CompleteImportResponse>, ApiError> {
    let account = resolve_import_account(&auth, None, &state.db).await?;
    validate_complete_import_issues(&body.issues)?;
    validate_import_status(body.status.as_deref())?;
    let summary_json = match body.summary {
        Some(summary) => Some(
            serde_json::to_string(&summary)
                .map_err(|e| ApiError::Internal(format!("serialize import summary: {e}")))?,
        ),
        None => None,
    };
    let args = crate::db::vault_imports::CompleteImportArgs {
        ok: body.ok,
        status: body.status,
        message_count: body.message_count,
        attachment_count: body.attachment_count,
        bytes_uploaded: body.bytes_uploaded,
        duration_ms: body.duration_ms,
        parse_ms: body.parse_ms,
        attachments_ms: body.attachments_ms,
        prepare_ms: body.prepare_ms,
        upload_ms: body.upload_ms,
        summary_json,
        issues: body
            .issues
            .into_iter()
            .map(|issue| crate::db::vault_imports::ImportIssueInput {
                kind: issue.kind,
                step: issue.step,
                item: issue.item,
                reason: issue.reason,
            })
            .collect(),
    };
    let mut conn = state.db.acquire().await?;
    let row = crate::db::vault_imports::complete_import(&mut conn, &account, import_id, &args)
        .await
        .map_err(
            |e| match e.downcast::<crate::db::vault_imports::ImportLookupError>() {
                Ok(lookup) => ApiError::from(lookup),
                Err(other) => ApiError::Internal(other.to_string()),
            },
        )?;

    create_import_saved_search(&mut conn, &account, &row).await;
    create_import_contact_group(&mut conn, &account, &row).await;

    Ok(Json(CompleteImportResponse {
        id: row.id,
        status: row.status,
        message_count: row.message_count,
        attachment_count: row.attachment_count,
        bytes_uploaded: row.bytes_uploaded,
    }))
}

/// Add the sidebar shortcut to the messages this run brought in.
///
/// Only runs that stored something get one: a saved search matching nothing is
/// worse than no saved search, and a run that stored nothing is still visible
/// in Import History either way.
///
/// The shortcut is a convenience, not a record, so a failure here is logged and
/// the import still reports success. The person may delete the saved search
/// afterwards; the `vault_imports` row it points at is permanent.
async fn create_import_saved_search(
    conn: &mut sqlx::AnyConnection,
    account_id: &str,
    row: &crate::db::vault_imports::VaultImportRow,
) {
    if row.message_count <= 0 {
        return;
    }
    let date_ymd = import_date_ymd(row);
    if let Err(e) = crate::db::saved_searches::create_for_import(
        conn,
        account_id,
        row.id,
        &row.source,
        &date_ymd,
    )
    .await
    {
        eprintln!(
            "warning: import {} stored {} messages but its saved search could not be created: {e:?}",
            row.id, row.message_count
        );
    }
}

/// One contact an import run touched, and whether the run created it.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct ImportContactRow {
    /// Contact id.
    pub id: i64,
    /// Preferred name; empty when the run learned an address and no name.
    pub name: String,
    /// True when this run created the contact, false when it only changed one
    /// that already existed.
    pub is_new: bool,
}

/// Response for `GET /v1/imports/{id}/contacts`.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct ImportContactsResponse {
    /// Contacts the run created or changed, newest first.
    pub contacts: Vec<ImportContactRow>,
    /// How many of them the run created.
    pub new_count: u64,
    /// How many it only changed.
    pub changed_count: u64,
}

/// List the contacts one import run created or changed.
///
/// New and changed are told apart by comparing each contact's `created_at`
/// against the moment the run started: a contact first recorded during the run
/// is new, one merely touched is changed.
#[utoipa::path(
    get,
    path = "/v1/imports/{id}/contacts",
    tag = "Import",
    security(("bearer" = [])),
    params(("id" = i64, Path, description = "Import session id")),
    responses(
        (status = 200, body = ImportContactsResponse),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody),
        (status = 404, body = crate::server::ErrorBody)
    )
)]
pub(crate) async fn import_contacts_handler(
    State(state): State<AppState>,
    ImportAccess(auth): ImportAccess,
    AxumPath(import_id): AxumPath<i64>,
) -> Result<Json<ImportContactsResponse>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let detail =
        crate::db::vault_imports::get_import_detail(&mut conn, &auth.account_id, import_id)
            .await
            .map_err(ApiError::from)?;
    let started_at = detail.row.started_at.clone();

    let rows: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT id, preferred_name, created_at FROM contacts
         WHERE account_id = $1 AND (created_at >= $2 OR last_modified >= $2)
         ORDER BY created_at DESC, id DESC",
    )
    .bind(&auth.account_id)
    .bind(&started_at)
    .fetch_all(&mut *conn)
    .await
    .map_err(|e| ApiError::Internal(format!("list import contacts: {e}")))?;

    let mut new_count = 0u64;
    let mut changed_count = 0u64;
    let contacts = rows
        .into_iter()
        .map(|(id, name, created_at)| {
            let is_new = created_at >= started_at;
            if is_new {
                new_count += 1;
            } else {
                changed_count += 1;
            }
            ImportContactRow { id, name, is_new }
        })
        .collect();

    Ok(Json(ImportContactsResponse {
        contacts,
        new_count,
        changed_count,
    }))
}

/// Create the Contact Group naming the contacts an import run touched.
///
/// The group is a shortcut pointing at the run: the person may delete it, and
/// the `vault_imports` row it describes is permanent either way. Membership is
/// a snapshot, because what a run brought in is a historical fact that should
/// not silently rewrite itself as contacts change later.
///
/// A failure here is reported and swallowed, the same as the saved search: the
/// messages are already in the vault, and losing a shortcut is not a reason to
/// call the import failed.
async fn create_import_contact_group(
    conn: &mut sqlx::AnyConnection,
    account_id: &str,
    row: &crate::db::vault_imports::VaultImportRow,
) {
    let started_at = row.started_at.as_str();
    if started_at.is_empty() {
        return;
    }
    let touched =
        match crate::db::contacts::contacts_touched_since(conn, account_id, started_at).await {
            Ok(ids) if ids.is_empty() => return,
            Ok(ids) => ids,
            Err(e) => {
                eprintln!(
                    "warning: import {} could not list the contacts it touched: {e:?}",
                    row.id
                );
                return;
            }
        };
    let name = import_contact_group_name(row);
    if let Err(e) = crate::named_membership::set_membership(
        crate::named_membership::group_spec(),
        conn,
        account_id,
        &touched,
        &name,
        true,
    )
    .await
    {
        eprintln!(
            "warning: import {} stored {} contacts but its Contact Group could not be created: {e:?}",
            row.id,
            touched.len()
        );
        return;
    }
    if let Err(e) = crate::db::contacts::set_group_kind(conn, account_id, &name, "import").await {
        eprintln!(
            "warning: import {}'s Contact Group was created but not marked as import-made: {e:?}",
            row.id
        );
    }
}

/// Name for an import run's Contact Group. The run id keeps it unique per
/// account, which `contact_groups.name` requires.
fn import_contact_group_name(row: &crate::db::vault_imports::VaultImportRow) -> String {
    format!("{} import {}", row.source, import_date_ymd(row))
}

/// Calendar date to name an import's saved search after: the day the run
/// finished, falling back to the day it started, then to today. All three are
/// UTC, because that is what `vault_imports` stores.
fn import_date_ymd(row: &crate::db::vault_imports::VaultImportRow) -> String {
    row.finished_at
        .as_deref()
        .or(Some(row.started_at.as_str()))
        .and_then(|ts| ts.get(..10))
        .filter(|d| d.len() == 10)
        .map(str::to_string)
        .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d").to_string())
}

fn parse_summary_json(summary_json: Option<String>) -> serde_json::Value {
    match summary_json {
        Some(raw) => serde_json::from_str(&raw).unwrap_or(serde_json::Value::String(raw)),
        None => serde_json::Value::Null,
    }
}

fn import_detail_response(detail: crate::db::vault_imports::ImportDetail) -> ImportDetailResponse {
    let row = detail.row;
    let issues = detail
        .issues
        .into_iter()
        .map(|issue| ImportDetailIssueResponse {
            kind: issue.kind,
            step: issue.step,
            item: issue.item,
            reason: issue.reason,
        })
        .collect();

    ImportDetailResponse {
        id: row.id,
        source: row.source,
        tool: row.tool,
        mode: row.mode,
        status: row.status,
        started_at: row.started_at,
        finished_at: row.finished_at,
        message_count: row.message_count,
        attachment_count: row.attachment_count,
        bytes_uploaded: row.bytes_uploaded,
        duration_ms: row.duration_ms,
        parse_ms: row.parse_ms,
        attachments_ms: row.attachments_ms,
        prepare_ms: row.prepare_ms,
        upload_ms: row.upload_ms,
        summary: parse_summary_json(row.summary_json),
        issues,
    }
}

/// One live import session, as the desktop app needs to resume it.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct ActiveImportSession {
    pub(crate) id: i64,
    pub(crate) source: String,
    pub(crate) mode: String,
    pub(crate) status: String,
    pub(crate) started_at: String,
    pub(crate) stage: Option<String>,
    pub(crate) staging_dir: Option<String>,
    pub(crate) device_id: Option<String>,
    /// Import form snapshot, or null.
    pub(crate) form: serde_json::Value,
    /// Source path, size, mtime, and message count, or null.
    pub(crate) source_fingerprint: serde_json::Value,
    /// Addresses the backup's device sent from (JSON array), or null.
    pub(crate) source_identities: serde_json::Value,
    /// What the user approved at the last gate they passed, or null.
    ///
    /// Same column `POST /v1/imports/{id}/stage` writes with its `summary`
    /// field — read back here so a reload between an approval and
    /// completion doesn't lose the plan the eventual outcome is diffed
    /// against.
    pub(crate) summary: serde_json::Value,
}

/// The account's live session, or null when there is none.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct ActiveImportResponse {
    pub(crate) session: Option<ActiveImportSession>,
}

/// The account's active import session, if it has one.
#[utoipa::path(
    get,
    path = "/v1/imports/active",
    tag = "Import",
    security(("bearer" = [])),
    responses(
        (status = 200, body = ActiveImportResponse),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody)
    )
)]
pub(crate) async fn imports_active_handler(
    State(state): State<AppState>,
    ImportAccess(auth): ImportAccess,
) -> Result<Json<ActiveImportResponse>, ApiError> {
    let account = resolve_import_account(&auth, None, &state.db).await?;
    let mut conn = state.db.acquire().await?;
    let row = crate::db::vault_imports::get_active_import(&mut conn, &account).await?;
    Ok(Json(ActiveImportResponse {
        session: row.map(|row| ActiveImportSession {
            id: row.id,
            source: row.source,
            mode: row.mode,
            status: row.status,
            started_at: row.started_at,
            stage: row.stage,
            staging_dir: row.staging_dir,
            device_id: row.device_id,
            form: parse_summary_json(row.form_json),
            source_fingerprint: parse_summary_json(row.source_fingerprint),
            source_identities: parse_summary_json(row.source_identities),
            summary: parse_summary_json(row.summary_json),
        }),
    }))
}

/// New stage for a live session.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct SetImportStageBody {
    pub(crate) stage: String,
    /// What the user approved at the gate they just passed, when they passed one.
    ///
    /// Recorded here rather than at completion so an approval survives a
    /// reload: the summary shown at a gate is recomputed from the folder, but
    /// what was approved is a different question and only the session
    /// remembers it. Absent leaves the stored `summary_json` untouched —
    /// most stage changes carry nothing, and treating absent as null would
    /// throw away the plan the outcome is later judged against.
    #[serde(default)]
    pub(crate) summary: Option<serde_json::Value>,
}

/// Confirmation that the stage moved.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct SetImportStageResponse {
    pub(crate) stage: String,
}

/// Move a live import session to another stage.
#[utoipa::path(
    post,
    path = "/v1/imports/{id}/stage",
    tag = "Import",
    security(("bearer" = [])),
    params(("id" = i64, Path, description = "Import session id")),
    request_body = SetImportStageBody,
    responses(
        (status = 200, body = SetImportStageResponse),
        (status = 400, body = crate::server::ErrorBody),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody),
        (status = 404, body = crate::server::ErrorBody)
    )
)]
pub(crate) async fn imports_stage_handler(
    State(state): State<AppState>,
    ImportAccess(auth): ImportAccess,
    AxumPath(import_id): AxumPath<i64>,
    Json(body): Json<SetImportStageBody>,
) -> Result<Json<SetImportStageResponse>, ApiError> {
    let account = resolve_import_account(&auth, None, &state.db).await?;
    let stage = crate::db::vault_imports::ImportStage::parse(&body.stage).ok_or_else(|| {
        ApiError::BadRequest(format!(
            "invalid import stage '{}'; expected one of parse, write, awaiting_gate_1, transcode, awaiting_gate_2, pushing",
            body.stage
        ))
    })?;
    let summary_json = optional_json_string(body.summary.as_ref(), "summary")?;
    let mut conn = state.db.acquire().await?;
    crate::db::vault_imports::set_import_stage(
        &mut conn,
        &account,
        import_id,
        stage,
        summary_json.as_deref(),
    )
    .await?;
    Ok(Json(SetImportStageResponse {
        stage: stage.as_str().to_string(),
    }))
}

/// Confirmation that a session was discarded.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct DiscardImportResponse {
    pub(crate) id: i64,
    pub(crate) status: String,
}

/// Discard a live import session, freeing the account's single slot.
#[utoipa::path(
    post,
    path = "/v1/imports/{id}/discard",
    tag = "Import",
    security(("bearer" = [])),
    params(("id" = i64, Path, description = "Import session id")),
    responses(
        (status = 200, body = DiscardImportResponse),
        (status = 400, body = crate::server::ErrorBody),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody),
        (status = 404, body = crate::server::ErrorBody)
    )
)]
pub(crate) async fn imports_discard_handler(
    State(state): State<AppState>,
    ImportAccess(auth): ImportAccess,
    AxumPath(import_id): AxumPath<i64>,
) -> Result<Json<DiscardImportResponse>, ApiError> {
    let account = resolve_import_account(&auth, None, &state.db).await?;
    let mut conn = state.db.acquire().await?;
    crate::db::vault_imports::discard_import(&mut conn, &account, import_id).await?;
    Ok(Json(DiscardImportResponse {
        id: import_id,
        status: "cancelled".into(),
    }))
}

/// Import one message-ir JSONL body into the vault.
#[utoipa::path(
    post,
    path = "/v1/import",
    tag = "Import",
    security(("bearer" = [])),
    params(
        ("source" = String, Query),
        ("account" = Option<String>, Query),
        ("mode" = Option<String>, Query, description = "Default append"),
        ("dedupe" = Option<bool>, Query),
        ("import_id" = Option<i64>, Query)
    ),
    request_body(
        content(
            ("application/x-ndjson"),
            ("application/jsonl")
        ),
        description = "message-ir JSONL as application/x-ndjson or application/jsonl. Attachments are uploaded first by SHA-256 through /v1/assets."
    ),
    responses(
        (status = 200, body = ImportResponse),
        (status = 400, body = crate::server::ErrorBody),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody),
        (status = 404, body = crate::server::ErrorBody),
        (
            status = 409,
            body = crate::server::ErrorBody,
            description = "The account already has an active import session"
        ),
        (status = 413, body = crate::server::ErrorBody),
        (
            status = 415,
            body = crate::server::ErrorBody,
            description = "The body is not JSON Lines (multipart/form-data is not accepted)"
        )
    )
)]
pub(crate) async fn import_handler(
    State(state): State<AppState>,
    ImportAccess(auth): ImportAccess,
    headers: HeaderMap,
    Query(mut query): Query<ImportQuery>,
    request: Request,
) -> Result<Json<ImportResponse>, ApiError> {
    let Some(ct) = content_type_base(&headers) else {
        return Err(ApiError::BadRequest(
            "Content-Type required (application/x-ndjson or application/jsonl)".into(),
        ));
    };

    if query.source.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "query param source is required".into(),
        ));
    }
    validate_source_id(&query.source).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let account = resolve_import_account(&auth, query.account.as_deref(), &state.db).await?;
    query.account = Some(account);

    if is_jsonl_content_type(ct) {
        let temp = tempfile::tempdir().map_err(|e| ApiError::Internal(format!("temp dir: {e}")))?;
        let jsonl_path = temp.path().join("_import.jsonl");
        let n = stream_body_to_file(request.into_body(), &jsonl_path, state.max_body_bytes).await?;
        if n == 0 {
            return Err(ApiError::BadRequest("request body is empty".into()));
        }
        // The import pipeline does blocking file IO (JSONL parse, asset
        // hashing and copies) — run it off the async workers so a large
        // import cannot stall unrelated requests.
        let handle = tokio::runtime::Handle::current();
        let response = tokio::task::spawn_blocking(move || {
            handle.block_on(run_import_path(state, query, jsonl_path))
        })
        .await
        .map_err(|e| ApiError::Internal(format!("import task failed: {e}")))?;
        drop(temp);
        return response;
    }

    // Only JSON Lines is an import body. Attachments never travel with it:
    // they are uploaded first, by SHA-256, through `/v1/assets`. A body in
    // another type is the wrong media type, not a malformed request.
    Err(ApiError::Status(
        axum::http::StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "Content-Type must be application/x-ndjson or application/jsonl".into(),
    ))
}

/// Bound on concurrent HTTP imports: each import holds one pooled connection
/// for its whole run, so at most this many may overlap and the remaining
/// connections stay available for auth, search, and export.
const MAX_CONCURRENT_IMPORTS: usize = 2;

fn import_semaphore() -> &'static tokio::sync::Semaphore {
    static SEMAPHORE: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    SEMAPHORE.get_or_init(|| tokio::sync::Semaphore::new(MAX_CONCURRENT_IMPORTS))
}

/// Turn an import's error into the HTTP failure a caller should see.
///
/// The two failures a sender can fix by changing the file travel up the
/// pipeline as `ImportFailure` and become a 400 with their own sentence.
/// Everything else (a disk or database error, a bug) is a 500: the message
/// goes to stderr and the client sees "internal server error".
fn classify_import_error(err: anyhow::Error) -> ApiError {
    match ImportFailure::in_error(&err) {
        Some(failure) => ApiError::BadRequest(failure.to_string()),
        None => ApiError::Internal(format!("{err:#}")),
    }
}

/// Callers validate `query.source`; `import_handler` is the only entry point,
/// and `source` names an on-disk directory.
async fn run_import_path(
    state: AppState,
    query: ImportQuery,
    jsonl_path: PathBuf,
) -> Result<Json<ImportResponse>, ApiError> {
    // An import holds one pooled connection for its whole run (JSONL parse,
    // asset IO, promote). Bound concurrent imports here so they can never
    // drain the pool; the semaphore is taken before the per-account lock so
    // lock order (semaphore → account → pool) is consistent everywhere.
    let _import_permit = import_semaphore()
        .acquire()
        .await
        .map_err(|_| ApiError::Internal("vault is shutting down".into()))?;
    let mode = ImportMode::parse(&query.mode).map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let cfg = Arc::clone(&state.cfg);
    let account = query
        .account
        .clone()
        .ok_or_else(|| ApiError::BadRequest("account is required".into()))?;
    let source_id = query.source.clone();
    let do_dedupe = query.dedupe;
    let query_import_id = query.import_id;

    let account_lock = {
        let mut map = state.account_import_locks.lock().await;
        map.entry(account.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    };
    let _guard = account_lock.lock().await;

    // One pooled connection held for the whole import; the import semaphore
    // taken above keeps enough of the pool free for other requests.
    let mut conn = state.db.acquire().await?;

    // Validate client-owned sessions before staging work so bad ids return 400.
    if let Some(id) = query_import_id {
        crate::db::vault_imports::require_reusable_import(
            &mut conn,
            &account,
            id,
            &source_id,
            mode.as_str(),
        )
        .await?;
    }

    // Attachment paths resolve only through assets already uploaded by
    // SHA-256; the import body never carries files of its own.
    let assets_dir = cfg.paths.assets_dir_for_account(&account, &source_id);

    // A client session (vault-push) is closed by the client. Otherwise open
    // one of our own so the Settings import table records curl and
    // single-POST runs too.
    let owned = if query_import_id.is_some() {
        None
    } else {
        crate::db::account_profile::ensure_account_row(&mut conn, &account).await?;
        Some(OwnedSession::start(&mut conn, &account, &source_id, mode, "http").await?)
    };
    let import_id = query_import_id.or_else(|| owned.as_ref().map(|session| session.id));

    let opts = ImportOptions::fixed(FixedImportArgs {
        assets_dir: &assets_dir,
        asset_root: &assets_dir,
        contacts: None,
        overwrite_contacts: false,
        mode,
        source: &source_id,
        account_id: &account,
        fill_content_keys: do_dedupe,
        import_id,
    });
    let import_result = import::import_jsonl_files_on_conn(
        &mut conn,
        &[jsonl_path],
        &opts,
        import::ImportSchemaMode::AssumeReady,
    )
    .await;
    if let Some(session) = owned {
        session.finish(&mut conn, &import_result).await;
    }
    let stats = import_result.map_err(classify_import_error)?;
    let dedupe_stats = if do_dedupe {
        Some(dedupe::dedupe_cross_source(&mut conn, &account, None, 2).await?)
    } else {
        None
    };

    Ok(Json(ImportResponse {
        source: source_id,
        account,
        stats,
        dedupe: dedupe_stats.map(|d| DedupeResponse {
            keys_filled: d.keys_filled,
            exact_groups: d.exact_groups,
            exact_flagged: d.exact_flagged,
            near_flagged: d.near_flagged,
        }),
    }))
}

#[cfg(test)]
mod tests;
