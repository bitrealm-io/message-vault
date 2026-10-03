//! Per-account Import Run records (one row per Import Run).

use anyhow::{Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

use crate::db::begin_write;
use crate::paging::{Direction, SortKey};

/// Where a running Import Run is: a part of one of its Stages, or a Review
/// between them (`CONTEXT.md`).
///
/// `status` records how a run ended; this records where it is. Both are
/// needed: a run can sit at `Write` while running, and at `Write` having
/// failed. `Parse` and `Write` are the two parts of the Staging Stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportStage {
    /// Staging, reading the backup. Nothing durable exists yet.
    Parse,
    /// Staging, writing conversation files and staging attachments.
    Write,
    /// The Staging Review: waiting for the person to approve the Media Stage.
    StagingReview,
    /// The Media Stage: converting or compressing staged attachments.
    Media,
    /// The Media Review: waiting for the person to approve what lands in the
    /// database.
    MediaReview,
    /// The Upload Stage: writing the staged messages into Message Crate.
    Upload,
}

impl ImportStage {
    /// Every stage, in the order a run passes through them.
    pub const ALL: [Self; 6] = [
        Self::Parse,
        Self::Write,
        Self::StagingReview,
        Self::Media,
        Self::MediaReview,
        Self::Upload,
    ];

    /// Stored spelling of this stage.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Parse => "parse",
            Self::Write => "write",
            Self::StagingReview => "staging_review",
            Self::Media => "media",
            Self::MediaReview => "media_review",
            Self::Upload => "upload",
        }
    }

    /// Parse a stored spelling, or `None` when it is not one of the six.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|stage| stage.as_str() == s)
    }
}

/// How an Import Run stands: the values `imports.status` holds, the values
/// `GET /v1/imports?status=` accepts, and the words every response carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ImportStatus {
    /// Still going: the one run an account may have open.
    Running,
    /// Finished with nothing to report.
    Completed,
    /// Finished, and some items were skipped or failed.
    CompletedWithIssues,
    /// Ended without finishing.
    Failed,
    /// Discarded by the person, or by the server on their behalf.
    Cancelled,
}

impl ImportStatus {
    /// Every status, in the order the documentation lists them.
    pub const ALL: [Self; 5] = [
        Self::Running,
        Self::Completed,
        Self::CompletedWithIssues,
        Self::Failed,
        Self::Cancelled,
    ];

    /// The value as the wire and the database spell it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::CompletedWithIssues => "completed_with_issues",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// The status `value` spells, or `None` for any other word.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == value)
    }
}

impl std::fmt::Display for ImportStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The Stage of an Import Run an issue came from: the values
/// `import_issues.stage` holds and every issue on the wire carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ImportIssueStage {
    /// Reading the backup and writing its messages and attachments into the
    /// Staging Directory.
    Staging,
    /// Converting or compressing the staged attachments.
    Media,
    /// Writing the staged messages and attachments into Message Crate.
    Upload,
}

impl ImportIssueStage {
    /// Every stage, in the order a run passes through them.
    pub const ALL: [Self; 3] = [Self::Staging, Self::Media, Self::Upload];

    /// The value as the wire and the database spell it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Staging => "staging",
            Self::Media => "media",
            Self::Upload => "upload",
        }
    }

    /// The stage `value` spells, or `None` for any other word.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == value)
    }
}

#[derive(Debug, Clone, Serialize)]
/// One row of `imports`: a per-account Import Run record.
pub struct ImportRow {
    /// Import Run id.
    pub id: i64,
    /// Account that owns the run.
    pub account_id: i64,
    /// Source id the run imports.
    pub source: String,
    /// Importing tool, e.g. `message-crate-push`.
    pub tool: Option<String>,
    /// Import mode (`replace` or `append`).
    pub mode: String,
    /// Whether cross-source dedupe runs after each batch.
    pub dedupe: bool,
    /// Lifecycle status.
    pub status: ImportStatus,
    /// UTC time the run started.
    pub started_at: String,
    /// UTC time the run finished, when it has.
    pub finished_at: Option<String>,
    /// Messages counted for the run.
    pub message_count: i64,
    /// Attachments counted for the run.
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
    /// Lifecycle stage while the run is live; `None` once it is over.
    pub stage: Option<String>,
    /// Absolute path to the staging folder on the client that owns it.
    pub staging_dir: Option<String>,
    /// Which install created the run.
    pub device_id: Option<String>,
    /// Import form snapshot, for restoring the screen.
    pub form_json: Option<String>,
    /// Source path, size, mtime, and message count.
    pub source_fingerprint: Option<String>,
    /// Addresses the backup's device sent from (JSON array).
    pub source_identities: Option<String>,
}

/// Outcome fields written when a run completes.
#[derive(Debug, Clone, Default)]
pub struct CompleteImportArgs {
    /// How the run ended: `completed`, `completed_with_issues` or `failed`.
    pub status: String,
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
    /// Per-file issues to record against the run.
    pub issues: Vec<ImportIssueInput>,
}

impl CompleteImportArgs {
    /// Build a success outcome from message and attachment counts.
    pub fn succeeded(messages: u64, attachments: u64) -> Self {
        Self {
            status: "completed".into(),
            message_count: Some(messages as i64),
            attachment_count: Some(attachments as i64),
            ..Default::default()
        }
    }

    /// Build a failure outcome; nothing else is recorded.
    pub fn failed() -> Self {
        Self {
            status: "failed".into(),
            ..Default::default()
        }
    }
}

/// One problem to record against an Import Run.
#[derive(Debug, Clone)]
pub struct ImportIssueInput {
    /// Issue category: `error` or `skip`.
    pub kind: String,
    /// Stage the issue came from.
    pub stage: ImportIssueStage,
    /// The file or message the issue is about.
    pub item: String,
    /// Human-readable explanation.
    pub reason: String,
}

/// One stored `import_issues` row.
#[derive(Debug, Clone, Serialize)]
pub struct ImportIssueRow {
    /// Issue row id.
    pub id: i64,
    /// Run the issue belongs to.
    pub import_id: i64,
    /// Issue category: `error` or `skip`.
    pub kind: String,
    /// Stage the issue came from.
    pub stage: ImportIssueStage,
    /// The file or message the issue is about.
    pub item: String,
    /// Human-readable explanation.
    pub reason: String,
    /// UTC time the issue was recorded.
    pub created_at: String,
}

/// An Import Run row plus its recorded issues.
#[derive(Debug, Clone, Serialize)]
pub struct ImportDetail {
    /// The run.
    pub row: ImportRow,
    /// Issues recorded for it.
    pub issues: Vec<ImportIssueRow>,
}

/// Failure looking up or reusing an Import Run.
#[derive(Debug, thiserror::Error)]
pub enum ImportLookupError {
    /// No run with this id for this account.
    #[error("import {import_id} not found for this account")]
    NotFound {
        /// The run id that was looked up.
        import_id: i64,
    },
    /// Run exists but cannot be reused (wrong status/source/mode).
    #[error("{message}")]
    InvalidRun {
        /// Why the run cannot be reused.
        message: String,
    },
    /// Database failure.
    #[error(transparent)]
    Db(#[from] anyhow::Error),
}

impl From<sqlx::Error> for ImportLookupError {
    fn from(value: sqlx::Error) -> Self {
        Self::Db(value.into())
    }
}

/// Everything recorded when a run begins.
pub struct StartImportArgs<'a> {
    /// Owning account.
    pub account_id: i64,
    /// IR source family (`imessage`, `whatsapp`, …), not a method id.
    pub source: &'a str,
    /// Import mode recorded by the importer.
    pub mode: &'a str,
    /// Whether cross-source dedupe runs after each batch.
    pub dedupe: bool,
    /// Client/tool name, when the caller names one.
    pub tool: Option<&'a str>,
    /// Stage the run opens at.
    pub stage: ImportStage,
    /// Absolute staging path on the client.
    pub staging_dir: Option<&'a str>,
    /// Which install is creating this run.
    pub device_id: Option<&'a str>,
    /// Import form snapshot as JSON.
    pub form_json: Option<&'a str>,
    /// Source fingerprint as JSON.
    pub source_fingerprint: Option<&'a str>,
    /// Backup device identity list as JSON.
    pub source_identities: Option<&'a str>,
}

impl<'a> StartImportArgs<'a> {
    /// A run opening at [`ImportStage::Parse`] with nothing recorded
    /// about the client: no staging folder, device, form snapshot,
    /// fingerprint, or identities. The CLI importer and most tests start
    /// here; a caller with more to record uses struct update syntax on
    /// top of it.
    pub fn new(account_id: i64, source: &'a str, mode: &'a str, tool: Option<&'a str>) -> Self {
        Self {
            account_id,
            source,
            mode,
            dedupe: false,
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

/// Why a run could not be started.
#[derive(Debug, thiserror::Error)]
pub enum StartImportError {
    /// This account already has a running Import Run. The partial unique index
    /// rejected the insert, so this holds even against a racing client.
    ///
    /// Naming the way out matters: a killed CLI import leaves a run open
    /// that blocks every later one. The desktop app's Import screen can
    /// resume or discard it; `message-crate-server imports discard` can
    /// discard it without the app.
    #[error(
        "this account already has a running Import Run; open Import in the desktop app to resume or discard it, or run `message-crate-server imports discard --account <account>`"
    )]
    AlreadyActive,
    /// Anything else.
    #[error(transparent)]
    Db(anyhow::Error),
}

/// Open a new Import Run.
///
/// # Errors
///
/// [`StartImportError::AlreadyActive`] when a running Import Run already exists
/// for this account; [`StartImportError::Db`] for any other failure.
pub async fn start_import(
    conn: &mut SqliteConnection,
    args: &StartImportArgs<'_>,
) -> std::result::Result<i64, StartImportError> {
    let started_at = Utc::now().to_rfc3339();
    let inserted: std::result::Result<i64, sqlx::Error> = sqlx::query_scalar(
        r"
        INSERT INTO imports (
            account_id, source, tool, mode, dedupe, status, started_at,
            message_count, attachment_count, bytes_uploaded,
            stage, staging_dir, device_id, form_json, source_fingerprint,
            source_identities
        ) VALUES ($1, $2, $3, $4, $12, 'running', $5, 0, 0, 0, $6, $7, $8, $9, $10, $11)
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
    .bind(i64::from(args.dedupe))
    .fetch_one(&mut *conn)
    .await;

    match inserted {
        Ok(id) => Ok(id),
        Err(err) if is_unique_violation(&err) => Err(StartImportError::AlreadyActive),
        Err(err) => Err(StartImportError::Db(err.into())),
    }
}

/// Whether this error is a unique-constraint violation, which SQLite
/// reports as `2067` (unique index) or `1555` (primary key).
fn is_unique_violation(err: &sqlx::Error) -> bool {
    let Some(db_err) = err.as_database_error() else {
        return false;
    };
    matches!(db_err.code().as_deref(), Some("2067" | "1555"))
}

/// Column list for `imports`, in the order reads map to a row.
const IMPORT_COLUMNS: &str = "id, account_id, source, tool, mode, status, started_at, \
     finished_at, message_count, attachment_count, bytes_uploaded, duration_ms, parse_ms, \
     attachments_ms, prepare_ms, upload_ms, summary_json, stage, staging_dir, device_id, \
     form_json, source_fingerprint, source_identities, dedupe";

/// Map one `imports` row by column position.
fn import_from_row(row: &SqliteRow) -> Result<ImportRow, sqlx::Error> {
    Ok(ImportRow {
        id: row.try_get(0)?,
        account_id: row.try_get(1)?,
        source: row.try_get(2)?,
        tool: row.try_get(3)?,
        mode: row.try_get(4)?,
        status: {
            let raw: String = row.try_get(5)?;
            ImportStatus::parse(&raw).ok_or_else(|| {
                sqlx::Error::Decode(format!("imports.status holds unknown value '{raw}'").into())
            })?
        },
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
        dedupe: row.try_get::<i64, _>(23)? != 0,
    })
}

/// Load an import row owned by `account_id`, or error.
pub async fn get_owned_import(
    conn: &mut SqliteConnection,
    account_id: i64,
    import_id: i64,
) -> std::result::Result<ImportRow, ImportLookupError> {
    let row = sqlx::query(&format!(
        "SELECT {IMPORT_COLUMNS}
         FROM imports
         WHERE id = $1 AND account_id = $2"
    ))
    .bind(import_id)
    .bind(account_id)
    .fetch_optional(&mut *conn)
    .await?;
    match row {
        Some(data) => Ok(import_from_row(&data)?),
        None => Err(ImportLookupError::NotFound { import_id }),
    }
}

/// The account's import when it is still running.
///
/// # Errors
///
/// [`ImportLookupError::NotFound`] when the account owns no such import,
/// [`ImportLookupError::InvalidRun`] when it is no longer running.
pub async fn require_running_import(
    conn: &mut SqliteConnection,
    account_id: i64,
    import_id: i64,
) -> std::result::Result<ImportRow, ImportLookupError> {
    let existing = get_owned_import(&mut *conn, account_id, import_id).await?;
    if existing.status != ImportStatus::Running {
        return Err(ImportLookupError::InvalidRun {
            message: format!(
                "import {import_id} is not running (status={})",
                existing.status
            ),
        });
    }
    Ok(existing)
}

/// Move a running Import Run to another stage, optionally recording what the user
/// approved at the Review they just passed.
///
/// `summary_json` is written to `imports.summary_json` only when
/// `Some`; `None` leaves whatever is already stored there untouched. Most
/// stage changes carry no summary, and treating absent as null would erase
/// the plan decision 15 later diffs the outcome against.
///
/// # Errors
///
/// [`ImportLookupError::NotFound`] when the account owns no such import,
/// [`ImportLookupError::InvalidRun`] when it is no longer running.
pub async fn set_import_stage(
    conn: &mut SqliteConnection,
    account_id: i64,
    import_id: i64,
    stage: ImportStage,
    summary_json: Option<&str>,
) -> std::result::Result<(), ImportLookupError> {
    // `COALESCE` keeps the stored summary when none is given. The status
    // check is in the update, so a run that finished after the caller read it
    // is never given a stage.
    let updated = sqlx::query(
        "UPDATE imports SET stage = $1, summary_json = COALESCE($2, summary_json)
         WHERE id = $3 AND account_id = $4 AND status = 'running'",
    )
    .bind(stage.as_str())
    .bind(summary_json)
    .bind(import_id)
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(not_running(conn, account_id, import_id).await);
    }
    Ok(())
}

/// Why a write guarded by `status = 'running'` changed no row: the account
/// owns no such run, or the run has finished.
async fn not_running(
    conn: &mut SqliteConnection,
    account_id: i64,
    import_id: i64,
) -> ImportLookupError {
    match require_running_import(conn, account_id, import_id).await {
        Err(err) => err,
        // Running again cannot happen: a run that has finished stays
        // finished. Reported as finished, which it was at the write.
        Ok(_) => ImportLookupError::InvalidRun {
            message: format!("import {import_id} is not running"),
        },
    }
}

/// Close a running Import Run the user gave up on.
///
/// Records `cancelled` and clears `stage`, which frees the account's
/// single active slot. Nothing reclaims a run on a timer — a run
/// is broken by an explicit discard or not at all.
///
/// # Errors
///
/// [`ImportLookupError::NotFound`] when the account owns no such import,
/// [`ImportLookupError::InvalidRun`] when it is no longer running.
pub async fn discard_import(
    conn: &mut SqliteConnection,
    account_id: i64,
    import_id: i64,
) -> std::result::Result<(), ImportLookupError> {
    // The status check is in the update, so a run that completed after the
    // caller read it keeps its record instead of being rewritten as cancelled.
    let updated = sqlx::query(
        "UPDATE imports
         SET status = 'cancelled', stage = NULL, finished_at = $1
         WHERE id = $2 AND account_id = $3 AND status = 'running'",
    )
    .bind(Utc::now().to_rfc3339())
    .bind(import_id)
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(not_running(conn, account_id, import_id).await);
    }
    Ok(())
}

/// Discard the account's running Import Run, whichever it is, and return the row
/// as it was before the discard; `None` when the account has no live
/// run.
///
/// This is the way out for a command-line operator: a killed
/// `message-crate-server import` leaves its run open, and the
/// partial unique index then refuses every later import. The operator knows
/// the account, not the run id, so the lookup happens here rather than
/// making them find the id first.
///
/// # Errors
///
/// Returns the database error.
pub async fn discard_running_import(
    conn: &mut SqliteConnection,
    account_id: i64,
) -> Result<Option<ImportRow>> {
    let row = sqlx::query(&format!(
        "SELECT {IMPORT_COLUMNS}
         FROM imports
         WHERE account_id = $1 AND status = 'running'"
    ))
    .bind(account_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let running = import_from_row(&row)?;
    discard_import(conn, account_id, running.id)
        .await
        .map_err(|err| anyhow::anyhow!(err))?;
    Ok(Some(running))
}

/// Finish a running import: prefer client counts, else derive from linked
/// messages. A run that has already finished is its permanent record and is
/// never rewritten, so completing one is
/// [`ImportLookupError::InvalidRun`] (`409`), checked first and again by
/// the update itself, so two completions racing cannot both land.
pub async fn complete_import(
    conn: &mut SqliteConnection,
    account_id: i64,
    import_id: i64,
    args: &CompleteImportArgs,
) -> Result<ImportRow> {
    for issue in &args.issues {
        validate_issue_kind(&issue.kind)?;
    }

    // The check, the counts, the update and the issue inserts are one write
    // transaction: the counts are of the run the update closes, and a failed
    // commit rolls back (sqlx drops the transaction).
    let mut tx = begin_write(conn).await?;
    let existing = require_running_import(&mut tx, account_id, import_id).await?;
    let finished_at = Utc::now().to_rfc3339();
    let status = args.status.as_str();

    let message_count = if let Some(n) = args.message_count {
        n
    } else {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM messages WHERE import_id = $1 AND account_id = $2",
        )
        .bind(import_id)
        .bind(account_id)
        .fetch_one(&mut *tx)
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
        .fetch_one(&mut *tx)
        .await?;
        n
    };
    let bytes_uploaded = args.bytes_uploaded.unwrap_or(existing.bytes_uploaded);

    let updated = sqlx::query(
        r"
        UPDATE imports
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
        WHERE id = $12 AND account_id = $13 AND status = 'running'
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
    if updated.rows_affected() == 0 {
        return Err(ImportLookupError::InvalidRun {
            message: format!("import {import_id} finished while it was being completed"),
        }
        .into());
    }
    insert_issues(&mut tx, import_id, &args.issues).await?;
    tx.commit().await?;

    Ok(get_owned_import(&mut *conn, account_id, import_id).await?)
}

/// Append issue rows for an import.
async fn insert_issues(
    conn: &mut SqliteConnection,
    import_id: i64,
    issues: &[ImportIssueInput],
) -> Result<()> {
    for issue in issues {
        sqlx::query(
            r"
            INSERT INTO import_issues (
                import_id, kind, stage, item, reason, created_at
            ) VALUES ($1, $2, $3, $4, $5, $6)
            ",
        )
        .bind(import_id)
        .bind(&issue.kind)
        .bind(issue.stage.as_str())
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
    conn: &mut SqliteConnection,
    account_id: i64,
    import_id: i64,
) -> std::result::Result<ImportDetail, ImportLookupError> {
    let row = get_owned_import(conn, account_id, import_id).await?;
    let issue_rows: Vec<(i64, i64, String, String, String, String, String)> = sqlx::query_as(
        r"
        SELECT id, import_id, kind, stage, item, reason, created_at
        FROM import_issues
        WHERE import_id = $1
        ORDER BY id ASC
        ",
    )
    .bind(import_id)
    .fetch_all(&mut *conn)
    .await?;
    let issues = issue_rows
        .into_iter()
        .map(|(id, import_id, kind, stage, item, reason, created_at)| {
            let stage = ImportIssueStage::parse(&stage).ok_or_else(|| {
                sqlx::Error::Decode(
                    format!("import_issues.stage holds unknown value '{stage}'").into(),
                )
            })?;
            Ok(ImportIssueRow {
                id,
                import_id,
                kind,
                stage,
                item,
                reason,
                created_at,
            })
        })
        .collect::<std::result::Result<Vec<_>, sqlx::Error>>()?;
    Ok(ImportDetail { row, issues })
}

/// One Import Run as `GET /v1/imports` lists it: the counts Settings shows,
/// and everything the desktop app needs to resume a running one.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ImportSummary {
    /// Import Run id.
    pub id: i64,
    /// Source id the run imports.
    pub source: String,
    /// Importing tool, e.g. `message-crate-push`.
    pub tool: Option<String>,
    /// Import mode (`replace` or `append`).
    pub mode: String,
    /// Whether cross-source dedupe runs after each batch.
    pub dedupe: bool,
    /// Lifecycle status.
    pub status: ImportStatus,
    /// UTC time the run started.
    pub started_at: String,
    /// UTC time the run finished, when it has.
    pub finished_at: Option<String>,
    /// Messages counted for the run.
    pub message_count: i64,
    /// Attachments counted for the run.
    pub attachment_count: i64,
    /// Bytes uploaded so far.
    pub bytes_uploaded: i64,
    /// Total wall-clock duration, when finished.
    pub duration_ms: Option<i64>,
    /// Where a running run is; null once it is over.
    pub stage: Option<String>,
    /// Absolute path to the staging folder on the client that owns the run.
    pub staging_dir: Option<String>,
    /// Which install created the run.
    pub device_id: Option<String>,
    /// Import form snapshot, or null.
    pub form: serde_json::Value,
    /// Source path, size, mtime, and message count, or null.
    pub source_fingerprint: serde_json::Value,
    /// Addresses the backup's device sent from (JSON array), or null.
    pub source_identities: serde_json::Value,
    /// What the user approved at the last Review they passed, or null. The
    /// column `PATCH /v1/imports/{id}` writes with its `summary`.
    pub summary: serde_json::Value,
}

/// A JSON text column as a value: the parsed JSON, the raw text when it is
/// not JSON, and null when the column is.
#[must_use]
pub fn json_column(raw: Option<String>) -> serde_json::Value {
    match raw {
        Some(raw) => serde_json::from_str(&raw).unwrap_or(serde_json::Value::String(raw)),
        None => serde_json::Value::Null,
    }
}

impl From<ImportRow> for ImportSummary {
    fn from(r: ImportRow) -> Self {
        ImportSummary {
            id: r.id,
            source: r.source,
            tool: r.tool,
            mode: r.mode,
            dedupe: r.dedupe,
            status: r.status,
            started_at: r.started_at,
            finished_at: r.finished_at,
            message_count: r.message_count,
            attachment_count: r.attachment_count,
            bytes_uploaded: r.bytes_uploaded,
            duration_ms: r.duration_ms,
            stage: r.stage,
            staging_dir: r.staging_dir,
            device_id: r.device_id,
            form: json_column(r.form_json),
            source_fingerprint: json_column(r.source_fingerprint),
            source_identities: json_column(r.source_identities),
            summary: json_column(r.summary_json),
        }
    }
}

/// The one key `GET /v1/imports` accepts in `sort=`: `started_at`, the same
/// key and default `GET /v1/exports` takes, because the two lists sit beside
/// each other in Settings → Storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportSort {
    /// When the run started, ties broken by id the same way.
    StartedAt,
}

/// The accepted keys, as `sort=` spells them.
pub const IMPORT_SORT_KEYS: [(&str, ImportSort); 1] = [("started_at", ImportSort::StartedAt)];

/// Newest first: what the list shows when `sort` is absent.
pub const DEFAULT_IMPORT_SORT: [SortKey<ImportSort>; 1] = [SortKey {
    key: ImportSort::StartedAt,
    direction: Direction::Desc,
}];

/// One page of an account's Import Runs, in the order `order` asks for,
/// narrowed to one `status` when given, with the total the page is cut from.
/// The rows are whole; each route shapes them for whoever reads them.
pub async fn list_imports_page(
    conn: &mut SqliteConnection,
    account_id: i64,
    status: Option<&str>,
    order: &[SortKey<ImportSort>],
    limit: i64,
    offset: i64,
) -> Result<(Vec<ImportRow>, u64)> {
    let status_sql = if status.is_some() {
        " AND status = $2"
    } else {
        ""
    };
    let count_sql = format!("SELECT COUNT(*) FROM imports WHERE account_id = $1{status_sql}");
    let mut count = sqlx::query_scalar::<_, i64>(&count_sql).bind(account_id);
    if let Some(status) = status {
        count = count.bind(status);
    }
    let total = count.fetch_one(&mut *conn).await?.max(0) as u64;

    let direction = order
        .iter()
        .find(|k| k.key == ImportSort::StartedAt)
        .map_or(Direction::Desc, |k| k.direction)
        .sql();
    let (limit_param, offset_param) = if status.is_some() {
        ("$3", "$4")
    } else {
        ("$2", "$3")
    };
    let sql = format!(
        "SELECT {IMPORT_COLUMNS}
         FROM imports
         WHERE account_id = $1{status_sql}
         ORDER BY started_at {direction}, id {direction}
         LIMIT {limit_param} OFFSET {offset_param}"
    );
    let mut query = sqlx::query(&sql).bind(account_id);
    if let Some(status) = status {
        query = query.bind(status);
    }
    let rows = query.bind(limit).bind(offset).fetch_all(&mut *conn).await?;
    let items = rows
        .iter()
        .map(import_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok((items, total))
}

/// How many issues the run recorded. The caller has already established
/// that `import_id` is the account's.
pub async fn issue_count(conn: &mut SqliteConnection, import_id: i64) -> Result<u64> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM import_issues WHERE import_id = $1")
        .bind(import_id)
        .fetch_one(&mut *conn)
        .await?;
    Ok(count.max(0) as u64)
}

/// Whether the run has stamped any message yet: the first batch of a
/// `replace` run wipes the source, and every batch after it appends.
pub async fn has_messages(conn: &mut SqliteConnection, import_id: i64) -> Result<bool> {
    let row = sqlx::query("SELECT 1 FROM messages WHERE import_id = $1 LIMIT 1")
        .bind(import_id)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row.is_some())
}

/// Whether the account has a running Import Run. Such a run may have
/// uploaded files that no row names yet, for a batch it has not sent.
pub async fn has_running_import(conn: &mut SqliteConnection, account_id: i64) -> Result<bool> {
    let row = sqlx::query("SELECT 1 FROM imports WHERE account_id = $1 AND status = 'running'")
        .bind(account_id)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row.is_some())
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
    /// Conversation that holds the attachment. Like the two fields after it,
    /// absent when the owner reads another account's storage: which
    /// conversation a file is in, and who it is with, is the holder's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<i64>,
    /// Conversation label, when set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_title: Option<String>,
    /// Raw text of the identity that keys the conversation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_identifier: Option<String>,
}

impl TopAttachment {
    /// The attachment as a file and nothing else: name, type and size.
    pub fn without_conversation(mut self) -> Self {
        self.conversation_id = None;
        self.conversation_title = None;
        self.chat_identifier = None;
        self
    }
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
    conn: &mut SqliteConnection,
    account_id: i64,
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
                conversation_id: Some(conversation_id),
                conversation_title,
                chat_identifier: Some(chat_identifier),
            },
        )
        .collect())
}

#[cfg(test)]
mod tests;
