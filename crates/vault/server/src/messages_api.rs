//! `GET /v1/messages`: one row per message matching a query in the search
//! language, paged like every other list.
//!
//! This is a read route, not Export. Opening a conversation is a lookup by id
//! (`GET /v1/conversations/{id}/messages`); searching across messages is a
//! list with a query, and this is that list. The thread's find box uses it
//! with `in:#id`, so a find reaches every message in the conversation rather
//! than whatever page the browser happens to hold (#313).

use crate::extract::{Json, Query};
use axum::extract::State;
use sqlx::AnyConnection;
use sqlx::{Executor, Row};

use crate::db::conversation_messages::{
    DEFAULT_MESSAGE_SORT, MESSAGE_SORT_KEYS, Message, load_messages, messages_from_sql,
};
use crate::db::dialect::engine_of;
use crate::db::engine::DbEngine;
use crate::db::sql::{bind_all, renumber_placeholders};
use crate::paging::{
    DEFAULT_LIST_LIMIT, MAX_LIST_OFFSET, Page, PageQuery, page_params, parse_sort,
};
use crate::server::{ApiError, AppState, FullAccess};

/// Compile a query against the Messages list of the search language.
///
/// # Errors
///
/// Returns a bad-request error when the query does not parse or uses a word
/// the Messages list does not have.
pub(crate) fn message_filter(
    engine: DbEngine,
    account_id: i64,
    query: &str,
    clock: (chrono_tz::Tz, chrono::NaiveDate),
) -> Result<crate::search::Filter, ApiError> {
    let (zone, today) = clock;
    Ok(crate::search::compile(crate::search::CompileRequest {
        list: crate::search::ListKind::Messages,
        query,
        account_id,
        engine,
        today,
        zone,
    })?)
}

/// `COUNT(*)` of the messages a compiled filter matches.
pub(crate) async fn count_matching_messages(
    conn: &mut AnyConnection,
    filter: &crate::search::Filter,
) -> Result<u64, ApiError> {
    let sql = format!(
        "SELECT COUNT(*)
         {messages_from_sql}
         WHERE {where_sql}",
        messages_from_sql = messages_from_sql(),
        where_sql = filter.where_sql(),
    );
    let n: i64 = (&mut *conn)
        .fetch_one(bind_all(&renumber_placeholders(&sql), filter.params()))
        .await?
        .try_get(0)?;
    Ok(n.max(0) as u64)
}

/// Messages matching `q`, oldest first unless `sort` says otherwise: the same
/// rows an Export Run with a `query` scope would hand over, behind a signed-in
/// session with the list defaults and the list's offset ceiling.
#[utoipa::path(
    get,
    path = "/v1/messages",
    tag = "Messages",
    security(("bearer" = [])),
    params(
        ("q" = Option<String>, Query, description = "Search query in the Messages list's words; empty matches every message"),
        ("limit" = Option<usize>, Query, description = "Page size, default 40, max 500"),
        ("offset" = Option<usize>, Query, description = "Page offset, max 50000"),
        ("sort" = Option<String>, Query, description = "`date` or `-date`. Default `date`, oldest first.")
    ),
    responses(
        (status = 200, body = crate::paging::Page<Message>),
        (status = 400, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem)
    )
)]
pub(crate) async fn messages_list_handler(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    Query(query): Query<PageQuery>,
) -> Result<Json<Page<Message>>, ApiError> {
    let page = page_params(
        query.limit,
        query.offset,
        DEFAULT_LIST_LIMIT,
        Some(MAX_LIST_OFFSET),
    )?;
    let mut conn = state.db.acquire().await?;
    let clock = crate::db::account_profile::account_clock(&mut conn, auth.account_id).await?;
    let filter = message_filter(
        engine_of(&conn),
        auth.account_id,
        query.q.as_deref().unwrap_or(""),
        clock,
    )?;
    let order = parse_sort(
        query.sort.as_deref(),
        &MESSAGE_SORT_KEYS,
        &DEFAULT_MESSAGE_SORT,
    )?;
    let total = count_matching_messages(&mut conn, &filter).await?;
    let items = load_messages(
        &mut conn,
        filter.where_sql(),
        filter.params(),
        &order,
        page.limit as u32,
        page.offset as u32,
    )
    .await?;
    Ok(Json(Page {
        items,
        total,
        limit: page.limit,
        offset: page.offset,
    }))
}

#[cfg(test)]
mod tests;
