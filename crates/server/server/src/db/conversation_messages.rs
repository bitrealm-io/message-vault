//! The message row loader shared by every route that reads messages: their
//! conversation, attachments and tapbacks, joined and grouped.
//!
//! The row shapes themselves live in `message-crate-api-types`, where `message-crate-pull`
//! reads them from the same definition rather than a hand-written mirror.
//!
//! `load_messages` takes an already-compiled `WHERE` fragment and its bound
//! params, so the caller decides what selects the rows — a search query, a
//! conversation id — while this module owns only the row shape and how it is
//! assembled. The counts and `WHERE` fragments those callers page with live
//! here too: the count a search's matches make, a conversation's page, and
//! the fragment an Export Run's `selection` scope adds.

use std::collections::HashMap;

use sqlx::SqliteConnection;
use sqlx::{Executor, Row};

pub use message_crate_api_types::{Attachment, Message, MessageConversation, Tapback};

use crate::db::ownership::owns_conversation;
use crate::db::participant_names::load_for_conversations;
use crate::db::sql::{SqlParam, bind_all, bind_args, group_rows_by_id};
use crate::paging::{Direction, Page, SortKey};
use crate::server::ApiError;

/// Sorted, deduplicated ids for an `IN` list.
fn unique_ids(ids: impl IntoIterator<Item = i64>) -> Vec<i64> {
    let mut ids: Vec<i64> = ids.into_iter().collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

struct RawRow {
    id: i64,
    conversation_id: i64,
    source: String,
    service: Option<String>,
    guid: Option<String>,
    timestamp: String,
    sort_order: i64,
    is_from_me: bool,
    sender: Option<String>,
    owner: Option<String>,
    subject: Option<String>,
    body: Option<String>,
    is_announcement: bool,
    is_reply: bool,
    thread_originator_guid: Option<String>,
    thread_originator_part: Option<i64>,
    num_replies: i64,
    chat_identifier: String,
    conversation_type: String,
    group_title: Option<String>,
}

/// FROM clause for message queries. The compiled filter mentions only `m`;
/// these joins are here for the SELECT list, which reports the conversation
/// and the three handles' raw text.
///
/// Export's count statements carry the same joins, because a search filter can
/// name a conversation column and would not compile against `messages` alone.
/// The conversation read route's count is `FROM messages m` with no joins: its
/// filter is a conversation id, an account id, and `duplicate_of`, all on
/// `m`. The two still count the same rows, because
/// `conversations.chat_handle_id` is `NOT NULL` with a foreign key to
/// `handles`, so the one inner join here never drops a row (`hs` and `ho` are
/// `LEFT JOIN`s and cannot drop one either).
pub(crate) fn messages_from_sql() -> String {
    format!("FROM messages m\n{}", conversation_join_sql())
}

/// Handles joins for a query already anchored on `messages m`.
/// `hc` supplies `c.chat_handle_id` raw text; `hs` supplies `m.sender_handle_id`
/// raw text (LEFT, since outgoing messages carry no sender handle); `ho`
/// supplies `m.owner_handle_id` raw text (LEFT, since a backup that names no
/// owner leaves it empty).
pub(crate) fn conversation_join_sql() -> String {
    "JOIN conversations c ON c.id = m.conversation_id
     JOIN handles hc ON hc.id = c.chat_handle_id
     LEFT JOIN handles hs ON hs.id = m.sender_handle_id
     LEFT JOIN handles ho ON ho.id = m.owner_handle_id"
        .into()
}

/// The one key every message list accepts in `sort=`: `date`, the message's
/// timestamp, with `sort_order` and `id` breaking ties the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageSort {
    /// The message's timestamp, ties broken by `sort_order` then `id`.
    Date,
}

/// The accepted keys, as `sort=` spells them.
pub const MESSAGE_SORT_KEYS: [(&str, MessageSort); 1] = [("date", MessageSort::Date)];

/// Oldest first: what every message list shows when `sort` is absent, so a
/// conversation reads top to bottom.
pub const DEFAULT_MESSAGE_SORT: [SortKey<MessageSort>; 1] = [SortKey {
    key: MessageSort::Date,
    direction: Direction::Asc,
}];

/// Load the message rows an already-compiled filter matches, joined with
/// their conversation, attachments and tapbacks.
///
/// `where_sql` and `params` are the caller's compiled `WHERE` fragment (a
/// search query, a conversation id) with placeholders in bind order; this
/// function appends the `ORDER BY`/`LIMIT`/`OFFSET` and does not touch the
/// total count, which stays the caller's job.
///
/// # Errors
///
/// Returns an error when a database statement fails.
pub async fn load_messages(
    conn: &mut SqliteConnection,
    where_sql: &str,
    params: &[SqlParam],
    order: &[SortKey<MessageSort>],
    limit: usize,
    offset: usize,
) -> Result<Vec<Message>, ApiError> {
    let direction = order
        .iter()
        .find(|k| k.key == MessageSort::Date)
        .map_or(Direction::Asc, |k| k.direction)
        .sql();
    load_messages_from(
        conn,
        &messages_from_sql(),
        where_sql,
        params,
        &format!("m.timestamp {direction}, m.sort_order {direction}, m.id {direction}"),
        limit,
        offset,
    )
    .await
}

/// The keys the Messages list, `GET /v1/messages`, accepts in `sort=`. It
/// has one more than a conversation's messages: a search can rank its
/// matches, and a conversation read in order cannot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchSort {
    /// The message's timestamp, as [`MessageSort::Date`].
    Date,
    /// How well the message matches the query's free-text words, best first:
    /// the full-text index's `bm25()`.
    Relevance,
}

/// The Messages list's keys, as `sort=` spells them.
pub const SEARCH_SORT_KEYS: [(&str, SearchSort); 2] = [
    ("date", SearchSort::Date),
    ("relevance", SearchSort::Relevance),
];

/// Oldest first, as every message list reads when `sort` is absent.
pub const DEFAULT_SEARCH_SORT: [SortKey<SearchSort>; 1] = [SortKey {
    key: SearchSort::Date,
    direction: Direction::Asc,
}];

/// The join a relevance order ranks by: every message the rank query
/// matches, with its `bm25()`, keyed by message id. Its one `?` is the rank
/// query.
///
/// `MATERIALIZED` is load-bearing. Without it SQLite (3.53) flattens the
/// subquery into the outer query and asks the full-text index once per
/// candidate message, `rowid = m.id AND MATCH ?`, the per-row cost #413
/// removed from the filter: 22 s instead of 0.1 s for `the` on the medium
/// Demo Account. Materialized, the index is asked once for the whole search.
const RANK_JOIN_SQL: &str = "
     LEFT JOIN (WITH ranked AS MATERIALIZED (
                  SELECT rowid AS rank_id, bm25(messages_fts) AS rank
                  FROM messages_fts WHERE messages_fts MATCH ?)
                SELECT rank_id, rank FROM ranked) r ON r.rank_id = m.id";

/// One page of the messages a search matches, in `order`.
///
/// A `relevance` key ranks by `bm25()` over `filter`'s rank query, read once
/// for the whole search through [`RANK_JOIN_SQL`] rather than per row (#413). A
/// message the filter matches without the index, by an attachment's file
/// name, has no rank and comes after every ranked one. Ties, and every
/// message under a date-only order, fall back to the date, newest first
/// after a relevance key, and then to `sort_order` and `id`.
///
/// # Errors
///
/// `validation-failed` when `order` names `relevance` and the filter has no
/// free-text word to rank by, or names `-relevance`, which has no meaning;
/// otherwise an error when a statement fails.
pub async fn load_search_page(
    conn: &mut SqliteConnection,
    filter: &crate::search::Filter,
    order: &[SortKey<SearchSort>],
    limit: usize,
    offset: usize,
) -> Result<Vec<Message>, ApiError> {
    let (sql, params) = search_page_sql(filter, order, limit, offset)?;
    fetch_message_page(conn, &sql, &params).await
}

/// The statement [`load_search_page`] runs, and its parameters.
///
/// # Errors
///
/// As [`load_search_page`], for a sort it refuses.
pub(crate) fn search_page_sql(
    filter: &crate::search::Filter,
    order: &[SortKey<SearchSort>],
    limit: usize,
    offset: usize,
) -> Result<(String, Vec<SqlParam>), ApiError> {
    if order
        .iter()
        .any(|k| k.key == SearchSort::Relevance && k.direction == Direction::Desc)
    {
        return Err(ApiError::validation(
            "sort: relevance has one direction, best match first; write `relevance`, not `-relevance`",
        ));
    }

    let ranked = order.iter().any(|k| k.key == SearchSort::Relevance);
    let mut from_sql = messages_from_sql();
    let mut params = Vec::new();
    if ranked {
        let Some(rank_query) = filter.rank_query() else {
            return Err(ApiError::validation(
                "sort: relevance needs a free-text word in q to rank by; sort by date instead",
            ));
        };
        from_sql.push_str(RANK_JOIN_SQL);
        params.push(SqlParam::Text(rank_query.to_string()));
    }
    params.extend_from_slice(filter.params());

    let mut terms = Vec::new();
    let mut date_direction = None;
    for key in order {
        match key.key {
            // `bm25()` is lower for a better match, so best first is
            // ascending, and an unranked message (NULL) comes last.
            SearchSort::Relevance => terms.push("r.rank IS NULL ASC, r.rank ASC".to_string()),
            SearchSort::Date => {
                let d = key.direction.sql();
                terms.push(format!("m.timestamp {d}, m.sort_order {d}"));
                date_direction = Some(key.direction);
            }
        }
    }
    let tie = date_direction.unwrap_or(Direction::Desc);
    if date_direction.is_none() {
        let d = tie.sql();
        terms.push(format!("m.timestamp {d}, m.sort_order {d}"));
    }
    terms.push(format!("m.id {}", tie.sql()));

    Ok(message_page_sql(
        &from_sql,
        filter.where_sql(),
        &params,
        &terms.join(", "),
        limit,
        offset,
    ))
}

/// [`load_messages`] with the caller's own `FROM` clause and `ORDER BY`.
///
/// `from_sql` must bind `messages m` and carry [`conversation_join_sql`],
/// because the `SELECT` list reads `c`, `hc`, `hs` and `ho`. An Export Run uses it
/// to page the message ids it stored at creation, in the order it stored them.
///
/// # Errors
///
/// Returns an error when a database statement fails.
pub(crate) async fn load_messages_from(
    conn: &mut SqliteConnection,
    from_sql: &str,
    where_sql: &str,
    params: &[SqlParam],
    order_by: &str,
    limit: usize,
    offset: usize,
) -> Result<Vec<Message>, ApiError> {
    let (sql, params) = message_page_sql(from_sql, where_sql, params, order_by, limit, offset);
    fetch_message_page(conn, &sql, &params).await
}

/// The statement [`load_messages_from`] runs, and its parameters with
/// `limit` and `offset` last.
fn message_page_sql(
    from_sql: &str,
    where_sql: &str,
    params: &[SqlParam],
    order_by: &str,
    limit: usize,
    offset: usize,
) -> (String, Vec<SqlParam>) {
    let sql = format!(
        "SELECT m.id, m.conversation_id, m.source, m.service, m.guid, m.timestamp,
                m.sort_order, m.is_from_me, hs.raw AS sender, m.subject, m.body,
                m.is_announcement, m.is_reply, m.thread_originator_guid,
                m.thread_originator_part, m.num_replies,
                hc.raw AS chat_identifier, c.conversation_type, c.group_title,
                ho.raw AS owner
         {from_sql}
         WHERE {where_sql}
         ORDER BY {order_by} LIMIT ? OFFSET ?"
    );
    let mut params = params.to_vec();
    // An `offset` too large for SQLite's `i64` is past the end of any table,
    // so it reads as the largest one rather than wrapping to the first page.
    params.push(SqlParam::Int(i64::try_from(limit).unwrap_or(i64::MAX)));
    params.push(SqlParam::Int(i64::try_from(offset).unwrap_or(i64::MAX)));
    (sql, params)
}

/// Runs a statement from [`message_page_sql`] and reads its rows as messages.
async fn fetch_message_page(
    conn: &mut SqliteConnection,
    sql: &str,
    params: &[SqlParam],
) -> Result<Vec<Message>, ApiError> {
    let rows = (&mut *conn).fetch_all(bind_all(sql, params)).await?;
    let page_rows: Vec<RawRow> = rows
        .iter()
        .map(|row| {
            Ok(RawRow {
                id: row.try_get::<i64, _>(0)?,
                conversation_id: row.try_get(1)?,
                source: row.try_get(2)?,
                service: row.try_get(3)?,
                guid: row.try_get(4)?,
                timestamp: row.try_get(5)?,
                sort_order: row.try_get(6)?,
                is_from_me: row.try_get::<i64, _>(7)? != 0,
                sender: row.try_get(8)?,
                subject: row.try_get(9)?,
                body: row.try_get(10)?,
                is_announcement: row.try_get::<i64, _>(11)? != 0,
                is_reply: row.try_get::<i64, _>(12)? != 0,
                thread_originator_guid: row.try_get(13)?,
                thread_originator_part: row.try_get(14)?,
                num_replies: row.try_get(15)?,
                chat_identifier: row.try_get(16)?,
                conversation_type: row.try_get(17)?,
                group_title: row.try_get(18)?,
                owner: row.try_get(19)?,
            })
        })
        .collect::<Result<Vec<RawRow>, ApiError>>()?;

    let conv_ids = unique_ids(page_rows.iter().map(|r| r.conversation_id));
    let participants = load_for_conversations(conn, &conv_ids).await?;
    let msg_ids: Vec<i64> = page_rows.iter().map(|r| r.id).collect();
    let attachments = load_attachments(conn, &msg_ids).await?;
    let tapbacks = load_tapbacks(conn, &msg_ids).await?;

    Ok(page_rows
        .into_iter()
        .map(|r| {
            let parts = participants
                .get(&r.conversation_id)
                .cloned()
                .unwrap_or_default();
            Message {
                id: r.id,
                source: r.source,
                service: r.service,
                guid: r.guid,
                timestamp: r.timestamp,
                sort_order: r.sort_order,
                is_from_me: r.is_from_me,
                sender: r.sender,
                owner: r.owner,
                subject: r.subject,
                text: r.body,
                is_announcement: r.is_announcement,
                is_reply: r.is_reply,
                thread_originator_guid: r.thread_originator_guid,
                thread_originator_part: r.thread_originator_part,
                num_replies: r.num_replies,
                conversation: MessageConversation {
                    id: r.conversation_id,
                    chat_identifier: r.chat_identifier,
                    conversation_type: r.conversation_type,
                    group_title: r.group_title,
                    participants: parts,
                },
                attachments: attachments.get(&r.id).cloned().unwrap_or_default(),
                tapbacks: tapbacks.get(&r.id).cloned().unwrap_or_default(),
            }
        })
        .collect())
}

/// Attachment rows for these messages, grouped by message id.
async fn load_attachments(
    conn: &mut SqliteConnection,
    message_ids: &[i64],
) -> Result<HashMap<i64, Vec<Attachment>>, ApiError> {
    group_rows_by_id(
        conn,
        message_ids,
        |placeholders| {
            format!(
                "SELECT message_id, path, original_name, mime_type, sha256, is_sticker, transcription,
                    missing_reason, derived_mime_type
             FROM attachments
             WHERE message_id IN ({placeholders})
             ORDER BY message_id, id"
            )
        },
        |row| {
            Ok((
                row.try_get::<i64, _>(0)?,
                Attachment {
                    path: row.try_get(1)?,
                    original_name: row.try_get(2)?,
                    mime_type: row.try_get(3)?,
                    sha256: row.try_get(4)?,
                    is_sticker: row.try_get::<i64, _>(5)? != 0,
                    transcription: row.try_get(6)?,
                    missing_reason: row.try_get(7)?,
                    preview_mime_type: row.try_get(8)?,
                },
            ))
        },
    )
    .await
}

/// Where the preview of one of the account's assets is stored, and its MIME
/// type: the path under the source's converted-assets directory. `None` when
/// the account holds no attachment with that fingerprint in `source`, or
/// `process-assets` wrote no preview for it.
pub async fn attachment_preview(
    conn: &mut SqliteConnection,
    account_id: i64,
    source: &str,
    sha256: &str,
) -> Result<Option<(String, Option<String>)>, ApiError> {
    // Every attachment row that shares a fingerprint in one source carries
    // the same preview, so any one of them answers.
    let row = sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT a.derived_assets_path, a.derived_mime_type
         FROM attachments a
         JOIN messages m ON m.id = a.message_id
         JOIN conversations c ON c.id = m.conversation_id
         WHERE c.account_id = $1 AND m.source = $2 AND a.sha256 = $3
           AND a.derived_assets_path IS NOT NULL AND a.derived_assets_path != ''
         LIMIT 1",
    )
    .bind(account_id)
    .bind(source)
    .bind(sha256)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row)
}

/// Tapback rows for these messages, grouped by message id.
async fn load_tapbacks(
    conn: &mut SqliteConnection,
    message_ids: &[i64],
) -> Result<HashMap<i64, Vec<Tapback>>, ApiError> {
    group_rows_by_id(
        conn,
        message_ids,
        |placeholders| {
            format!(
                "SELECT t.message_id, t.part_index, t.kind, t.emoji, t.is_from_me,
                    hs.raw AS sender
             FROM tapbacks t
             LEFT JOIN handles hs ON hs.id = t.sender_handle_id
             WHERE t.message_id IN ({placeholders})
             ORDER BY t.message_id, t.id"
            )
        },
        |row| {
            Ok((
                row.try_get::<i64, _>(0)?,
                Tapback {
                    part_index: row.try_get(1)?,
                    kind: row.try_get(2)?,
                    emoji: row.try_get(3)?,
                    is_from_me: row.try_get::<i64, _>(4)? != 0,
                    sender: row.try_get(5)?,
                },
            ))
        },
    )
    .await
}

/// `COUNT(*)` of the messages a compiled filter matches.
pub(crate) async fn count_matching_messages(
    conn: &mut SqliteConnection,
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
        .fetch_one(bind_all(&sql, filter.params()))
        .await?
        .try_get(0)?;
    Ok(n.max(0) as u64)
}

/// The `WHERE` fragment a `selection` scope adds to the messages it reads:
/// messages in any of `conversation_ids`, or any of `message_ids`. At least
/// one of the two lists is non-empty.
pub(crate) fn selection_where(
    conversation_ids: &[i64],
    message_ids: &[i64],
) -> (String, Vec<SqlParam>) {
    let placeholders = |n: usize| vec!["?"; n].join(", ");
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
    (format!("({})", branches.join(" OR ")), params)
}

/// The `WHERE` a conversation's message page and its `total` share: the
/// conversation itself, the account scope, and Export's not-duplicate filter
/// (`ListKind::Messages`'s default in `search::emit::compile`). There is no
/// other filter: a read by id takes none, and narrowing a conversation is a
/// search, `GET /v1/messages?q=in:#{id} …` (`docs/architecture/http-api.md`,
/// "Methods"). Trash plays no part here: reading one conversation's messages
/// is not gated by trash, the same rule
/// [`get_conversation_summary`](crate::db::conversations::get_conversation_summary)
/// follows for the conversation itself.
fn conversation_messages_where(conversation_id: i64, account_id: i64) -> (String, Vec<SqlParam>) {
    (
        "m.conversation_id = ? AND m.account_id = ? AND m.duplicate_of IS NULL".to_string(),
        vec![SqlParam::Int(conversation_id), SqlParam::Int(account_id)],
    )
}

/// Where a page of one conversation's messages sits in the conversation, in
/// the order the page is sorted. A screen that jumps to a message does not
/// know its offset, so it names the message and the answer's `offset` says
/// where the page landed (`docs/architecture/http-api.md`, "Lists").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageWindow {
    /// Starting this many messages in.
    Offset(usize),
    /// With this message in the middle, or as near the middle as the ends
    /// of the conversation allow.
    Around(i64),
    /// The messages just before this one, without it.
    Before(i64),
    /// The messages just after this one, without it.
    After(i64),
}

impl MessageWindow {
    /// The query parameter that names the message, and the message's id.
    const fn anchor(self) -> Option<(&'static str, i64)> {
        match self {
            Self::Offset(_) => None,
            Self::Around(id) => Some(("around", id)),
            Self::Before(id) => Some(("before", id)),
            Self::After(id) => Some(("after", id)),
        }
    }
}

/// How many messages a page beside a message takes from each side of it, and
/// whether it holds the message itself. `position` is how many of the
/// conversation's `total` messages come before it in the page's order.
///
/// A page around a message puts it in the middle; where an end of the
/// conversation leaves one side short, the other side makes up the page, so
/// a jump to the first or last message still fills `limit`.
fn window_sides(
    window: MessageWindow,
    position: usize,
    total: usize,
    limit: usize,
) -> (usize, bool, usize) {
    let before_available = position;
    let after_available = total.saturating_sub(position + 1);
    match window {
        MessageWindow::Offset(_) => (0, false, 0),
        MessageWindow::Before(_) => (limit.min(before_available), false, 0),
        MessageWindow::After(_) => (0, false, limit.min(after_available)),
        MessageWindow::Around(_) => {
            let others = limit - 1;
            let after_wanted = after_available.min(others - others / 2);
            let before = before_available.min(others - after_wanted);
            let after = after_available.min(others - before);
            (before, true, after)
        }
    }
}

/// One page of a conversation's messages, in the order `order` names, at the
/// place `window` names. `None` when the conversation does not exist or
/// belongs to another account — checked before the message query runs, so an
/// unknown id and another account's conversation id are indistinguishable
/// from the outside, the same guarantee
/// [`get_conversation_summary`](crate::db::conversations::get_conversation_summary)
/// gives.
///
/// # Errors
///
/// `validation-failed` when `window` names a message this conversation does
/// not show: one of another conversation, a duplicate, or no message at all.
/// `Internal` when a statement fails.
pub async fn get_conversation_messages(
    conn: &mut SqliteConnection,
    account_id: i64,
    conversation_id: i64,
    order: &[SortKey<MessageSort>],
    limit: usize,
    window: MessageWindow,
) -> Result<Option<Page<Message>>, ApiError> {
    if !owns_conversation(conn, account_id, conversation_id).await? {
        return Ok(None);
    }

    let (where_sql, params) = conversation_messages_where(conversation_id, account_id);

    let count_sql = format!("SELECT COUNT(*) FROM messages m WHERE {where_sql}");
    let total: i64 = sqlx::query_scalar_with(&count_sql, bind_args(&params))
        .fetch_one(&mut *conn)
        .await?;
    let total = total.max(0) as u64;

    let Some((parameter, anchor_id)) = window.anchor() else {
        let offset = match window {
            MessageWindow::Offset(offset) => offset,
            _ => 0,
        };
        let items = load_messages(conn, &where_sql, &params, order, limit, offset).await?;
        return Ok(Some(Page {
            items,
            total,
            limit,
            offset,
        }));
    };

    // The message's place in the order: its timestamp, `sort_order` and id,
    // the three keys every message list sorts by.
    let anchor: Option<(String, i64)> = sqlx::query_as_with(
        &format!("SELECT m.timestamp, m.sort_order FROM messages m WHERE {where_sql} AND m.id = ?"),
        bind_args(
            &params
                .iter()
                .cloned()
                .chain([SqlParam::Int(anchor_id)])
                .collect::<Vec<_>>(),
        ),
    )
    .fetch_optional(&mut *conn)
    .await?;
    let Some((timestamp, sort_order)) = anchor else {
        return Err(ApiError::validation(format!(
            "{parameter}: message {anchor_id} is not in this conversation"
        )));
    };

    let direction = order
        .iter()
        .find(|k| k.key == MessageSort::Date)
        .map_or(Direction::Asc, |k| k.direction);
    let (precedes, follows, reverse) = match direction {
        Direction::Asc => ("<", ">", Direction::Desc),
        Direction::Desc => (">", "<", Direction::Asc),
    };
    let order_by = |d: Direction| {
        let d = d.sql();
        format!("m.timestamp {d}, m.sort_order {d}, m.id {d}")
    };
    let beside = |op: &str| {
        let mut side = params.clone();
        side.extend([
            SqlParam::Text(timestamp.clone()),
            SqlParam::Int(sort_order),
            SqlParam::Int(anchor_id),
        ]);
        (
            format!("{where_sql} AND (m.timestamp, m.sort_order, m.id) {op} (?, ?, ?)"),
            side,
        )
    };

    let (preceding_where, preceding_params) = beside(precedes);
    let position: i64 = sqlx::query_scalar_with(
        &format!("SELECT COUNT(*) FROM messages m WHERE {preceding_where}"),
        bind_args(&preceding_params),
    )
    .fetch_one(&mut *conn)
    .await?;
    let position = usize::try_from(position.max(0)).unwrap_or(usize::MAX);
    let (before, holds_anchor, after) = window_sides(
        window,
        position,
        usize::try_from(total).unwrap_or(usize::MAX),
        limit,
    );

    let from_sql = messages_from_sql();
    // The nearest messages before it, read outward from it and turned back
    // into the page's order.
    let mut items = if before > 0 {
        let mut rows = load_messages_from(
            conn,
            &from_sql,
            &preceding_where,
            &preceding_params,
            &order_by(reverse),
            before,
            0,
        )
        .await?;
        rows.reverse();
        rows
    } else {
        Vec::new()
    };
    if holds_anchor {
        let mut own = params.clone();
        own.push(SqlParam::Int(anchor_id));
        items.extend(
            load_messages_from(
                conn,
                &from_sql,
                &format!("{where_sql} AND m.id = ?"),
                &own,
                &order_by(direction),
                1,
                0,
            )
            .await?,
        );
    }
    if after > 0 {
        let (following_where, following_params) = beside(follows);
        items.extend(
            load_messages_from(
                conn,
                &from_sql,
                &following_where,
                &following_params,
                &order_by(direction),
                after,
                0,
            )
            .await?,
        );
    }

    let offset = match window {
        MessageWindow::After(_) => position + 1,
        _ => position - before,
    };
    Ok(Some(Page {
        items,
        total,
        limit,
        offset,
    }))
}
