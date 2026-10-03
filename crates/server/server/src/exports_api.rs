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
use message_crate_api_types::{ExportQueryList, ExportRun, ExportScope, ExportStatus};
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;

use crate::db::conversation_messages::{
    DEFAULT_MESSAGE_SORT, MESSAGE_SORT_KEYS, Message, selection_where,
};
use crate::db::exports::{
    self, DEFAULT_EXPORT_SORT, EXPORT_SORT_KEYS, ExportPageOpts, StartExportArgs, export_messages,
};
use crate::db::ownership::{OwnedTable, missing_ids};
use crate::messages_api::message_filter;
use crate::paging::{DEFAULT_LIST_LIMIT, MAX_LIST_OFFSET, Page, page_params, parse_sort};
use crate::server::{ApiError, AppState, Created, ExportAccess};

/// Most ids one `selection` scope may name in either list, so the `IN` list
/// stays under SQLite's variable cap; the same figure `POST /v1/contacts/summaries`
/// uses.
pub const MAX_SELECTION_IDS: usize = 500;

/// Which of the three forms an Export Run's scope took, without what it
/// asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExportScopeKind {
    /// Everything the account holds.
    Everything,
    /// A query in the search language.
    Query,
    /// Conversations and messages picked by hand.
    Selection,
}

/// An Export Run as the owner reads it under another account: the form of
/// its scope, its tool, times, outcome and counts
/// (`docs/adr/0008-the-owner-holds-no-messages.md`, "What the owner may
/// see"). A query's text is a search over the account's messages, and a
/// selection names its conversations, so neither is here, and a field
/// reaches the owner only by being added here.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct OwnerExportRun {
    /// Export Run id.
    id: i64,
    /// The form of the scope the run asked for.
    scope_kind: ExportScopeKind,
    /// Exporting tool, e.g. `message-crate-pull`, when the client named one.
    tool: Option<String>,
    /// Lifecycle status.
    status: ExportStatus,
    /// UTC time the run started.
    started_at: String,
    /// UTC time the run finished, when it has.
    finished_at: Option<String>,
    /// Messages the scope matched when the run was created.
    message_count: i64,
    /// Distinct conversations with at least one matching message.
    conversation_count: i64,
    /// Distinct attachment fingerprints among the matching messages.
    attachment_count: i64,
    /// Sum of the known sizes of those distinct attachments, in bytes.
    total_bytes: i64,
    /// How far the run's messages have been read, in places.
    messages_delivered: i64,
}

impl From<ExportRun> for OwnerExportRun {
    fn from(run: ExportRun) -> Self {
        let scope_kind = match run.scope {
            ExportScope::Everything => ExportScopeKind::Everything,
            ExportScope::Query { .. } => ExportScopeKind::Query,
            ExportScope::Selection { .. } => ExportScopeKind::Selection,
        };
        Self {
            id: run.id,
            scope_kind,
            tool: run.tool,
            status: run.status,
            started_at: run.started_at,
            finished_at: run.finished_at,
            message_count: run.message_count,
            conversation_count: run.conversation_count,
            attachment_count: run.attachment_count,
            total_bytes: run.total_bytes,
            messages_delivered: run.messages_delivered,
        }
    }
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
    conn: &mut SqliteConnection,
    account_id: i64,
    scope: &ExportScope,
    tool: Option<&str>,
    clock: (chrono_tz::Tz, chrono::NaiveDate),
) -> Result<ExportRun, ApiError> {
    let mut tx = crate::db::begin_write(conn).await?;
    let filter = scope_filter(&mut tx, account_id, scope, clock).await?;
    crate::db::account_profile::ensure_account_row(&mut tx, account_id).await?;
    let export_id = exports::start_export(
        &mut tx,
        &StartExportArgs {
            account_id,
            scope,
            tool,
        },
    )
    .await?;

    exports::list_run_messages(&mut tx, export_id, &filter).await?;
    let counts = exports::export_counts(&mut tx, export_id).await?;
    exports::record_counts(&mut tx, export_id, counts).await?;
    let run = exports::get_export(&mut tx, account_id, export_id)
        .await?
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("export run vanished before commit")))?;
    tx.commit().await?;
    Ok(run)
}

/// Compile an Export Run's scope to the filter every read of it uses.
///
/// `everything` is the empty query; `query` is the search language against
/// the list it names: the matching messages for the Messages list, and every
/// message of the conversations shown for the Conversations list; `selection`
/// is the empty query's defaults narrowed to the picked conversation and
/// message ids, each checked against the account first, so a run can never be
/// created over rows the caller may not read.
///
/// # Errors
///
/// `search-query-invalid` for a query the language refuses;
/// `validation-failed` for a blank query, an empty selection, a selection
/// over [`MAX_SELECTION_IDS`], or an id the account does not hold.
pub async fn scope_filter(
    conn: &mut SqliteConnection,
    account_id: i64,
    scope: &ExportScope,
    clock: (chrono_tz::Tz, chrono::NaiveDate),
) -> Result<crate::search::Filter, ApiError> {
    match scope {
        ExportScope::Everything => message_filter(account_id, "", clock),
        ExportScope::Query { list, q } => {
            if q.trim().is_empty() {
                return Err(ApiError::validation(
                    "scope.q is blank; export everything with {\"kind\": \"everything\"}",
                ));
            }
            match list {
                ExportQueryList::Messages => message_filter(account_id, q, clock),
                ExportQueryList::Conversations => {
                    let (zone, today) = clock;
                    Ok(crate::search::compile_messages_of_conversations(
                        crate::search::CompileRequest {
                            list: crate::search::ListKind::Conversations,
                            query: q,
                            account_id,
                            today,
                            zone,
                        },
                    )?)
                }
            }
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
            let missing_conversations = missing_ids(
                conn,
                OwnedTable::Conversations,
                account_id,
                conversation_ids,
            )
            .await?;
            let missing_messages =
                missing_ids(conn, OwnedTable::Messages, account_id, message_ids).await?;
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

            let (fragment, params) = selection_where(conversation_ids, message_ids);
            Ok(message_filter(account_id, "", clock)?.and_where(&fragment, params))
        }
    }
}

/// Body of `POST /v1/exports`: the scope, and the tool that asked.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct CreateExportRequest {
    /// What to export.
    pub(crate) scope: ExportScope,
    /// Client/tool name recorded on the run, e.g. `message-crate-pull`.
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
    conn: &mut SqliteConnection,
    account_id: i64,
    export_id: i64,
) -> Result<ExportRun, ApiError> {
    exports::get_export(conn, account_id, export_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("export {export_id} not found for this account")))
}

/// The account's run with this id while it is still running, or the
/// `state-conflict` that says how it ended.
async fn running_export(
    conn: &mut SqliteConnection,
    account_id: i64,
    export_id: i64,
) -> Result<ExportRun, ApiError> {
    let run = owned_export(conn, account_id, export_id).await?;
    if run.status != ExportStatus::Running {
        return Err(not_running(export_id, run.status));
    }
    Ok(run)
}

/// The `state-conflict` for a run that has ended with `status`.
fn not_running(export_id: i64, status: ExportStatus) -> ApiError {
    ApiError::StateConflict(format!(
        "export {export_id} is not running (status={status})"
    ))
}

/// Close a running run with `status`, answering it as it now stands.
async fn close_export(
    state: &AppState,
    account_id: i64,
    export_id: i64,
    status: ExportStatus,
) -> Result<Json<ExportRun>, ApiError> {
    let mut conn = state.db.acquire().await?;
    running_export(&mut conn, account_id, export_id).await?;
    if !exports::finish_export(&mut conn, account_id, export_id, status).await? {
        // Another closer won between the read above and this write. The run
        // is read again, so the answer names how it ended.
        let run = owned_export(&mut conn, account_id, export_id).await?;
        return Err(not_running(export_id, run.status));
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
        crate::problem::openapi::SearchQueryInvalid
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
        ("status" = Option<ExportStatus>, Query, description = "Only the runs with this status"),
        ("limit" = Option<usize>, Query, description = "Page size, default 40, at most 500"),
        ("offset" = Option<usize>, Query, description = "Rows to skip, at most 50000"),
        ("sort" = Option<String>, Query, description = "`started_at` or `-started_at`. Default `-started_at`, newest first.")
    ),
    responses(
        (status = 200, body = Page<ExportRun>),
    )
)]
pub(crate) async fn list_exports(
    State(state): State<AppState>,
    ExportAccess(auth): ExportAccess,
    Query(query): Query<ListExportsQuery>,
) -> Result<Json<Page<ExportRun>>, ApiError> {
    let mut conn = state.db.acquire().await?;
    exports_page(&mut conn, auth.account_id, query)
        .await
        .map(Json)
}

/// One account's Export Runs as a page, in full. `GET /v1/exports` answers
/// from it for the credential's account and `GET /v1/accounts/{id}/exports`
/// for the account named, so the two lists cannot drift; the second shapes
/// it for the owner as [`OwnerExportRun`].
pub(crate) async fn exports_page(
    conn: &mut SqliteConnection,
    account: i64,
    query: ListExportsQuery,
) -> Result<Page<ExportRun>, ApiError> {
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
        && ExportStatus::parse(status).is_none()
    {
        return Err(ApiError::validation(format!(
            "status: unknown value '{status}'; accepted values are {}",
            ExportStatus::ALL.map(ExportStatus::as_str).join(", ")
        )));
    }

    let (items, total) = exports::list_exports_page(
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

/// One Export Run.
#[utoipa::path(
    get,
    path = "/v1/exports/{id}",
    tag = "Export",
    security(("session" = ["export"]), ("api-token" = ["export"])),
    params(("id" = i64, Path, description = "Export Run id")),
    responses(
        (status = 200, body = ExportRun),
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
/// fewer than `limit` items, and a client advances `offset` by `limit`. Each
/// page read raises the run's `messages_delivered` to the places reached.
#[utoipa::path(
    get,
    path = "/v1/exports/{id}/messages",
    tag = "Export",
    security(("session" = ["export"]), ("api-token" = ["export"])),
    params(
        ("id" = i64, Path, description = "Export Run id"),
        ("limit" = Option<usize>, Query, description = "Page size, default 40, max 500"),
        ("offset" = Option<usize>, Query, description = "Places to skip in the run's list; a client advances it by `limit`. No cap, an offset past the end is an empty page"),
        ("sort" = Option<String>, Query, description = "`date` or `-date`. Default `date`, oldest first.")
    ),
    responses(
        (status = 200, body = Page<Message>),
        crate::problem::openapi::StateConflict
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
    // A page past the end reached nothing, so it raises nothing, but it is
    // still checked: the run may have closed while the page was read, and a
    // closed run hands nothing over.
    let reached = if (page.offset as u64) < total {
        (page.offset as u64 + page.limit as u64).min(total)
    } else {
        0
    };
    let reached = i64::try_from(reached).unwrap_or(i64::MAX);
    if !exports::record_delivered(&mut conn, account, export_id, reached).await? {
        let run = owned_export(&mut conn, account, export_id).await?;
        return Err(not_running(export_id, run.status));
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
        crate::problem::openapi::StateConflict
    )
)]
pub(crate) async fn complete_export(
    State(state): State<AppState>,
    ExportAccess(auth): ExportAccess,
    AxumPath(export_id): AxumPath<i64>,
) -> Result<Json<ExportRun>, ApiError> {
    close_export(&state, auth.account_id, export_id, ExportStatus::Completed).await
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
        crate::problem::openapi::StateConflict
    )
)]
pub(crate) async fn cancel_export(
    State(state): State<AppState>,
    ExportAccess(auth): ExportAccess,
    AxumPath(export_id): AxumPath<i64>,
) -> Result<Json<ExportRun>, ApiError> {
    close_export(&state, auth.account_id, export_id, ExportStatus::Cancelled).await
}

#[cfg(test)]
mod tests;
