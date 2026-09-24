//! What the contact screens read: the contact list, one contact's name and
//! totals, the selection summaries, and which identifiers the account has no
//! contact for. `contacts_api` answers the routes; the queries live here.

use std::collections::{HashMap, HashSet};

use anyhow::Result as AnyResult;
use serde::Serialize;
use sqlx::AnyConnection;

use crate::db::contacts::UNKNOWN_CONTACT_SQL;
use crate::db::dialect::{engine_of, group_concat_unit_separator, name_ci_expr};
use crate::db::engine::DbEngine;
use crate::db::handles::{infer_handle_type_from_shape, normalize_handle};
use crate::db::sql::{SqlParam, bind_args, in_placeholders, renumber_placeholders};
use crate::paging::{Direction, MAX_CONTACT_SUMMARY_IDS, Page, SortKey};
use crate::search::emit::{NOT_TRASHED_CONTACT, NOT_TRASHED_CONVERSATION};
use crate::server::ApiError;

/// Contact row for the list: name, handles, groups.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ContactSummary {
    /// Contact id.
    pub id: i64,
    /// The contact's preferred name; empty when it has none.
    pub name: String,
    /// True when the contact is in the Unknown Contact Group: it has no
    /// identity, or it has identities and no preferred name.
    pub unknown: bool,
    /// Number of identities linked to the contact.
    pub identity_count: u64,
    /// Normalized (and raw when distinct) handle values for client-side filter.
    #[serde(default)]
    pub handles: Vec<String>,
    /// When the contact’s address-book shape last changed (`datetime('now')`).
    pub last_modified: String,
    /// When the vault last heard from the contact: the newest message one of
    /// the contact's handles sent (RFC 3339, UTC). Null when none of them
    /// ever sent a message. Not the contact's last activity: a message the
    /// account owner sent, or another member of a group chat, does not count.
    pub last_heard_at: Option<String>,
    /// Group names on this contact (A–Z).
    #[serde(default)]
    pub groups: Vec<String>,
}

/// Contact-level first/last seen and message counts for the selection table.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ContactSelectionSummary {
    /// Contact id.
    pub id: i64,
    /// The contact's preferred name; empty when it has none.
    pub name: String,
    /// Date of the contact's first message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_date: Option<String>,
    /// Date of the contact's last message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_date: Option<String>,
    /// 1:1 conversations with the contact.
    pub individual_conversations: u64,
    /// Group conversations with the contact.
    pub group_conversations: u64,
    /// Messages in 1:1 conversations with the contact.
    pub individual_message_count: u64,
    /// Messages in group conversations with the contact.
    pub group_message_count: u64,
}

/// A contact is linked to a conversation when one of its handles is either
/// the conversation's chat handle or a participant handle in it.
///
/// `contact_id_expr` is the SQL expression for the contact id (`$N` or `ct.id`).
fn involves_contact_expr(contact_id_expr: &str) -> String {
    format!(
        "EXISTS (
       SELECT 1 FROM contact_handles ch
       WHERE ch.account_id = c.account_id
         AND ch.contact_id = {contact_id_expr}
         AND (
           ch.handle_id = c.chat_handle_id
           OR EXISTS (
             SELECT 1 FROM participants p
             WHERE p.conversation_id = c.id AND p.handle_id = ch.handle_id
           )
         )
     )"
    )
}

/// Expects two bind parameters: `account_id` ($1), `contact_id` ($2).
/// Alias `c` = conversations.
fn involves_contact_sql() -> String {
    involves_contact_expr("$2")
}

/// The keys `GET /v1/contacts` accepts in `sort=`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContactSort {
    /// Display name, case-folded.
    Name,
    /// When the vault last heard from the contact (`last_heard_at`).
    LastHeard,
}

/// The accepted keys, as `sort=` spells them.
pub const CONTACT_SORT_KEYS: [(&str, ContactSort); 2] = [
    ("name", ContactSort::Name),
    ("last_heard", ContactSort::LastHeard),
];

/// A to Z: what the list shows when `sort` is absent.
pub const DEFAULT_CONTACT_SORT: [SortKey<ContactSort>; 1] = [SortKey {
    key: ContactSort::Name,
    direction: Direction::Asc,
}];

/// The `ORDER BY` body for a parsed `sort`, over the derived table the list
/// query builds (`name` and `last_heard_at` are real columns there).
///
/// `name` is a select-list alias and the sort applies lower() to it. SQLite
/// allows that; Postgres only allows a bare alias in ORDER BY, so the rows
/// are sorted as a derived table where `name` is a real column.
///
/// `last_heard_at` is NULL for a contact none of whose handles ever sent a
/// message, and the two engines disagree about where NULLs belong: SQLite
/// sorts them lowest, Postgres puts them last ascending and first
/// descending. Leading with `(last_heard_at IS NULL)` — false before true
/// on both — pins those contacts to the end in either direction.
///
/// `ct.id` breaks ties in the direction of the last key, so paging cannot
/// repeat a row.
fn contact_order_by(engine: DbEngine, keys: &[SortKey<ContactSort>]) -> String {
    let mut parts: Vec<String> = keys
        .iter()
        .map(|k| match k.key {
            ContactSort::Name => {
                format!("{} {}", name_ci_expr(engine, "name"), k.direction.sql())
            }
            ContactSort::LastHeard => format!(
                "(last_heard_at IS NULL) ASC, last_heard_at {}",
                k.direction.sql()
            ),
        })
        .collect();
    let tie = keys.last().map_or(Direction::Asc, |k| k.direction);
    parts.push(format!("ct.id {}", tie.sql()));
    format!("ORDER BY {}", parts.join(", "))
}

/// One page of the contact list for `q`, a query in the search language.
///
/// # Errors
///
/// `BadRequest` for a query the language refuses; `Internal` when a
/// statement fails.
pub async fn list_contacts_sorted(
    conn: &mut AnyConnection,
    account_id: i64,
    q: &str,
    order: &[SortKey<ContactSort>],
    limit: usize,
    offset: usize,
    clock: (chrono_tz::Tz, chrono::NaiveDate),
) -> Result<Page<ContactSummary>, ApiError> {
    let engine = engine_of(conn);
    let (zone, today) = clock;
    let filter = crate::search::compile(crate::search::CompileRequest {
        list: crate::search::ListKind::Contacts,
        query: q,
        account_id,
        engine,
        today,
        zone,
    })?;
    let where_sql = filter.where_sql();

    let count_sql = renumber_placeholders(&format!(
        "SELECT COUNT(*) FROM contacts ct WHERE {where_sql}"
    ));
    let total: i64 = sqlx::query_scalar_with(&count_sql, bind_args(filter.params()))
        .fetch_one(&mut *conn)
        .await?;
    let total = total.max(0) as u64;

    let order_by = contact_order_by(engine, order);
    // `last_heard_at` is the newest message one of the contact's handles sent.
    // A flagged duplicate carries the same timestamp as the message it
    // duplicates, so it cannot move the maximum and is not filtered out; that
    // keeps the lookup on `ix_messages_sender_timestamp` alone.
    let sql = renumber_placeholders(&format!(
        "SELECT * FROM (SELECT ct.id,
                trim(ct.preferred_name) AS name,
                CASE WHEN {unknown} THEN 1 ELSE 0 END AS is_unknown,
                (SELECT COUNT(*)
                 FROM contact_handles ch
                 WHERE ch.account_id = ct.account_id AND ch.contact_id = ct.id) AS identity_count,
                (SELECT {handles_agg}
                 FROM (
                   SELECT DISTINCT h.normalized AS val
                   FROM contact_handles ch
                   JOIN handles h ON h.id = ch.handle_id
                   WHERE ch.account_id = ct.account_id AND ch.contact_id = ct.id
                     AND h.normalized IS NOT NULL AND trim(h.normalized) != ''
                   UNION
                   SELECT DISTINCT h.raw AS val
                   FROM contact_handles ch
                   JOIN handles h ON h.id = ch.handle_id
                   WHERE ch.account_id = ct.account_id AND ch.contact_id = ct.id
                     AND h.raw IS NOT NULL AND trim(h.raw) != ''
                 )) AS handles,
                ct.last_modified,
                (SELECT MAX(m.timestamp)
                 FROM contact_handles ch
                 JOIN messages m ON m.sender_handle_id = ch.handle_id
                 WHERE ch.account_id = ct.account_id AND ch.contact_id = ct.id) AS last_heard_at,
                (SELECT {groups_agg}
                 FROM contact_group_members clm
                 JOIN contact_groups cl ON cl.id = clm.group_id
                 WHERE clm.contact_id = ct.id AND cl.account_id = ct.account_id) AS groups
         FROM contacts ct
         WHERE {where_sql}) AS ct
         {order_by}
         LIMIT ? OFFSET ?",
        unknown = UNKNOWN_CONTACT_SQL,
        handles_agg = group_concat_unit_separator(engine, "val"),
        groups_agg = group_concat_unit_separator(engine, "cl.name"),
    ));
    let mut params = filter.params().to_vec();
    params.push(SqlParam::Int(limit as i64));
    params.push(SqlParam::Int(offset as i64));
    let rows: Vec<ContactRow> = sqlx::query_as_with(&sql, bind_args(&params))
        .fetch_all(&mut *conn)
        .await?;

    let contacts = rows
        .into_iter()
        .map(
            |(
                id,
                name,
                is_unknown,
                identity_count,
                handles_blob,
                last_modified,
                last_heard_at,
                groups_blob,
            )| {
                let handles = handles_blob
                    .map(|s| {
                        s.split('\u{1f}')
                            .filter_map(message_ir::nonempty)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let mut groups = groups_blob
                    .map(|s| {
                        s.split('\u{1f}')
                            .filter_map(message_ir::nonempty)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                groups.sort_by_key(|a| a.to_ascii_lowercase());
                ContactSummary {
                    id,
                    name,
                    unknown: is_unknown != 0,
                    identity_count: identity_count.max(0) as u64,
                    handles,
                    last_modified,
                    last_heard_at,
                    groups,
                }
            },
        )
        .collect();

    Ok(Page {
        items: contacts,
        total,
        limit,
        offset,
    })
}

type ContactRow = (
    i64,
    String,
    i64,
    i64,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
);

/// The contact's preferred name (empty when it has none), whether it is
/// Unknown, and its last-modified stamp; `None` when it is missing, another
/// account's, or in the trash.
///
/// # Errors
///
/// Returns an error when the statement fails.
pub async fn contact_name_and_modified(
    conn: &mut AnyConnection,
    account_id: i64,
    contact_id: i64,
) -> Result<Option<(String, bool, String)>, sqlx::Error> {
    let row: Option<(String, i64, String)> = sqlx::query_as(&format!(
        "SELECT trim(ct.preferred_name),
                CASE WHEN {unknown} THEN 1 ELSE 0 END,
                ct.last_modified
         FROM contacts ct
         WHERE ct.id = $1 AND ct.account_id = $2
           AND {NOT_TRASHED_CONTACT}",
        unknown = UNKNOWN_CONTACT_SQL,
    ))
    .bind(contact_id)
    .bind(account_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(|(name, unknown, last_modified)| (name, unknown != 0, last_modified)))
}

/// Conversation and message counts across every handle of one contact.
pub struct ContactTotals {
    /// 1:1 conversations the contact appears in.
    pub direct: u64,
    /// Group conversations the contact appears in.
    pub groups: u64,
    /// Messages across all of the contact's conversations.
    pub messages: u64,
}

/// Counts over the conversations the contact is in, trashed ones excluded.
/// Scoped to those conversations on purpose: grouping the whole account's
/// messages table dominated drawer latency.
///
/// # Errors
///
/// Returns an error when the statement fails.
pub async fn contact_totals(
    conn: &mut AnyConnection,
    account_id: i64,
    contact_id: i64,
) -> Result<ContactTotals, sqlx::Error> {
    let (direct, groups, messages): (i64, i64, i64) = sqlx::query_as(&format!(
        "WITH involved AS (
               SELECT c.id, c.conversation_type
               FROM conversations c
               WHERE c.account_id = $1
                 AND {involves_contact_sql}
                 AND {not_trashed_conversation}
             )
             SELECT
               (SELECT COUNT(*) FROM involved WHERE conversation_type = 'individual'),
               (SELECT COUNT(*) FROM involved WHERE conversation_type = 'group'),
               (SELECT COUNT(*) FROM messages m
                WHERE m.duplicate_of IS NULL
                  AND m.conversation_id IN (SELECT id FROM involved))",
        involves_contact_sql = involves_contact_sql(),
        not_trashed_conversation = NOT_TRASHED_CONVERSATION,
    ))
    .bind(account_id)
    .bind(contact_id)
    .fetch_one(&mut *conn)
    .await?;
    Ok(ContactTotals {
        direct: direct.max(0) as u64,
        groups: groups.max(0) as u64,
        messages: messages.max(0) as u64,
    })
}

/// First/last seen and message counts for many contacts in one grouped query.
///
/// Unknown, trashed, and duplicate ids are skipped. At most
/// [`MAX_CONTACT_SUMMARY_IDS`] ids are read so the `IN` list stays under
/// SQLite's variable cap.
///
/// # Errors
///
/// Returns an internal error when a database statement fails.
pub async fn get_contact_summaries(
    conn: &mut AnyConnection,
    account_id: i64,
    ids: &[i64],
) -> Result<Vec<ContactSelectionSummary>, ApiError> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for id in ids.iter().copied() {
        if id <= 0 || !seen.insert(id) {
            continue;
        }
        unique.push(id);
        if unique.len() == MAX_CONTACT_SUMMARY_IDS {
            break;
        }
    }
    if unique.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = in_placeholders(2, unique.len());
    let involves = involves_contact_expr("selected.id");
    let sql = format!(
        "WITH selected AS (
            SELECT ct.id,
                   ct.account_id,
                   trim(ct.preferred_name) AS name
            FROM contacts ct
            WHERE ct.account_id = $1
              AND ct.id IN ({placeholders})
              AND {NOT_TRASHED_CONTACT}
         ),
         involved AS (
            SELECT selected.id AS contact_id, c.id AS conversation_id, c.conversation_type
            FROM selected
            JOIN conversations c ON c.account_id = selected.account_id
              AND {involves}
              AND {NOT_TRASHED_CONVERSATION}
         )
         SELECT
            s.id,
            s.name,
            MIN(m.timestamp) AS start_date,
            MAX(m.timestamp) AS end_date,
            COUNT(DISTINCT CASE WHEN i.conversation_type = 'individual' THEN i.conversation_id END),
            COUNT(DISTINCT CASE WHEN i.conversation_type = 'group' THEN i.conversation_id END),
            COUNT(DISTINCT CASE WHEN i.conversation_type = 'individual' THEN m.id END),
            COUNT(DISTINCT CASE WHEN i.conversation_type = 'group' THEN m.id END)
         FROM selected s
         LEFT JOIN involved i ON i.contact_id = s.id
         LEFT JOIN messages m ON m.conversation_id = i.conversation_id
           AND m.duplicate_of IS NULL
         GROUP BY s.id, s.name",
    );

    let mut q = sqlx::query_as::<_, ContactSelectionRow>(&sql);
    q = q.bind(account_id);
    for id in &unique {
        q = q.bind(*id);
    }
    let rows: Vec<ContactSelectionRow> = q.fetch_all(&mut *conn).await?;

    let mut by_id = HashMap::new();
    for (
        id,
        name,
        start_date,
        end_date,
        individual_conversations,
        group_conversations,
        individual_message_count,
        group_message_count,
    ) in rows
    {
        by_id.insert(
            id,
            ContactSelectionSummary {
                id,
                name,
                start_date,
                end_date,
                individual_conversations: individual_conversations.max(0) as u64,
                group_conversations: group_conversations.max(0) as u64,
                individual_message_count: individual_message_count.max(0) as u64,
                group_message_count: group_message_count.max(0) as u64,
            },
        );
    }
    Ok(unique
        .into_iter()
        .filter_map(|id| by_id.remove(&id))
        .collect())
}

type ContactSelectionRow = (
    i64,
    String,
    Option<String>,
    Option<String>,
    i64,
    i64,
    i64,
    i64,
);

/// Which of `identifiers` this account has no contact for.
///
/// A trashed contact does not count as known: an import that meets one of
/// its handles discards it and makes a fresh contact from the backup
/// (ADR-0013), so the person will see a new contact appear, which is what
/// this count promises.
///
/// Matches on the same normalized form the import pipeline stores in
/// `handles.normalized` ([`normalize_handle`]), so an export spelling like
/// `+1 555 0100` is recognized against a vault contact stored as
/// `+15550100`. Blanks are dropped; duplicates are collapsed by *normalized*
/// form (two spellings of the same person must not both count as "new"),
/// keeping the first-seen raw (trimmed) spelling and first-seen order.
///
/// # Errors
///
/// Returns an error when a database statement fails.
pub async fn unknown_contact_identifiers(
    conn: &mut AnyConnection,
    account_id: i64,
    identifiers: &[String],
) -> AnyResult<Vec<String>> {
    let mut seen_normalized = HashSet::new();
    // (first-seen trimmed spelling, normalized form), one entry per distinct
    // normalized value.
    let mut unique: Vec<(String, String)> = Vec::new();
    for raw in identifiers {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Import prefers the handle type the source declared (SMS, email
        // header, ...); here there is no declared type, so this infers one
        // from the string's shape instead. The two can diverge: a
        // source-declared phone number whose digits don't look phone-shaped
        // (e.g. a short code) would infer as Other here and normalize
        // differently than the vault's stored (Phone-typed) form, reading as
        // "new" even though import would have linked it. Acceptable for a
        // best-effort gate count; not a source of silent data loss.
        let normalized = normalize_handle(trimmed, infer_handle_type_from_shape(trimmed)).0;
        if seen_normalized.insert(normalized.clone()) {
            unique.push((trimmed.to_string(), normalized));
        }
    }
    if unique.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = in_placeholders(2, unique.len());
    let sql = format!(
        "SELECT DISTINCT h.normalized
         FROM handles h
         JOIN contact_handles ch ON ch.account_id = h.account_id AND ch.handle_id = h.id
         JOIN contacts ct ON ct.account_id = ch.account_id AND ct.id = ch.contact_id
         WHERE h.account_id = $1
           AND h.normalized IN ({placeholders})
           AND {NOT_TRASHED_CONTACT}",
    );
    let mut q = sqlx::query_scalar::<_, String>(&sql).bind(account_id);
    for (_, normalized) in &unique {
        q = q.bind(normalized);
    }
    let known: HashSet<String> = q.fetch_all(&mut *conn).await?.into_iter().collect();

    Ok(unique
        .into_iter()
        .filter(|(_, norm)| !known.contains(norm))
        .map(|(raw, _)| raw)
        .collect())
}
