//! Per-account Export Run records: one row per `POST /v1/exports`, holding
//! what was asked for and how much matched, never message content; and the
//! list of message places each run hands over, which its pages read.

use anyhow::{Context, Result};
use chrono::Utc;
use message_crate_api_types::{ExportQueryList, ExportRun, ExportScope, ExportStatus};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqliteRow;
use sqlx::{Executor, Row, SqliteConnection};

use crate::db::begin_write;

use crate::db::conversation_messages::{
    Message, MessageSort, conversation_join_sql, load_messages_from, messages_from_sql,
};
use crate::db::sql::{SqlParam, bind_all};
use crate::paging::{Direction, Page, SortKey};
use crate::server::ApiError;

/// Which of the three forms an Export Run's scope took, without what it
/// asked for: the values `exports.scope_kind` holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExportScopeKind {
    /// Everything the account holds.
    Everything,
    /// A query in the search language.
    Query,
    /// Conversations and messages picked by hand.
    Selection,
}

impl ExportScopeKind {
    /// The form `scope` takes.
    #[must_use]
    pub fn of(scope: &ExportScope) -> Self {
        match scope {
            ExportScope::Everything => Self::Everything,
            ExportScope::Query { .. } => Self::Query,
            ExportScope::Selection { .. } => Self::Selection,
        }
    }

    /// The form `value` spells, or `None` for any other word.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "everything" => Some(Self::Everything),
            "query" => Some(Self::Query),
            "selection" => Some(Self::Selection),
            _ => None,
        }
    }
}

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

/// The four counts the server computes when a run is created.
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

/// Everything recorded when a run begins, before its messages are listed.
#[derive(Debug, Clone)]
pub struct StartExportArgs<'a> {
    /// Owning account.
    pub account_id: i64,
    /// What the run asked for, stored as given.
    pub scope: &'a ExportScope,
    /// Client/tool name, when the client named one.
    pub tool: Option<&'a str>,
}

/// Column list for `exports`, in the order [`export_from_row`] reads.
const EXPORT_COLUMNS: &str = "id, scope_kind, scope_query, scope_conversation_ids, \
     scope_message_ids, tool, status, started_at, finished_at, message_count, \
     conversation_count, attachment_count, total_bytes, messages_delivered, scope_list";

/// Map one `exports` row by column position.
fn export_from_row(row: &SqliteRow) -> Result<ExportRun> {
    let kind: String = row.try_get(1)?;
    let scope = match kind.as_str() {
        "everything" => ExportScope::Everything,
        "query" => ExportScope::Query {
            list: match row.try_get::<Option<String>, _>(14)?.as_deref() {
                Some("conversations") => ExportQueryList::Conversations,
                Some("messages") => ExportQueryList::Messages,
                other => anyhow::bail!("exports.scope_list holds unknown value {other:?}"),
            },
            q: row.try_get::<Option<String>, _>(2)?.unwrap_or_default(),
        },
        "selection" => ExportScope::Selection {
            conversation_ids: id_list(row.try_get(3)?)?,
            message_ids: id_list(row.try_get(4)?)?,
        },
        other => anyhow::bail!("exports.scope_kind holds unknown value '{other}'"),
    };
    Ok(ExportRun {
        id: row.try_get(0)?,
        scope,
        tool: row.try_get(5)?,
        status: {
            let raw: String = row.try_get(6)?;
            ExportStatus::parse(&raw)
                .ok_or_else(|| anyhow::anyhow!("exports.status holds unknown value '{raw}'"))?
        },
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
        Some(text) => serde_json::from_str(&text).context("exports id list is not JSON"),
        None => Ok(Vec::new()),
    }
}

/// Record a new run as `running` with zero counts and return its id. The
/// caller lists the run's messages and then sets the counts with
/// [`record_counts`], in the same transaction.
///
/// # Errors
///
/// Returns an error when the insert fails.
pub async fn start_export(conn: &mut SqliteConnection, args: &StartExportArgs<'_>) -> Result<i64> {
    let (kind, list, query, conversation_ids, message_ids) = match args.scope {
        ExportScope::Everything => ("everything", None, None, None, None),
        ExportScope::Query { list, q } => {
            ("query", Some(list.as_str()), Some(q.as_str()), None, None)
        }
        ExportScope::Selection {
            conversation_ids,
            message_ids,
        } => (
            "selection",
            None,
            None,
            Some(serde_json::to_string(conversation_ids)?),
            Some(serde_json::to_string(message_ids)?),
        ),
    };
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO exports (
            account_id, scope_kind, scope_list, scope_query, scope_conversation_ids,
            scope_message_ids, tool, status, started_at, message_count, conversation_count,
            attachment_count, total_bytes, messages_delivered
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, 'running', $8, 0, 0, 0, 0, 0)
         RETURNING id",
    )
    .bind(args.account_id)
    .bind(kind)
    .bind(list)
    .bind(query)
    .bind(conversation_ids)
    .bind(message_ids)
    .bind(args.tool)
    .bind(Utc::now().to_rfc3339())
    .fetch_one(&mut *conn)
    .await?;
    Ok(id)
}

/// Set the four counts on a run, computed from the messages it listed.
///
/// # Errors
///
/// Returns an error when the update fails.
pub async fn record_counts(
    conn: &mut SqliteConnection,
    export_id: i64,
    counts: ExportCounts,
) -> Result<()> {
    sqlx::query(
        "UPDATE exports
         SET message_count = $1, conversation_count = $2, attachment_count = $3,
             total_bytes = $4
         WHERE id = $5",
    )
    .bind(counts.messages)
    .bind(counts.conversations)
    .bind(counts.attachments)
    .bind(counts.total_bytes)
    .bind(export_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// The account's run with this id, or `None` when the account owns no such
/// run. Another account's run reads as `None` on purpose: its existence is
/// not the caller's to learn.
///
/// # Errors
///
/// Returns an error when the read fails or the row cannot be mapped.
pub async fn get_export(
    conn: &mut SqliteConnection,
    account_id: i64,
    export_id: i64,
) -> Result<Option<ExportRun>> {
    let row = sqlx::query(&format!(
        "SELECT {EXPORT_COLUMNS} FROM exports WHERE id = $1 AND account_id = $2"
    ))
    .bind(export_id)
    .bind(account_id)
    .fetch_optional(&mut *conn)
    .await?;
    row.as_ref().map(export_from_row).transpose()
}

/// Close a running run with `status`, stamp `finished_at`, and delete the
/// list of messages it matched at creation: a closed run hands nothing over.
/// Returns `false` when the run was not running, which is the caller's
/// `409`: the status check and the write are one statement, so two closers
/// racing cannot both win.
///
/// # Errors
///
/// Returns an error when a statement fails.
pub async fn finish_export(
    conn: &mut SqliteConnection,
    account_id: i64,
    export_id: i64,
    status: ExportStatus,
) -> Result<bool> {
    let mut tx = begin_write(conn).await?;
    let done = sqlx::query(
        "UPDATE exports SET status = $1, finished_at = $2
         WHERE id = $3 AND account_id = $4 AND status = 'running'",
    )
    .bind(status.as_str())
    .bind(Utc::now().to_rfc3339())
    .bind(export_id)
    .bind(account_id)
    .execute(&mut *tx)
    .await?;
    if done.rows_affected() != 1 {
        return Ok(false);
    }
    sqlx::query("DELETE FROM export_messages WHERE export_id = $1")
        .bind(export_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

/// Raise `messages_delivered` to `delivered` when that is higher, on a run
/// that is still running. A page read again does not count twice, and a page
/// read out of order does not lower the mark. Returns `false` when the run
/// was not running, which is the caller's `409`: the page was read from a
/// run that closed meanwhile, and the status check and the write are one
/// statement, so a finished run's record is never changed.
///
/// # Errors
///
/// Returns an error when the update fails.
pub async fn record_delivered(
    conn: &mut SqliteConnection,
    account_id: i64,
    export_id: i64,
    delivered: i64,
) -> Result<bool> {
    let updated = sqlx::query(
        "UPDATE exports
         SET messages_delivered = CASE
             WHEN messages_delivered < $1 THEN $1 ELSE messages_delivered END
         WHERE id = $2 AND account_id = $3 AND status = 'running'",
    )
    .bind(delivered)
    .bind(export_id)
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    Ok(updated.rows_affected() == 1)
}

/// One page of an account's Export Runs, narrowed to one `status` when
/// given, with the total the page is cut from.
///
/// # Errors
///
/// Returns an error when a read fails or a row cannot be mapped.
pub async fn list_exports_page(
    conn: &mut SqliteConnection,
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
    let count_sql = format!("SELECT COUNT(*) FROM exports WHERE account_id = $1{status_sql}");
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
        "SELECT {EXPORT_COLUMNS}
         FROM exports
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

/// Give each message `filter` matches its place in the run, oldest first, so
/// a page is a range of places whatever happens to the database meanwhile.
///
/// # Errors
///
/// Returns an error when the statement fails.
pub async fn list_run_messages(
    conn: &mut SqliteConnection,
    export_id: i64,
    filter: &crate::search::Filter,
) -> Result<(), sqlx::Error> {
    let list_sql = format!(
        "INSERT INTO export_messages (export_id, row_order, message_id)
         SELECT ?, ROW_NUMBER() OVER (ORDER BY m.timestamp, m.sort_order, m.id), m.id
         {messages_from_sql}
         WHERE {where_sql}",
        messages_from_sql = messages_from_sql(),
        where_sql = filter.where_sql(),
    );
    let mut params = vec![SqlParam::Int(export_id)];
    params.extend_from_slice(filter.params());
    (&mut *conn).execute(bind_all(&list_sql, &params)).await?;
    Ok(())
}

/// The four counts a run records at creation, over the messages it listed.
///
/// Attachment count is unique non-empty SHA-256 fingerprints on those
/// messages; `total_bytes` sums the known `attachments.size_bytes` for those
/// fingerprints.
///
/// # Errors
///
/// Returns an error when a statement fails.
pub async fn export_counts(
    conn: &mut SqliteConnection,
    export_id: i64,
) -> Result<ExportCounts, sqlx::Error> {
    let row = sqlx::query(
        "SELECT COUNT(*), COUNT(DISTINCT m.conversation_id)
         FROM export_messages e
         JOIN messages m ON m.id = e.message_id
         WHERE e.export_id = $1",
    )
    .bind(export_id)
    .fetch_one(&mut *conn)
    .await?;
    let (messages, conversations): (i64, i64) = (row.try_get(0)?, row.try_get(1)?);

    let row = sqlx::query(
        "SELECT COUNT(*), COALESCE(SUM(sz), 0)
         FROM (
           SELECT MAX(a.size_bytes) AS sz
           FROM export_messages e
           JOIN attachments a ON a.message_id = e.message_id
           WHERE e.export_id = $1
             AND a.sha256 IS NOT NULL
             AND length(trim(a.sha256)) > 0
           GROUP BY lower(trim(a.sha256))
         ) fingerprints",
    )
    .bind(export_id)
    .fetch_one(&mut *conn)
    .await?;
    let (attachments, total_bytes): (i64, i64) = (row.try_get(0)?, row.try_get(1)?);

    Ok(ExportCounts {
        messages: messages.max(0),
        conversations: conversations.max(0),
        attachments: attachments.max(0),
        total_bytes: total_bytes.max(0),
    })
}

/// Options for one page of a running Export Run's messages.
#[derive(Debug, Clone)]
pub struct ExportPageOpts {
    /// The run to read, already checked to be the caller's and running.
    pub export_id: i64,
    /// How many places the run listed at creation: its `message_count`.
    pub total: u64,
    /// Places on the page. Already validated by the handler: `1..=MAX_LIST_LIMIT`.
    pub limit: usize,
    /// Places to skip in the run's list.
    pub offset: usize,
    /// The parsed `sort`; [`DEFAULT_MESSAGE_SORT`](crate::db::conversation_messages::DEFAULT_MESSAGE_SORT)
    /// when the caller has none.
    pub order: Vec<SortKey<MessageSort>>,
}

/// The first and last place (exclusive, inclusive) a page covers in a run's
/// list of `total` places, for `offset` and `limit` read in `direction`.
/// Newest first counts places from the end of the list.
fn page_places(total: u64, offset: usize, limit: usize, direction: Direction) -> (i64, i64) {
    let total = i64::try_from(total).unwrap_or(i64::MAX);
    let offset = i64::try_from(offset).unwrap_or(i64::MAX);
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    match direction {
        Direction::Asc => (offset, offset.saturating_add(limit)),
        Direction::Desc => {
            let last = total.saturating_sub(offset);
            (last.saturating_sub(limit), last)
        }
    }
}

/// One page of a running Export Run's messages: the places `offset` to
/// `offset + limit` of the list the run made at creation.
///
/// `total` is always the number of places the run listed. A message deleted
/// since creation leaves its place empty, so that page carries fewer items
/// than `limit`; a caller steps `offset` by `limit`, not by the items it got.
/// An offset past the end returns an empty page.
///
/// # Errors
///
/// Returns an internal error when a database statement fails.
pub async fn export_messages(
    conn: &mut SqliteConnection,
    opts: ExportPageOpts,
) -> Result<Page<Message>, ApiError> {
    let direction = opts
        .order
        .iter()
        .find(|k| k.key == MessageSort::Date)
        .map_or(Direction::Asc, |k| k.direction);
    let (after, through) = page_places(opts.total, opts.offset, opts.limit, direction);
    let from_sql = format!(
        "FROM export_messages e
         JOIN messages m ON m.id = e.message_id
         {conversation_join_sql}",
        conversation_join_sql = conversation_join_sql(),
    );
    let messages = load_messages_from(
        conn,
        &from_sql,
        "e.export_id = ? AND e.row_order > ? AND e.row_order <= ?",
        &[
            SqlParam::Int(opts.export_id),
            SqlParam::Int(after),
            SqlParam::Int(through),
        ],
        &format!("e.row_order {}", direction.sql()),
        opts.limit,
        0,
    )
    .await?;

    Ok(Page {
        items: messages,
        total: opts.total,
        limit: opts.limit,
        offset: opts.offset,
    })
}

/// Record what started the Export Run on its row: a Session and the app it
/// named, or an API token's label and hint as they are now.
///
/// # Errors
///
/// Returns an error when the update fails.
pub async fn record_credential(
    conn: &mut SqliteConnection,
    export_id: i64,
    credential: &crate::db::audit_trail::CredentialUsed,
) -> Result<()> {
    let (kind, app_kind, app_build, label, hint) = credential.run_columns();
    sqlx::query(
        "UPDATE exports SET credential = $1, app_kind = $2, app_build = $3,
                api_token_label = $4, api_token_hint = $5
         WHERE id = $6",
    )
    .bind(kind)
    .bind(app_kind)
    .bind(app_build)
    .bind(label)
    .bind(hint)
    .bind(export_id)
    .execute(&mut *conn)
    .await
    .with_context(|| format!("record what started export {export_id}"))?;
    Ok(())
}

/// Ready the account's Export Runs to outlive it, just before the account is
/// deleted: each keeps `username`, what was asked for and how much matched,
/// a run still open is closed as `cancelled` at `now`, and its list of
/// messages, search text and picked ids go (ADR 0020).
///
/// # Errors
///
/// Returns an error when a statement fails.
pub async fn detach_from_account(
    conn: &mut SqliteConnection,
    account_id: i64,
    username: &str,
    now: &str,
) -> Result<()> {
    sqlx::query(
        "UPDATE exports SET status = 'cancelled', finished_at = $2
         WHERE account_id = $1 AND status = 'running'",
    )
    .bind(account_id)
    .bind(now)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "DELETE FROM export_messages
         WHERE export_id IN (SELECT id FROM exports WHERE account_id = $1)",
    )
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE exports SET username = $2, scope_query = NULL,
                scope_conversation_ids = NULL, scope_message_ids = NULL
         WHERE account_id = $1",
    )
    .bind(account_id)
    .bind(username)
    .execute(&mut *conn)
    .await?;
    Ok(())
}
