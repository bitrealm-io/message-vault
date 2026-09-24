//! Export Runs: `POST /v1/exports` records what was asked for, lists the
//! messages it matches and counts them, `GET /v1/exports/{id}/messages` pages
//! that list, and `complete` or `cancel` closes the run and drops the list
//! (`docs/architecture/http-api.md`, "Runs").
//!
//! Every route here takes the `export` scope on a session or an API token.
//! A program with an export token reads messages only through a run it
//! started, so every read of message data by a program leaves a record.

use crate::extract::{Json, Path as AxumPath, Query};
use axum::extract::State;
use serde::Deserialize;
use sqlx::{AnyConnection, Connection};
use sqlx::{Executor, Row};
use vault_api_types::{ExportRun, ExportScope};

use crate::db::conversation_messages::{
    DEFAULT_MESSAGE_SORT, MESSAGE_SORT_KEYS, Message, MessageSort, conversation_join_sql,
    load_messages_from, messages_from_sql,
};
use crate::db::dialect::{begin_immediate_sql, engine_of};
use crate::db::sql::{SqlParam, bind_all, renumber_placeholders};
use crate::db::vault_exports::{
    self, DEFAULT_EXPORT_SORT, EXPORT_SORT_KEYS, EXPORT_STATUSES, ExportCounts, StartExportArgs,
};
use crate::messages_api::message_filter;
use crate::paging::{
    DEFAULT_LIST_LIMIT, Direction, MAX_LIST_OFFSET, Page, SortKey, page_params, parse_sort,
};
use crate::server::{ApiError, AppState, Created, ExportAccess};

/// Most ids one `selection` scope may name in either list, so the `IN` list
/// stays under SQLite's variable cap; the same figure `POST /v1/contacts/summaries`
/// uses.
pub const MAX_SELECTION_IDS: usize = 500;

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
    /// The parsed `sort`; [`DEFAULT_MESSAGE_SORT`] when the caller has none.
    pub order: Vec<SortKey<MessageSort>>,
}

/// Start an Export Run over `scope`: compile the scope, list the ids of the
/// messages it matches now, count them, and record the run as `running`, all
/// in one transaction. The run's pages read that list, never the scope again,
/// so what a run hands over is fixed when it is created.
///
/// # Errors
///
/// The scope's own failures ([`scope_filter`]), or an internal error when a
/// database statement fails. On any error nothing is recorded.
pub async fn start_export_run(
    conn: &mut AnyConnection,
    account_id: i64,
    scope: &ExportScope,
    tool: Option<&str>,
    clock: (chrono_tz::Tz, chrono::NaiveDate),
) -> Result<ExportRun, ApiError> {
    let engine = engine_of(conn);
    let mut tx = conn.begin_with(begin_immediate_sql(engine)).await?;
    let filter = scope_filter(&mut tx, account_id, scope, clock).await?;
    crate::db::account_profile::ensure_account_row(&mut tx, account_id).await?;
    let export_id = vault_exports::start_export(
        &mut tx,
        &StartExportArgs {
            account_id,
            scope,
            tool,
        },
    )
    .await?;

    // Each matched message gets its place in the run, oldest first, so a
    // page is a range of places whatever happens to the vault meanwhile.
    let list_sql = format!(
        "INSERT INTO vault_export_messages (export_id, row_order, message_id)
         SELECT ?, ROW_NUMBER() OVER (ORDER BY m.timestamp, m.sort_order, m.id), m.id
         {messages_from_sql}
         WHERE {where_sql}",
        messages_from_sql = messages_from_sql(),
        where_sql = filter.where_sql(),
    );
    let mut params = vec![SqlParam::Int(export_id)];
    params.extend_from_slice(filter.params());
    (&mut *tx)
        .execute(bind_all(&renumber_placeholders(&list_sql), &params))
        .await?;

    let counts = export_counts(&mut tx, export_id).await?;
    vault_exports::record_counts(&mut tx, export_id, counts).await?;
    let run = vault_exports::get_export(&mut tx, account_id, export_id)
        .await?
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("export run vanished before commit")))?;
    tx.commit().await?;
    Ok(run)
}

/// Compile an Export Run's scope to the filter every read of it uses.
///
/// `everything` is the empty query; `query` is the search language against
/// the Messages list; `selection` is the empty query's defaults narrowed to
/// the picked conversation and message ids, each checked against the account
/// first, so a run can never be created over rows the caller may not read.
///
/// # Errors
///
/// `search-query-invalid` for a query the language refuses;
/// `validation-failed` for a blank query, an empty selection, a selection
/// over [`MAX_SELECTION_IDS`], or an id the account does not hold.
pub async fn scope_filter(
    conn: &mut AnyConnection,
    account_id: i64,
    scope: &ExportScope,
    clock: (chrono_tz::Tz, chrono::NaiveDate),
) -> Result<crate::search::Filter, ApiError> {
    let engine = engine_of(conn);
    match scope {
        ExportScope::Everything => message_filter(engine, account_id, "", clock),
        ExportScope::Query { q } => {
            if q.trim().is_empty() {
                return Err(ApiError::validation(
                    "scope.q is blank; export everything with {\"kind\": \"everything\"}",
                ));
            }
            message_filter(engine, account_id, q, clock)
        }
        ExportScope::Selection {
            conversation_ids,
            message_ids,
        } => {
            if conversation_ids.is_empty() && message_ids.is_empty() {
                return Err(ApiError::validation(
                    "scope.conversation_ids and scope.message_ids are both empty; a selection names at least one",
                ));
            }
            let mut errors = Vec::new();
            for (field, ids) in [
                ("scope.conversation_ids", conversation_ids),
                ("scope.message_ids", message_ids),
            ] {
                if ids.len() > MAX_SELECTION_IDS {
                    errors.push(format!(
                        "{field} names {} ids; at most {MAX_SELECTION_IDS} are accepted",
                        ids.len()
                    ));
                }
            }
            if !errors.is_empty() {
                return Err(ApiError::ValidationFailed(errors));
            }
            let missing_conversations =
                missing_ids(conn, "conversations", account_id, conversation_ids).await?;
            let missing_messages = missing_ids(conn, "messages", account_id, message_ids).await?;
            for (field, missing) in [
                ("scope.conversation_ids", missing_conversations),
                ("scope.message_ids", missing_messages),
            ] {
                if !missing.is_empty() {
                    let listed = missing
                        .iter()
                        .map(i64::to_string)
                        .collect::<Vec<_>>()
                        .join(", ");
                    errors.push(format!("{field}: {listed} not found for this account"));
                }
            }
            if !errors.is_empty() {
                return Err(ApiError::ValidationFailed(errors));
            }

            let mut branches = Vec::new();
            let mut params = Vec::new();
            if !conversation_ids.is_empty() {
                branches.push(format!(
                    "m.conversation_id IN ({})",
                    placeholders(conversation_ids.len())
                ));
                params.extend(conversation_ids.iter().map(|id| SqlParam::Int(*id)));
            }
            if !message_ids.is_empty() {
                branches.push(format!("m.id IN ({})", placeholders(message_ids.len())));
                params.extend(message_ids.iter().map(|id| SqlParam::Int(*id)));
            }
            let fragment = format!("({})", branches.join(" OR "));
            Ok(message_filter(engine, account_id, "", clock)?.and_where(&fragment, params))
        }
    }
}

/// `?, ?, ?` for an `IN` list of `n` values.
fn placeholders(n: usize) -> String {
    vec!["?"; n].join(", ")
}

/// The ids in `ids` that `table` does not hold for `account_id`, in the
/// order given.
async fn missing_ids(
    conn: &mut AnyConnection,
    table: &str,
    account_id: i64,
    ids: &[i64],
) -> Result<Vec<i64>, ApiError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT id FROM {table} WHERE account_id = ? AND id IN ({})",
        placeholders(ids.len())
    );
    let mut params = vec![SqlParam::Int(account_id)];
    params.extend(ids.iter().map(|id| SqlParam::Int(*id)));
    let rows = (&mut *conn)
        .fetch_all(bind_all(&renumber_placeholders(&sql), &params))
        .await?;
    let found = rows
        .iter()
        .map(|row| row.try_get::<i64, _>(0))
        .collect::<Result<std::collections::HashSet<_>, _>>()?;
    let mut missing: Vec<i64> = ids
        .iter()
        .copied()
        .filter(|id| !found.contains(id))
        .collect();
    missing.dedup();
    Ok(missing)
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
    conn: &mut AnyConnection,
    opts: ExportPageOpts,
) -> Result<Page<Message>, ApiError> {
    let direction = opts
        .order
        .iter()
        .find(|k| k.key == MessageSort::Date)
        .map_or(Direction::Asc, |k| k.direction);
    let (after, through) = page_places(opts.total, opts.offset, opts.limit, direction);
    let from_sql = format!(
        "FROM vault_export_messages e
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
        opts.limit as u32,
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

/// The four counts a run records at creation, over the messages it listed.
///
/// Attachment count is unique non-empty SHA-256 fingerprints on those
/// messages; `total_bytes` sums the known `attachments.size_bytes` for those
/// fingerprints.
///
/// # Errors
///
/// Returns an internal error when a database statement fails.
async fn export_counts(conn: &mut AnyConnection, export_id: i64) -> Result<ExportCounts, ApiError> {
    let row = sqlx::query(
        "SELECT COUNT(*), COUNT(DISTINCT m.conversation_id)
         FROM vault_export_messages e
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
           FROM vault_export_messages e
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

/// Body of `POST /v1/exports`: the scope, and the tool that asked.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct CreateExportRequest {
    /// What to export.
    pub(crate) scope: ExportScope,
    /// Client/tool name recorded on the run, e.g. `vault-pull`.
    #[serde(default)]
    pub(crate) tool: Option<String>,
}

/// Query string of `GET /v1/exports`.
#[derive(Debug, Deserialize)]
pub(crate) struct ListExportsQuery {
    #[serde(default)]
    pub(crate) status: Option<String>,
    #[serde(default)]
    pub(crate) limit: Option<usize>,
    #[serde(default)]
    pub(crate) offset: Option<usize>,
    #[serde(default)]
    pub(crate) sort: Option<String>,
}

/// Query string of `GET /v1/exports/{id}/messages`.
#[derive(Debug, Deserialize)]
pub(crate) struct ListExportMessagesQuery {
    #[serde(default)]
    pub(crate) limit: Option<usize>,
    #[serde(default)]
    pub(crate) offset: Option<usize>,
    /// `date` or `-date`.
    #[serde(default)]
    pub(crate) sort: Option<String>,
}

/// The account's run with this id, or `not-found`.
async fn owned_export(
    conn: &mut AnyConnection,
    account_id: i64,
    export_id: i64,
) -> Result<ExportRun, ApiError> {
    vault_exports::get_export(conn, account_id, export_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("export {export_id} not found for this account")))
}

/// The account's run with this id while it is still running, or the
/// `state-conflict` that says how it ended.
async fn running_export(
    conn: &mut AnyConnection,
    account_id: i64,
    export_id: i64,
) -> Result<ExportRun, ApiError> {
    let run = owned_export(conn, account_id, export_id).await?;
    if run.status != "running" {
        return Err(ApiError::StateConflict(format!(
            "export {export_id} is not running (status={})",
            run.status
        )));
    }
    Ok(run)
}

/// Close a running run with `status`, answering it as it now stands.
async fn close_export(
    state: &AppState,
    account_id: i64,
    export_id: i64,
    status: &str,
) -> Result<Json<ExportRun>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let run = running_export(&mut conn, account_id, export_id).await?;
    if !vault_exports::finish_export(&mut conn, account_id, export_id, status).await? {
        // Another closer won between the read above and this write.
        return Err(ApiError::StateConflict(format!(
            "export {export_id} is not running (status={})",
            run.status
        )));
    }
    Ok(Json(owned_export(&mut conn, account_id, export_id).await?))
}

/// Start an Export Run: compile the scope, list and count the messages it
/// matches now, and record the run as `running`. Read that list at
/// `GET /v1/exports/{id}/messages`, then close the run with `complete` or
/// `cancel`.
#[utoipa::path(
    post,
    path = "/v1/exports",
    tag = "Export",
    security(("session" = ["export"]), ("api-token" = ["export"])),
    request_body = CreateExportRequest,
    responses(
        (
            status = 201,
            body = ExportRun,
            headers(("Location" = String, description = "Path of the new run"))
        ),
        (status = 400, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem)
    )
)]
pub(crate) async fn create_export(
    State(state): State<AppState>,
    ExportAccess(auth): ExportAccess,
    Json(body): Json<CreateExportRequest>,
) -> Result<Created<ExportRun>, ApiError> {
    let account = auth.account_id;
    let tool = body.tool.as_deref().and_then(message_ir::trimmed);
    let mut conn = state.db.acquire().await?;
    let clock = crate::db::account_profile::account_clock(&mut conn, account).await?;
    let run = start_export_run(&mut conn, account, &body.scope, tool, clock).await?;
    Ok(Created {
        location: format!("/v1/exports/{}", run.id),
        body: run,
    })
}

/// The account's Export Runs as a page, newest first unless `sort` says
/// otherwise, narrowed to one `status` when given.
#[utoipa::path(
    get,
    path = "/v1/exports",
    tag = "Export",
    security(("session" = ["export"]), ("api-token" = ["export"])),
    params(
        ("status" = Option<String>, Query, description = "One of running, completed, failed, cancelled"),
        ("limit" = Option<usize>, Query, description = "Page size, default 40, at most 500"),
        ("offset" = Option<usize>, Query, description = "Rows to skip, at most 50000"),
        ("sort" = Option<String>, Query, description = "`started_at` or `-started_at`. Default `-started_at`, newest first.")
    ),
    responses(
        (status = 200, body = Page<ExportRun>),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem)
    )
)]
pub(crate) async fn list_exports(
    State(state): State<AppState>,
    ExportAccess(auth): ExportAccess,
    Query(query): Query<ListExportsQuery>,
) -> Result<Json<Page<ExportRun>>, ApiError> {
    exports_page(&state, auth.account_id, query).await
}

/// One account's Export Runs as a page. `GET /v1/exports` answers it for the
/// credential's account and `GET /v1/accounts/{id}/exports` for the account
/// named, so the two lists cannot drift.
pub(crate) async fn exports_page(
    state: &AppState,
    account: i64,
    query: ListExportsQuery,
) -> Result<Json<Page<ExportRun>>, ApiError> {
    let page = page_params(
        query.limit,
        query.offset,
        DEFAULT_LIST_LIMIT,
        Some(MAX_LIST_OFFSET),
    )?;
    let order = parse_sort(
        query.sort.as_deref(),
        &EXPORT_SORT_KEYS,
        &DEFAULT_EXPORT_SORT,
    )?;
    let status = query
        .status
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(status) = status
        && !EXPORT_STATUSES.contains(&status)
    {
        return Err(ApiError::validation(format!(
            "status: unknown value '{status}'; accepted values are {}",
            EXPORT_STATUSES.join(", ")
        )));
    }

    let mut conn = state.db.acquire().await?;
    let (items, total) = vault_exports::list_exports_page(
        &mut conn,
        account,
        status,
        &order,
        page.limit as i64,
        page.offset as i64,
    )
    .await?;
    Ok(Json(Page {
        items,
        total,
        limit: page.limit,
        offset: page.offset,
    }))
}

/// One Export Run.
#[utoipa::path(
    get,
    path = "/v1/exports/{id}",
    tag = "Export",
    security(("session" = ["export"]), ("api-token" = ["export"])),
    params(("id" = i64, Path, description = "Export Run id")),
    responses(
        (status = 200, body = ExportRun),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem)
    )
)]
pub(crate) async fn get_export(
    State(state): State<AppState>,
    ExportAccess(auth): ExportAccess,
    AxumPath(export_id): AxumPath<i64>,
) -> Result<Json<ExportRun>, ApiError> {
    let mut conn = state.db.acquire().await?;
    Ok(Json(
        owned_export(&mut conn, auth.account_id, export_id).await?,
    ))
}

/// The messages a running Export Run matched when it was created, a page at
/// a time, oldest first unless `sort` says otherwise. An import, a trash or
/// a new day since creation changes nothing here. A message deleted since
/// leaves its place empty: `total` stays `message_count`, a page can hold
/// fewer than `limit` items, and a client steps `offset` by `limit`. Each
/// page read raises the run's `messages_delivered` to the places reached.
#[utoipa::path(
    get,
    path = "/v1/exports/{id}/messages",
    tag = "Export",
    security(("session" = ["export"]), ("api-token" = ["export"])),
    params(
        ("id" = i64, Path, description = "Export Run id"),
        ("limit" = Option<usize>, Query, description = "Page size, default 40, max 500"),
        ("offset" = Option<usize>, Query, description = "Places to skip in the run's list; a client steps it by `limit`. No cap, an offset past the end is an empty page"),
        ("sort" = Option<String>, Query, description = "`date` or `-date`. Default `date`, oldest first.")
    ),
    responses(
        (status = 200, body = Page<Message>),
        (status = 422, body = crate::problem::Problem),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem),
        (status = 409, body = crate::problem::Problem, description = "The run is no longer running")
    )
)]
pub(crate) async fn list_export_messages(
    State(state): State<AppState>,
    ExportAccess(auth): ExportAccess,
    AxumPath(export_id): AxumPath<i64>,
    Query(query): Query<ListExportMessagesQuery>,
) -> Result<Json<Page<Message>>, ApiError> {
    let account = auth.account_id;
    let page = page_params(query.limit, query.offset, DEFAULT_LIST_LIMIT, None)?;
    let order = parse_sort(
        query.sort.as_deref(),
        &MESSAGE_SORT_KEYS,
        &DEFAULT_MESSAGE_SORT,
    )?;

    let mut conn = state.db.acquire().await?;
    let run = running_export(&mut conn, account, export_id).await?;
    let total = u64::try_from(run.message_count).unwrap_or(0);
    let body = export_messages(
        &mut conn,
        ExportPageOpts {
            export_id,
            total,
            limit: page.limit,
            offset: page.offset,
            order,
        },
    )
    .await?;
    // A page past the end reached nothing, so it moves nothing.
    if (page.offset as u64) < total {
        let reached = (page.offset as u64 + page.limit as u64).min(total);
        let reached = i64::try_from(reached).unwrap_or(i64::MAX);
        vault_exports::record_delivered(&mut conn, account, export_id, reached).await?;
    }
    Ok(Json(body))
}

/// Record that the client finished reading the run.
#[utoipa::path(
    post,
    path = "/v1/exports/{id}/complete",
    tag = "Export",
    security(("session" = ["export"]), ("api-token" = ["export"])),
    params(("id" = i64, Path, description = "Export Run id")),
    responses(
        (status = 200, body = ExportRun),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem),
        (status = 409, body = crate::problem::Problem, description = "The run is already finished")
    )
)]
pub(crate) async fn complete_export(
    State(state): State<AppState>,
    ExportAccess(auth): ExportAccess,
    AxumPath(export_id): AxumPath<i64>,
) -> Result<Json<ExportRun>, ApiError> {
    close_export(&state, auth.account_id, export_id, "completed").await
}

/// Record that the client gave the run up.
#[utoipa::path(
    post,
    path = "/v1/exports/{id}/cancel",
    tag = "Export",
    security(("session" = ["export"]), ("api-token" = ["export"])),
    params(("id" = i64, Path, description = "Export Run id")),
    responses(
        (status = 200, body = ExportRun),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem),
        (status = 409, body = crate::problem::Problem, description = "The run is already finished")
    )
)]
pub(crate) async fn cancel_export(
    State(state): State<AppState>,
    ExportAccess(auth): ExportAccess,
    AxumPath(export_id): AxumPath<i64>,
) -> Result<Json<ExportRun>, ApiError> {
    close_export(&state, auth.account_id, export_id, "cancelled").await
}

#[cfg(test)]
mod tests;
