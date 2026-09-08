//! Shared handle identity helpers (same format for matching + infer type from shape).

use std::collections::HashMap;

use anyhow::Result;
use message_ir::{HandleService, HandleType};
use sqlx::AnyConnection;

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
