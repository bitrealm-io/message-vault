//! GUI session Bearer tokens (`mc-user-…`); one per account, rotates on login.
//! Opening and ending a Session writes its entries in the Audit Trail.

use anyhow::{Context, Result, bail};
pub use message_crate_api_types::AppKind;
use rand::TryRng;
use sqlx::SqliteConnection;

use crate::db::audit_trail::{self, AuditActor, AuditReason};

const TOKEN_ALPHANUM: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

/// Default GUI session lifetime (30 days).
pub const SESSION_TTL_SECS: u64 = 30 * 24 * 60 * 60;

/// Generate a new GUI session token (`mc-user-` + 32 alphanumeric characters).
///
/// # Errors
///
/// Returns an error when random bytes cannot be generated.
pub fn generate_session_token() -> Result<String> {
    generate_prefixed_token("mc-user-")
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
    crate::assets_api::sha256_hex(token.as_bytes())
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

/// Current Unix time in seconds.
fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The app a request says it comes from, and the Build that app reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectingApp {
    /// Desktop app or website.
    pub kind: AppKind,
    /// The app's Build, such as `0.9.0+343fe0d8`.
    pub build: String,
}

impl ConnectingApp {
    /// Pair a stored kind and Build; `None` unless both are present and the
    /// kind is one the server knows.
    fn from_columns(kind: Option<String>, build: Option<String>) -> Option<Self> {
        let kind = AppKind::parse(kind.as_deref()?)?;
        Some(Self {
            kind,
            build: build?,
        })
    }
}

/// A live session: whose it is, and the app last recorded on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// The account the session logs in.
    pub account_id: i64,
    /// `None` until a request names its app.
    pub app: Option<ConnectingApp>,
}

/// Look up the live session this Bearer names (by hash). Expired rows are removed.
///
/// # Errors
///
/// Returns an error when the lookup or delete fails.
pub async fn lookup_session(conn: &mut SqliteConnection, token: &str) -> Result<Option<Session>> {
    let token_hash = hash_api_token(token);
    let found: Option<(i64, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT account_id, expires_at, app_kind, app_build \
         FROM account_session_tokens WHERE token_hash = $1",
    )
    .bind(token_hash.as_str())
    .fetch_optional(&mut *conn)
    .await?;
    let Some((account_id, expires_at, app_kind, app_build)) = found else {
        return Ok(None);
    };
    let now = now_unix_secs();
    // An `expires_at` that does not parse counts as expired.
    if !expires_at.parse::<u64>().is_ok_and(|expires| expires > now) {
        let _ = sqlx::query("DELETE FROM account_session_tokens WHERE token_hash = $1")
            .bind(token_hash.as_str())
            .execute(&mut *conn)
            .await;
        return Ok(None);
    }
    Ok(Some(Session {
        account_id,
        app: ConnectingApp::from_columns(app_kind, app_build),
    }))
}

/// Record the app a session's request came from, when it is not the one
/// already recorded. In practice that is one write at login and one after an
/// app update; every other request finds the same value and writes nothing.
///
/// # Errors
///
/// Returns an error when the update fails.
pub async fn record_connecting_app(
    conn: &mut SqliteConnection,
    session: &Session,
    app: &ConnectingApp,
) -> Result<()> {
    if session.app.as_ref() == Some(app) {
        return Ok(());
    }
    sqlx::query(
        "UPDATE account_session_tokens SET app_kind = $1, app_build = $2 WHERE account_id = $3",
    )
    .bind(app.kind.as_str())
    .bind(app.build.as_str())
    .bind(session.account_id)
    .execute(&mut *conn)
    .await
    .with_context(|| format!("record the connecting app for {}", session.account_id))?;
    Ok(())
}

/// The app recorded on an account's session, or `None` when the account has
/// no session or no request has named an app.
///
/// # Errors
///
/// Returns an error when the lookup fails.
pub async fn connecting_app_for_account(
    conn: &mut SqliteConnection,
    account_id: i64,
) -> Result<Option<ConnectingApp>> {
    let found: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT app_kind, app_build FROM account_session_tokens WHERE account_id = $1",
    )
    .bind(account_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(found.and_then(|(kind, build)| ConnectingApp::from_columns(kind, build)))
}

/// Replace the account's session token with a new one and return the
/// plaintext once, with the Unix second it now expires at. The Session
/// carries on: a password change renews it, and the caller records the
/// renewed expiry in the Audit Trail.
///
/// One upsert, never a lookup and then an insert: two logins at once both
/// find no row, and the second insert would break the `account_id` primary
/// key. The later login's token replaces the earlier one's.
///
/// # Errors
///
/// Returns an error when a token cannot be generated or the write fails.
pub async fn rotate_account_session_token(
    conn: &mut SqliteConnection,
    account_id: i64,
) -> Result<(String, u64)> {
    let token = generate_session_token()?;
    let token_hash = hash_api_token(&token);
    let created_at = unix_secs_string();
    let expires = now_unix_secs().saturating_add(SESSION_TTL_SECS);
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
    .bind(expires.to_string())
    .execute(&mut *conn)
    .await
    .with_context(|| format!("rotate session token for {account_id}"))?;
    Ok((token, expires))
}

/// The Audit Trail's `logged_in` entry for the account's session, when a
/// login made it and it has not expired. An expired row stays until its
/// token is presented again, and its end is read from its expiry, so ending
/// it again would rewrite when it ended.
pub(crate) async fn live_login_entry(
    conn: &mut SqliteConnection,
    account_id: i64,
) -> Result<Option<i64>> {
    let found: Option<(Option<i64>, String)> = sqlx::query_as(
        "SELECT login_entry_id, expires_at FROM account_session_tokens WHERE account_id = $1",
    )
    .bind(account_id)
    .fetch_optional(&mut *conn)
    .await?;
    let now = now_unix_secs();
    // An `expires_at` that does not parse counts as expired, as in `lookup_session`.
    Ok(found.and_then(|(login_entry_id, expires_at)| {
        login_entry_id.filter(|_| expires_at.parse::<u64>().is_ok_and(|expires| expires > now))
    }))
}

/// Log in: open the account's one Session and return its token once.
///
/// A Session the account already had is replaced, and the Audit Trail says
/// so: a login on the desktop ends the website's. The new Session's
/// `logged_in` entry names the app the request named, and the session row
/// records that app from the start. Called by every route that opens a
/// Session for a person: login, claiming the Message Crate, and registering.
///
/// # Errors
///
/// Returns an error when a token cannot be generated or a write fails.
pub async fn open_session(
    conn: &mut SqliteConnection,
    account_id: i64,
    username: &str,
    app: Option<&ConnectingApp>,
) -> Result<String> {
    let actor = AuditActor::logged_in_as(account_id);
    if let Some(previous) = live_login_entry(conn, account_id).await? {
        audit_trail::record_session_end(conn, previous, AuditReason::Replaced, actor).await?;
    }
    let token = generate_session_token()?;
    let token_hash = hash_api_token(&token);
    let created_at = unix_secs_string();
    let expires = now_unix_secs().saturating_add(SESSION_TTL_SECS);
    let login_entry_id =
        audit_trail::record_login(conn, (account_id, username), app, expires).await?;
    sqlx::query(
        r"
        INSERT INTO account_session_tokens
            (account_id, token_hash, created_at, expires_at, app_kind, app_build, login_entry_id)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ON CONFLICT(account_id) DO UPDATE SET
            token_hash = excluded.token_hash,
            created_at = excluded.created_at,
            expires_at = excluded.expires_at,
            app_kind = excluded.app_kind,
            app_build = excluded.app_build,
            login_entry_id = excluded.login_entry_id
        ",
    )
    .bind(account_id)
    .bind(token_hash)
    .bind(created_at)
    .bind(expires.to_string())
    .bind(app.map(|app| app.kind.as_str()))
    .bind(app.map(|app| app.build.as_str()))
    .bind(login_entry_id)
    .execute(&mut *conn)
    .await
    .with_context(|| format!("open a session for {account_id}"))?;
    Ok(token)
}

/// Create a fresh session token for an account and return the plaintext.
/// No login made it, so the Audit Trail holds no entry for it: the tests and
/// the credential matrix use it to stand a Session up directly.
///
/// # Errors
///
/// Returns an error when a token cannot be generated or the insert fails.
#[cfg(test)]
pub async fn insert_account_session_token(
    conn: &mut SqliteConnection,
    account_id: i64,
) -> Result<String> {
    insert_account_session_token_with_ttl(conn, account_id, SESSION_TTL_SECS).await
}

/// Create a fresh session token with a custom lifetime and return the plaintext.
///
/// # Errors
///
/// Returns an error when a token cannot be generated or the insert fails.
#[cfg(test)]
pub async fn insert_account_session_token_with_ttl(
    conn: &mut SqliteConnection,
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

/// Log out: end the Session the presented token names, and record that it
/// ended. Returns whether the token named a Session. The lookup, the record
/// and the delete run in one write transaction, so a login replacing the
/// Session meanwhile, or a second logout with the same token, cannot record
/// its end twice.
///
/// # Errors
///
/// Returns an error when the lookup, the record or the delete fails.
pub async fn revoke_session_token(conn: &mut SqliteConnection, token: &str) -> Result<bool> {
    let token_hash = hash_api_token(token);
    let mut tx = crate::db::begin_write(conn).await?;
    let found: Option<(i64, Option<i64>)> = sqlx::query_as(
        "SELECT account_id, login_entry_id FROM account_session_tokens WHERE token_hash = $1",
    )
    .bind(token_hash.as_str())
    .fetch_optional(&mut *tx)
    .await?;
    let Some((account_id, login_entry_id)) = found else {
        return Ok(false);
    };
    if let Some(login_entry_id) = login_entry_id {
        audit_trail::record_session_end(
            &mut tx,
            login_entry_id,
            AuditReason::LoggedOut,
            AuditActor::logged_in_as(account_id),
        )
        .await?;
    }
    sqlx::query("DELETE FROM account_session_tokens WHERE token_hash = $1")
        .bind(token_hash)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

/// End the account's Session, if it has one, and record it as `revoked` by
/// `actor`. Used by `reset-owner-password`, the shell command for an owner
/// who has lost their password, so the old password's login ends with it,
/// and by a Demo Account rebuild. The owner setting another account's
/// password through `/v1` does not call this: that sets the password and
/// nothing more, and the account's session carries on.
///
/// It reads the Session and then writes, so it takes the caller's write
/// transaction, and the end is recorded with whatever else the caller does.
///
/// # Errors
///
/// Returns an error when the record or the delete fails.
pub async fn revoke_account_sessions(
    tx: &mut crate::db::WriteTx<'_>,
    account_id: i64,
    actor: AuditActor,
) -> Result<()> {
    if let Some(login_entry_id) = live_login_entry(tx, account_id).await? {
        audit_trail::record_session_end(tx, login_entry_id, AuditReason::Revoked, actor).await?;
    }
    sqlx::query("DELETE FROM account_session_tokens WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut **tx)
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
    /// Every session and API token in every existing database is stored as this
    /// hash and looked up by it, so changing the algorithm logs everyone out
    /// and invalidates every issued API token at once. A test that only
    /// checked the length and determinism would pass after such a change —
    /// any 64-character hex hash is stable and 64 characters long. These
    /// digests are SHA-256 of the exact bytes shown, reproducible with
    /// `printf 'mc-user-abc' | sha256sum`.
    #[test]
    fn hash_is_sha256_of_the_token_bytes() {
        for (token, expected) in [
            (
                "mc-user-abc",
                "2f149722de58d39a2c4c28c6889a5bcdfeef221933624142cc2339411007a7a8",
            ),
            (
                "mc-tok-abc123",
                "892e92b80959400565df97dd4983106d1c7d61ecf6814a4694f8d878d0745cc1",
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
                 token in every existing database is looked up by this digest"
            );
        }
        assert_ne!(hash_api_token("mc-user-abc"), hash_api_token("mc-user-xyz"));
    }

    #[test]
    fn generate_prefixed_token_uses_os_entropy() {
        let a = generate_prefixed_token("mc-user-").unwrap();
        let b = generate_prefixed_token("mc-user-").unwrap();
        assert!(a.starts_with("mc-user-"));
        assert_eq!(a.len(), "mc-user-".len() + 32);
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
        assert!(lookup_session(&mut conn, &token).await.unwrap().is_none());
    }

    /// A session that ran out an hour ago is compared with the real clock.
    /// An `expires_at` of `'1'` is before any clock reading, so it cannot
    /// show that the clock is read at all.
    #[tokio::test]
    async fn lookup_rejects_a_session_that_expired_an_hour_ago() {
        let (pool, _dir) = crate::db::engine::test_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        schema::ensure_accounts_schema(&mut conn).await.unwrap();
        sqlx::query("INSERT INTO accounts (id, username) VALUES ($1, 'alice')")
            .bind(7_i64)
            .execute(&mut *conn)
            .await
            .unwrap();
        let token = insert_account_session_token(&mut conn, 7).await.unwrap();
        assert!(lookup_session(&mut conn, &token).await.unwrap().is_some());

        let an_hour_ago = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 3600;
        sqlx::query("UPDATE account_session_tokens SET expires_at = $1")
            .bind(an_hour_ago.to_string())
            .execute(&mut *conn)
            .await
            .unwrap();
        assert!(lookup_session(&mut conn, &token).await.unwrap().is_none());
    }

    /// A login made now is good for thirty days: the row's expiry is that
    /// far past the clock, give or take the seconds the insert took.
    #[tokio::test]
    async fn a_session_expires_thirty_days_after_login() {
        const THIRTY_DAYS_SECS: u64 = 2_592_000;
        let (pool, _dir) = crate::db::engine::test_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        schema::ensure_accounts_schema(&mut conn).await.unwrap();
        sqlx::query("INSERT INTO accounts (id, username) VALUES ($1, 'alice')")
            .bind(7_i64)
            .execute(&mut *conn)
            .await
            .unwrap();
        let before = now_unix_secs();
        insert_account_session_token(&mut conn, 7).await.unwrap();
        let after = now_unix_secs();

        let expires: String = sqlx::query_scalar(
            "SELECT expires_at FROM account_session_tokens WHERE account_id = 7",
        )
        .fetch_one(&mut *conn)
        .await
        .unwrap();
        let expires: u64 = expires.parse().unwrap();
        assert!(
            (before + THIRTY_DAYS_SECS..=after + THIRTY_DAYS_SECS).contains(&expires),
            "expires {} seconds after login",
            expires - before
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
        assert!(token.starts_with("mc-user-"));
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
        assert!(lookup_session(&mut conn, &token).await.unwrap().is_some());
        assert!(revoke_session_token(&mut conn, &token).await.unwrap());
        assert!(lookup_session(&mut conn, &token).await.unwrap().is_none());
    }
}
