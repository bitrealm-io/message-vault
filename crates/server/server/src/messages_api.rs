//! `GET /v1/messages`: one row per message matching a query in the search
//! language, paged like every other list.
//!
//! This is a read route, not Export. Opening a conversation is a lookup by id
//! (`GET /v1/conversations/{id}/messages`); searching across messages is a
//! list with a query, and this is that list. The thread's find box uses it
//! with `in:#id`, so a find reaches every message in the conversation rather
//! than whatever page the browser happens to hold (#313).

use crate::extract::{Json, Path, Query};
use axum::extract::State;

use crate::db::conversation_messages::{
    DEFAULT_MESSAGE_SORT, DEFAULT_SEARCH_SORT, Message, SEARCH_SORT_KEYS, count_matching_messages,
    load_messages, load_search_page,
};
use crate::db::sql::SqlParam;
use crate::paging::{ListRequest, Page, PageQuery};
use crate::server::{ApiError, AppState, FullAccess};

/// Compile a query against the Messages list of the search language.
///
/// # Errors
///
/// Returns a `422 Unprocessable Entity` search-query-invalid problem when the
/// query does not parse or uses a word the Messages list does not have.
pub(crate) fn message_filter(
    account_id: i64,
    query: &str,
    clock: (chrono_tz::Tz, chrono::NaiveDate),
) -> Result<crate::search::Filter, ApiError> {
    let (zone, today) = clock;
    Ok(crate::search::compile(crate::search::CompileRequest {
        list: crate::search::ListKind::Messages,
        query,
        account_id,
        today,
        zone,
    })?)
}

/// Messages matching `q`, oldest first unless `sort` says otherwise: the same
/// rows an Export Run with a `query` scope would hand over, behind a logged-in
/// session with the list defaults and the list's offset ceiling.
///
/// `sort=relevance` puts the best match first, ranked by the full-text
/// index's `bm25()` on the query's free-text words: the words not behind `-`
/// or `not`. A query with no such word has nothing to rank by, and
/// `relevance` is then `validation-failed`. Ties, and a message found only by
/// an attachment's file name, which the index does not rank, follow by date,
/// newest first.
#[utoipa::path(
    get,
    path = "/v1/messages",
    tag = "Messages",
    security(("session" = [])),
    params(
        ("q" = Option<String>, Query, description = "Search query in the Messages list's words; empty matches every message"),
        ("limit" = Option<usize>, Query, description = "Page size, default 40, max 500"),
        ("offset" = Option<usize>, Query, description = "Page offset, max 50000"),
        ("sort" = Option<String>, Query, description = "`date`, `-date` or `relevance` (best match first; needs a free-text word in `q`). Default `date`, oldest first.")
    ),
    responses(
        (status = 200, body = crate::paging::Page<Message>),
        crate::problem::openapi::SearchQueryInvalid
    )
)]
pub(crate) async fn list_messages(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    Query(query): Query<PageQuery>,
) -> Result<Json<Page<Message>>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let list = ListRequest::read(
        &mut conn,
        auth.account_id,
        query,
        &SEARCH_SORT_KEYS,
        &DEFAULT_SEARCH_SORT,
    )
    .await?;
    let filter = message_filter(auth.account_id, &list.q, list.clock)?;
    let items = load_search_page(
        &mut conn,
        &filter,
        &list.order,
        list.page.limit,
        list.page.offset,
    )
    .await?;
    let total = count_matching_messages(&mut conn, &filter).await?;
    Ok(Json(Page {
        items,
        total,
        limit: list.page.limit,
        offset: list.page.offset,
    }))
}

/// One message by id, in the shape the Messages list gives each row.
///
/// Read-only. A message is never written through this route: an import
/// writes messages, and trashing is a conversation operation
/// (`docs/architecture/http-api.md`, "Methods"). The id is enough: an id
/// names one message, and a lookup by id does not depend on how it was
/// found, so the route returns any message of the caller's account, one in
/// a trashed conversation and a duplicate included, and takes no `q`.
/// Another account's message is `404`.
#[utoipa::path(
    get,
    path = "/v1/messages/{id}",
    tag = "Messages",
    security(("session" = [])),
    params(("id" = i64, Path, description = "Message id")),
    responses(
        (status = 200, body = Message),
    )
)]
pub(crate) async fn get_message(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    Path(message_id): Path<i64>,
) -> Result<Json<Message>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let mut items = load_messages(
        &mut conn,
        "m.account_id = ? AND m.id = ?",
        &[SqlParam::Int(auth.account_id), SqlParam::Int(message_id)],
        &DEFAULT_MESSAGE_SORT,
        1,
        0,
    )
    .await?;
    items
        .pop()
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("no message {message_id}")))
}

#[cfg(test)]
mod tests;
