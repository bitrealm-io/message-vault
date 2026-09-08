//! Read-only message export query used by `GET /v1/export/messages`
//! and `GET /v1/export/messages/count`.

use crate::extract::{Json, Query};
use axum::extract::State;
use serde::{Deserialize, Serialize};
use sqlx::AnyConnection;
use sqlx::{Executor, Row};

use crate::db::conversation_messages::{
    Message, conversation_join_sql, load_messages, messages_from_sql,
};
use crate::db::dialect::engine_of;
use crate::db::sql::{bind_all, renumber_placeholders};
use crate::messages_api::{count_matching_messages, message_filter};
use crate::server::{ApiError, AppState, ExportAccess, resolve_import_account};

use crate::paging::{DEFAULT_EXPORT_LIMIT, Page, page_params};

/// Options for one exported page of messages.
#[derive(Debug, Clone)]
pub struct ExportPageOpts<'a> {
    /// Vault account to export from.
    pub account_id: &'a str,
    /// Search query string, in the search language.
    pub query: &'a str,
    /// Max messages on the page. Already validated by the handler: `1..=MAX_LIST_LIMIT`.
    pub limit: usize,
    /// Row offset.
    pub offset: usize,
    /// The account's time zone and today's date in it: the zone anchors the
    /// date words' boundaries, the day anchors relative dates in `query`.
    pub clock: (chrono_tz::Tz, chrono::NaiveDate),
}

/// Options for one export count query.
#[derive(Debug, Clone)]
pub struct ExportCountOpts<'a> {
    /// Vault account to count from.
    pub account_id: &'a str,
    /// Search query string, in the search language.
    pub query: &'a str,
    /// The account's time zone and today's date in it.
    pub clock: (chrono_tz::Tz, chrono::NaiveDate),
}

/// Match counts for an export query.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ExportCountResponse {
    /// Matching messages.
    pub messages: u64,
    /// Distinct conversations with at least one matching message.
    pub conversations: u64,
    /// Unique attachment fingerprints among matching messages.
    pub attachments: u64,
    /// Sum of known `size_bytes` for those unique fingerprints (unknown sizes omitted).
    pub total_bytes: u64,
}

/// Export messages matching a query in the search language, a page at a time.
///
/// An empty query returns every non-trashed, non-duplicate message for the account.
/// An offset past the end returns an empty page carrying the true `total`.
///
/// # Errors
///
/// Returns a bad-request error for an invalid query, or an internal
/// error when a database statement fails.
pub async fn export_messages(
    conn: &mut AnyConnection,
    opts: ExportPageOpts<'_>,
) -> Result<Page<Message>, ApiError> {
    let filter = message_filter(engine_of(conn), opts.account_id, opts.query, opts.clock)?;
    let total = count_matching_messages(conn, &filter).await?;

    let messages = load_messages(
        conn,
        filter.where_sql(),
        filter.params(),
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

/// Aggregate counts for messages matching a query in the search language (no paging).
///
/// Attachment count is unique non-empty SHA-256 fingerprints (a short
/// fingerprint of the file contents) on matching messages.
/// `total_bytes` sums known `attachments.size_bytes` for those fingerprints.
///
/// # Errors
///
/// Returns a bad-request error for an invalid query, or an internal error when
/// a database statement fails.
pub async fn export_message_count(
    conn: &mut AnyConnection,
    opts: ExportCountOpts<'_>,
) -> Result<ExportCountResponse, ApiError> {
    let filter = message_filter(engine_of(conn), opts.account_id, opts.query, opts.clock)?;
    let params = filter.params();

    let messages = count_matching_messages(conn, &filter).await?;

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

    Ok(ExportCountResponse {
        messages,
        conversations: conversations.max(0) as u64,
        attachments: attachments.max(0) as u64,
        total_bytes: total_bytes.max(0) as u64,
    })
}

#[derive(Debug, Deserialize)]
pub(crate) struct ExportMessagesQuery {
    #[serde(default)]
    pub(crate) q: String,
    #[serde(default)]
    pub(crate) limit: Option<usize>,
    #[serde(default)]
    pub(crate) offset: Option<usize>,
    #[serde(default)]
    pub(crate) account: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ExportMessagesCountQuery {
    #[serde(default)]
    q: String,
    #[serde(default)]
    account: Option<String>,
}

/// Count messages, conversations, and attachment fingerprints matching a
/// query.
#[utoipa::path(
    get,
    path = "/v1/export/messages/count",
    tag = "Export",
    security(("bearer" = [])),
    params(
        ("q" = String, Query, description = "Query in the search language; empty is every non-trashed message"),
        ("account" = Option<String>, Query)
    ),
    responses(
        (status = 200, body = crate::export_api::ExportCountResponse),
        (status = 400, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem)
    )
)]
pub(crate) async fn export_messages_count_handler(
    State(state): State<AppState>,
    ExportAccess(auth): ExportAccess,
    Query(query): Query<ExportMessagesCountQuery>,
) -> Result<Json<ExportCountResponse>, ApiError> {
    let account = resolve_import_account(&auth, query.account.as_deref(), &state.db).await?;
    let q = query.q;

    let mut conn = state.db.acquire().await?;
    let clock = crate::db::account_profile::account_clock(&mut conn, &account).await?;
    let body = export_message_count(
        &mut conn,
        ExportCountOpts {
            account_id: &account,
            query: &q,
            clock,
        },
    )
    .await?;
    Ok(Json(body))
}

/// Export messages matching a query in the search language, a page at a time.
#[utoipa::path(
    get,
    path = "/v1/export/messages",
    tag = "Export",
    security(("bearer" = [])),
    params(
        ("q" = String, Query, description = "Query in the search language; empty is every non-trashed message"),
        ("limit" = Option<usize>, Query, description = "Page size, default 100, max 500"),
        ("offset" = Option<usize>, Query, description = "Page offset; no cap, an offset past the end is an empty page"),
        ("account" = Option<String>, Query)
    ),
    responses(
        (status = 200, body = crate::paging::Page<vault_api_types::Message>),
        (status = 400, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem)
    )
)]
pub(crate) async fn export_messages_handler(
    State(state): State<AppState>,
    ExportAccess(auth): ExportAccess,
    Query(query): Query<ExportMessagesQuery>,
) -> Result<Json<Page<Message>>, ApiError> {
    let account = resolve_import_account(&auth, query.account.as_deref(), &state.db).await?;
    let page = page_params(query.limit, query.offset, DEFAULT_EXPORT_LIMIT, None)?;

    let mut conn = state.db.acquire().await?;
    let clock = crate::db::account_profile::account_clock(&mut conn, &account).await?;
    let body = export_messages(
        &mut conn,
        ExportPageOpts {
            account_id: &account,
            query: &query.q,
            limit: page.limit,
            offset: page.offset,
            clock,
        },
    )
    .await?;
    Ok(Json(body))
}

#[cfg(test)]
mod tests;
