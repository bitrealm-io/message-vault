//! The `/v1/conversations` routes: the list, one conversation, its sources
//! and messages, and moving it to the trash, restoring it, and deleting it
//! for good. The queries are in `db::conversations`,
//! `db::conversation_messages`, and `db::trash`.

use std::sync::Arc;

use crate::extract::{Json, Path as AxumPath, Query};
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;

use crate::db::conversation_messages::{
    DEFAULT_MESSAGE_SORT, MESSAGE_SORT_KEYS, Message, MessageWindow, get_conversation_messages,
};
use crate::db::conversations::{
    CONVERSATION_SORT_KEYS, ConversationSource, ConversationSummary, DEFAULT_CONVERSATION_SORT,
    get_conversation_summary, list_conversation_source_stats, list_conversations_sorted,
};
use crate::db::trash::{DeleteOutcome, Trashable, delete_trashed, move_to_trash, restore};
use crate::paging::{
    DEFAULT_LIST_LIMIT, ListRequest, Page, PageQuery, page_of, page_params, parse_sort,
};
use crate::server::{ApiError, AppState, FullAccess, FullDeleteAccess};
use crate::trash_api::remove_orphaned_files;

/// Page through conversations with participants, message counts, and tags.
/// Newest activity first unless `sort` says otherwise.
#[utoipa::path(
    get,
    path = "/v1/conversations",
    tag = "Conversations",
    security(("session" = [])),
    params(
        ("q" = Option<String>, Query, description = "Conversation search; empty lists all non-trashed"),
        ("limit" = Option<usize>, Query, description = "Page size, default 40, max 500"),
        ("offset" = Option<usize>, Query, description = "Page offset, max 50000"),
        ("sort" = Option<String>, Query, description = "Comma-separated keys, `-` for descending: `date` (last message) or `messages` (message count). Default `-date`.")
    ),
    responses(
        (status = 200, body = crate::paging::Page<ConversationSummary>),
        crate::problem::openapi::SearchQueryInvalid
    )
)]
pub(crate) async fn list_conversations(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    Query(query): Query<PageQuery>,
) -> Result<Json<Page<ConversationSummary>>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let list = ListRequest::read(
        &mut conn,
        auth.account_id,
        query,
        &CONVERSATION_SORT_KEYS,
        &DEFAULT_CONVERSATION_SORT,
    )
    .await?;
    let result = list_conversations_sorted(
        &mut conn,
        auth.account_id,
        &list.q,
        &list.order,
        list.page.limit,
        list.page.offset,
        list.clock,
    )
    .await?;
    Ok(Json(result))
}

/// One conversation, in the same shape a list row already has — so a caller
/// that opens a conversation from a list does not have to convert between two
/// shapes, and paging through the whole list to find one id is never
/// necessary. Trash is a property the list applies, not a bar to reading:
/// a trashed conversation still answers here.
#[utoipa::path(
    get,
    path = "/v1/conversations/{id}",
    tag = "Conversations",
    security(("session" = [])),
    params(("id" = i64, Path, description = "Conversation id")),
    responses(
        (status = 200, body = ConversationSummary),
    )
)]
pub(crate) async fn get_conversation(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    AxumPath(conversation_id): AxumPath<i64>,
) -> Result<Json<ConversationSummary>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let conversation =
        get_conversation_summary(&mut conn, auth.account_id, conversation_id).await?;
    conversation
        .map(Json)
        .ok_or_else(|| ApiError::NotFound("conversation not found".into()))
}

/// Per-backup message counts for one conversation (the Sources panel).
#[utoipa::path(
    get,
    path = "/v1/conversations/{id}/sources",
    tag = "Conversations",
    security(("session" = [])),
    params(
        ("id" = i64, Path, description = "Conversation id"),
        ("limit" = Option<usize>, Query, description = "Page size, default 40, max 500"),
        ("offset" = Option<usize>, Query, description = "Page offset")
    ),
    responses(
        (status = 200, body = crate::paging::Page<ConversationSource>),
    )
)]
pub(crate) async fn list_conversation_sources(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    AxumPath(conversation_id): AxumPath<i64>,
    Query(query): Query<PageQuery>,
) -> Result<Json<Page<ConversationSource>>, ApiError> {
    let params = page_params(query.limit, query.offset, DEFAULT_LIST_LIMIT, None)?;
    let mut conn = state.db.acquire().await?;
    let rows = list_conversation_source_stats(&mut conn, auth.account_id, conversation_id).await?;
    rows.map(|rows| Json(page_of(rows, params)))
        .ok_or_else(|| ApiError::NotFound("conversation not found".into()))
}

/// Query string for a conversation's messages.
#[derive(Debug, Deserialize)]
pub(crate) struct ListConversationMessagesQuery {
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    sort: Option<String>,
    #[serde(default)]
    around: Option<i64>,
    #[serde(default)]
    before: Option<i64>,
    #[serde(default)]
    after: Option<i64>,
}

impl ListConversationMessagesQuery {
    /// Where the page sits: at `offset`, or beside the message `around`,
    /// `before` or `after` names. At most one of the four may be sent,
    /// because each one alone says where the page starts.
    fn window(&self) -> Result<MessageWindow, ApiError> {
        let sent: Vec<&str> = [
            ("offset", self.offset.is_some()),
            ("around", self.around.is_some()),
            ("before", self.before.is_some()),
            ("after", self.after.is_some()),
        ]
        .into_iter()
        .filter_map(|(name, sent)| sent.then_some(name))
        .collect();
        if sent.len() > 1 {
            return Err(ApiError::validation(format!(
                "{} each say where the page starts; send at most one of offset, around, before and after",
                sent.join(" and ")
            )));
        }
        Ok(match (self.around, self.before, self.after) {
            (Some(id), _, _) => MessageWindow::Around(id),
            (_, Some(id), _) => MessageWindow::Before(id),
            (_, _, Some(id)) => MessageWindow::After(id),
            _ => MessageWindow::Offset(self.offset.unwrap_or(0)),
        })
    }
}

/// A conversation's messages, ascending by timestamp then `sort_order`. The
/// read path a screen uses to open a conversation: no search query to compose,
/// just the conversation id.
///
/// The page starts at `offset`, or beside one message: `around` puts the
/// message in the middle of the page, and `before` and `after` answer the
/// messages just before or just after it in the page's order, without it.
/// The answer's `offset` says where the page sits either way, so a screen
/// that jumps to a message (a search result, a Find match, the first message
/// of a year) can keep reading in both directions from there. `offset` has
/// no cap: every message of a long conversation must be reachable.
#[utoipa::path(
    get,
    path = "/v1/conversations/{id}/messages",
    tag = "Conversations",
    security(("session" = [])),
    params(
        ("id" = i64, Path, description = "Conversation id"),
        ("limit" = Option<usize>, Query, description = "Page size, default 40, max 500"),
        ("offset" = Option<usize>, Query, description = "Page offset, no maximum. Not with `around`, `before` or `after`."),
        ("sort" = Option<String>, Query, description = "`date` or `-date`. Default `date`, oldest first."),
        ("around" = Option<i64>, Query, description = "Message id: the page with this message in the middle. Not with `offset`, `before` or `after`."),
        ("before" = Option<i64>, Query, description = "Message id: the page just before this message in the page's order, without it. Not with `offset`, `around` or `after`."),
        ("after" = Option<i64>, Query, description = "Message id: the page just after this message in the page's order, without it. Not with `offset`, `around` or `before`.")
    ),
    responses(
        (status = 200, body = crate::paging::Page<message_crate_api_types::Message>),
    )
)]
pub(crate) async fn list_conversation_messages(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    AxumPath(conversation_id): AxumPath<i64>,
    Query(query): Query<ListConversationMessagesQuery>,
) -> Result<Json<Page<Message>>, ApiError> {
    let page = page_params(query.limit, query.offset, DEFAULT_LIST_LIMIT, None)?;
    let order = parse_sort(
        query.sort.as_deref(),
        &MESSAGE_SORT_KEYS,
        &DEFAULT_MESSAGE_SORT,
    )?;
    let window = query.window()?;
    let mut conn = state.db.acquire().await?;
    let result = get_conversation_messages(
        &mut conn,
        auth.account_id,
        conversation_id,
        &order,
        page.limit,
        window,
    )
    .await?;
    result
        .map(Json)
        .ok_or_else(|| ApiError::NotFound("conversation not found".into()))
}

/// Put a conversation in the trash. Idempotent: trashing an
/// already-trashed conversation still answers 204.
#[utoipa::path(
    post,
    path = "/v1/conversations/{id}/trash",
    tag = "Conversations",
    security(("session" = [])),
    params(("id" = i64, Path, description = "Conversation id")),
    responses(
        (status = 204, description = "Trashed"),
    )
)]
pub(crate) async fn trash_conversation(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    AxumPath(conversation_id): AxumPath<i64>,
) -> Result<StatusCode, ApiError> {
    let mut conn = state.db.acquire().await?;
    if move_to_trash(
        &mut conn,
        auth.account_id,
        Trashable::Conversation(conversation_id),
    )
    .await?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound("conversation not found".into()))
    }
}

/// Take a conversation out of the trash. Idempotent: restoring a
/// conversation that was not trashed still answers 204.
#[utoipa::path(
    post,
    path = "/v1/conversations/{id}/restore",
    tag = "Conversations",
    security(("session" = [])),
    params(("id" = i64, Path, description = "Conversation id")),
    responses(
        (status = 204, description = "Restored"),
    )
)]
pub(crate) async fn restore_conversation(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    AxumPath(conversation_id): AxumPath<i64>,
) -> Result<StatusCode, ApiError> {
    let mut conn = state.db.acquire().await?;
    if restore(
        &mut conn,
        auth.account_id,
        Trashable::Conversation(conversation_id),
    )
    .await?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound("conversation not found".into()))
    }
}

/// Permanently delete a trashed conversation: the conversation, its
/// messages, and any attachment file no other message still uses. Trash is
/// the only door to deletion, so a conversation that is not in the trash
/// answers 409 rather than being deleted from wherever it was.
#[utoipa::path(
    delete,
    path = "/v1/conversations/{id}",
    tag = "Conversations",
    security(("session" = ["delete"])),
    params(("id" = i64, Path, description = "Conversation id")),
    responses(
        (status = 204, description = "Deleted"),
        crate::problem::openapi::StateConflict
    )
)]
pub(crate) async fn delete_conversation(
    State(state): State<AppState>,
    FullDeleteAccess(auth): FullDeleteAccess,
    AxumPath(conversation_id): AxumPath<i64>,
) -> Result<StatusCode, ApiError> {
    let outcome = {
        let mut conn = state.db.acquire().await?;
        delete_trashed(
            &mut conn,
            auth.account_id,
            Trashable::Conversation(conversation_id),
        )
        .await?
    };
    match outcome {
        DeleteOutcome::Deleted(orphaned) => {
            remove_orphaned_files(Arc::clone(&state.cfg), auth.account_id, orphaned).await?;
            Ok(StatusCode::NO_CONTENT)
        }
        DeleteOutcome::NotOwned => Err(ApiError::NotFound("conversation not found".into())),
        DeleteOutcome::NotTrashed => Err(ApiError::StateConflict(
            "the conversation is not in the trash; move it to the trash first".into(),
        )),
    }
}

#[cfg(test)]
mod tests;
