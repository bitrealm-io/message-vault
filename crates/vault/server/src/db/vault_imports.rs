//! Per-account vault import session records (one row per vault-push / CLI import run).

use std::error::Error;
use std::fmt;

use anyhow::{Result, bail};
use chrono::Utc;
use serde::Serialize;
use sqlx::any::AnyRow;
use sqlx::{AnyConnection, Connection, Row};

use crate::db::dialect;

/// Where a live import session is in its lifecycle.
///
/// `status` records how a run ended; this records where it is. Both are
/// needed: a session can sit at `Write` while running, and at `Write`
/// having failed.
///
/// All six stages exist because they are the design's vocabulary, but the
/// gates and the media pass are not built yet — only `Parse`, `Write`, and
/// `Pushing` are reachable today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportStage {
    /// Reading the backup. Nothing durable exists yet.
    Parse,
    /// Writing conversation files and staging attachments.
    Write,
    /// Waiting for the user to approve spending time on the media step.
    AwaitingGate1,
    /// Converting or compressing staged media.
    Transcode,
    /// Waiting for the user to approve what lands in the vault.
    AwaitingGate2,
    /// Uploading to the vault.
    Pushing,
}

impl ImportStage {
    /// Stored spelling of this stage.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Parse => "parse",
            Self::Write => "write",
            Self::AwaitingGate1 => "awaiting_gate_1",
            Self::Transcode => "transcode",
            Self::AwaitingGate2 => "awaiting_gate_2",
            Self::Pushing => "pushing",
        }
    }

    /// Parse a stored spelling, or `None` when it is not one of the six.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "parse" => Some(Self::Parse),
            "write" => Some(Self::Write),
            "awaiting_gate_1" => Some(Self::AwaitingGate1),
            "transcode" => Some(Self::Transcode),
            "awaiting_gate_2" => Some(Self::AwaitingGate2),
            "pushing" => Some(Self::Pushing),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
/// One row of `vault_imports`: a per-account import session record.
pub struct VaultImportRow {
    /// Import session id.
    pub id: i64,
    /// Vault account that owns the session.
    pub account_id: String,
    /// Source id the session imports.
    pub source: String,
    /// Importing tool, e.g. `vault-push`.
    pub tool: Option<String>,
    /// Import mode (`replace` or `append`).
    pub mode: String,
    /// Lifecycle status (`running`, `completed`, `completed_with_issues`, or `failed`).
    pub status: String,
    /// UTC time the session started.
    pub started_at: String,
    /// UTC time the session finished, when it has.
    pub finished_at: Option<String>,
    /// Messages counted for the session.
    pub message_count: i64,
    /// Attachments counted for the session.
    pub attachment_count: i64,
    /// Bytes uploaded so far.
    pub bytes_uploaded: i64,
    /// Total wall-clock duration, when finished.
    pub duration_ms: Option<i64>,
    /// Time spent parsing JSONL, when finished.
    pub parse_ms: Option<i64>,
    /// Time spent copying, converting, or skipping attachments, when finished.
    pub attachments_ms: Option<i64>,
    /// Time spent preparing conversation files, when finished.
    pub prepare_ms: Option<i64>,
    /// Time spent uploading assets, when finished.
    pub upload_ms: Option<i64>,
    /// Client-provided summary payload.
    pub summary_json: Option<String>,
    /// Lifecycle stage while the session is live; `None` once it is over.
    pub stage: Option<String>,
    /// Absolute path to the staging folder on the client that owns it.
    pub staging_dir: Option<String>,
    /// Which install created the session.
    pub device_id: Option<String>,
    /// Import form snapshot, for restoring the screen.
    pub form_json: Option<String>,
    /// Source path, size, mtime, and message count.
    pub source_fingerprint: Option<String>,
    /// Addresses the backup's device sent from (JSON array).
    pub source_identities: Option<String>,
}

/// Outcome fields written when a session completes.
#[derive(Debug, Clone, Default)]
pub struct CompleteImportArgs {
    /// True when the import finished successfully.
    pub ok: bool,
    /// Explicit outcome status; falls back to `ok` when `None`.
    pub status: Option<String>,
    /// Messages imported; counted from the database when omitted.
    pub message_count: Option<i64>,
    /// Attachments imported; counted from the database when omitted.
    pub attachment_count: Option<i64>,
    /// Bytes uploaded.
    pub bytes_uploaded: Option<i64>,
    /// Total wall-clock duration.
    pub duration_ms: Option<i64>,
    /// Time spent parsing JSONL.
    pub parse_ms: Option<i64>,
    /// Time spent copying, converting, or skipping attachments.
    pub attachments_ms: Option<i64>,
    /// Time spent preparing conversation files.
    pub prepare_ms: Option<i64>,
    /// Time spent uploading assets.
    pub upload_ms: Option<i64>,
    /// Client-provided summary payload.
    pub summary_json: Option<String>,
    /// Per-file issues to record against the session.
    pub issues: Vec<ImportIssueInput>,
}

impl CompleteImportArgs {
    /// Build a success outcome from message and attachment counts.
    pub fn succeeded(messages: u64, attachments: u64) -> Self {
        Self {
            ok: true,
            message_count: Some(messages as i64),
            attachment_count: Some(attachments as i64),
            ..Default::default()
        }
    }

    /// Build a failure outcome; nothing else is recorded.
    pub fn failed() -> Self {
        Self {
            ok: false,
            ..Default::default()
        }
    }
}

/// Finish an import session, logging a warning if the row update fails.
pub async fn complete_import_or_warn(
    conn: &mut AnyConnection,
    account_id: &str,
    import_id: i64,
    args: &CompleteImportArgs,
) {
    if let Err(e) = complete_import(conn, account_id, import_id, args).await {
        eprintln!("warning: complete_import({import_id}) failed: {e}");
    }
}

/// One problem to record against an import session.
#[derive(Debug, Clone)]
pub struct ImportIssueInput {
    /// Issue category, e.g. `file_missing`.
    pub kind: String,
    /// Pipeline stage that reported it.
    pub step: String,
    /// The file or message the issue is about.
    pub item: String,
    /// Human-readable explanation.
    pub reason: String,
}

/// One stored `vault_import_issues` row.
#[derive(Debug, Clone, Serialize)]
pub struct ImportIssueRow {
    /// Issue row id.
    pub id: i64,
    /// Session the issue belongs to.
    pub import_id: i64,
    /// Issue category, e.g. `file_missing`.
    pub kind: String,
    /// Pipeline stage that reported it.
    pub step: String,
    /// The file or message the issue is about.
    pub item: String,
    /// Human-readable explanation.
    pub reason: String,
    /// UTC time the issue was recorded.
    pub created_at: String,
}

/// An import session row plus its recorded issues.
#[derive(Debug, Clone, Serialize)]
pub struct ImportDetail {
    /// The session.
    pub row: VaultImportRow,
    /// Issues recorded for it.
    pub issues: Vec<ImportIssueRow>,
}

/// Failure looking up or reusing an import session.
#[derive(Debug)]
pub enum ImportLookupError {
    /// No session with this id for this account.
    NotFound {
        /// The session id that was looked up.
        import_id: i64,
    },
    /// Session exists but cannot be reused (wrong status/source/mode).
    InvalidSession {
        /// Why the session cannot be reused.
        message: String,
    },
    /// Database failure.
    Db(anyhow::Error),
}

impl fmt::Display for ImportLookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { import_id } => {
                write!(f, "import {import_id} not found for this account")
            }
            Self::InvalidSession { message } => f.write_str(message),
            Self::Db(err) => err.fmt(f),
        }
    }
}

impl Error for ImportLookupError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::NotFound { .. } | Self::InvalidSession { .. } => None,
            Self::Db(err) => err.source(),
        }
    }
}

impl From<sqlx::Error> for ImportLookupError {
    fn from(value: sqlx::Error) -> Self {
        Self::Db(value.into())
    }
}

impl From<anyhow::Error> for ImportLookupError {
    fn from(value: anyhow::Error) -> Self {
        Self::Db(value)
    }
}

/// Everything recorded when a session begins.
pub struct StartImportArgs<'a> {
    /// Owning vault account.
    pub account_id: &'a str,
    /// IR source family (`imessage`, `whatsapp`, …), not a method id.
    pub source: &'a str,
    /// Import mode recorded by the importer.
    pub mode: &'a str,
    /// Client/tool name, when the caller names one.
    pub tool: Option<&'a str>,
    /// Stage the session opens at.
    pub stage: ImportStage,
    /// Absolute staging path on the client.
    pub staging_dir: Option<&'a str>,
    /// Which install is creating this session.
    pub device_id: Option<&'a str>,
    /// Import form snapshot as JSON.
    pub form_json: Option<&'a str>,
    /// Source fingerprint as JSON.
    pub source_fingerprint: Option<&'a str>,
    /// Backup device identity list as JSON.
    pub source_identities: Option<&'a str>,
}

impl<'a> StartImportArgs<'a> {
    /// A session opening at [`ImportStage::Parse`] with nothing recorded
    /// about the client: no staging folder, device, form snapshot,
    /// fingerprint, or identities. The CLI importer and most tests start
    /// here; a caller with more to record uses struct update syntax on
    /// top of it.
    pub fn new(account_id: &'a str, source: &'a str, mode: &'a str, tool: Option<&'a str>) -> Self {
        Self {
            account_id,
            source,
            mode,
            tool,
            stage: ImportStage::Parse,
            staging_dir: None,
            device_id: None,
            form_json: None,
            source_fingerprint: None,
            source_identities: None,
        }
    }
}

/// Why a session could not be started.
#[derive(Debug)]
pub enum StartImportError {
    /// This account already has a live session. The partial unique index
    /// rejected the insert, so this holds even against a racing client.
    AlreadyActive,
    /// Anything else.
    Db(anyhow::Error),
}

impl std::fmt::Display for StartImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Naming the way out matters: a killed CLI import leaves a
            // session open that blocks every later one, and the desktop
            // app's Import screen is where it can be resumed or discarded.
            Self::AlreadyActive => write!(
                f,
                "this account already has an active import session; open Import in the desktop app to resume or discard it"
            ),
            Self::Db(e) => write!(f, "{e}"),
        }
    }
}

impl Error for StartImportError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::AlreadyActive => None,
            Self::Db(err) => err.source(),
        }
    }
}

/// Open a new import session.
///
/// # Errors
///
/// [`StartImportError::AlreadyActive`] when a live session already exists
/// for this account; [`StartImportError::Db`] for any other failure.
pub async fn start_import(
    conn: &mut AnyConnection,
    args: &StartImportArgs<'_>,
) -> std::result::Result<i64, StartImportError> {
    let started_at = Utc::now().to_rfc3339();
    let inserted: std::result::Result<i64, sqlx::Error> = sqlx::query_scalar(
        r"
        INSERT INTO vault_imports (
            account_id, source, tool, mode, status, started_at,
            message_count, attachment_count, bytes_uploaded,
            stage, staging_dir, device_id, form_json, source_fingerprint,
            source_identities
        ) VALUES ($1, $2, $3, $4, 'running', $5, 0, 0, 0, $6, $7, $8, $9, $10, $11)
        RETURNING id
        ",
    )
    .bind(args.account_id)
    .bind(args.source)
    .bind(args.tool)
    .bind(args.mode)
    .bind(started_at)
    .bind(args.stage.as_str())
    .bind(args.staging_dir)
    .bind(args.device_id)
    .bind(args.form_json)
    .bind(args.source_fingerprint)
    .bind(args.source_identities)
    .fetch_one(&mut *conn)
    .await;

    match inserted {
        Ok(id) => Ok(id),
        Err(err) if is_unique_violation(&err) => Err(StartImportError::AlreadyActive),
        Err(err) => Err(StartImportError::Db(err.into())),
    }
}

/// Whether this error is a unique-constraint violation on either engine.
///
/// SQLite reports `2067` / `1555`; Postgres reports SQLSTATE `23505`.
fn is_unique_violation(err: &sqlx::Error) -> bool {
    let Some(db_err) = err.as_database_error() else {
        return false;
    };
    if db_err.code().as_deref() == Some("23505") {
        return true;
    }
    matches!(db_err.code().as_deref(), Some("2067" | "1555"))
}

/// Column list for `vault_imports`, in the order reads map to a row.
const VAULT_IMPORT_COLUMNS: &str = "id, account_id, source, tool, mode, status, started_at, \
     finished_at, message_count, attachment_count, bytes_uploaded, duration_ms, parse_ms, \
     attachments_ms, prepare_ms, upload_ms, summary_json, stage, staging_dir, device_id, \
     form_json, source_fingerprint, source_identities";

/// Map one `vault_imports` row by column position.
fn vault_import_from_row(row: &AnyRow) -> Result<VaultImportRow, sqlx::Error> {
    Ok(VaultImportRow {
        id: row.try_get(0)?,
        account_id: row.try_get(1)?,
        source: row.try_get(2)?,
        tool: row.try_get(3)?,
        mode: row.try_get(4)?,
        status: row.try_get(5)?,
        started_at: row.try_get(6)?,
        finished_at: row.try_get(7)?,
        message_count: row.try_get(8)?,
        attachment_count: row.try_get(9)?,
        bytes_uploaded: row.try_get(10)?,
        duration_ms: row.try_get(11)?,
        parse_ms: row.try_get(12)?,
        attachments_ms: row.try_get(13)?,
        prepare_ms: row.try_get(14)?,
        upload_ms: row.try_get(15)?,
        summary_json: row.try_get(16)?,
        stage: row.try_get(17)?,
        staging_dir: row.try_get(18)?,
        device_id: row.try_get(19)?,
        form_json: row.try_get(20)?,
        source_fingerprint: row.try_get(21)?,
        source_identities: row.try_get(22)?,
    })
}

/// Load an import row owned by `account_id`, or error.
pub async fn get_owned_import(
    conn: &mut AnyConnection,
    account_id: &str,
    import_id: i64,
) -> std::result::Result<VaultImportRow, ImportLookupError> {
    let row = sqlx::query(&format!(
        "SELECT {VAULT_IMPORT_COLUMNS}
         FROM vault_imports
         WHERE id = $1 AND account_id = $2"
    ))
    .bind(import_id)
    .bind(account_id)
    .fetch_optional(&mut *conn)
    .await?;
    match row {
        Some(data) => Ok(vault_import_from_row(&data)?),
        None => Err(ImportLookupError::NotFound { import_id }),
    }
}

/// The account's live session, if it has one.
///
/// "Live" is `status = 'running'` — the same predicate the partial unique
/// index uses, so this can never return two rows.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn get_active_import(
    conn: &mut AnyConnection,
    account_id: &str,
) -> Result<Option<VaultImportRow>> {
    let row = sqlx::query(&format!(
        "SELECT {VAULT_IMPORT_COLUMNS}
         FROM vault_imports
         WHERE account_id = $1 AND status = 'running'"
    ))
    .bind(account_id)
    .fetch_optional(&mut *conn)
    .await?;
    match row {
        Some(data) => Ok(Some(vault_import_from_row(&data)?)),
        None => Ok(None),
    }
}

/// The account's import when it is still running.
///
/// # Errors
///
/// [`ImportLookupError::NotFound`] when the account owns no such import,
/// [`ImportLookupError::InvalidSession`] when it is no longer running.
async fn require_running_import(
    conn: &mut AnyConnection,
    account_id: &str,
    import_id: i64,
) -> std::result::Result<VaultImportRow, ImportLookupError> {
    let existing = get_owned_import(&mut *conn, account_id, import_id).await?;
    if existing.status != "running" {
        return Err(ImportLookupError::InvalidSession {
            message: format!(
                "import {import_id} is not running (status={})",
                existing.status
            ),
        });
    }
    Ok(existing)
}

/// Move a live session to another stage, optionally recording what the user
/// approved at the gate they just passed.
///
/// `summary_json` is written to `vault_imports.summary_json` only when
/// `Some`; `None` leaves whatever is already stored there untouched. Most
/// stage changes carry no summary, and treating absent as null would erase
/// the plan decision 15 later diffs the outcome against.
///
/// # Errors
///
/// [`ImportLookupError::NotFound`] when the account owns no such import,
/// [`ImportLookupError::InvalidSession`] when it is no longer running.
pub async fn set_import_stage(
    conn: &mut AnyConnection,
    account_id: &str,
    import_id: i64,
    stage: ImportStage,
    summary_json: Option<&str>,
) -> std::result::Result<(), ImportLookupError> {
    require_running_import(conn, account_id, import_id).await?;
    match summary_json {
        Some(summary) => {
            sqlx::query(
                "UPDATE vault_imports SET stage = $1, summary_json = $2
                 WHERE id = $3 AND account_id = $4",
            )
            .bind(stage.as_str())
            .bind(summary)
            .bind(import_id)
            .bind(account_id)
            .execute(&mut *conn)
            .await?;
        }
        None => {
            sqlx::query("UPDATE vault_imports SET stage = $1 WHERE id = $2 AND account_id = $3")
                .bind(stage.as_str())
                .bind(import_id)
                .bind(account_id)
                .execute(&mut *conn)
                .await?;
        }
    }
    Ok(())
}

/// Close a live session the user gave up on.
///
/// Records `cancelled` and clears `stage`, which frees the account's
/// single active slot. Nothing reclaims a session on a timer — a session
/// is broken by an explicit discard or not at all.
///
/// # Errors
///
/// [`ImportLookupError::NotFound`] when the account owns no such import,
/// [`ImportLookupError::InvalidSession`] when it is no longer running.
pub async fn discard_import(
    conn: &mut AnyConnection,
    account_id: &str,
    import_id: i64,
) -> std::result::Result<(), ImportLookupError> {
    require_running_import(conn, account_id, import_id).await?;
    sqlx::query(
        "UPDATE vault_imports
         SET status = 'cancelled', stage = NULL, finished_at = $1
         WHERE id = $2 AND account_id = $3",
    )
    .bind(Utc::now().to_rfc3339())
    .bind(import_id)
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Like [`get_owned_import`], but the session must still be `running` and match
/// the source/mode the client is about to import with.
pub async fn require_reusable_import(
    conn: &mut AnyConnection,
    account_id: &str,
    import_id: i64,
    source: &str,
    mode: &str,
) -> std::result::Result<VaultImportRow, ImportLookupError> {
    let row = get_owned_import(conn, account_id, import_id).await?;
    if row.status != "running" {
        return Err(ImportLookupError::InvalidSession {
            message: format!("import {import_id} is not running (status={})", row.status),
        });
    }
    if row.source != source {
        return Err(ImportLookupError::InvalidSession {
            message: format!(
                "import {import_id} source mismatch (session={}, request={})",
                row.source, source
            ),
        });
    }
    if row.mode != mode {
        return Err(ImportLookupError::InvalidSession {
            message: format!(
                "import {import_id} mode mismatch (session={}, request={})",
                row.mode, mode
            ),
        });
    }
    Ok(row)
}

/// Finish an import: prefer client counts, else derive from linked messages.
pub async fn complete_import(
    conn: &mut AnyConnection,
    account_id: &str,
    import_id: i64,
    args: &CompleteImportArgs,
) -> Result<VaultImportRow> {
    let existing = get_owned_import(&mut *conn, account_id, import_id).await?;
    let finished_at = Utc::now().to_rfc3339();
    let status = args
        .status
        .as_deref()
        .unwrap_or(if args.ok { "completed" } else { "failed" });

    for issue in &args.issues {
        validate_issue_kind(&issue.kind)?;
    }

    let message_count = if let Some(n) = args.message_count {
        n
    } else {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM messages WHERE import_id = $1 AND account_id = $2",
        )
        .bind(import_id)
        .bind(account_id)
        .fetch_one(&mut *conn)
        .await?;
        n
    };
    let attachment_count = if let Some(n) = args.attachment_count {
        n
    } else {
        let n: i64 = sqlx::query_scalar(
            r"
            SELECT COUNT(*) FROM attachments a
            JOIN messages m ON m.id = a.message_id
            WHERE m.import_id = $1 AND m.account_id = $2
            ",
        )
        .bind(import_id)
        .bind(account_id)
        .fetch_one(&mut *conn)
        .await?;
        n
    };
    let bytes_uploaded = args.bytes_uploaded.unwrap_or(existing.bytes_uploaded);

    // `BEGIN IMMEDIATE` on SQLite matches today's write lock; Postgres uses a
    // plain BEGIN (no statement-level equivalent). Either way the update and
    // the issue inserts land as one unit, and a failed commit rolls back
    // (sqlx drops the transaction).
    let mut tx = conn
        .begin_with(dialect::begin_immediate_sql(dialect::engine_of(conn)))
        .await?;
    sqlx::query(
        r"
        UPDATE vault_imports
        SET status = $1,
            finished_at = $2,
            message_count = $3,
            attachment_count = $4,
            bytes_uploaded = $5,
            duration_ms = $6,
            parse_ms = $7,
            attachments_ms = $8,
            prepare_ms = $9,
            upload_ms = $10,
            summary_json = $11,
            stage = NULL
        WHERE id = $12 AND account_id = $13
        ",
    )
    .bind(status)
    .bind(finished_at)
    .bind(message_count)
    .bind(attachment_count)
    .bind(bytes_uploaded)
    .bind(args.duration_ms)
    .bind(args.parse_ms)
    .bind(args.attachments_ms)
    .bind(args.prepare_ms)
    .bind(args.upload_ms)
    .bind(args.summary_json.as_deref())
    .bind(import_id)
    .bind(account_id)
    .execute(&mut *tx)
    .await?;
    insert_issues(&mut tx, import_id, &args.issues).await?;
    tx.commit().await?;

    Ok(get_owned_import(&mut *conn, account_id, import_id).await?)
}

/// Append issue rows for an import.
async fn insert_issues(
    conn: &mut AnyConnection,
    import_id: i64,
    issues: &[ImportIssueInput],
) -> Result<()> {
    for issue in issues {
        sqlx::query(
            r"
            INSERT INTO vault_import_issues (
                import_id, kind, step, item, reason, created_at
            ) VALUES ($1, $2, $3, $4, $5, $6)
            ",
        )
        .bind(import_id)
        .bind(&issue.kind)
        .bind(&issue.step)
        .bind(&issue.item)
        .bind(&issue.reason)
        .bind(Utc::now().to_rfc3339())
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Only `error` and `skip` are stored issue kinds.
fn validate_issue_kind(kind: &str) -> Result<()> {
    match kind {
        "error" | "skip" => Ok(()),
        other => bail!("invalid import issue kind '{other}'; expected 'error' or 'skip'"),
    }
}

/// Load one import row and its issue list.
pub async fn get_import_detail(
    conn: &mut AnyConnection,
    account_id: &str,
    import_id: i64,
) -> std::result::Result<ImportDetail, ImportLookupError> {
    let row = get_owned_import(conn, account_id, import_id).await?;
    let issue_rows: Vec<(i64, i64, String, String, String, String, String)> = sqlx::query_as(
        r"
        SELECT id, import_id, kind, step, item, reason, created_at
        FROM vault_import_issues
        WHERE import_id = $1
        ORDER BY id ASC
        ",
    )
    .bind(import_id)
    .fetch_all(&mut *conn)
    .await?;
    let issues = issue_rows
        .into_iter()
        .map(
            |(id, import_id, kind, step, item, reason, created_at)| ImportIssueRow {
                id,
                import_id,
                kind,
                step,
                item,
                reason,
                created_at,
            },
        )
        .collect();
    Ok(ImportDetail { row, issues })
}

/// Serializable slice of a session used in list responses.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ImportSummary {
    /// Import session id.
    pub id: i64,
    /// Source id the session imports.
    pub source: String,
    /// Importing tool, e.g. `vault-push`.
    pub tool: Option<String>,
    /// Import mode (`replace` or `append`).
    pub mode: String,
    /// Lifecycle status (`running`, `completed`, `completed_with_issues`, or `failed`).
    pub status: String,
    /// UTC time the session started.
    pub started_at: String,
    /// UTC time the session finished, when it has.
    pub finished_at: Option<String>,
    /// Messages counted for the session.
    pub message_count: i64,
    /// Attachments counted for the session.
    pub attachment_count: i64,
    /// Bytes uploaded so far.
    pub bytes_uploaded: i64,
    /// Total wall-clock duration, when finished.
    pub duration_ms: Option<i64>,
}

impl From<VaultImportRow> for ImportSummary {
    fn from(r: VaultImportRow) -> Self {
        ImportSummary {
            id: r.id,
            source: r.source,
            tool: r.tool,
            mode: r.mode,
            status: r.status,
            started_at: r.started_at,
            finished_at: r.finished_at,
            message_count: r.message_count,
            attachment_count: r.attachment_count,
            bytes_uploaded: r.bytes_uploaded,
            duration_ms: r.duration_ms,
        }
    }
}

/// List imports for an account, newest first. Returns serializable summaries.
pub async fn list_imports(
    conn: &mut AnyConnection,
    account_id: &str,
) -> Result<Vec<ImportSummary>> {
    list_imports_for_account(conn, account_id, 100)
        .await
        .map(|rows| rows.into_iter().map(Into::into).collect())
}

/// List imports for an account, newest first.
pub async fn list_imports_for_account(
    conn: &mut AnyConnection,
    account_id: &str,
    limit: i64,
) -> Result<Vec<VaultImportRow>> {
    let rows = sqlx::query(&format!(
        "SELECT {VAULT_IMPORT_COLUMNS}
         FROM vault_imports
         WHERE account_id = $1
         ORDER BY started_at DESC, id DESC
         LIMIT $2"
    ))
    .bind(account_id)
    .bind(limit)
    .fetch_all(&mut *conn)
    .await?;
    rows.iter()
        .map(vault_import_from_row)
        .collect::<Result<_, _>>()
        .map_err(Into::into)
}

const ACCOUNT_ATTACHMENTS_FROM: &str = r"
        FROM attachments a
        JOIN messages m ON m.id = a.message_id
        WHERE m.account_id = $1
        ";

/// Total attachment bytes for an account (original `size_bytes`).
pub async fn account_attachment_bytes(conn: &mut AnyConnection, account_id: &str) -> Result<i64> {
    let n: i64 = sqlx::query_scalar(&format!(
        "SELECT COALESCE(SUM(a.size_bytes), 0) {ACCOUNT_ATTACHMENTS_FROM}"
    ))
    .bind(account_id)
    .fetch_one(&mut *conn)
    .await?;
    Ok(n)
}

/// Attachment row count for an account.
pub async fn account_attachment_count(conn: &mut AnyConnection, account_id: &str) -> Result<i64> {
    let n: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) {ACCOUNT_ATTACHMENTS_FROM}"))
        .bind(account_id)
        .fetch_one(&mut *conn)
        .await?;
    Ok(n)
}

/// One of an account's largest attachments by byte size.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct TopAttachment {
    /// Attachment id.
    pub id: i64,
    /// File name from the export.
    pub original_name: Option<String>,
    /// MIME type, when known.
    pub mime_type: Option<String>,
    /// Attachment byte size.
    pub size_bytes: i64,
    /// Conversation that holds the attachment.
    pub conversation_id: i64,
    /// Conversation label, when set.
    pub conversation_title: Option<String>,
    /// Raw text of the conversation's chat handle (via `handles`).
    pub chat_identifier: String,
}

/// Raw row for [`top_attachments_by_size`] before mapping to [`TopAttachment`].
type TopAttachmentRow = (
    i64,
    Option<String>,
    Option<String>,
    i64,
    i64,
    Option<String>,
    String,
);

/// Largest attachments for an account.
pub async fn top_attachments_by_size(
    conn: &mut AnyConnection,
    account_id: &str,
    limit: i64,
) -> Result<Vec<TopAttachment>> {
    let rows: Vec<TopAttachmentRow> = sqlx::query_as(
        r"
            SELECT a.id,
                   a.original_name,
                   a.mime_type,
                   COALESCE(a.size_bytes, 0),
                   c.id,
                   c.group_title,
                   h.raw
            FROM attachments a
            JOIN messages m ON m.id = a.message_id
            JOIN conversations c ON c.id = m.conversation_id
            JOIN handles h ON h.id = c.chat_handle_id
            WHERE m.account_id = $1
              AND COALESCE(a.size_bytes, 0) > 0
            ORDER BY a.size_bytes DESC, a.id DESC
            LIMIT $2
            ",
    )
    .bind(account_id)
    .bind(limit)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                original_name,
                mime_type,
                size_bytes,
                conversation_id,
                conversation_title,
                chat_identifier,
            )| TopAttachment {
                id,
                original_name,
                mime_type,
                size_bytes,
                conversation_id,
                conversation_title,
                chat_identifier,
            },
        )
        .collect())
}

#[cfg(test)]
mod tests;
