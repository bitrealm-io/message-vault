//! Shared handle identity helpers (same format for matching + infer type from shape).

use std::collections::HashMap;

use anyhow::Result;
use message_ir::{HandleService, HandleType};
use serde::Serialize;
use sqlx::AnyConnection;

use crate::search::bridge::{TrashScope, contact_sent_messages_from};

/// `(account_id, normalized, handle_type, service)` → handle id for one import.
pub type HandleIdCache = HashMap<(String, String, String, String), i64>;

/// One standard form of a handle for identity matching, per type, plus a
/// human-readable note when that form is ambiguous (guarded policy).
///
/// Phone: E.164 when the raw is unambiguous (`+`-prefixed, or a US national
/// number); otherwise digits-as-is with a review note — a trunk-zero
/// `020 7946 0000` becomes `02079460000` flagged, never `+02079460000`.
/// Email: lowercased. Username/Other: verbatim (trimmed).
pub fn normalize_handle(raw: &str, handle_type: HandleType) -> (String, Option<String>) {
    phone::normalize_typed_handle(raw, handle_type)
}

/// Infer a handle type from the handle's shape when the source does not say.
///
/// Mirrors the shared rule in message-ir-format: `@` → Email; digit-heavy
/// phone-shaped strings → Phone (covers SMS/iMessage/WhatsApp numbers);
/// anything else (Discord usernames, group chat ids) → Other.
pub fn infer_handle_type_from_shape(handle: &str) -> HandleType {
    let h = handle.trim();
    if h.contains('@') {
        return HandleType::Email;
    }
    let has_digit = h.bytes().any(|b| b.is_ascii_digit());
    let all_phone_chars = h.bytes().all(|b| {
        b.is_ascii_digit() || matches!(b, b'+' | b'-' | b' ' | b'(' | b')' | b'.' | b'#' | b'*')
    });
    if !h.is_empty() && has_digit && all_phone_chars {
        return HandleType::Phone;
    }
    HandleType::Other
}

/// Insert or reuse a `handles` row. Returns the id and whether this call newly
/// inserted a flagged (review-note) row.
pub async fn upsert_handle_row(
    conn: &mut AnyConnection,
    account_id: i64,
    raw: &str,
    handle_type: HandleType,
    service: Option<&str>,
) -> Result<(i64, bool)> {
    let (normalized, note) = normalize_handle(raw, handle_type);
    let platform = HandleService::parse(service.unwrap_or(HandleService::Phone.as_str()));
    let service_str = platform.as_str();
    let inserted = sqlx::query(
        "INSERT INTO handles (account_id, raw, normalized, normalized_note, handle_type, service)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT DO NOTHING",
    )
    .bind(account_id)
    .bind(raw)
    .bind(normalized.as_str())
    .bind(note.as_deref())
    .bind(handle_type.as_str())
    .bind(service_str)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    let id: i64 = sqlx::query_scalar(
        "SELECT id FROM handles
         WHERE account_id = $1 AND normalized = $2 AND handle_type = $3 AND service = $4",
    )
    .bind(account_id)
    .bind(normalized.as_str())
    .bind(handle_type.as_str())
    .bind(service_str)
    .fetch_one(&mut *conn)
    .await?;
    Ok((id, inserted > 0 && note.is_some()))
}

/// Same as [`upsert_handle_row`], but skip the two SQL statements when this
/// import already resolved the same identity. Third value is `true` on a
/// cache hit so callers can skip leftover per-row work (sibling contact link).
pub async fn upsert_handle_row_cached(
    conn: &mut AnyConnection,
    cache: &mut HandleIdCache,
    account_id: i64,
    raw: &str,
    handle_type: HandleType,
    service: Option<&str>,
) -> Result<(i64, bool, bool)> {
    let (normalized, _) = normalize_handle(raw, handle_type);
    let platform = HandleService::parse(service.unwrap_or(HandleService::Phone.as_str()));
    let key = (
        account_id.to_string(),
        normalized,
        handle_type.as_str().to_string(),
        platform.as_str().to_string(),
    );
    if let Some(&id) = cache.get(&key) {
        return Ok((id, false, true));
    }
    let (id, flagged) = upsert_handle_row(conn, account_id, raw, handle_type, service).await?;
    cache.insert(key, id);
    Ok((id, flagged, false))
}

/// One identity of a contact or an account, and its messages: for a contact,
/// the messages the contact sent from it; for an account, the messages held
/// at it, sent from or received at that address (ADR-0015). The contact
/// drawer and the Profile screen show the same table, so they read the same
/// row.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Identity {
    /// The identity as the vault stores it: E.164 for a number, lower case
    /// for an address.
    pub address: String,
    /// `phone`, `email`, or `whatsapp`.
    pub service: String,
    /// When the identity's oldest message was sent, or null when there is
    /// none.
    pub start_date: Option<String>,
    /// When the newest such message was sent, or null when there is none.
    pub end_date: Option<String>,
    /// Direct and group conversations: for a contact, those the identity
    /// takes part in, whoever wrote in them; for an account, those holding
    /// at least one of the identity's messages. Trashed conversations are
    /// excluded.
    pub conversations: u64,
    /// The identity's messages in one-to-one conversations, trashed
    /// conversations and duplicates excluded.
    pub direct_messages: u64,
    /// The identity's messages in group conversations, on the same terms.
    pub group_messages: u64,
}

/// Whose identities [`identities`] reads.
#[derive(Debug, Clone, Copy)]
pub enum IdentitiesOf {
    /// The identities the account holds as its own.
    Account(i64),
    /// The identities linked to one of the account's contacts.
    Contact { account_id: i64, contact_id: i64 },
}

/// One row of [`identities`]: address, service, first and last timestamp,
/// conversation count, direct and group message counts.
type IdentityRow = (
    String,
    String,
    Option<String>,
    Option<String>,
    i64,
    i64,
    i64,
);

/// The identities of an account or of a contact, each with its messages,
/// phones before emails and each in order.
pub async fn identities(conn: &mut AnyConnection, of: IdentitiesOf) -> Result<Vec<Identity>> {
    let not_trashed = crate::search::emit::NOT_TRASHED_CONVERSATION;
    // `messages` is a `FROM … WHERE …` over identity `l`'s messages. It ends
    // in its WHERE clause, so a column can narrow it by conversation type.
    //
    // The account holder is never a participant, so an account's identity
    // counts the messages held at it and the conversations holding them
    // (ADR-0015). A contact's identity counts the messages it sent, by the
    // one definition every contact count reads, and the conversations it
    // takes part in, whoever wrote in them (#913).
    let (account_id, linked, scope, messages, conversations) = match of {
        IdentitiesOf::Account(account_id) => {
            let messages = format!(
                "FROM messages m
                   JOIN conversations c ON c.id = m.conversation_id AND {not_trashed}
                 WHERE m.owner_handle_id = l.handle_id AND m.account_id = $1
                   AND m.duplicate_of IS NULL"
            );
            let conversations = format!("(SELECT COUNT(DISTINCT c.id) {messages})");
            (
                account_id,
                "SELECT handle_id FROM account_handles WHERE account_id = $1",
                "",
                messages,
                conversations,
            )
        }
        IdentitiesOf::Contact { account_id, .. } => (
            account_id,
            "SELECT handle_id FROM contact_handles WHERE account_id = $1 AND contact_id = $2",
            "JOIN contacts ct ON ct.account_id = $1 AND ct.id = $2",
            contact_sent_messages_from("l.handle_id", TrashScope::LeftOut),
            format!(
                "(SELECT COUNT(*) FROM conversations c
                  WHERE c.account_id = $1
                    AND (c.chat_handle_id = l.handle_id
                         OR EXISTS (
                           SELECT 1 FROM participants p
                           WHERE p.conversation_id = c.id AND p.handle_id = l.handle_id
                         ))
                    AND {not_trashed})"
            ),
        ),
    };
    let sql = format!(
        "WITH linked AS ({linked})
         SELECT h.normalized,
                CASE WHEN h.handle_type = 'email' THEN 'email'
                     WHEN h.service = 'whatsapp' THEN 'whatsapp'
                     ELSE 'phone' END AS service,
                (SELECT MIN(m.timestamp) {messages}),
                (SELECT MAX(m.timestamp) {messages}),
                {conversations},
                (SELECT COUNT(*) {messages} AND c.conversation_type = 'individual'),
                (SELECT COUNT(*) {messages} AND c.conversation_type = 'group')
         FROM linked l
         JOIN handles h ON h.id = l.handle_id
         {scope}
         ORDER BY CASE WHEN h.handle_type = 'email' THEN 1 ELSE 0 END, h.normalized",
    );
    let mut query = sqlx::query_as::<_, IdentityRow>(&sql).bind(account_id);
    if let IdentitiesOf::Contact { contact_id, .. } = of {
        query = query.bind(contact_id);
    }
    let rows = query.fetch_all(&mut *conn).await?;
    Ok(rows
        .into_iter()
        .map(
            |(handle, service, start_date, end_date, conversations, direct, group)| Identity {
                address: handle,
                service,
                start_date,
                end_date,
                conversations: conversations.max(0) as u64,
                direct_messages: direct.max(0) as u64,
                group_messages: group.max(0) as u64,
            },
        )
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema;

    const TEST_ACCOUNT: i64 = 7;

    #[tokio::test]
    async fn upsert_handle_row_cached_reuses_id_without_second_row() {
        let (pool, _dir) = crate::db::engine::test_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        schema::ensure_vault_schema(&mut conn).await.unwrap();
        crate::db::account_profile::ensure_account_row(&mut conn, TEST_ACCOUNT)
            .await
            .unwrap();
        let mut cache = HandleIdCache::new();
        let (first, _, first_cached) = upsert_handle_row_cached(
            &mut conn,
            &mut cache,
            TEST_ACCOUNT,
            "+15555550100",
            HandleType::Phone,
            Some("phone"),
        )
        .await
        .unwrap();
        let (second, flagged, second_cached) = upsert_handle_row_cached(
            &mut conn,
            &mut cache,
            TEST_ACCOUNT,
            "+15555550100",
            HandleType::Phone,
            Some("phone"),
        )
        .await
        .unwrap();
        assert_eq!(first, second);
        assert!(!first_cached);
        assert!(!flagged);
        assert!(second_cached);
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM handles WHERE account_id = $1 AND normalized = '+15555550100'",
        )
        .bind(TEST_ACCOUNT)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
        assert_eq!(n, 1);
    }
}
