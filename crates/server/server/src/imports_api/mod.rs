//! Import message-ir JSONL into the database.
//!
//! The pipeline runs in two stages: `staging` parses JSONL files and writes
//! staging rows, and `promote` copies staging rows into the production tables.
//! `staging` calls `contact_name` as it goes, to link handles to contacts and
//! merge display names.
//! The HTTP handlers for the `/v1/imports` routes, an Import Run and the
//! batches posted into it, live at the end of this module.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result, bail};
pub use message_crate_api_types::ImportMode;
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use tempfile::TempDir;

use crate::extract::{Json, Path as AxumPath, Query};
use axum::extract::{Request, State};
use axum::http::HeaderMap;

use crate::assets_api::AssetStats;
use crate::config::{PathsConfig, validate_source_id};
#[cfg(test)]
use crate::db::engine;
use crate::db::imports::{self, CompleteImportArgs};
use crate::db::maintenance;
use crate::db::schema;
use media::MediaMode;

pub mod contact_name;
pub mod failure;
pub mod promote;
pub mod staging;

pub use failure::{ImportError, ImportFailure, MISSING_GUID_LINES_NAMED};

use staging::StagingInserts;

use crate::dedupe;
use crate::imports_api::{self};
use crate::paging::{
    DEFAULT_LIST_LIMIT, MAX_LIST_OFFSET, Page, PageQuery, page_params, parse_sort,
};
use crate::server::{
    ApiError, AppState, Created, ImportAccess, content_type_base, is_jsonl_content_type,
    resolve_import_account, stream_body_to_file,
};

/// Full import settings: paths, mode, and media handling.
#[derive(Debug, Clone)]
pub struct ImportOptions<'a> {
    /// Content-addressed asset store when [`Self::source_from_jsonl`] is false.
    pub assets_dir: &'a Path,
    /// Root for resolving relative attachment paths in JSONL.
    pub asset_root: &'a Path,
    /// Import mode: replace or append.
    pub mode: ImportMode,
    /// Fixed source id (HTTP / `--source` override). Ignored when `source_from_jsonl`.
    pub source: &'a str,
    /// Account the import writes into.
    pub account_id: i64,
    /// Fill missing `content_key` values during promote (needed before cross-source dedupe).
    pub fill_content_keys: bool,
    /// Optional Import Run id (messages stamped on promote).
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
    /// Import mode: replace or append.
    pub mode: ImportMode,
    /// Fixed source id applied to every conversation.
    pub source: &'a str,
    /// Account the import writes into.
    pub account_id: i64,
    /// Fill missing `content_key` values during promote.
    pub fill_content_keys: bool,
    /// Optional Import Run id (messages stamped on promote).
    pub import_id: Option<i64>,
}

impl<'a> ImportOptions<'a> {
    /// HTTP / tests / reset-demo: fixed source + assets dir, copy media.
    pub fn fixed(args: FixedImportArgs<'a>) -> Self {
        Self {
            assets_dir: args.assets_dir,
            asset_root: args.asset_root,
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
    /// Contacts the import created for participants nothing else owned.
    pub contacts_created: u64,
    /// Messages hidden as duplicates within this import.
    pub messages_deduped: u64,
    /// Messages added by an append-mode import.
    pub messages_appended: u64,
    /// Import mode.
    pub mode: ImportMode,
    /// Flagged phone handles (ambiguous; review note set) inserted by this import.
    pub phones_needing_review: u64,
    /// Identities of type `other` this import met for people: a name the
    /// backup gave with no address, or a sender such as `AMAZON`. Each one is
    /// a person the exporter could not tie to an address, so a count above
    /// zero says the import is incomplete.
    pub other_identities: u64,
}

impl ImportStats {
    /// Add one staged file's counts onto the running import totals.
    fn merge_file(&mut self, other: &ImportStats) {
        self.conversations += other.conversations;
        self.participants += other.participants;
        self.messages += other.messages;
        self.attachments += other.attachments;
        self.tapbacks += other.tapbacks;
        self.contacts_created += other.contacts_created;
        self.messages_deduped += other.messages_deduped;
        self.phones_needing_review += other.phones_needing_review;
        self.other_identities += other.other_identities;
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

/// An import session this process opened for one run, as opposed to one a
/// client (message-crate-push) owns and closes itself. Whoever starts one must
/// finish it whatever the import does, so the Settings import table never
/// shows a run stuck in progress.
pub(crate) struct OwnedSession {
    account_id: i64,
    /// The `imports` row id; the import stamps it on every message.
    pub id: i64,
}

impl OwnedSession {
    /// Record a session at the parse stage. Nothing client-side (staging
    /// folder, device, form) is known for a run started here.
    ///
    /// # Errors
    ///
    /// Returns an error when the account already has a live session or the
    /// row cannot be inserted.
    pub(crate) async fn start(
        conn: &mut SqliteConnection,
        account_id: i64,
        source: &str,
        mode: ImportMode,
        tool: &str,
    ) -> std::result::Result<Self, imports::StartImportError> {
        let id = imports::start_import(
            conn,
            &imports::StartImportArgs::new(account_id, source, mode.as_str(), Some(tool)),
        )
        .await?;
        Ok(Self { account_id, id })
    }

    /// Mark the session succeeded with the run's counts, or failed. Not
    /// being able to record the outcome is a warning on stderr, never an
    /// error: the import's own result is what the caller returns.
    pub(crate) async fn finish(self, conn: &mut SqliteConnection, result: &Result<ImportStats>) {
        let outcome = match result {
            Ok(stats) => CompleteImportArgs::succeeded(stats.messages, stats.attachments),
            Err(_) => CompleteImportArgs::failed(),
        };
        // The import itself is done either way; a failure to record that is
        // worth a log line, not an error the caller would have to unwind.
        if let Err(error) = complete_run(conn, self.account_id, self.id, &outcome).await {
            tracing::warn!(import_id = self.id, error = %error, "complete_import failed");
        }
    }
}

/// Record an Import Run's outcome, then make the run's Saved Search and
/// Contact Group. Every path that completes a run calls this: the HTTP
/// `complete_import`, the server's `import` command and the Demo Account
/// build (through [`OwnedSession::finish`]), so each run gets the same
/// shortcuts however it was made.
///
/// # Errors
///
/// Returns the error of recording the outcome, such as
/// [`imports::ImportLookupError`] for a run that is not running. A shortcut
/// that cannot be made is a warning, never an error.
pub(crate) async fn complete_run(
    conn: &mut SqliteConnection,
    account_id: i64,
    import_id: i64,
    outcome: &CompleteImportArgs,
) -> Result<imports::ImportRow> {
    let row = imports::complete_import(conn, account_id, import_id, outcome).await?;
    create_import_saved_search(conn, account_id, &row).await;
    create_import_contact_group(conn, account_id, &row).await;
    Ok(row)
}

/// Whether import should run DDL/schema ensure on the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportSchemaMode {
    /// Tests: ensure the schema first.
    Ensure,
    /// HTTP serve hot path: schema already ensured on the warm connection.
    AssumeReady,
}

/// Test helper: open a configured database and run one import.
///
/// Production paths run on their own connection: HTTP serve through
/// [`import_jsonl_files_on_conn`], and the `import` command and the Demo
/// Account build through [`import_on_conn`].
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
    import_on_conn(&mut conn, paths, opts, ImportSchemaMode::Ensure).await
}

/// A fixed source needs no check here: every caller that passes one has
/// already put it through `validate_source_id`, or passes a constant.
fn validate_import_options(opts: &ImportOptions<'_>) -> Result<()> {
    if opts.source_from_jsonl && opts.paths.is_none() {
        bail!("source_from_jsonl requires config paths for per-source assets");
    }
    Ok(())
}

/// Import onto an existing connection (warm serve path or tests).
///
/// # Errors
///
/// Returns [`ImportError::Rejected`] when the file breaks a rule the sender
/// can fix, [`ImportError::Run`] when the import run is no longer running,
/// and [`ImportError::Internal`] when the options are invalid or
/// staging or promote fails for any other reason.
pub async fn import_jsonl_files_on_conn(
    conn: &mut SqliteConnection,
    paths: &[PathBuf],
    opts: &ImportOptions<'_>,
    schema_mode: ImportSchemaMode,
) -> Result<ImportStats, ImportError> {
    Ok(import_on_conn(conn, paths, opts, schema_mode).await?)
}

/// [`import_jsonl_files_on_conn`], with every failure still inside `anyhow`,
/// for the callers that only report it: the `import` command, `reset-demo`
/// and [`import_jsonl_files`].
pub(crate) async fn import_on_conn(
    conn: &mut SqliteConnection,
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
        schema::ensure_schema(conn).await?;
    }
    crate::db::account_profile::ensure_account_row(conn, opts.account_id).await?;

    if schema_mode == ImportSchemaMode::Ensure {
        say("  sql:      ensuring schema + resetting staging for account…");
    } else {
        say("  sql:      resetting staging for account…");
    }
    crate::db::staging::reset_for_account(conn, opts.account_id).await?;
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
        mode: opts.mode,
        ..Default::default()
    };
    let started = Instant::now();
    // Stats on already-committed rows so promote's guid join can use the
    // indexes. Outside the transaction, because a failed ANALYZE is only a
    // warning.
    maintenance::analyze_import_tables(conn).await;

    // Staging and promote share one transaction. Staging makes contacts and,
    // by ADR-0013, discards trashed ones; none of that may outlive a promote
    // that fails. The write lock is taken up front (IMMEDIATE) so two
    // imports for different accounts cannot race into SQLITE_BUSY at the
    // first INSERT. The run is checked again under that lock: a run discarded
    // or completed while the batch uploaded takes no messages.
    let mut tx = crate::db::begin_write(conn).await?;
    if let Some(import_id) = opts.import_id {
        crate::db::imports::require_running_import(&mut tx, opts.account_id, import_id).await?;
    }
    let asset_stats = stage_all_files(&mut tx, paths, opts, &mut stats, started).await?;

    say(&format!(
        "  import:   promoting staging → production ({:.0}s so far)…",
        started.elapsed().as_secs_f64()
    ));
    promote_step(&mut tx, opts, &wipe_sources, &mut stats).await?;
    crate::db::staging::reset_for_account(&mut tx, opts.account_id).await?;
    tx.commit().await?;

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

/// Stage every file into the staging tables inside the import's transaction
/// `tx`, and print progress along the way.
///
/// # Errors
///
/// Returns an error when a file cannot be read or a row cannot be written.
async fn stage_all_files(
    tx: &mut SqliteConnection,
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
    let mut stmts = StagingInserts::new(opts.account_id, opts.import_id);

    for (idx, path) in paths.iter().enumerate() {
        let file_stats = staging::import_file_to_staging(
            tx,
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
    }
    Ok(asset_stats)
}

/// Move staged rows into production and fold the promote counts into `stats`.
///
/// In append mode the promote step is the only place the final row counts
/// are known, so they replace the staging counts.
///
/// # Errors
///
/// Returns an error when a promote statement fails.
async fn promote_step(
    tx: &mut SqliteConnection,
    opts: &ImportOptions<'_>,
    wipe_sources: &[String],
    stats: &mut ImportStats,
) -> Result<()> {
    let promote_stats = promote::promote_append(
        tx,
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

/// What one batch imports under. Every field is read from the Import Run's
/// row, never from the request: the run was created with them once, so two
/// batches cannot disagree.
#[derive(Debug, Clone)]
pub(crate) struct BatchContext {
    pub(crate) account: i64,
    pub(crate) import_id: i64,
    pub(crate) source: String,
    pub(crate) mode: ImportMode,
    pub(crate) dedupe: bool,
}

impl BatchContext {
    /// The context a running row gives a batch. A mode the row spells in a
    /// way the enum does not know is read as `append`, the mode that never
    /// removes anything.
    pub(crate) fn from_row(row: &crate::db::imports::ImportRow) -> Self {
        let mode = match row.mode.as_str() {
            "replace" => ImportMode::Replace,
            _ => ImportMode::Append,
        };
        Self {
            account: row.account_id,
            import_id: row.id,
            source: row.source.clone(),
            mode,
            dedupe: row.dedupe,
        }
    }
}

/// Import result: stats plus optional dedupe counts.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct CreateImportBatchResponse {
    source: String,
    account: i64,
    #[serde(flatten)]
    stats: ImportStats,
    #[serde(skip_serializing_if = "Option::is_none")]
    dedupe: Option<DedupeCounts>,
}

/// Cross-source dedupe outcome.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct DedupeCounts {
    keys_filled: u64,
    exact_groups: u64,
    exact_flagged: u64,
    near_flagged: u64,
}

/// Source, mode, dedupe and tool for a new Import Run. The bearer token
/// names the account.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct CreateImportRequest {
    pub(crate) source: String,
    #[serde(default)]
    pub(crate) mode: ImportMode,
    /// Run cross-source soft-dedupe after each batch.
    #[serde(default)]
    pub(crate) dedupe: bool,
    #[serde(default)]
    pub(crate) tool: Option<String>,
    /// Stage the run opens at. Defaults to `parse`.
    #[serde(default)]
    pub(crate) stage: Option<String>,
    /// Absolute staging path on the client that owns this Import Run.
    #[serde(default)]
    pub(crate) staging_dir: Option<String>,
    /// Which install is creating the Import Run.
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

/// The new Import Run's id.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct CreateImportResponse {
    pub(crate) id: i64,
}

/// Final stats and issues for a running Import Run. The outcome is stated
/// once, as `status`. The run's message and attachment counts are not part
/// of it: the server counts what the run holds, since a resumed Upload's
/// client knows only what the resume sent.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct CompleteImportRequest {
    /// How the run ended: `completed`, `completed_with_issues` or `failed`.
    pub(crate) status: String,
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
    pub(crate) issues: Vec<CompleteImportIssueRequest>,
}

/// One parse/convert/upload issue from the import.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct CompleteImportIssueRequest {
    pub(crate) kind: String,
    pub(crate) step: String,
    pub(crate) item: String,
    pub(crate) reason: String,
}

fn validate_complete_import_issues(issues: &[CompleteImportIssueRequest]) -> Result<(), ApiError> {
    for issue in issues {
        match issue.kind.as_str() {
            "error" | "skip" => {}
            other => {
                return Err(ApiError::validation(format!(
                    "invalid import issue kind '{other}'; expected 'error' or 'skip'"
                )));
            }
        }
    }
    Ok(())
}

fn validate_import_status(status: &str) -> Result<(), ApiError> {
    match status {
        "completed" | "completed_with_issues" | "failed" => Ok(()),
        other => Err(ApiError::validation(format!(
            "invalid import status '{other}'; expected 'completed', 'completed_with_issues', or 'failed'"
        ))),
    }
}

/// Keys a stored form snapshot must never carry.
///
/// The invariant: `imports.form_json` is a durable record read back to
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
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("serialize {field}: {e}"))),
    }
}

/// `GET /v1/imports`: a page, narrowed to one `status` when given. One of the
/// two lists with a filter parameter, beside the Export Run list
/// (`docs/architecture/http-api.md`): it has no search language.
#[derive(Debug, Deserialize)]
pub(crate) struct ListImportsQuery {
    #[serde(default)]
    pub(crate) status: Option<String>,
    #[serde(default)]
    pub(crate) limit: Option<usize>,
    #[serde(default)]
    pub(crate) offset: Option<usize>,
    /// `started_at` or `-started_at`; newest first when absent.
    #[serde(default)]
    pub(crate) sort: Option<String>,
}

/// One stored import issue.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct ImportIssue {
    pub(crate) kind: String,
    pub(crate) step: String,
    item: String,
    reason: String,
}

/// An Import Run: one per import, the same record wherever the interface
/// hands one out. It holds the counts Settings shows, everything the desktop
/// app needs to resume a running run, and the issues the run recorded.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct ImportRun {
    /// Import Run id.
    pub(crate) id: i64,
    /// Source id the run imports.
    pub(crate) source: String,
    /// Importing tool, e.g. `message-crate-push`.
    pub(crate) tool: Option<String>,
    /// Import mode (`replace` or `append`).
    pub(crate) mode: String,
    /// Whether cross-source dedupe runs after each batch.
    pub(crate) dedupe: bool,
    /// Lifecycle status.
    pub(crate) status: crate::db::imports::ImportStatus,
    /// UTC time the run started.
    pub(crate) started_at: String,
    /// UTC time the run finished, when it has.
    pub(crate) finished_at: Option<String>,
    /// Messages counted for the run.
    pub(crate) message_count: i64,
    /// Attachments counted for the run.
    pub(crate) attachment_count: i64,
    /// Bytes uploaded so far.
    pub(crate) bytes_uploaded: i64,
    /// Total wall-clock duration, when finished.
    pub(crate) duration_ms: Option<i64>,
    /// Time spent parsing, when finished.
    pub(crate) parse_ms: Option<i64>,
    /// Time spent on attachments, when finished.
    pub(crate) attachments_ms: Option<i64>,
    /// Time spent preparing conversation files, when finished.
    pub(crate) prepare_ms: Option<i64>,
    /// Time spent uploading, when finished.
    pub(crate) upload_ms: Option<i64>,
    /// Where a running run is; null once it is over.
    pub(crate) stage: Option<String>,
    /// Absolute path to the staging folder on the client that owns the run.
    pub(crate) staging_dir: Option<String>,
    /// Which install created the run.
    pub(crate) device_id: Option<String>,
    /// Import form snapshot, or null.
    pub(crate) form: serde_json::Value,
    /// Source path, size, mtime, and message count, or null.
    pub(crate) source_fingerprint: serde_json::Value,
    /// Addresses the backup's device sent from (JSON array), or null.
    pub(crate) source_identities: serde_json::Value,
    /// What the user approved at the last gate they passed, or null. The
    /// column `PATCH /v1/imports/{id}` writes with its `summary`.
    pub(crate) summary: serde_json::Value,
    /// Issues the run recorded, oldest first.
    pub(crate) issues: Vec<ImportIssue>,
    /// Contacts this run created.
    pub(crate) contacts_new: u64,
    /// Contacts it only changed.
    pub(crate) contacts_changed: u64,
}

/// An Import Run as the owner reads it under another account: its source,
/// mode, times, outcome and counts, and nothing of what the backup held
/// (`docs/adr/0008-the-owner-holds-no-messages.md`, "What the owner may
/// see"). The run's summary, its issues and its form say whom the account
/// talks to, so they stay out, and a field reaches the owner only by being
/// added here.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct OwnerImportRun {
    /// Import Run id.
    pub(crate) id: i64,
    /// Source id the run imports.
    source: String,
    /// Importing tool, e.g. `message-crate-push`.
    tool: Option<String>,
    /// Import mode (`replace` or `append`).
    mode: String,
    /// Lifecycle status.
    status: crate::db::imports::ImportStatus,
    /// UTC time the run started.
    started_at: String,
    /// UTC time the run finished, when it has.
    finished_at: Option<String>,
    /// Messages counted for the run.
    message_count: i64,
    /// Attachments counted for the run.
    attachment_count: i64,
    /// Bytes uploaded so far.
    bytes_uploaded: i64,
    /// Total wall-clock duration, when finished.
    duration_ms: Option<i64>,
    /// Time spent parsing, when finished.
    parse_ms: Option<i64>,
    /// Time spent on attachments, when finished.
    attachments_ms: Option<i64>,
    /// Time spent preparing conversation files, when finished.
    prepare_ms: Option<i64>,
    /// Time spent uploading, when finished.
    upload_ms: Option<i64>,
    /// The whole numbers the run's summary reported, by name. The summary's
    /// other values, among them the addresses a staging summary lists, are
    /// the account's own.
    counts: std::collections::BTreeMap<String, i64>,
    /// Issues the run recorded. Each names the conversation it was about, so
    /// the owner reads how many and not which.
    issue_count: u64,
    /// Contacts this run created.
    contacts_new: u64,
    /// Contacts it only changed.
    contacts_changed: u64,
}

/// The owner's view of one Import Run, read from the run's row, its issue
/// count and its contact tally.
pub(crate) async fn owner_import_run(
    conn: &mut SqliteConnection,
    row: crate::db::imports::ImportRow,
) -> Result<OwnerImportRun, ApiError> {
    let issue_count = crate::db::imports::issue_count(conn, row.id).await?;
    let contacts = crate::db::import_contacts::counts(conn, row.id)
        .await
        .map_err(ApiError::Internal)?;
    let counts = match crate::db::imports::json_column(row.summary_json) {
        serde_json::Value::Object(summary) => summary
            .into_iter()
            .filter_map(|(name, value)| value.as_i64().map(|n| (name, n)))
            .collect(),
        _ => std::collections::BTreeMap::new(),
    };
    Ok(OwnerImportRun {
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
        counts,
        issue_count,
        contacts_new: contacts.new_count,
        contacts_changed: contacts.changed_count,
    })
}

/// The account's Import Runs, newest first, as a page. `status=running`
/// finds the one run the desktop app may resume.
#[utoipa::path(
    get,
    path = "/v1/imports",
    tag = "Import",
    security(("session" = ["import"]), ("api-token" = ["import"])),
    params(
        ("status" = Option<crate::db::imports::ImportStatus>, Query, description = "Only the runs with this status"),
        ("limit" = Option<usize>, Query, description = "Page size, default 40, at most 500"),
        ("offset" = Option<usize>, Query, description = "Rows to skip, at most 50000"),
        ("sort" = Option<String>, Query, description = "`started_at` or `-started_at`. Default `-started_at`, newest first.")
    ),
    responses(
        (status = 200, body = Page<ImportRun>),
    )
)]
pub(crate) async fn list_imports(
    State(state): State<AppState>,
    ImportAccess(auth): ImportAccess,
    Query(query): Query<ListImportsQuery>,
) -> Result<Json<Page<ImportRun>>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let rows = import_rows_page(&mut conn, resolve_import_account(&auth), query).await?;
    import_runs_page(&mut conn, rows).await.map(Json)
}

/// A page of import rows as a page of Import Runs, each with its issues and
/// contact tally.
pub(crate) async fn import_runs_page(
    conn: &mut SqliteConnection,
    rows: Page<crate::db::imports::ImportRow>,
) -> Result<Page<ImportRun>, ApiError> {
    let mut items = Vec::with_capacity(rows.items.len());
    for row in rows.items {
        items.push(import_run(conn, row).await?);
    }
    Ok(Page {
        items,
        total: rows.total,
        limit: rows.limit,
        offset: rows.offset,
    })
}

/// One account's Import Runs as a page of rows. `GET /v1/imports` answers
/// from it for the credential's account and `GET /v1/accounts/{id}/imports`
/// for the account named, so the two lists cannot drift; each shapes the
/// rows for whoever reads them.
pub(crate) async fn import_rows_page(
    conn: &mut SqliteConnection,
    account: i64,
    query: ListImportsQuery,
) -> Result<Page<crate::db::imports::ImportRow>, ApiError> {
    let page = page_params(
        query.limit,
        query.offset,
        DEFAULT_LIST_LIMIT,
        Some(MAX_LIST_OFFSET),
    )?;
    let order = parse_sort(
        query.sort.as_deref(),
        &crate::db::imports::IMPORT_SORT_KEYS,
        &crate::db::imports::DEFAULT_IMPORT_SORT,
    )?;
    let status = query
        .status
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(status) = status
        && crate::db::imports::ImportStatus::parse(status).is_none()
    {
        return Err(ApiError::validation(format!(
            "status: unknown value '{status}'; accepted values are {}",
            crate::db::imports::ImportStatus::ALL
                .map(crate::db::imports::ImportStatus::as_str)
                .join(", ")
        )));
    }

    let (items, total) = crate::db::imports::list_imports_page(
        conn,
        account,
        status,
        &order,
        page.limit as i64,
        i64::try_from(page.offset).map_err(anyhow::Error::from)?,
    )
    .await?;
    Ok(Page {
        items,
        total,
        limit: page.limit,
        offset: page.offset,
    })
}

/// Status, timings, and issues for one Import Run.
#[utoipa::path(
    get,
    path = "/v1/imports/{id}",
    tag = "Import",
    security(("session" = ["import"]), ("api-token" = ["import"])),
    params(("id" = i64, Path, description = "Import Run id")),
    responses(
        (status = 200, body = ImportRun),
    )
)]
pub(crate) async fn get_import(
    State(state): State<AppState>,
    ImportAccess(auth): ImportAccess,
    AxumPath(import_id): AxumPath<i64>,
) -> Result<Json<ImportRun>, ApiError> {
    let mut conn = state.db.acquire().await?;
    full_import_run(&mut conn, auth.account_id, import_id)
        .await
        .map(Json)
}

/// One of an account's Import Runs in full. A run that is another account's
/// is a 404.
pub(crate) async fn full_import_run(
    conn: &mut SqliteConnection,
    account: i64,
    import_id: i64,
) -> Result<ImportRun, ApiError> {
    let row = crate::db::imports::get_owned_import(conn, account, import_id)
        .await
        .map_err(ApiError::from)?;
    import_run(conn, row).await
}

/// Start an Import Run and return its id. Finish the run at
/// POST /v1/imports/{id}/complete.
#[utoipa::path(
    post,
    path = "/v1/imports",
    tag = "Import",
    security(("session" = ["import"]), ("api-token" = ["import"])),
    request_body = CreateImportRequest,
    responses(
        (
            status = 201,
            body = CreateImportResponse,
            headers(("Location" = String, description = "Path of the new import"))
        ),
        crate::problem::openapi::StateConflict
    )
)]
pub(crate) async fn create_import(
    State(state): State<AppState>,
    ImportAccess(auth): ImportAccess,
    Json(body): Json<CreateImportRequest>,
) -> Result<Created<CreateImportResponse>, ApiError> {
    if body.source.trim().is_empty() {
        // Blank answers as missing does: one validation failure.
        return Err(ApiError::validation("source is required"));
    }
    validate_source_id(&body.source).map_err(|e| ApiError::validation(e.to_string()))?;
    let account = resolve_import_account(&auth);
    let stage = match body.stage.as_deref() {
        None => crate::db::imports::ImportStage::Parse,
        Some(raw) => crate::db::imports::ImportStage::parse(raw).ok_or_else(|| {
            ApiError::validation(format!(
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
    crate::db::account_profile::ensure_account_row(&mut conn, account).await?;
    let args = crate::db::imports::StartImportArgs {
        account_id: account,
        source: &body.source,
        mode: body.mode.as_str(),
        dedupe: body.dedupe,
        tool: body.tool.as_deref(),
        stage,
        staging_dir: body.staging_dir.as_deref(),
        device_id: body.device_id.as_deref(),
        form_json: form_json.as_deref(),
        source_fingerprint: fingerprint_json.as_deref(),
        source_identities: identities_json.as_deref(),
    };
    // The run and what started it land together, so a run never reads as
    // one the server started.
    let mut tx = crate::db::begin_write(&mut conn).await?;
    let id = crate::db::imports::start_import(&mut tx, &args).await?;
    crate::db::imports::record_credential(&mut tx, id, &auth.credential).await?;
    tx.commit().await?;

    Ok(Created {
        location: format!("/v1/imports/{id}"),
        body: CreateImportResponse { id },
    })
}

/// Record the outcome of an Import Run started with POST /v1/imports. The
/// answer is the run as it now stands.
#[utoipa::path(
    post,
    path = "/v1/imports/{id}/complete",
    tag = "Import",
    security(("session" = ["import"]), ("api-token" = ["import"])),
    params(("id" = i64, Path, description = "Import Run id")),
    request_body = CompleteImportRequest,
    responses(
        (status = 200, body = ImportRun),
        crate::problem::openapi::StateConflict
    )
)]
pub(crate) async fn complete_import(
    State(state): State<AppState>,
    ImportAccess(auth): ImportAccess,
    AxumPath(import_id): AxumPath<i64>,
    Json(body): Json<CompleteImportRequest>,
) -> Result<Json<ImportRun>, ApiError> {
    let account = resolve_import_account(&auth);
    validate_complete_import_issues(&body.issues)?;
    validate_import_status(&body.status)?;
    let summary_json =
        match body.summary {
            Some(summary) => Some(serde_json::to_string(&summary).map_err(|e| {
                ApiError::Internal(anyhow::anyhow!("serialize import summary: {e}"))
            })?),
            None => None,
        };
    let args = crate::db::imports::CompleteImportArgs {
        status: body.status,
        message_count: None,
        attachment_count: None,
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
            .map(|issue| crate::db::imports::ImportIssueInput {
                kind: issue.kind,
                step: issue.step,
                item: issue.item,
                reason: issue.reason,
            })
            .collect(),
    };
    let mut conn = state.db.acquire().await?;
    let row = complete_run(&mut conn, account, import_id, &args)
        .await
        .map_err(
            |e| match e.downcast::<crate::db::imports::ImportLookupError>() {
                Ok(lookup) => ApiError::from(lookup),
                Err(other) => ApiError::Internal(other),
            },
        )?;
    let run = import_run(&mut conn, row).await;
    drop(conn);
    crate::asset_store::sweep_after_run(&state.db, &state.cfg.paths, account).await;
    run.map(Json)
}

/// Add the sidebar shortcut to the messages this run brought in.
///
/// Only runs that stored something get one: a saved search matching nothing is
/// worse than no saved search, and a run that stored nothing is still visible
/// in Import History either way.
///
/// The shortcut is a convenience, not a record, so a failure here is logged and
/// the import still reports success. The person may delete the saved search
/// afterwards; the `imports` row it points at is permanent.
async fn create_import_saved_search(
    conn: &mut sqlx::SqliteConnection,
    account_id: i64,
    row: &crate::db::imports::ImportRow,
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
        tracing::warn!(
            import_id = row.id,
            messages = row.message_count,
            error = ?e,
            "the import's saved search could not be created"
        );
    }
}

/// List the contacts one import run created or changed, with the reason for
/// each, most consequential first.
///
/// The run recorded each reason as it staged (`db::import_contacts`), so the
/// list is what the import decided, not what timestamps suggest. How many of
/// each the run made is on the run's own record, `GET /v1/imports/{id}`: a
/// page can only count its own rows, and the panel states the whole run's
/// tally.
#[utoipa::path(
    get,
    path = "/v1/imports/{id}/contacts",
    tag = "Import",
    security(("session" = ["import"]), ("api-token" = ["import"])),
    params(
        ("id" = i64, Path, description = "Import Run id"),
        ("limit" = Option<usize>, Query, description = "Page size, default 40, max 500"),
        ("offset" = Option<usize>, Query, description = "Page offset")
    ),
    responses(
        (status = 200, body = Page<crate::db::import_contacts::ImportContact>),
    )
)]
pub(crate) async fn list_import_contacts(
    State(state): State<AppState>,
    ImportAccess(auth): ImportAccess,
    AxumPath(import_id): AxumPath<i64>,
    Query(query): Query<PageQuery>,
) -> Result<Json<Page<crate::db::import_contacts::ImportContact>>, ApiError> {
    let params = page_params(query.limit, query.offset, DEFAULT_LIST_LIMIT, None)?;
    let mut conn = state.db.acquire().await?;
    crate::db::imports::get_owned_import(&mut conn, auth.account_id, import_id)
        .await
        .map_err(ApiError::from)?;
    let (items, total) =
        crate::db::import_contacts::page(&mut conn, import_id, params.limit, params.offset)
            .await
            .map_err(ApiError::Internal)?;
    Ok(Json(Page {
        items,
        total,
        limit: params.limit,
        offset: params.offset,
    }))
}

/// Create the Contact Group naming the contacts an import run touched.
///
/// The group is a shortcut pointing at the run: the person may delete it, and
/// the `imports` row it describes is permanent either way. Membership is
/// a snapshot, because what a run brought in is a historical fact that should
/// not silently rewrite itself as contacts change later.
///
/// A failure here is reported and swallowed, the same as the saved search: the
/// messages are already in the database, and losing a shortcut is not a reason to
/// call the import failed.
async fn create_import_contact_group(
    conn: &mut sqlx::SqliteConnection,
    account_id: i64,
    row: &crate::db::imports::ImportRow,
) {
    let touched = match crate::db::import_contacts::contact_ids(conn, row.id).await {
        Ok(ids) if ids.is_empty() => return,
        Ok(ids) => ids,
        Err(e) => {
            tracing::warn!(
                import_id = row.id,
                error = %crate::server::error_chain(&e),
                "the import could not list the contacts it touched"
            );
            return;
        }
    };
    let group_id = match crate::db::contacts::create_import_group(
        conn,
        account_id,
        &import_contact_group_name(row),
    )
    .await
    {
        Ok((id, _)) => id,
        Err(e) => {
            tracing::warn!(
                import_id = row.id,
                error = %crate::server::error_chain(&e),
                "the import's Contact Group could not be created"
            );
            return;
        }
    };
    if let Err(e) = crate::db::named_membership::patch_members(
        crate::db::named_membership::group_spec(),
        conn,
        account_id,
        group_id,
        &touched,
        &[],
    )
    .await
    {
        tracing::warn!(
            import_id = row.id,
            contacts = touched.len(),
            error = ?e,
            "the import's Contact Group was created but its contacts could not be added"
        );
    }
}

/// Name an Import Run's Contact Group starts from: the source and the day
/// the run finished. `contact_groups.name` is unique per account, and each
/// run gets a group of its own, so `create_import_group` adds " 2", " 3", …
/// when the name is taken.
fn import_contact_group_name(row: &crate::db::imports::ImportRow) -> String {
    format!("{} import {}", row.source, import_date_ymd(row))
}

/// [`import_contact_group_name`] as an SQL expression over an `imports` row
/// aliased `i`, for a test query that finds a run's Contact Group. It falls
/// back from `finished_at` to `started_at` as [`import_date_ymd`] does, and
/// leaves out the fall back to today, which only a timestamp shorter than a
/// date takes, and the " 2" a taken name gets.
#[cfg(test)]
pub(crate) const IMPORT_CONTACT_GROUP_NAME_SQL: &str =
    "i.source || ' import ' || substr(coalesce(i.finished_at, i.started_at), 1, 10)";

/// Calendar date to name an import's saved search after: the day the run
/// finished, falling back to the day it started, then to today. All three are
/// UTC, because that is what `imports` stores.
fn import_date_ymd(row: &crate::db::imports::ImportRow) -> String {
    row.finished_at
        .as_deref()
        .or(Some(row.started_at.as_str()))
        .and_then(|ts| ts.get(..10))
        .filter(|d| d.len() == 10)
        .map_or_else(
            || chrono::Utc::now().format("%Y-%m-%d").to_string(),
            str::to_string,
        )
}

/// One Import Run in full, read from its row, its issues and its contact
/// tally. The caller has already established that the row is the account's.
pub(crate) async fn import_run(
    conn: &mut SqliteConnection,
    row: crate::db::imports::ImportRow,
) -> Result<ImportRun, ApiError> {
    let issues = crate::db::imports::list_import_issues(conn, row.id)
        .await
        .map_err(ApiError::Internal)?;
    let contacts = crate::db::import_contacts::counts(conn, row.id)
        .await
        .map_err(ApiError::Internal)?;
    let issues = issues
        .into_iter()
        .map(|issue| ImportIssue {
            kind: issue.kind,
            step: issue.step,
            item: issue.item,
            reason: issue.reason,
        })
        .collect();

    Ok(ImportRun {
        id: row.id,
        source: row.source,
        tool: row.tool,
        mode: row.mode,
        dedupe: row.dedupe,
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
        stage: row.stage,
        staging_dir: row.staging_dir,
        device_id: row.device_id,
        form: crate::db::imports::json_column(row.form_json),
        source_fingerprint: crate::db::imports::json_column(row.source_fingerprint),
        source_identities: crate::db::imports::json_column(row.source_identities),
        summary: crate::db::imports::json_column(row.summary_json),
        issues,
        contacts_new: contacts.new_count,
        contacts_changed: contacts.changed_count,
    })
}

/// New stage for a running Import Run.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct UpdateImportRequest {
    pub(crate) stage: String,
    /// What the user approved at the gate they just passed, when they passed one.
    ///
    /// Recorded here rather than at completion so an approval survives a
    /// reload: the summary shown at a gate is recomputed from the folder, but
    /// what was approved is a different question and only the run
    /// remembers it. Absent leaves the stored `summary_json` untouched —
    /// most stage changes carry nothing, and treating absent as null would
    /// throw away the plan the outcome is later judged against.
    #[serde(default)]
    pub(crate) summary: Option<serde_json::Value>,
}

/// Move a running Import Run to another stage.
///
/// The stage is a field of the run, so moving it is a `PATCH` of the run
/// rather than a `POST` to a `stage` sub-resource: a path segment names a
/// resource, and `stage` is not one (`docs/architecture/http-api.md`,
/// "Naming a route"). The answer is the run itself, the same shape
/// `GET /v1/imports/{id}` returns, so a caller reads one record wherever it
/// asks.
#[utoipa::path(
    patch,
    path = "/v1/imports/{id}",
    tag = "Import",
    security(("session" = ["import"]), ("api-token" = ["import"])),
    params(("id" = i64, Path, description = "Import Run id")),
    request_body = UpdateImportRequest,
    responses(
        (status = 200, body = ImportRun),
        crate::problem::openapi::StateConflict
    )
)]
pub(crate) async fn update_import(
    State(state): State<AppState>,
    ImportAccess(auth): ImportAccess,
    AxumPath(import_id): AxumPath<i64>,
    Json(body): Json<UpdateImportRequest>,
) -> Result<Json<ImportRun>, ApiError> {
    let account = resolve_import_account(&auth);
    let stage = crate::db::imports::ImportStage::parse(&body.stage).ok_or_else(|| {
        ApiError::validation(format!(
            "invalid import stage '{}'; expected one of parse, write, awaiting_gate_1, transcode, awaiting_gate_2, pushing",
            body.stage
        ))
    })?;
    let summary_json = optional_json_string(body.summary.as_ref(), "summary")?;
    let mut conn = state.db.acquire().await?;
    crate::db::imports::set_import_stage(
        &mut conn,
        account,
        import_id,
        stage,
        summary_json.as_deref(),
    )
    .await?;
    full_import_run(&mut conn, account, import_id)
        .await
        .map(Json)
}

/// Discard a running Import Run, freeing the account's single slot. The
/// answer is the run, now `cancelled`.
#[utoipa::path(
    post,
    path = "/v1/imports/{id}/discard",
    tag = "Import",
    security(("session" = ["import"]), ("api-token" = ["import"])),
    params(("id" = i64, Path, description = "Import Run id")),
    responses(
        (status = 200, body = ImportRun),
        crate::problem::openapi::StateConflict
    )
)]
pub(crate) async fn discard_import(
    State(state): State<AppState>,
    ImportAccess(auth): ImportAccess,
    AxumPath(import_id): AxumPath<i64>,
) -> Result<Json<ImportRun>, ApiError> {
    let account = resolve_import_account(&auth);
    let mut conn = state.db.acquire().await?;
    crate::db::imports::discard_import(&mut conn, account, import_id).await?;
    let run = full_import_run(&mut conn, account, import_id).await;
    drop(conn);
    crate::asset_store::sweep_after_run(&state.db, &state.cfg.paths, account).await;
    run.map(Json)
}

/// Import one message-ir JSONL body.
///
/// Every message needs a non-empty `guid`. A batch with a message without
/// one is refused with `422`, naming its lines, and nothing in it is stored.
/// A message whose `guid` the source already holds is skipped, so a batch
/// sent again after its answer was lost stores nothing twice.
#[utoipa::path(
    post,
    path = "/v1/imports/{id}/batches",
    params(("id" = i64, Path, description = "Import Run id")),
    tag = "Import",
    security(("session" = ["import"]), ("api-token" = ["import"])),
    request_body(
        content(
            ("application/x-ndjson"),
            ("application/jsonl")
        ),
        description = "message-ir JSONL as application/x-ndjson or application/jsonl. Attachments are uploaded first by SHA-256 through /v1/assets."
    ),
    responses(
        (status = 200, body = CreateImportBatchResponse),
        crate::problem::openapi::StateConflict,
    )
)]
pub(crate) async fn create_import_batch(
    State(state): State<AppState>,
    ImportAccess(auth): ImportAccess,
    AxumPath(import_id): AxumPath<i64>,
    headers: HeaderMap,
    request: Request,
) -> Result<Json<CreateImportBatchResponse>, ApiError> {
    let Some(ct) = content_type_base(&headers) else {
        return Err(ApiError::UnsupportedMediaType(
            "Content-Type required (application/x-ndjson or application/jsonl)".into(),
        ));
    };

    let account = resolve_import_account(&auth);
    // The run's row says what the batch imports under; a run that is not
    // running, or belongs to another account, refuses the batch here before
    // any of the body is read.
    let context = {
        let mut conn = state.db.acquire().await?;
        let row = crate::db::imports::require_running_import(&mut conn, account, import_id).await?;
        BatchContext::from_row(&row)
    };

    if is_jsonl_content_type(ct) {
        let temp = tempfile::tempdir()
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("temp dir: {e}")))?;
        let jsonl_path = temp.path().join("_import.jsonl");
        let n = stream_body_to_file(
            request.into_body(),
            &jsonl_path,
            crate::server::MAX_REQUEST_BODY_BYTES,
        )
        .await?;
        if n == 0 {
            return Err(ImportFailure::Empty.into());
        }
        // The import pipeline does blocking file IO (JSONL parse, asset
        // hashing and copies) — run it off the async workers so a large
        // import cannot stall unrelated requests.
        let handle = tokio::runtime::Handle::current();
        let response = tokio::task::spawn_blocking(move || {
            handle.block_on(run_import_path(state, context, jsonl_path))
        })
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("import task failed: {e}")))?;
        drop(temp);
        return response;
    }

    // Only JSON Lines is an import body. Attachments never travel with it:
    // they are uploaded first, by SHA-256, through `/v1/assets`. A body in
    // another type is the wrong media type, not a malformed request.
    Err(ApiError::UnsupportedMediaType(
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

/// `create_import_batch` is the only entry point, and the run's `source`,
/// validated when the run was created, names an on-disk directory.
async fn run_import_path(
    state: AppState,
    context: BatchContext,
    jsonl_path: PathBuf,
) -> Result<Json<CreateImportBatchResponse>, ApiError> {
    // An import holds one pooled connection for its whole run (JSONL parse,
    // asset IO, promote). Bound concurrent imports here so they can never
    // drain the pool; the semaphore is taken before the per-account lock so
    // lock order (semaphore → account → pool) is consistent everywhere.
    let _import_permit = import_semaphore()
        .acquire()
        .await
        .map_err(|_| ApiError::Internal(anyhow::anyhow!("server is shutting down")))?;
    let cfg = Arc::clone(&state.cfg);
    let BatchContext {
        account,
        import_id,
        source: source_id,
        mode: run_mode,
        dedupe: do_dedupe,
    } = context;

    let _guard = state.account_import_locks.lock(account.to_string()).await;

    // One pooled connection held for the whole import; the import semaphore
    // taken above keeps enough of the pool free for other requests.
    let mut conn = state.db.acquire().await?;

    // A `replace` run wipes the source once, on its first batch; every batch
    // after that appends. The row's stamped messages say which this is.
    let mode = if run_mode == ImportMode::Replace
        && !crate::db::imports::has_messages(&mut conn, import_id).await?
    {
        ImportMode::Replace
    } else {
        ImportMode::Append
    };

    // Attachment paths resolve only through assets already uploaded by
    // SHA-256; the import body never carries files of its own.
    let assets_dir = cfg.paths.assets_dir_for_account(account, &source_id);

    let opts = ImportOptions::fixed(FixedImportArgs {
        assets_dir: &assets_dir,
        asset_root: &assets_dir,
        mode,
        source: &source_id,
        account_id: account,
        fill_content_keys: do_dedupe,
        import_id: Some(import_id),
    });
    let import_result = imports_api::import_jsonl_files_on_conn(
        &mut conn,
        &[jsonl_path],
        &opts,
        imports_api::ImportSchemaMode::AssumeReady,
    )
    .await;
    let stats = import_result?;
    let dedupe_stats = if do_dedupe {
        Some(dedupe::dedupe_cross_source(&mut conn, account, None, 2).await?)
    } else {
        None
    };

    Ok(Json(CreateImportBatchResponse {
        source: source_id,
        account,
        stats,
        dedupe: dedupe_stats.map(|d| DedupeCounts {
            keys_filled: d.keys_filled,
            exact_groups: d.exact_groups,
            exact_flagged: d.exact_flagged,
            near_flagged: d.near_flagged,
        }),
    }))
}

#[cfg(test)]
mod tests;
