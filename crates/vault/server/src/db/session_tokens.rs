//! GUI session Bearer tokens (`mv-user-…`); one per account, rotates on login.

use anyhow::{Context, Result, bail};
use rand::TryRng;
use sqlx::AnyConnection;

const TOKEN_ALPHANUM: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

/// Default GUI session lifetime (30 days).
pub const SESSION_TTL_SECS: u64 = 30 * 24 * 60 * 60;

/// Generate a new GUI session token (`mv-user-` + 32 alphanumeric characters).
///
/// # Errors
///
/// Returns an error when random bytes cannot be generated.
pub fn generate_session_token() -> Result<String> {
    generate_prefixed_token("mv-user-")
}

/// A random 32-character token after `prefix`, from the OS random source.
pub(crate) fn generate_prefixed_token(prefix: &str) -> Result<String> {
    let mut buf = [0u8; 32];
    fill_random(&mut buf)?;
    let mut suffix = String::with_capacity(32);
    for b in buf {
        suffix.push(TOKEN_ALPHANUM[(b as usize) % TOKEN_ALPHANUM.len()] as char);
    }
    Ok(format!("{prefix}{suffix}"))
}

/// SHA-256 hex fingerprint of a plaintext token (stored in DB; used for Bearer lookup).
pub fn hash_api_token(token: &str) -> String {
    crate::assets::sha256_hex(token.as_bytes())
}

/// Fill `buf` from the OS random source, refusing an all-zero result.
fn fill_random(buf: &mut [u8]) -> Result<()> {
    rand::rngs::SysRng
        .try_fill_bytes(buf)
        .map_err(|e| anyhow::anyhow!("secure random unavailable: {e}"))?;
    if buf.iter().all(|&b| b == 0) {
        bail!("secure random returned an empty entropy buffer");
    }
    Ok(())
}

/// Expiry timestamp for a session issued at `now_secs`.
fn session_expiry_unix(now_secs: u64) -> String {
    format!("{}", now_secs.saturating_add(SESSION_TTL_SECS))
}

/// Current Unix time in seconds.
fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Look up which account owns this session Bearer (by hash). Expired rows are removed.
///
/// # Errors
///
/// Returns an error when the lookup or delete fails.
pub async fn lookup_account_for_token(
    conn: &mut AnyConnection,
    token: &str,
) -> Result<Option<i64>> {
    let token_hash = hash_api_token(token);
    let found: Option<(i64, String)> = sqlx::query_as(
        "SELECT account_id, expires_at FROM account_session_tokens WHERE token_hash = $1",
    )
    .bind(token_hash.as_str())
    .fetch_optional(&mut *conn)
    .await?;
    let Some((account_id, expires_at)) = found else {
        return Ok(None);
    };
    let expires = expires_at.parse::<u64>().unwrap_or(0);
    let now = now_unix_secs();
    // Legacy rows migrated with expires_at='0' are treated as expired; rotate via login.
    if expires == 0 || expires <= now {
        let _ = sqlx::query("DELETE FROM account_session_tokens WHERE token_hash = $1")
            .bind(token_hash.as_str())
            .execute(&mut *conn)
            .await;
        return Ok(None);
    }
    Ok(Some(account_id))
}

/// Create or replace the account's session token hash; returns plaintext once.
///
/// # Errors
///
/// Returns an error when a token cannot be generated or the write fails.
pub async fn rotate_account_session_token(
    conn: &mut AnyConnection,
    account_id: i64,
) -> Result<String> {
    let token = generate_session_token()?;
    let token_hash = hash_api_token(&token);
    let created_at = unix_secs_string();
    let expires_at = session_expiry_unix(now_unix_secs());
    sqlx::query(
        r"
        INSERT INTO account_session_tokens (account_id, token_hash, created_at, expires_at)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT(account_id) DO UPDATE SET
            token_hash = excluded.token_hash,
            created_at = excluded.created_at,
            expires_at = excluded.expires_at
        ",
    )
    .bind(account_id)
    .bind(token_hash)
    .bind(created_at)
    .bind(expires_at)
    .execute(&mut *conn)
    .await
    .with_context(|| format!("rotate session token for {account_id}"))?;
    Ok(token)
}

/// Create a fresh session token for an account and return the plaintext.
///
/// # Errors
///
/// Returns an error when a token cannot be generated or the insert fails.
pub async fn insert_account_session_token(
    conn: &mut AnyConnection,
    account_id: i64,
) -> Result<String> {
    insert_account_session_token_with_ttl(conn, account_id, SESSION_TTL_SECS).await
}

/// Create a fresh session token with a custom lifetime and return the plaintext.
///
/// # Errors
///
/// Returns an error when a token cannot be generated or the insert fails.
pub async fn insert_account_session_token_with_ttl(
    conn: &mut AnyConnection,
    account_id: i64,
    ttl_secs: u64,
) -> Result<String> {
    let token = generate_session_token()?;
    let token_hash = hash_api_token(&token);
    let created_at = unix_secs_string();
    let expires_at = format!("{}", now_unix_secs().saturating_add(ttl_secs));
    sqlx::query(
        "INSERT INTO account_session_tokens (account_id, token_hash, created_at, expires_at)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(account_id)
    .bind(token_hash)
    .bind(created_at)
    .bind(expires_at)
    .execute(&mut *conn)
    .await
    .with_context(|| format!("insert session token for {account_id}"))?;
    Ok(token)
}

/// Session token for GUI: if a row exists, rotate it; otherwise insert.
///
/// # Errors
///
/// Returns an error when the lookup or token write fails.
pub async fn get_or_create_session_token(
    conn: &mut AnyConnection,
    account_id: i64,
) -> Result<String> {
    let existing: Option<String> =
        sqlx::query_scalar("SELECT token_hash FROM account_session_tokens WHERE account_id = $1")
            .bind(account_id)
            .fetch_optional(&mut *conn)
            .await?;
    match existing {
        Some(_) => rotate_account_session_token(conn, account_id).await,
        None => insert_account_session_token(conn, account_id).await,
    }
}

/// Revoke the presented session token (logout). Returns whether a row was deleted.
///
/// # Errors
///
/// Returns an error when the delete fails.
pub async fn revoke_session_token(conn: &mut AnyConnection, token: &str) -> Result<bool> {
    let token_hash = hash_api_token(token);
    let n = sqlx::query("DELETE FROM account_session_tokens WHERE token_hash = $1")
        .bind(token_hash)
        .execute(&mut *conn)
        .await?
        .rows_affected();
    Ok(n > 0)
}

/// Revoke every session token belonging to `account_id` (normally at most
/// one row). Used when an administrator resets someone else's password, so
/// the reset actually ends their existing sign-in rather than merely
/// changing what a future one would need.
pub async fn revoke_account_sessions(conn: &mut AnyConnection, account_id: i64) -> Result<()> {
    sqlx::query("DELETE FROM account_session_tokens WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut *conn)
        .await
        .with_context(|| format!("revoke sessions for {account_id}"))?;
    Ok(())
}

/// Current Unix time in seconds as a string, for timestamp columns.
pub(crate) fn unix_secs_string() -> String {
    format!("{}", now_unix_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema;

    /// Known-answer vectors, not a round trip.
    ///
    /// Every session and API token in every existing vault is stored as this
    /// hash and looked up by it, so changing the algorithm signs everyone out
    /// and invalidates every issued API token at once. A test that only
    /// checked the length and determinism would pass after such a change —
    /// any 64-character hex hash is stable and 64 characters long. These
    /// digests are SHA-256 of the exact bytes shown, reproducible with
    /// `printf 'mv-user-abc' | sha256sum`.
    #[test]
    fn hash_is_sha256_of_the_token_bytes() {
        for (token, expected) in [
            (
                "mv-user-abc",
                "0df4b3a1f371a0685d2ab53463e293d0a3cf12b3ffa63ccbbf4bdfa9792e5d1e",
            ),
            (
                "mv-tok-abc123",
                "949cf0bf8e4e42a454c6a16d67f6a6c414ac39ea6bee6003298da8cc90b10273",
            ),
            (
                "",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
        ] {
            assert_eq!(
                hash_api_token(token),
                expected,
                "the stored hash of {token:?} changed; every session and API \
                 token in every existing vault is looked up by this digest"
            );
        }
        assert_ne!(hash_api_token("mv-user-abc"), hash_api_token("mv-user-xyz"));
    }

    #[test]
    fn generate_prefixed_token_uses_os_entropy() {
        let a = generate_prefixed_token("mv-user-").unwrap();
        let b = generate_prefixed_token("mv-user-").unwrap();
        assert!(a.starts_with("mv-user-"));
        assert_eq!(a.len(), "mv-user-".len() + 32);
        assert_ne!(a, b);
    }

    #[tokio::test]
    async fn lookup_rejects_expired_session() {
        let (pool, _dir) = crate::db::engine::test_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        schema::ensure_accounts_schema(&mut conn).await.unwrap();
        sqlx::query("INSERT INTO accounts (id, username) VALUES ($1, 'alice')")
            .bind(7_i64)
            .execute(&mut *conn)
            .await
            .unwrap();
        let token = insert_account_session_token(&mut conn, 7).await.unwrap();
        // Force expiry into the past.
        sqlx::query("UPDATE account_session_tokens SET expires_at = '1'")
            .execute(&mut *conn)
            .await
            .unwrap();
        assert!(
            lookup_account_for_token(&mut conn, &token)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn insert_session_with_ttl_sets_expires_at() {
        let (pool, _dir) = crate::db::engine::test_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        schema::ensure_accounts_schema(&mut conn).await.unwrap();
        sqlx::query("INSERT INTO accounts (id, username) VALUES ($1, 'alice')")
            .bind(7_i64)
            .execute(&mut *conn)
            .await
            .unwrap();
        let before = now_unix_secs();
        let token = insert_account_session_token_with_ttl(&mut conn, 7, 120)
            .await
            .unwrap();
        assert!(token.starts_with("mv-user-"));
        let expires: String = sqlx::query_scalar(
            "SELECT expires_at FROM account_session_tokens WHERE account_id = 7",
        )
        .fetch_one(&mut *conn)
        .await
        .unwrap();
        let exp: u64 = expires.parse().unwrap();
        assert!(exp >= before + 120);
        assert!(exp <= before + 130);
    }

    #[tokio::test]
    async fn revoke_session_token_removes_row() {
        let (pool, _dir) = crate::db::engine::test_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        schema::ensure_accounts_schema(&mut conn).await.unwrap();
        sqlx::query("INSERT INTO accounts (id, username) VALUES ($1, 'alice')")
            .bind(7_i64)
            .execute(&mut *conn)
            .await
            .unwrap();
        let token = insert_account_session_token(&mut conn, 7).await.unwrap();
        assert!(
            lookup_account_for_token(&mut conn, &token)
                .await
                .unwrap()
                .is_some()
        );
        assert!(revoke_session_token(&mut conn, &token).await.unwrap());
        assert!(
            lookup_account_for_token(&mut conn, &token)
                .await
                .unwrap()
                .is_none()
        );
    }
}
