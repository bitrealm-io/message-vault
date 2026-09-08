//! The message row loader shared by every route that reads messages: their
//! conversation, attachments and tapbacks, joined and grouped.
//!
//! The row shapes themselves live in `vault-api-types`, where `vault-pull`
//! reads them from the same definition rather than a hand-written mirror.
//!
//! `load_messages` takes an already-compiled `WHERE` fragment and its bound
//! params, so the caller decides what selects the rows — a search query, a
//! conversation id — while this module owns only the row shape and how it is
//! assembled.

use std::collections::HashMap;

use sqlx::AnyConnection;
use sqlx::{Executor, Row};

pub use vault_api_types::{Attachment, Message, MessageConversation, Tapback};

use crate::db::participant_names::load_for_conversations;
use crate::db::sql::{SqlParam, bind_all, group_rows_by_id, renumber_placeholders};
use crate::paging::{Direction, SortKey};
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
/// and the two handles' raw text.
///
/// Export's count statements carry the same joins, because a search filter can
/// name a conversation column and would not compile against `messages` alone.
/// The conversation read route's count is `FROM messages m` with no joins: its
/// filter is a conversation id and a timestamp range, both on `m`. The two
/// still count the same rows, because `conversations.chat_handle_id` is
/// `NOT NULL` with a foreign key to `handles`, so the one inner join here
/// never drops a row (`hs` is a `LEFT JOIN` and cannot drop one either).
pub(crate) fn messages_from_sql() -> String {
    format!("FROM messages m\n{}", conversation_join_sql())
}

/// Handles joins for a query already anchored on `messages m`.
/// `hc` supplies `c.chat_handle_id` raw text; `hs` supplies `m.sender_handle_id`
/// raw text (LEFT, since outgoing messages carry no sender handle).
pub(crate) fn conversation_join_sql() -> String {
    "JOIN conversations c ON c.id = m.conversation_id
     JOIN handles hc ON hc.id = c.chat_handle_id
     LEFT JOIN handles hs ON hs.id = m.sender_handle_id"
        .into()
}

/// Load the message rows an already-compiled filter matches, joined with
/// their conversation, attachments and tapbacks.
///
/// `where_sql` and `params` are the caller's compiled `WHERE` fragment (a
/// search query for Export, a conversation id for the read routes) with
/// placeholders in bind order; this function appends the `ORDER BY`/`LIMIT`/
/// `OFFSET` and does not touch the total count, which stays the caller's job.
///
/// # Errors
///
/// Returns an error when a database statement fails.
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

pub async fn load_messages(
    conn: &mut AnyConnection,
    where_sql: &str,
    params: &[SqlParam],
    order: &[SortKey<MessageSort>],
    limit: u32,
    offset: u32,
) -> Result<Vec<Message>, ApiError> {
    let direction = order
        .iter()
        .find(|k| k.key == MessageSort::Date)
        .map_or(Direction::Asc, |k| k.direction)
        .sql();
    let mut sql = format!(
        "SELECT m.id, m.conversation_id, m.source, m.service, m.guid, m.timestamp,
                m.sort_order, m.is_from_me, hs.raw AS sender, m.subject, m.body,
                m.is_announcement, m.is_reply, m.thread_originator_guid,
                m.thread_originator_part, m.num_replies,
                hc.raw AS chat_identifier, c.conversation_type, c.group_title
         {messages_from_sql}
         WHERE {where_sql}",
        messages_from_sql = messages_from_sql(),
    );
    let mut params = params.to_vec();
    sql.push_str(&format!(
        " ORDER BY m.timestamp {direction}, m.sort_order {direction}, m.id {direction} LIMIT ? OFFSET ?"
    ));
    params.push(SqlParam::Int(limit as i64));
    params.push(SqlParam::Int(offset as i64));

    let sql = renumber_placeholders(&sql);
    let rows = (&mut *conn).fetch_all(bind_all(&sql, &params)).await?;
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
    conn: &mut AnyConnection,
    message_ids: &[i64],
) -> Result<HashMap<i64, Vec<Attachment>>, ApiError> {
    group_rows_by_id(
        conn,
        message_ids,
        |placeholders| {
            format!(
                "SELECT message_id, path, original_name, mime_type, sha256, is_sticker, transcription,
                    missing_reason
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
                },
            ))
        },
    )
    .await
}

/// Tapback rows for these messages, grouped by message id.
async fn load_tapbacks(
    conn: &mut AnyConnection,
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
