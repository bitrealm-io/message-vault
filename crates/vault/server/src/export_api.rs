//! Export Runs: `POST /v1/exports` records what was asked for and how much
//! matched, `GET /v1/exports/{id}/messages` pages the rows the scope selects,
//! and `complete` or `cancel` closes the run (`docs/agents/http-api-rules.md`,
//! "Runs").
//!
//! Every route here takes the `export` scope on a session or an API token.
//! A program with an export token reads messages only through a run it
//! started, so every read of message data by a program leaves a record.

use crate::extract::{Json, Path as AxumPath, Query};
use axum::extract::State;
use serde::Deserialize;
use sqlx::AnyConnection;
use sqlx::{Executor, Row};
use vault_api_types::{ExportRun, ExportScope};

use crate::db::conversation_messages::{
    DEFAULT_MESSAGE_SORT, MESSAGE_SORT_KEYS, Message, MessageSort, conversation_join_sql,
    load_messages, messages_from_sql,
};
use crate::db::dialect::engine_of;
use crate::db::sql::{SqlParam, bind_all, renumber_placeholders};
use crate::db::vault_exports::{
    self, DEFAULT_EXPORT_SORT, EXPORT_SORT_KEYS, EXPORT_STATUSES, ExportCounts, StartExportArgs,
};
use crate::messages_api::{count_matching_messages, message_filter};
use crate::paging::{
    DEFAULT_EXPORT_LIMIT, DEFAULT_LIST_LIMIT, MAX_LIST_OFFSET, Page, SortKey, page_params,
    parse_sort,
};
use crate::server::{ApiError, AppState, Created, ExportAccess};

/// Most ids one `selection` scope may name in either list, so the `IN` list
/// stays under SQLite's variable cap; the same figure `POST /v1/contacts/summaries`
/// uses.
pub const MAX_SELECTION_IDS: usize = 500;

/// Options for one exported page of messages.
#[derive(Debug, Clone)]
pub struct ExportPageOpts<'a> {
    /// Vault account to export from.
    pub account_id: i64,
    /// What to export.
    pub scope: &'a ExportScope,
    /// Max messages on the page. Already validated by the handler: `1..=MAX_LIST_LIMIT`.
    pub limit: usize,
    /// Row offset.
    pub offset: usize,
    /// The account's time zone and today's date in it: the zone anchors the
    /// date words' boundaries, the day anchors relative dates in a query.
    pub clock: (chrono_tz::Tz, chrono::NaiveDate),
    /// The parsed `sort`; [`DEFAULT_MESSAGE_SORT`] when the caller has none.
    pub order: Vec<SortKey<MessageSort>>,
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

/// One page of the messages a scope selects.
///
/// An offset past the end returns an empty page carrying the true `total`.
///
/// # Errors
///
/// The scope's own failures ([`scope_filter`]), or an internal error when a
/// database statement fails.
pub async fn export_messages(
    conn: &mut AnyConnection,
    opts: ExportPageOpts<'_>,
) -> Result<Page<Message>, ApiError> {
    let filter = scope_filter(conn, opts.account_id, opts.scope, opts.clock).await?;
    let total = count_matching_messages(conn, &filter).await?;

    let messages = load_messages(
        conn,
        filter.where_sql(),
        filter.params(),
        &opts.order,
        opts.limit as u32,
        opts.offset as u32,
    )
    .await?;

    Ok(Page {
        items: messages,
        total,
        limit: opts.limit,
        offset: opts.offset,
    })
}

/// The four counts a run records at creation, for a compiled scope.
///
/// Attachment count is unique non-empty SHA-256 fingerprints on matching
/// messages; `total_bytes` sums the known `attachments.size_bytes` for those
/// fingerprints.
///
/// # Errors
///
/// Returns an internal error when a database statement fails.
pub async fn export_counts(
    conn: &mut AnyConnection,
    filter: &crate::search::Filter,
) -> Result<ExportCounts, ApiError> {
    let params = filter.params();
    let messages = count_matching_messages(conn, filter).await?;

    let conv_sql = format!(
        "SELECT COUNT(DISTINCT c.id)
         {messages_from_sql}
         WHERE {where_sql}",
        messages_from_sql = messages_from_sql(),
        where_sql = filter.where_sql(),
    );
    let conversations: i64 = (&mut *conn)
        .fetch_one(bind_all(&renumber_placeholders(&conv_sql), params))
        .await?
        .try_get(0)?;

    let att_sql = format!(
        "SELECT COUNT(*), COALESCE(SUM(sz), 0)
         FROM (
           SELECT MAX(a.size_bytes) AS sz
           FROM attachments a
           JOIN messages m ON m.id = a.message_id
           {conversation_join_sql}
           WHERE {where_sql}
             AND a.sha256 IS NOT NULL
             AND length(trim(a.sha256)) > 0
           GROUP BY lower(trim(a.sha256))
         )",
        conversation_join_sql = conversation_join_sql(),
        where_sql = filter.where_sql(),
    );
    let row = (&mut *conn)
        .fetch_one(bind_all(&renumber_placeholders(&att_sql), params))
        .await?;
    let (attachments, total_bytes): (i64, i64) = (row.try_get(0)?, row.try_get(1)?);

    Ok(ExportCounts {
        messages: i64::try_from(messages).unwrap_or(i64::MAX),
        conversations: conversations.max(0),
        attachments: attachments.max(0),
        total_bytes: total_bytes.max(0),
    })
}

/// Body of `POST /v1/exports`: the scope, and the tool that asked.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct CreateExportBody {
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
pub(crate) struct ExportMessagesQuery {
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

/// Start an Export Run: compile the scope, count what it matches, and
/// record the run as `running`. Read its messages at
/// `GET /v1/exports/{id}/messages`, then close it with `complete` or `cancel`.
#[utoipa::path(
    post,
    path = "/v1/exports",
    tag = "Export",
    security(("session" = ["export"]), ("api-token" = ["export"])),
    request_body = CreateExportBody,
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
pub(crate) async fn exports_create_handler(
    State(state): State<AppState>,
    ExportAccess(auth): ExportAccess,
    Json(body): Json<CreateExportBody>,
) -> Result<Created<ExportRun>, ApiError> {
    let account = auth.account_id;
    let tool = body.tool.as_deref().and_then(message_ir::trimmed);
    let mut conn = state.db.acquire().await?;
    let clock = crate::db::account_profile::account_clock(&mut conn, account).await?;
    let filter = scope_filter(&mut conn, account, &body.scope, clock).await?;
    let counts = export_counts(&mut conn, &filter).await?;
    crate::db::account_profile::ensure_account_row(&mut conn, account).await?;
    let run = vault_exports::start_export(
        &mut conn,
        &StartExportArgs {
            account_id: account,
            scope: &body.scope,
            tool,
            counts,
        },
    )
    .await?;
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
pub(crate) async fn exports_list_handler(
    State(state): State<AppState>,
    ExportAccess(auth): ExportAccess,
    Query(query): Query<ListExportsQuery>,
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
        auth.account_id,
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
pub(crate) async fn exports_get_handler(
    State(state): State<AppState>,
    ExportAccess(auth): ExportAccess,
    AxumPath(export_id): AxumPath<i64>,
) -> Result<Json<ExportRun>, ApiError> {
    let mut conn = state.db.acquire().await?;
    Ok(Json(
        owned_export(&mut conn, auth.account_id, export_id).await?,
    ))
}

/// The messages a running Export Run's scope selects, a page at a time,
/// oldest first unless `sort` says otherwise. Each page read raises the
/// run's `messages_delivered` to the rows handed over so far.
#[utoipa::path(
    get,
    path = "/v1/exports/{id}/messages",
    tag = "Export",
    security(("session" = ["export"]), ("api-token" = ["export"])),
    params(
        ("id" = i64, Path, description = "Export Run id"),
        ("limit" = Option<usize>, Query, description = "Page size, default 100, max 500"),
        ("offset" = Option<usize>, Query, description = "Page offset; no cap, an offset past the end is an empty page"),
        ("sort" = Option<String>, Query, description = "`date` or `-date`. Default `date`, oldest first.")
    ),
    responses(
        (status = 200, body = Page<Message>),
        (status = 400, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem),
        (status = 409, body = crate::problem::Problem, description = "The run is no longer running")
    )
)]
pub(crate) async fn export_messages_handler(
    State(state): State<AppState>,
    ExportAccess(auth): ExportAccess,
    AxumPath(export_id): AxumPath<i64>,
    Query(query): Query<ExportMessagesQuery>,
) -> Result<Json<Page<Message>>, ApiError> {
    let account = auth.account_id;
    let page = page_params(query.limit, query.offset, DEFAULT_EXPORT_LIMIT, None)?;
    let order = parse_sort(
        query.sort.as_deref(),
        &MESSAGE_SORT_KEYS,
        &DEFAULT_MESSAGE_SORT,
    )?;

    let mut conn = state.db.acquire().await?;
    let run = running_export(&mut conn, account, export_id).await?;
    let clock = crate::db::account_profile::account_clock(&mut conn, account).await?;
    let body = export_messages(
        &mut conn,
        ExportPageOpts {
            account_id: account,
            scope: &run.scope,
            limit: page.limit,
            offset: page.offset,
            clock,
            order,
        },
    )
    .await?;
    // An empty page past the end handed nothing over, so it moves nothing.
    if !body.items.is_empty() {
        let delivered = i64::try_from(page.offset + body.items.len()).unwrap_or(i64::MAX);
        vault_exports::record_delivered(&mut conn, account, export_id, delivered).await?;
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
pub(crate) async fn exports_complete_handler(
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
pub(crate) async fn exports_cancel_handler(
    State(state): State<AppState>,
    ExportAccess(auth): ExportAccess,
    AxumPath(export_id): AxumPath<i64>,
) -> Result<Json<ExportRun>, ApiError> {
    close_export(&state, auth.account_id, export_id, "cancelled").await
}

#[cfg(test)]
mod tests;
