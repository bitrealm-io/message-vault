//! The conversation list and one conversation's summary and sources: what
//! `GET /v1/conversations`, `GET /v1/conversations/{id}` and its `sources`
//! read. `conversations_api` answers the routes; the queries live here.

use std::collections::{HashMap, HashSet};

use serde::Serialize;
use sqlx::AnyConnection;

use crate::db::dialect::engine_of;
use crate::db::ownership::owns_conversation;
use crate::db::participant_names::{Participant, load_for_conversations};
use crate::db::sql::{
    SqlParam, bind_args, fold_in_id_chunks, in_placeholders, renumber_placeholders,
};
use crate::paging::{Direction, Page, SortKey};
use crate::server::ApiError;

/// The keys `GET /v1/conversations` accepts in `sort=`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationSort {
    /// Timestamp of the most recent non-duplicate message in the thread.
    Date,
    /// Number of non-duplicate messages in the thread.
    Messages,
}

/// The accepted keys, as `sort=` spells them.
pub const CONVERSATION_SORT_KEYS: [(&str, ConversationSort); 2] = [
    ("date", ConversationSort::Date),
    ("messages", ConversationSort::Messages),
];

/// Newest activity first: what the list shows when `sort` is absent.
pub const DEFAULT_CONVERSATION_SORT: [SortKey<ConversationSort>; 1] = [SortKey {
    key: ConversationSort::Date,
    direction: Direction::Desc,
}];

/// The `ORDER BY` body for a parsed `sort`.
///
/// Every part is a fixed literal chosen by matching on the enum, so no part
/// of the request reaches the SQL text. Both columns are output aliases of
/// the page query, which SQLite and Postgres each allow in `ORDER BY`.
/// `c.id` breaks ties, in the direction of the last key, so paging cannot
/// repeat or skip a row.
///
/// `last_message_at` is NULL for a thread whose every message is a
/// duplicate, and the two engines disagree about where NULLs belong:
/// SQLite sorts them lowest, while Postgres defaults to NULLS LAST when
/// ascending and NULLS FIRST when descending. Leading with
/// `(last_message_at IS NULL)` — false before true on both — pins those
/// threads to the end in either direction and keeps the two engines
/// agreeing.
fn conversation_order_by(keys: &[SortKey<ConversationSort>]) -> String {
    let mut parts: Vec<String> = keys
        .iter()
        .map(|k| match k.key {
            ConversationSort::Date => format!(
                "(last_message_at IS NULL) ASC, last_message_at {}",
                k.direction.sql()
            ),
            ConversationSort::Messages => format!("message_count {}", k.direction.sql()),
        })
        .collect();
    let tie = keys.last().map_or(Direction::Desc, |k| k.direction);
    parts.push(format!("c.id {}", tie.sql()));
    parts.join(", ")
}

/// Conversation row for the list: participants, counts, tags.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ConversationSummary {
    /// The conversation's id; search for it as `in:#<id>`.
    pub id: i64,
    /// Participants with names and handles.
    pub participants: Vec<Participant>,
    /// Messages in the conversation (excluding hidden duplicates).
    pub message_count: u64,
    /// Timestamp of the last message.
    pub last_message_at: String,
    /// Timestamp of the conversation's first message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_range_start: Option<String>,
    /// Timestamp of the conversation's last message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_range_end: Option<String>,
    /// Platform service of the conversation, e.g. `imessage`.
    pub service: String,
    /// True for group conversations.
    pub is_group: bool,
    /// Group label from the export, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Message tags on this conversation.
    pub tags: Vec<String>,
}

struct RawConversation {
    id: i64,
    conversation_type: String,
    group_title: Option<String>,
    message_count: i64,
    last_message_at: Option<String>,
    date_range_start: Option<String>,
    date_range_end: Option<String>,
}

type RawConversationRow = (
    i64,
    String,
    Option<String>,
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// One page of the conversation list for `q`, a query in the search language.
///
/// # Errors
///
/// `BadRequest` for a query the language refuses; `Internal` when a
/// statement fails.
pub async fn list_conversations_sorted(
    conn: &mut AnyConnection,
    account_id: i64,
    q: &str,
    order: &[SortKey<ConversationSort>],
    limit: usize,
    offset: usize,
    clock: (chrono_tz::Tz, chrono::NaiveDate),
) -> Result<Page<ConversationSummary>, ApiError> {
    let engine = engine_of(conn);
    let (zone, today) = clock;
    let filter = crate::search::compile(crate::search::CompileRequest {
        list: crate::search::ListKind::Conversations,
        query: q,
        account_id,
        engine,
        today,
        zone,
    })?;
    let where_sql = filter.where_sql();

    let count_sql = renumber_placeholders(&format!(
        "SELECT COUNT(*) FROM conversations c WHERE {where_sql}"
    ));
    let total: i64 = sqlx::query_scalar_with(&count_sql, bind_args(filter.params()))
        .fetch_one(&mut *conn)
        .await?;
    let total = total.max(0) as u64;

    let mut params = filter.params().to_vec();
    params.push(SqlParam::Int(limit as i64));
    params.push(SqlParam::Int(offset as i64));
    // The sort reads computed columns (`last_message_at`, `message_count`)
    // inside expressions. SQLite lets ORDER BY name a select-list alias
    // anywhere; Postgres only as a bare name. Sorting the rows as a derived
    // table makes those aliases real columns on both engines.
    let sql = renumber_placeholders(&format!(
        "SELECT * FROM ({select} WHERE {where_sql}) AS c ORDER BY {order_by} LIMIT ? OFFSET ?",
        select = CONVERSATION_ROW_SELECT,
        order_by = conversation_order_by(order),
    ));
    let out = load_conversation_rows(conn, account_id, &sql, &params).await?;
    Ok(Page {
        items: out,
        total,
        limit,
        offset,
    })
}

/// One conversation by id, scoped to `account_id`. `None` when the id does
/// not exist or belongs to another account — the two cases look identical to
/// the caller, which is what keeps this a 404 rather than a 403.
///
/// # Errors
///
/// `Internal` when a statement fails.
pub async fn get_conversation_summary(
    conn: &mut AnyConnection,
    account_id: i64,
    conversation_id: i64,
) -> Result<Option<ConversationSummary>, ApiError> {
    let sql = renumber_placeholders(&format!(
        "{CONVERSATION_ROW_SELECT} WHERE c.id = ? AND c.account_id = ?"
    ));
    let params = [SqlParam::Int(conversation_id), SqlParam::Int(account_id)];
    let out = load_conversation_rows(conn, account_id, &sql, &params).await?;
    Ok(out.into_iter().next())
}

/// The row shape shared by the conversation list and the single-conversation
/// read: id, type, title, and the counts/timestamps computed from `messages`.
/// Callers append their own `WHERE`, `ORDER BY`, and paging.
const CONVERSATION_ROW_SELECT: &str = "SELECT c.id,
                c.conversation_type,
                c.group_title,
                (SELECT COUNT(*) FROM messages m
                 WHERE m.conversation_id = c.id AND m.duplicate_of IS NULL) AS message_count,
                (SELECT MAX(m.timestamp) FROM messages m
                 WHERE m.conversation_id = c.id AND m.duplicate_of IS NULL) AS last_message_at,
                (SELECT MIN(m.timestamp) FROM messages m
                 WHERE m.conversation_id = c.id AND m.duplicate_of IS NULL) AS date_range_start,
                (SELECT MAX(m.timestamp) FROM messages m
                 WHERE m.conversation_id = c.id AND m.duplicate_of IS NULL) AS date_range_end
         FROM conversations c";

/// Run a `CONVERSATION_ROW_SELECT`-shaped query and assemble
/// [`ConversationSummary`] rows: participants, sources, and tags, exactly as
/// the list builds them. Shared so the list and the single-conversation read
/// cannot drift into two different notions of what a conversation summary is.
async fn load_conversation_rows(
    conn: &mut AnyConnection,
    account_id: i64,
    sql: &str,
    params: &[SqlParam],
) -> Result<Vec<ConversationSummary>, ApiError> {
    let rows: Vec<RawConversationRow> = sqlx::query_as_with(sql, bind_args(params))
        .fetch_all(&mut *conn)
        .await?;
    let rows: Vec<RawConversation> = rows
        .into_iter()
        .map(
            |(
                id,
                conversation_type,
                group_title,
                message_count,
                last_message_at,
                date_range_start,
                date_range_end,
            )| RawConversation {
                id,
                conversation_type,
                group_title,
                message_count,
                last_message_at,
                date_range_start,
                date_range_end,
            },
        )
        .collect();

    let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
    let mut participants = load_for_conversations(conn, &ids).await?;
    let source_sets = load_conversation_sources(conn, &ids).await?;
    let mut tag_sets = crate::db::named_membership::names_for_items(
        crate::db::named_membership::tag_spec(),
        conn,
        account_id,
        &ids,
    )
    .await
    .map_err(ApiError::Internal)?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let last = row
            .last_message_at
            .clone()
            .unwrap_or_else(|| "1970-01-01T00:00:00Z".into());
        let is_group = row.conversation_type.eq_ignore_ascii_case("group");
        let service = display_service_label(
            source_sets
                .get(&row.id)
                .map(Vec::as_slice)
                .unwrap_or_default(),
        );
        let parts = participants.remove(&row.id).unwrap_or_default();
        out.push(ConversationSummary {
            id: row.id,
            participants: parts,
            message_count: row.message_count.max(0) as u64,
            last_message_at: last,
            date_range_start: row.date_range_start,
            date_range_end: row.date_range_end,
            service,
            is_group,
            label: row
                .group_title
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            tags: tag_sets.remove(&row.id).unwrap_or_default(),
        });
    }
    Ok(out)
}

const IMESSAGE_SOURCE: &str = "imessage";
const SBR_SOURCE: &str = "sms-backup-restore";
const WHATSAPP_SOURCE: &str = "whatsapp";

/// Header label from distinct message sources in a conversation.
pub fn display_service_label(sources: &[String]) -> String {
    let set: HashSet<&str> = sources.iter().map(|s| s.as_str()).collect();
    if set.contains(SBR_SOURCE) {
        return "SMS/MMS".into();
    }
    if set.len() == 1 && set.contains(IMESSAGE_SOURCE) {
        return IMESSAGE_SOURCE.into();
    }
    if set.len() == 1 && set.contains(WHATSAPP_SOURCE) {
        return "WhatsApp".into();
    }
    if set.len() == 1 {
        return sources[0].trim().to_string();
    }
    "unknown".into()
}

/// Source ids per conversation, for conversations holding messages from more than one import.
async fn load_conversation_sources(
    conn: &mut AnyConnection,
    conversation_ids: &[i64],
) -> Result<HashMap<i64, Vec<String>>, ApiError> {
    fold_in_id_chunks(conn, conversation_ids, |conn, chunk| {
        Box::pin(async move {
            let placeholders = in_placeholders(1, chunk.len());
            let sql = format!(
                "SELECT conversation_id, source
                 FROM messages
                 WHERE duplicate_of IS NULL
                   AND conversation_id IN ({placeholders})
                 GROUP BY conversation_id, source
                 ORDER BY conversation_id, source"
            );
            let mut q = sqlx::query_as::<_, (i64, String)>(&sql);
            for id in chunk {
                q = q.bind(*id);
            }
            let rows = q.fetch_all(&mut *conn).await?;
            let mut out = Vec::new();
            for (cid, source) in rows {
                if source.trim().is_empty() {
                    continue;
                }
                out.push((cid, source));
            }
            Ok(out)
        })
    })
    .await
}

/// One backup source with message counts and share.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ConversationSource {
    /// Backup source name.
    pub backup_name: String,
    /// Messages in this conversation from this source.
    pub message_count: u64,
    /// Messages only this source has (not hidden duplicates).
    pub unique_count: u64,
    /// Share of the conversation's unique messages, 0–100.
    pub percentage: f64,
}

/// Per-source message counts for the Sources panel.
///
/// # Errors
///
/// Returns an internal error when a database statement fails.
pub async fn list_conversation_source_stats(
    conn: &mut AnyConnection,
    account_id: i64,
    conversation_id: i64,
) -> Result<Option<Vec<ConversationSource>>, ApiError> {
    if !owns_conversation(conn, account_id, conversation_id).await? {
        return Ok(None);
    }

    let rows: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT source,
                    COUNT(*) AS message_count,
                    SUM(CASE WHEN duplicate_of IS NULL THEN 1 ELSE 0 END) AS unique_count
             FROM messages
             WHERE conversation_id = $1
             GROUP BY source
             ORDER BY source",
    )
    .bind(conversation_id)
    .fetch_all(&mut *conn)
    .await?;

    let total_unique: i64 = rows.iter().map(|(_, _, u)| *u).sum();
    let sources = rows
        .into_iter()
        .map(|(source, message_count, unique_count)| {
            let percentage = if total_unique > 0 {
                (unique_count as f64) * 100.0 / (total_unique as f64)
            } else {
                0.0
            };
            ConversationSource {
                backup_name: source,
                message_count: message_count.max(0) as u64,
                unique_count: unique_count.max(0) as u64,
                percentage: (percentage * 10.0).round() / 10.0,
            }
        })
        .collect();
    Ok(Some(sources))
}
