//! Per-account Export Run records: one row per `POST /v1/exports`, holding
//! what was asked for and how much matched, never message content.

use anyhow::{Context, Result};
use chrono::Utc;
use sqlx::any::AnyRow;
use sqlx::{AnyConnection, Row};
use vault_api_types::{ExportRun, ExportScope};

use crate::paging::{Direction, SortKey};

/// The values `vault_exports.status` holds, and so the values
/// `GET /v1/exports?status=` accepts.
pub const EXPORT_STATUSES: [&str; 4] = ["running", "completed", "failed", "cancelled"];

/// The one key `GET /v1/exports` accepts in `sort=`: `started_at`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportSort {
    /// When the run started, ties broken by id the same way.
    StartedAt,
}

/// The accepted keys, as `sort=` spells them.
pub const EXPORT_SORT_KEYS: [(&str, ExportSort); 1] = [("started_at", ExportSort::StartedAt)];

/// Newest first: what the list shows when `sort` is absent.
pub const DEFAULT_EXPORT_SORT: [SortKey<ExportSort>; 1] = [SortKey {
    key: ExportSort::StartedAt,
    direction: Direction::Desc,
}];

/// The four counts the vault computes when a run is created.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExportCounts {
    /// Messages the scope matches.
    pub messages: i64,
    /// Distinct conversations with at least one matching message.
    pub conversations: i64,
    /// Distinct attachment fingerprints among the matching messages.
    pub attachments: i64,
    /// Sum of the known sizes of those distinct attachments, in bytes.
    pub total_bytes: i64,
}

/// Everything recorded when a run begins.
#[derive(Debug, Clone)]
pub struct StartExportArgs<'a> {
    /// Owning vault account.
    pub account_id: i64,
    /// What the run asked for, stored as given.
    pub scope: &'a ExportScope,
    /// Client/tool name, when the client named one.
    pub tool: Option<&'a str>,
    /// The counts computed for the scope at creation.
    pub counts: ExportCounts,
}

/// Column list for `vault_exports`, in the order [`export_from_row`] reads.
const VAULT_EXPORT_COLUMNS: &str = "id, scope_kind, scope_query, scope_conversation_ids, \
     scope_message_ids, tool, status, started_at, finished_at, message_count, \
     conversation_count, attachment_count, total_bytes, messages_delivered";

/// Map one `vault_exports` row by column position.
fn export_from_row(row: &AnyRow) -> Result<ExportRun> {
    let kind: String = row.try_get(1)?;
    let scope = match kind.as_str() {
        "everything" => ExportScope::Everything,
        "query" => ExportScope::Query {
            q: row.try_get::<Option<String>, _>(2)?.unwrap_or_default(),
        },
        "selection" => ExportScope::Selection {
            conversation_ids: id_list(row.try_get(3)?)?,
            message_ids: id_list(row.try_get(4)?)?,
        },
        other => anyhow::bail!("vault_exports.scope_kind holds unknown value '{other}'"),
    };
    Ok(ExportRun {
        id: row.try_get(0)?,
        scope,
        tool: row.try_get(5)?,
        status: row.try_get(6)?,
        started_at: row.try_get(7)?,
        finished_at: row.try_get(8)?,
        message_count: row.try_get(9)?,
        conversation_count: row.try_get(10)?,
        attachment_count: row.try_get(11)?,
        total_bytes: row.try_get(12)?,
        messages_delivered: row.try_get(13)?,
    })
}

/// A stored JSON array of ids, or an empty list when the column is NULL.
fn id_list(raw: Option<String>) -> Result<Vec<i64>> {
    match raw {
        Some(text) => serde_json::from_str(&text).context("vault_exports id list is not JSON"),
        None => Ok(Vec::new()),
    }
}

/// Record a new run as `running` and return it as stored.
///
/// # Errors
///
/// Returns an error when the insert or the read-back fails.
pub async fn start_export(
    conn: &mut AnyConnection,
    args: &StartExportArgs<'_>,
) -> Result<ExportRun> {
    let (kind, query, conversation_ids, message_ids) = match args.scope {
        ExportScope::Everything => ("everything", None, None, None),
        ExportScope::Query { q } => ("query", Some(q.as_str()), None, None),
        ExportScope::Selection {
            conversation_ids,
            message_ids,
        } => (
            "selection",
            None,
            Some(serde_json::to_string(conversation_ids)?),
            Some(serde_json::to_string(message_ids)?),
        ),
    };
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO vault_exports (
            account_id, scope_kind, scope_query, scope_conversation_ids, scope_message_ids,
            tool, status, started_at, message_count, conversation_count, attachment_count,
            total_bytes, messages_delivered
         ) VALUES ($1, $2, $3, $4, $5, $6, 'running', $7, $8, $9, $10, $11, 0)
         RETURNING id",
    )
    .bind(args.account_id)
    .bind(kind)
    .bind(query)
    .bind(conversation_ids)
    .bind(message_ids)
    .bind(args.tool)
    .bind(Utc::now().to_rfc3339())
    .bind(args.counts.messages)
    .bind(args.counts.conversations)
    .bind(args.counts.attachments)
    .bind(args.counts.total_bytes)
    .fetch_one(&mut *conn)
    .await?;
    get_export(conn, args.account_id, id)
        .await?
        .context("export run vanished between insert and read")
}

/// The account's run with this id, or `None` when the account owns no such
/// run. Another account's run reads as `None` on purpose: its existence is
/// not the caller's to learn.
///
/// # Errors
///
/// Returns an error when the read fails or the row cannot be mapped.
pub async fn get_export(
    conn: &mut AnyConnection,
    account_id: i64,
    export_id: i64,
) -> Result<Option<ExportRun>> {
    let row = sqlx::query(&format!(
        "SELECT {VAULT_EXPORT_COLUMNS} FROM vault_exports WHERE id = $1 AND account_id = $2"
    ))
    .bind(export_id)
    .bind(account_id)
    .fetch_optional(&mut *conn)
    .await?;
    row.as_ref().map(export_from_row).transpose()
}

/// Close a running run with `status` and stamp `finished_at`. Returns
/// `false` when the run was not running, which is the caller's `409`: the
/// status check and the write are one statement, so two closers racing
/// cannot both win.
///
/// # Errors
///
/// Returns an error when the update fails.
pub async fn finish_export(
    conn: &mut AnyConnection,
    account_id: i64,
    export_id: i64,
    status: &str,
) -> Result<bool> {
    let done = sqlx::query(
        "UPDATE vault_exports SET status = $1, finished_at = $2
         WHERE id = $3 AND account_id = $4 AND status = 'running'",
    )
    .bind(status)
    .bind(Utc::now().to_rfc3339())
    .bind(export_id)
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// Raise `messages_delivered` to `delivered` when that is higher. A page read
/// again does not count twice, and a page read out of order does not lower
/// the mark.
///
/// # Errors
///
/// Returns an error when the update fails.
pub async fn record_delivered(
    conn: &mut AnyConnection,
    account_id: i64,
    export_id: i64,
    delivered: i64,
) -> Result<()> {
    sqlx::query(
        "UPDATE vault_exports
         SET messages_delivered = CASE
             WHEN messages_delivered < $1 THEN $1 ELSE messages_delivered END
         WHERE id = $2 AND account_id = $3",
    )
    .bind(delivered)
    .bind(export_id)
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// One page of an account's Export Runs, narrowed to one `status` when
/// given, with the total the page is cut from.
///
/// # Errors
///
/// Returns an error when a read fails or a row cannot be mapped.
pub async fn list_exports_page(
    conn: &mut AnyConnection,
    account_id: i64,
    status: Option<&str>,
    order: &[SortKey<ExportSort>],
    limit: i64,
    offset: i64,
) -> Result<(Vec<ExportRun>, u64)> {
    let status_sql = if status.is_some() {
        " AND status = $2"
    } else {
        ""
    };
    let count_sql = format!("SELECT COUNT(*) FROM vault_exports WHERE account_id = $1{status_sql}");
    let mut count = sqlx::query_scalar::<_, i64>(&count_sql).bind(account_id);
    if let Some(status) = status {
        count = count.bind(status);
    }
    let total = count.fetch_one(&mut *conn).await?.max(0) as u64;

    let direction = order
        .iter()
        .find(|k| k.key == ExportSort::StartedAt)
        .map_or(Direction::Desc, |k| k.direction)
        .sql();
    let (limit_param, offset_param) = if status.is_some() {
        ("$3", "$4")
    } else {
        ("$2", "$3")
    };
    let sql = format!(
        "SELECT {VAULT_EXPORT_COLUMNS}
         FROM vault_exports
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
        .map(export_from_row)
        .collect::<Result<Vec<_>>>()?;
    Ok((items, total))
}
