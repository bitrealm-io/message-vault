//! Contact list/detail used by `GET /v1/contacts`,
//! `GET /v1/contacts/{id}` and `PATCH /v1/contacts/{id}`, `POST /v1/contacts/summaries`,
//! and `POST /v1/contacts/match`.

use std::collections::{HashMap, HashSet};

use crate::extract::{Json, Path as AxumPath, Query};
use anyhow::Result as AnyResult;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use message_ir::HandleType;
use serde::{Deserialize, Serialize};
use sqlx::AnyConnection;

use crate::db::contacts::{self, contact_id_for_handle};
use crate::db::dialect::{engine_of, group_concat_unit_separator, name_ci_expr};
use crate::db::handles::{infer_handle_type_from_shape, normalize_handle};
use crate::db::sql::{SqlParam, bind_args, in_placeholders, renumber_placeholders};
use crate::db::trash::{DeleteOutcome, Trashable, delete_trashed, move_to_trash, restore};
use crate::paging::{
    DEFAULT_LIST_LIMIT, Direction, MAX_CONTACT_SUMMARY_IDS, MAX_LIST_OFFSET, Page, PageQuery,
    SortKey, page_params, parse_sort,
};
use crate::search::emit::{NOT_TRASHED_CONTACT, NOT_TRASHED_CONVERSATION};
use crate::server::{ApiError, AppState, FullAccess, FullDeleteAccess, content_type_base};

/// Contact row for the list: name, handles, groups.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ContactSummary {
    /// Contact id.
    pub id: i64,
    /// Contact display name.
    pub name: String,
    /// Number of handles linked to the contact.
    pub handle_count: u64,
    /// Normalized (and raw when distinct) handle values for client-side filter.
    #[serde(default)]
    pub handles: Vec<String>,
    /// When the contact’s address-book shape last changed (`datetime('now')`).
    pub last_modified: String,
    /// Group names on this contact (A–Z).
    #[serde(default)]
    pub groups: Vec<String>,
}

/// One handle on a contact with service and message stats.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ContactHandleInfo {
    /// Normalized handle value.
    pub handle: String,
    /// Platform service, e.g. `whatsapp`, when the handle is linked with one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    /// Date of the first message involving this handle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_date: Option<String>,
    /// Date of the last message involving this handle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_date: Option<String>,
    /// 1:1 conversations this handle appears in.
    pub individual_conversations: u64,
    /// Group conversations this handle appears in.
    pub group_conversations: u64,
    /// Messages in 1:1 conversations involving this handle.
    pub individual_message_count: u64,
    /// Messages in group conversations involving this handle.
    pub group_message_count: u64,
}

/// A handle value plus optional platform service.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ContactHandlePayload {
    /// Handle value to link.
    pub handle: String,
    /// Platform service (`phone`, `email`, or `whatsapp`); inferred when omitted.
    #[serde(default)]
    pub service: Option<String>,
}

/// The previous and new handle values for a link change.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ContactUpdateHandlePayload {
    /// Handle value currently linked.
    pub previous_handle: String,
    /// Replacement handle value.
    pub handle: String,
    /// Platform service for the new handle.
    #[serde(default)]
    pub service: Option<String>,
}

/// The handle to unlink.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ContactRemoveHandlePayload {
    /// Handle value to unlink.
    pub handle: String,
    /// Platform service, when the handle is linked with one.
    #[serde(default)]
    pub service: Option<String>,
}

/// Body for `PATCH /v1/contacts/{id}`. Exactly one mutation field should be set.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ContactMutationBody {
    /// New display name; `None` leaves it unchanged.
    #[serde(default)]
    pub name: Option<String>,
    /// Handle link to add.
    #[serde(default)]
    pub add_handle: Option<ContactHandlePayload>,
    /// Handle link to replace.
    #[serde(default)]
    pub update_handle: Option<ContactUpdateHandlePayload>,
    /// Handle link to remove.
    #[serde(default)]
    pub remove_handle: Option<ContactRemoveHandlePayload>,
}

/// Full contact view: every handle with stats, plus totals across them.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ContactDetail {
    /// Contact id.
    pub id: i64,
    /// Contact display name.
    pub name: String,
    /// Every handle linked to the contact, with per-handle stats.
    pub handles: Vec<ContactHandleInfo>,
    /// 1:1 conversations the contact appears in.
    pub direct_conversations: u64,
    /// Group conversations the contact appears in.
    pub group_conversations: u64,
    /// Messages across all of the contact's conversations.
    pub total_messages: u64,
    /// When the contact’s address-book shape last changed (`datetime('now')`).
    pub last_modified: String,
    /// Group names on this contact (A–Z).
    #[serde(default)]
    pub groups: Vec<String>,
}

/// Body for `POST /v1/contacts/summaries`.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ContactSummariesBody {
    /// Contact ids to summarize; an empty list covers every contact.
    #[serde(default)]
    pub ids: Vec<i64>,
}

/// Contact-level first/last seen and message counts for the selection table.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ContactSelectionSummary {
    /// Contact id.
    pub id: i64,
    /// Contact display name.
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

/// Response for `POST /v1/contacts/summaries`.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ContactSummariesPage {
    /// One summary per requested contact.
    pub items: Vec<ContactSelectionSummary>,
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

/// One page of the contact list for `q`, a query in the search language.
///
/// # Errors
///
/// `BadRequest` for a query the language refuses; `Internal` when a
/// statement fails.
/// The keys `GET /v1/contacts` accepts in `sort=`. One today; a key with
/// no caller is a key not offered (last heard from is #497).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContactSort {
    /// Display name, case-folded.
    Name,
}

/// The accepted keys, as `sort=` spells them.
pub const CONTACT_SORT_KEYS: [(&str, ContactSort); 1] = [("name", ContactSort::Name)];

/// A to Z: what the list shows when `sort` is absent.
pub const DEFAULT_CONTACT_SORT: [SortKey<ContactSort>; 1] = [SortKey {
    key: ContactSort::Name,
    direction: Direction::Asc,
}];

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

    // `name` is a select-list alias and the sort applies lower() to it. SQLite
    // allows that; Postgres only allows a bare alias in ORDER BY, so the rows
    // are sorted as a derived table where `name` is a real column. `ct.id`
    // breaks ties in the same direction, so paging cannot repeat a row.
    let order_by = {
        let mut parts: Vec<String> = order
            .iter()
            .map(|k| match k.key {
                ContactSort::Name => {
                    format!("{} {}", name_ci_expr(engine, "name"), k.direction.sql())
                }
            })
            .collect();
        let tie = order.last().map_or(Direction::Asc, |k| k.direction);
        parts.push(format!("ct.id {}", tie.sql()));
        format!("ORDER BY {}", parts.join(", "))
    };
    let sql = renumber_placeholders(&format!(
        "SELECT * FROM (SELECT ct.id,
                COALESCE(NULLIF(trim(ct.preferred_name), ''), '(unknown)') AS name,
                (SELECT COUNT(*)
                 FROM contact_handles ch
                 WHERE ch.account_id = ct.account_id AND ch.contact_id = ct.id) AS handle_count,
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
                (SELECT {groups_agg}
                 FROM contact_group_members clm
                 JOIN contact_groups cl ON cl.id = clm.group_id
                 WHERE clm.contact_id = ct.id AND cl.account_id = ct.account_id) AS groups
         FROM contacts ct
         WHERE {where_sql}) AS ct
         {order_by}
         LIMIT ? OFFSET ?",
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
            |(id, name, handle_count, handles_blob, last_modified, groups_blob)| {
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
                    handle_count: handle_count.max(0) as u64,
                    handles,
                    last_modified,
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

type ContactRow = (i64, String, i64, Option<String>, String, Option<String>);

/// Full contact view: per-handle service + date range + direct message count,
/// plus conversation and total-message stats across all the contact's handles.
///
/// # Errors
///
/// Returns an internal error when a database statement fails.
pub async fn get_contact_detail(
    conn: &mut AnyConnection,
    account_id: i64,
    contact_id: i64,
) -> Result<Option<ContactDetail>, ApiError> {
    let Some((name, last_modified)) =
        contact_name_and_modified(conn, account_id, contact_id).await?
    else {
        return Ok(None);
    };
    let handles = contact_handle_stats(conn, account_id, contact_id).await?;
    let totals = contact_totals(conn, account_id, contact_id).await?;
    let contact_groups = crate::named_membership::names_for_item(
        crate::named_membership::group_spec(),
        conn,
        account_id,
        contact_id,
    )
    .await?;

    Ok(Some(ContactDetail {
        id: contact_id,
        name,
        handles,
        direct_conversations: totals.direct,
        group_conversations: totals.groups,
        total_messages: totals.messages,
        last_modified,
        groups: contact_groups,
    }))
}

/// The contact's display name (`(unknown)` when blank) and last-modified
/// stamp, or `None` when it is missing, another account's, or in the trash.
async fn contact_name_and_modified(
    conn: &mut AnyConnection,
    account_id: i64,
    contact_id: i64,
) -> Result<Option<(String, String)>, ApiError> {
    Ok(sqlx::query_as(&format!(
        "SELECT COALESCE(NULLIF(trim(preferred_name), ''), '(unknown)'),
                last_modified
         FROM contacts ct
         WHERE ct.id = $1 AND ct.account_id = $2
           AND {NOT_TRASHED_CONTACT}",
    ))
    .bind(contact_id)
    .bind(account_id)
    .fetch_optional(&mut *conn)
    .await?)
}

/// One row of [`contact_handle_stats`]: handle, service, first and last
/// timestamp, then direct and group conversation and message counts.
type ContactHandleRow = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
    i64,
    i64,
    i64,
);

impl From<ContactHandleRow> for ContactHandleInfo {
    fn from(
        (
            handle,
            service,
            start_date,
            end_date,
            individual_conversations,
            group_conversations,
            individual_message_count,
            group_message_count,
        ): ContactHandleRow,
    ) -> Self {
        Self {
            handle,
            service,
            start_date,
            end_date,
            individual_conversations: individual_conversations.max(0) as u64,
            group_conversations: group_conversations.max(0) as u64,
            individual_message_count: individual_message_count.max(0) as u64,
            group_message_count: group_message_count.max(0) as u64,
        }
    }
}

/// One entry per linked handle, ordered by handle: the date range and the
/// conversation and message counts over the direct and group conversations
/// that include it, trashed conversations excluded.
async fn contact_handle_stats(
    conn: &mut AnyConnection,
    account_id: i64,
    contact_id: i64,
) -> Result<Vec<ContactHandleInfo>, ApiError> {
    let rows: Vec<ContactHandleRow> = sqlx::query_as(&format!(
        "SELECT h.raw,
                    NULLIF(trim(h.service), '') AS service,
                    MIN(m.timestamp) AS first_ts,
                    MAX(m.timestamp) AS last_ts,
                    COUNT(DISTINCT CASE WHEN c.conversation_type = 'individual' THEN c.id END),
                    COUNT(DISTINCT CASE WHEN c.conversation_type = 'group' THEN c.id END),
                    COUNT(DISTINCT CASE WHEN c.conversation_type = 'individual' THEN m.id END),
                    COUNT(DISTINCT CASE WHEN c.conversation_type = 'group' THEN m.id END)
             FROM contact_handles ch
             JOIN handles h ON h.id = ch.handle_id
             LEFT JOIN conversations c ON c.account_id = ch.account_id
               AND (c.chat_handle_id = ch.handle_id
                    OR EXISTS (
                      SELECT 1 FROM participants p
                      WHERE p.conversation_id = c.id AND p.handle_id = ch.handle_id
                    ))
               AND {NOT_TRASHED_CONVERSATION}
             LEFT JOIN messages m ON m.conversation_id = c.id AND m.duplicate_of IS NULL
             WHERE ch.account_id = $1 AND ch.contact_id = $2
             GROUP BY ch.handle_id, h.raw, h.service
             ORDER BY h.raw",
    ))
    .bind(account_id)
    .bind(contact_id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows.into_iter().map(ContactHandleInfo::from).collect())
}

/// Conversation and message counts across every handle of one contact.
struct ContactTotals {
    direct: u64,
    groups: u64,
    messages: u64,
}

/// Counts over the conversations the contact is in, trashed ones excluded.
/// Scoped to those conversations on purpose: grouping the whole account's
/// messages table dominated drawer latency.
async fn contact_totals(
    conn: &mut AnyConnection,
    account_id: i64,
    contact_id: i64,
) -> Result<ContactTotals, ApiError> {
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
                   COALESCE(NULLIF(trim(ct.preferred_name), ''), '(unknown)') AS name
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

/// Most identifiers one request to `POST /v1/contacts/match` may ask about.
///
/// A staged folder can reference thousands of participants; the client
/// batches. The query runs as a single statement with no chunking, so the
/// cap keeps its bind count bounded (501 identifiers would mean 502 binds);
/// raising it needs `SQLITE_IN_CHUNK`-style chunking first, not just a
/// bigger number.
pub(crate) const MAX_MATCH_IDENTIFIERS: usize = 500;

/// Body for `POST /v1/contacts/unmatched-handles`.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct UnmatchedHandlesBody {
    /// Raw identifiers — phone numbers, emails — as they appear in an export.
    identifiers: Vec<String>,
}

/// Response for `POST /v1/contacts/unmatched-handles`.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct UnmatchedHandlesResponse {
    /// The subset this account has no contact for: trimmed, in first-seen
    /// order, blanks dropped and duplicates (by normalized form) collapsed
    /// to their first spelling.
    unknown: Vec<String>,
}

/// Which of `identifiers` this account has no contact for.
///
/// A trashed contact still counts as known: trash sets a person aside, it
/// does not make them absent, and an import that meets their handle reuses
/// that contact rather than creating a second one for the same person.
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
async fn unknown_contact_identifiers(
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
           AND h.normalized IN ({placeholders})",
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

/// Largest address book the load route accepts, in bytes.
///
/// A phone's contacts export is measured in tens of kilobytes; a few megabytes
/// is already far past any real address book, and the whole file is read into
/// memory before parsing.
pub(crate) const MAX_ADDRESS_BOOK_BYTES: usize = 8 * 1024 * 1024;

/// What loading an address book changed.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct AddressBookLoadResponse {
    /// Contacts written from the file.
    pub contacts: u64,
    /// Phone identities linked to those contacts.
    pub phones: u64,
    /// Identities written with a review note (an ambiguous number).
    pub phones_needing_review: u64,
}

/// Load a VCF or vCard CSV address book into this account. The body is the
/// file itself, and `Content-Type` says which: `text/vcard` or `text/csv`.
///
/// This is a standalone act against the vault, never part of an Import Run:
/// contacts are vault state, and a person may load them before or after
/// bringing messages in. Only the rows the address book owns are replaced, so
/// Contact Groups, names the person typed, and identities an import discovered
/// all survive. How the file is read is the open question in #270; this route
/// is where that answer lands.
#[utoipa::path(
    post,
    path = "/v1/contacts",
    tag = "Contacts",
    security(("bearer" = [])),
    request_body(
        content(
            ("text/vcard"),
            ("text/csv")
        ),
        description = "The address book file: a vCard file as text/vcard, or a vCard CSV export as text/csv."
    ),
    responses(
        (status = 200, body = AddressBookLoadResponse),
        (status = 400, body = crate::problem::Problem),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 413, body = crate::problem::Problem),
        (status = 415, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem)
    )
)]
pub(crate) async fn contacts_create_handler(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Json<AddressBookLoadResponse>, ApiError> {
    let Some(name) = address_book_file_name(content_type_base(&headers)) else {
        return Err(ApiError::UnsupportedMediaType(
            "Content-Type must be text/vcard or text/csv".into(),
        ));
    };
    if body.len() > MAX_ADDRESS_BOOK_BYTES {
        return Err(ApiError::PayloadTooLarge(format!(
            "address book is larger than {MAX_ADDRESS_BOOK_BYTES} bytes"
        )));
    }
    let content = std::str::from_utf8(&body)
        .map_err(|_| ApiError::MalformedBody("address book is not UTF-8 text".into()))?;
    if content.trim().is_empty() {
        return Err(ApiError::validation("address book is empty"));
    }
    // The loader detects VCF versus vCard CSV from the path, so the upload is
    // written to a temp file under the name its media type earns.
    let dir = tempfile::tempdir()
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("create temp dir: {e}")))?;
    let path = dir.path().join(name);
    std::fs::write(&path, content.as_bytes())
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("write address book: {e}")))?;

    let mut conn = state.db.acquire().await?;
    let stats = contacts::load_contacts_if_needed(&mut conn, Some(&path), true, auth.account_id)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("load address book: {e}")))?;
    Ok(Json(AddressBookLoadResponse {
        contacts: stats.contacts,
        phones: stats.phones,
        phones_needing_review: stats.phones_needing_review,
    }))
}

/// The temp file name a media type earns, so the loader's extension check
/// reads the format the client declared. `text/x-vcard` is the older
/// spelling some exporters still write.
fn address_book_file_name(content_type: Option<&str>) -> Option<&'static str> {
    match content_type.map(str::to_ascii_lowercase).as_deref() {
        Some("text/vcard" | "text/x-vcard") => Some("address-book.vcf"),
        Some("text/csv") => Some("address-book.csv"),
        _ => None,
    }
}

/// Report which identifiers this account has no vault contact for.
#[utoipa::path(
    post,
    path = "/v1/contacts/unmatched-handles",
    tag = "Contacts",
    security(("bearer" = [])),
    request_body = UnmatchedHandlesBody,
    responses(
        (status = 200, body = UnmatchedHandlesResponse),
        (status = 400, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem)
    )
)]
pub(crate) async fn unmatched_handles_handler(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    Json(body): Json<UnmatchedHandlesBody>,
) -> Result<Json<UnmatchedHandlesResponse>, ApiError> {
    if body.identifiers.len() > MAX_MATCH_IDENTIFIERS {
        return Err(ApiError::validation(format!(
            "at most {MAX_MATCH_IDENTIFIERS} identifiers"
        )));
    }
    let mut conn = state.db.acquire().await?;
    let unknown =
        unknown_contact_identifiers(&mut conn, auth.account_id, &body.identifiers).await?;
    Ok(Json(UnmatchedHandlesResponse { unknown }))
}

/// Handle type from the service the caller named, falling back to the handle's shape.
fn infer_handle_type(raw: &str, service: Option<&str>) -> HandleType {
    let svc = service
        .map(|s| s.trim().to_ascii_lowercase())
        .unwrap_or_default();
    match svc.as_str() {
        "phone" | "sms" | "imessage" | "whatsapp" => HandleType::Phone,
        "email" => HandleType::Email,
        "" => infer_handle_type_from_shape(raw),
        _ => HandleType::Other,
    }
}

/// Why a contact edit did not happen.
///
/// The two cases answer with different statuses and the difference is not one
/// a message can be read for, so it is carried in the type. It used to be
/// guessed: `mutate_contact` returned `anyhow` and the handler downcast the
/// error, calling it a 400 unless it found a `sqlx::Error` underneath. That
/// made "not a database error" mean "the person's fault", so any other
/// internal failure — one wrapped in `context`, one from a helper — reached
/// the person as a 400 wearing an internal sentence.
#[derive(Debug)]
pub enum ContactEditError {
    /// The request asks for something the vault will not do, and the person
    /// can fix it by changing the request. The sentence is written for them.
    Refused(String),
    /// Something failed that changing the request would not help. The cause
    /// goes to the log, not to the person.
    Failed(anyhow::Error),
}

impl From<sqlx::Error> for ContactEditError {
    fn from(error: sqlx::Error) -> Self {
        Self::Failed(error.into())
    }
}

/// Anything a helper hands back through `?` is a failure, not a refusal: a
/// refusal is raised deliberately, here, as [`ContactEditError::Refused`].
impl From<anyhow::Error> for ContactEditError {
    fn from(error: anyhow::Error) -> Self {
        Self::Failed(error)
    }
}

impl From<ContactEditError> for ApiError {
    fn from(error: ContactEditError) -> Self {
        match error {
            ContactEditError::Refused(message) => Self::validation(message),
            ContactEditError::Failed(cause) => Self::Internal(cause),
        }
    }
}

/// Shorthand for the eight things a contact edit refuses.
macro_rules! refuse {
    ($($arg:tt)*) => {
        return Err(ContactEditError::Refused(format!($($arg)*)))
    };
}

/// The one edit a `PATCH /v1/contacts/{id}` body asks for.
enum ContactEdit<'a> {
    /// Give the contact a name the person typed.
    Rename(&'a str),
    /// Link a handle.
    AddHandle(&'a ContactHandlePayload),
    /// Swap one linked handle for another.
    UpdateHandle(&'a ContactUpdateHandlePayload),
    /// Unlink a handle.
    RemoveHandle(&'a ContactRemoveHandlePayload),
}

impl ContactMutationBody {
    /// The single edit the body asks for.
    ///
    /// # Errors
    ///
    /// Refused when the body sets none of the four fields or more than one.
    fn edit(&self) -> Result<ContactEdit<'_>, ContactEditError> {
        let mut edits = [
            self.name.as_deref().map(ContactEdit::Rename),
            self.add_handle.as_ref().map(ContactEdit::AddHandle),
            self.update_handle.as_ref().map(ContactEdit::UpdateHandle),
            self.remove_handle.as_ref().map(ContactEdit::RemoveHandle),
        ]
        .into_iter()
        .flatten();
        match (edits.next(), edits.next()) {
            (Some(edit), None) => Ok(edit),
            _ => {
                refuse!("exactly one of name, add_handle, update_handle, remove_handle is required")
            }
        }
    }
}

/// Apply a contact mutation. Returns false when the contact is missing.
///
/// # Errors
///
/// Returns an error when the mutation is invalid or a database write fails.
pub async fn mutate_contact(
    conn: &mut AnyConnection,
    account_id: i64,
    contact_id: i64,
    body: &ContactMutationBody,
) -> Result<bool, ContactEditError> {
    let mut editor = ContactEditor {
        conn,
        account_id,
        contact_id,
    };
    if !editor.exists().await? {
        return Ok(false);
    }
    editor.apply(body.edit()?).await
}

/// One contact of one account under edit. Every edit reads and writes the
/// contact's handle links, so the three things they all need live here and
/// the edits are methods.
struct ContactEditor<'a> {
    conn: &'a mut AnyConnection,
    account_id: i64,
    contact_id: i64,
}

impl ContactEditor<'_> {
    /// True when the contact belongs to this account and is not in the trash.
    async fn exists(&mut self) -> AnyResult<bool> {
        let found: Option<i64> = sqlx::query_scalar(&format!(
            "SELECT ct.id
             FROM contacts ct
             WHERE ct.id = $1 AND ct.account_id = $2
               AND {NOT_TRASHED_CONTACT}",
        ))
        .bind(self.contact_id)
        .bind(self.account_id)
        .fetch_optional(&mut *self.conn)
        .await?;
        Ok(found.is_some())
    }

    /// Apply the edit. `true` once the contact is as the edit asked, whether
    /// or not anything had to change.
    async fn apply(&mut self, edit: ContactEdit<'_>) -> Result<bool, ContactEditError> {
        match edit {
            ContactEdit::Rename(name) => self.rename(name).await,
            ContactEdit::AddHandle(add) => self.add_handle(add).await,
            ContactEdit::UpdateHandle(upd) => self.update_handle(upd).await,
            ContactEdit::RemoveHandle(rem) => self.remove_handle(rem).await,
        }
    }

    /// Name the contact.
    async fn rename(&mut self, name: &str) -> Result<bool, ContactEditError> {
        let name = name.trim();
        if name.is_empty() {
            refuse!("name must not be empty");
        }
        // Typing a name in the drawer is the most deliberate naming act in
        // the product, so the row stops being the import's and becomes the
        // person's. `contacts::propose_name` is where that rule lives, along
        // with what it means for the import and the address book.
        contacts::propose_name(
            &mut *self.conn,
            self.account_id,
            self.contact_id,
            name,
            contacts::Origin::User,
        )
        .await?;
        Ok(true)
    }

    /// Link a handle, creating its row when the vault has never seen it.
    async fn add_handle(&mut self, add: &ContactHandlePayload) -> Result<bool, ContactEditError> {
        let raw = add.handle.trim();
        if raw.is_empty() {
            refuse!("handle must not be empty");
        }
        let handle_id = self.handle_row(raw, add.service.as_deref()).await?;
        if self.claim(handle_id).await? {
            // Already linked: no address-book change.
            return Ok(true);
        }
        // The person attached this identity themselves, so a later address
        // book load leaves it alone.
        contacts::link_handle_to_contact(
            &mut *self.conn,
            self.account_id,
            handle_id,
            self.contact_id,
            contacts::Origin::User,
        )
        .await?;
        self.touched().await
    }

    /// Replace one linked handle with another.
    async fn update_handle(
        &mut self,
        upd: &ContactUpdateHandlePayload,
    ) -> Result<bool, ContactEditError> {
        let prev = upd.previous_handle.trim();
        let next = upd.handle.trim();
        if prev.is_empty() || next.is_empty() {
            refuse!("previous_handle and handle must not be empty");
        }
        let service = upd.service.as_deref();
        let Some(old_id) = self.linked_handle(prev, service).await? else {
            refuse!("previous handle not found on contact");
        };
        let new_id = self.handle_row(next, service).await?;
        if old_id == new_id {
            return self.retype_handle(new_id, service).await;
        }
        if self.claim(new_id).await? {
            // The new handle is already on this contact, so the edit amounts
            // to dropping the previous one.
            self.unlink(old_id).await?;
        } else {
            sqlx::query(
                "UPDATE contact_handles SET handle_id = $1
                 WHERE account_id = $2 AND contact_id = $3 AND handle_id = $4",
            )
            .bind(new_id)
            .bind(self.account_id)
            .bind(self.contact_id)
            .bind(old_id)
            .execute(&mut *self.conn)
            .await?;
        }
        self.touched().await
    }

    /// The same handle named twice: only a given service changes.
    async fn retype_handle(
        &mut self,
        handle_id: i64,
        service: Option<&str>,
    ) -> Result<bool, ContactEditError> {
        let Some(service) = service.and_then(message_ir::trimmed) else {
            return Ok(true);
        };
        sqlx::query("UPDATE handles SET service = $1 WHERE id = $2")
            .bind(service)
            .bind(handle_id)
            .execute(&mut *self.conn)
            .await?;
        self.touched().await
    }

    /// Unlink a handle. The handle row itself stays: messages still cite it.
    async fn remove_handle(
        &mut self,
        rem: &ContactRemoveHandlePayload,
    ) -> Result<bool, ContactEditError> {
        let raw = rem.handle.trim();
        if raw.is_empty() {
            refuse!("handle must not be empty");
        }
        let Some(handle_id) = self.linked_handle(raw, rem.service.as_deref()).await? else {
            refuse!("handle not found on contact");
        };
        self.unlink(handle_id).await?;
        self.touched().await
    }

    /// Id of the handle row for `raw` that is linked to this contact, if any.
    /// With a service, only that service's row; without one, the phone row
    /// first, then WhatsApp, then anything else.
    async fn linked_handle(&mut self, raw: &str, service: Option<&str>) -> AnyResult<Option<i64>> {
        let needle = raw.trim();
        if needle.is_empty() {
            return Ok(None);
        }
        let mut sql = String::from(
            "SELECT ch.handle_id
             FROM contact_handles ch
             JOIN handles h ON h.id = ch.handle_id
             WHERE ch.account_id = $1 AND ch.contact_id = $2
               AND (h.raw = $3 OR h.normalized = $3)",
        );
        let id = if let Some(svc) = service.and_then(message_ir::trimmed) {
            sql.push_str(" AND h.service = $4 LIMIT 1");
            let platform = message_ir::HandleService::parse(svc);
            sqlx::query_scalar::<_, i64>(&sql)
                .bind(self.account_id)
                .bind(self.contact_id)
                .bind(needle)
                .bind(platform.as_str())
                .fetch_optional(&mut *self.conn)
                .await?
        } else {
            sql.push_str(
                " ORDER BY CASE h.service WHEN 'phone' THEN 0 WHEN 'whatsapp' THEN 1 ELSE 2 END
                 LIMIT 1",
            );
            sqlx::query_scalar::<_, i64>(&sql)
                .bind(self.account_id)
                .bind(self.contact_id)
                .bind(needle)
                .fetch_optional(&mut *self.conn)
                .await?
        };
        Ok(id)
    }

    /// Insert or find the handle row for `raw`, typed by `service`, without
    /// linking it to the account owner: contact-owned handles must never
    /// become owner identities.
    async fn handle_row(&mut self, raw: &str, service: Option<&str>) -> AnyResult<i64> {
        let handle_type = infer_handle_type(raw, service);
        let (id, _) = crate::db::handles::upsert_handle_row(
            &mut *self.conn,
            self.account_id,
            raw.trim(),
            handle_type,
            service.and_then(message_ir::trimmed),
        )
        .await?;
        Ok(id)
    }

    /// True when the handle is already linked to this contact. A handle
    /// belongs to one contact per account (the primary key on
    /// `contact_handles`), so one linked elsewhere is refused.
    async fn claim(&mut self, handle_id: i64) -> Result<bool, ContactEditError> {
        match contact_id_for_handle(&mut *self.conn, self.account_id, handle_id).await? {
            Some(owner) if owner == self.contact_id => Ok(true),
            Some(_) => refuse!("handle already linked to another contact"),
            None => Ok(false),
        }
    }

    /// Drop the link between the contact and the handle.
    async fn unlink(&mut self, handle_id: i64) -> AnyResult<()> {
        sqlx::query(
            "DELETE FROM contact_handles
             WHERE account_id = $1 AND contact_id = $2 AND handle_id = $3",
        )
        .bind(self.account_id)
        .bind(self.contact_id)
        .bind(handle_id)
        .execute(&mut *self.conn)
        .await?;
        Ok(())
    }

    /// Bump the contact's updated-at and report success, for edits that
    /// changed the links but not the contact row.
    async fn touched(&mut self) -> Result<bool, ContactEditError> {
        contacts::touch_contact(&mut *self.conn, self.account_id, self.contact_id).await?;
        Ok(true)
    }
}

/// Page through the account's contacts (id, name, handles, groups).
#[utoipa::path(
    get,
    path = "/v1/contacts",
    tag = "Contacts",
    security(("bearer" = [])),
    params(
        ("q" = Option<String>, Query, description = "Contact search; empty lists all"),
        ("limit" = Option<usize>, Query, description = "Page size, default 40, max 500"),
        ("offset" = Option<usize>, Query, description = "Page offset, max 50000"),
        ("sort" = Option<String>, Query, description = "`name` or `-name`. Default `name`.")
    ),
    responses(
        (status = 200, body = Page<ContactSummary>),
        (status = 400, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem)
    )
)]
pub(crate) async fn contacts_list_handler(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    Query(query): Query<PageQuery>,
) -> Result<Json<Page<ContactSummary>>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let q = query.q.unwrap_or_default();
    let page = page_params(
        query.limit,
        query.offset,
        DEFAULT_LIST_LIMIT,
        Some(MAX_LIST_OFFSET),
    )?;
    let order = parse_sort(
        query.sort.as_deref(),
        &CONTACT_SORT_KEYS,
        &DEFAULT_CONTACT_SORT,
    )?;
    let clock = crate::db::account_profile::account_clock(&mut conn, auth.account_id).await?;
    let result = list_contacts_sorted(
        &mut conn,
        auth.account_id,
        &q,
        &order,
        page.limit,
        page.offset,
        clock,
    )
    .await?;
    Ok(Json(result))
}

/// First/last message dates and counts for a list of contact ids.
#[utoipa::path(
    post,
    path = "/v1/contacts/summaries",
    tag = "Contacts",
    security(("bearer" = [])),
    request_body = ContactSummariesBody,
    responses(
        (status = 200, body = ContactSummariesPage),
        (status = 400, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem)
    )
)]
pub(crate) async fn contact_summaries_handler(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    Json(body): Json<ContactSummariesBody>,
) -> Result<Json<ContactSummariesPage>, ApiError> {
    if body.ids.len() > MAX_CONTACT_SUMMARY_IDS {
        return Err(ApiError::validation(format!(
            "at most {MAX_CONTACT_SUMMARY_IDS} contact ids"
        )));
    }
    let mut conn = state.db.acquire().await?;
    let page = get_contact_summaries(&mut conn, auth.account_id, &body.ids)
        .await
        .map(|items| ContactSummariesPage { items })?;
    Ok(Json(page))
}

/// Full contact view: per-handle services, message stats, and group
/// memberships.
#[utoipa::path(
    get,
    path = "/v1/contacts/{id}",
    tag = "Contacts",
    security(("bearer" = [])),
    params(("id" = i64, Path, description = "Contact id")),
    responses(
        (status = 200, body = ContactDetail),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem)
    )
)]
pub(crate) async fn contact_detail_handler(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    AxumPath(contact_id): AxumPath<i64>,
) -> Result<Json<ContactDetail>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let detail = get_contact_detail(&mut conn, auth.account_id, contact_id).await?;
    detail
        .map(Json)
        .ok_or_else(|| ApiError::NotFound("contact not found".into()))
}

/// Rename a contact or change its linked handles.
#[utoipa::path(
    patch,
    path = "/v1/contacts/{id}",
    tag = "Contacts",
    security(("bearer" = [])),
    params(("id" = i64, Path, description = "Contact id")),
    request_body = ContactMutationBody,
    responses(
        (status = 200, body = ContactDetail),
        (status = 400, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem)
    )
)]
pub(crate) async fn contact_mutate_handler(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    AxumPath(contact_id): AxumPath<i64>,
    Json(body): Json<ContactMutationBody>,
) -> Result<Json<ContactDetail>, ApiError> {
    let mut conn = state.db.acquire().await?;
    match mutate_contact(&mut conn, auth.account_id, contact_id, &body).await {
        Ok(false) => Err(ApiError::NotFound("contact not found".into())),
        Err(e) => Err(e.into()),
        Ok(true) => get_contact_detail(&mut conn, auth.account_id, contact_id)
            .await?
            .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("contact missing after mutate")))
            .map(Json),
    }
}

/// Put a contact in the trash. Idempotent: trashing an already-trashed
/// contact still answers 204.
#[utoipa::path(
    post,
    path = "/v1/contacts/{id}/trash",
    tag = "Contacts",
    security(("bearer" = [])),
    params(("id" = i64, Path, description = "Contact id")),
    responses(
        (status = 204, description = "Trashed"),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem)
    )
)]
pub(crate) async fn contact_trash_handler(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    AxumPath(contact_id): AxumPath<i64>,
) -> Result<StatusCode, ApiError> {
    let mut conn = state.db.acquire().await?;
    if move_to_trash(&mut conn, auth.account_id, Trashable::Contact(contact_id)).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound("contact not found".into()))
    }
}

/// Take a contact out of the trash. Idempotent: restoring a contact that
/// was not trashed still answers 204.
#[utoipa::path(
    post,
    path = "/v1/contacts/{id}/restore",
    tag = "Contacts",
    security(("bearer" = [])),
    params(("id" = i64, Path, description = "Contact id")),
    responses(
        (status = 204, description = "Restored"),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem)
    )
)]
pub(crate) async fn contact_restore_handler(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    AxumPath(contact_id): AxumPath<i64>,
) -> Result<StatusCode, ApiError> {
    let mut conn = state.db.acquire().await?;
    if restore(&mut conn, auth.account_id, Trashable::Contact(contact_id)).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound("contact not found".into()))
    }
}

/// Delete a trashed contact the way a phone's Delete Contact does: the name
/// and the person's edits go, the contact becomes Unknown again and leaves
/// the trash, and every conversation it was in stays as it is, showing the
/// handle. Conversations are never deleted with a contact. A contact that is
/// not in the trash answers 409.
#[utoipa::path(
    delete,
    path = "/v1/contacts/{id}",
    tag = "Contacts",
    security(("bearer" = [])),
    params(("id" = i64, Path, description = "Contact id")),
    responses(
        (status = 204, description = "Deleted: the contact is Unknown again"),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem),
        (status = 409, body = crate::problem::Problem, description = "The contact is not in the trash")
    )
)]
pub(crate) async fn contact_delete_handler(
    State(state): State<AppState>,
    FullDeleteAccess(auth): FullDeleteAccess,
    AxumPath(contact_id): AxumPath<i64>,
) -> Result<StatusCode, ApiError> {
    let mut conn = state.db.acquire().await?;
    match delete_trashed(&mut conn, auth.account_id, Trashable::Contact(contact_id)).await? {
        // A contact owns no files, so there is nothing to remove from disk.
        DeleteOutcome::Deleted(_) => Ok(StatusCode::NO_CONTENT),
        DeleteOutcome::NotOwned => Err(ApiError::NotFound("contact not found".into())),
        DeleteOutcome::NotTrashed => Err(ApiError::StateConflict(
            "the contact is not in the trash; move it to the trash first".into(),
        )),
    }
}

#[cfg(test)]
mod tests;
