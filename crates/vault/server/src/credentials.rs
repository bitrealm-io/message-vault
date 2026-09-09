//! Passwords, usernames, and the rate limiter on attempts to present them.
//!
//! Nothing here is a route. The Session routes (`session_api`), the accounts
//! collection (`accounts_api`), claiming (`vault_api`) and the owner's shell
//! commands (`owner_cli`) all hash, verify and validate through this module,
//! so a password rule is written once.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::Result;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use sqlx::{AnyConnection, Connection};

use crate::db::{account_profile, api_tokens, session_tokens};
use crate::server::ApiError;

/// Max password bytes accepted before hashing (creation, sign-in, change).
pub(crate) const MAX_PASSWORD_BYTES: usize = 1024;
const MIN_PASSWORD_CHARS: usize = 8;
/// Sliding window for the routes a stranger may call with a credential.
pub(crate) const AUTH_RATE_WINDOW: Duration = Duration::from_secs(60);
pub(crate) const AUTH_RATE_MAX: usize = 20;

static DUMMY_PASSWORD_HASH: OnceLock<String> = OnceLock::new();

/// Sliding-window hit counts for the unauthenticated credential routes, keyed
/// by bucket (`register:<username>`, `session:<username>`, `claim`).
///
/// This lives on `AppState` rather than in a process-global static: a served
/// vault builds exactly one state, so the limiter still spans the whole server,
/// while each test vault gets its own counts and cannot rate-limit an unrelated
/// test running beside it in the same binary.
pub(crate) type AuthRateLimits = Arc<Mutex<HashMap<String, VecDeque<Instant>>>>;

/// Reject when `bucket` has seen at least [`AUTH_RATE_MAX`] hits in
/// [`AUTH_RATE_WINDOW`].
pub(crate) fn check_auth_rate_limit(limits: &AuthRateLimits, bucket: &str) -> Result<(), ApiError> {
    check_auth_rate_limit_at(limits, bucket, Instant::now())
}

/// [`check_auth_rate_limit`] with the clock as an argument, so a test can move it.
fn check_auth_rate_limit_at(
    limits: &AuthRateLimits,
    bucket: &str,
    now: Instant,
) -> Result<(), ApiError> {
    let mut map = limits
        .lock()
        .map_err(|_| ApiError::Internal(anyhow::anyhow!("auth rate limiter poisoned")))?;
    // Forget every bucket whose newest hit is outside the window. Buckets are
    // named by whatever username the client sends, so without this a client
    // spraying usernames grows the map for the life of the process.
    map.retain(|_, hits| {
        hits.back()
            .is_some_and(|newest| now.duration_since(*newest) <= AUTH_RATE_WINDOW)
    });
    let entry = map.entry(bucket.to_string()).or_default();
    while let Some(oldest) = entry.front() {
        if now.duration_since(*oldest) <= AUTH_RATE_WINDOW {
            break;
        }
        entry.pop_front();
    }
    if entry.len() >= AUTH_RATE_MAX {
        // The oldest hit inside the window is the next to leave it; until it
        // does, every attempt is refused. Never zero: a client told to wait
        // nothing would retry at once and be refused again.
        let oldest = entry.front().copied().unwrap_or(now);
        let remaining = AUTH_RATE_WINDOW.saturating_sub(now.duration_since(oldest));
        return Err(ApiError::RateLimited {
            retry_after_secs: remaining.as_secs().max(1),
        });
    }
    entry.push_back(now);
    Ok(())
}

// ---------------------------------------------------------------------------
// Password helpers
// ---------------------------------------------------------------------------

/// Hash a plaintext password with argon2id.
///
/// # Errors
///
/// Returns an error when the password cannot be hashed.
pub(crate) fn hash_password(password: &str) -> Result<String> {
    // argon2 0.6 generates the salt itself, from the system RNG, and sizes it
    // to the algorithm's recommendation. That replaces a hand-rolled 16-byte
    // fill and base64 encode, which is not code worth owning on an auth path.
    let hash = Argon2::default()
        .hash_password(password.as_bytes())
        .map_err(|e| anyhow::anyhow!("password hash failed: {e}"))?;
    Ok(hash.to_string())
}

/// Verify a plaintext password against an argon2 hash.
pub(crate) fn verify_password(hash: &str, password: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// True when `password` matches the stored hash.
///
/// A missing or empty hash means the account has no password, so only an empty
/// password is accepted. Otherwise argon2 is used.
pub(crate) fn passwords_match(password_hash: Option<&str>, password: &str) -> bool {
    match password_hash {
        None | Some("") => password.is_empty(),
        Some(hash) => verify_password(hash, password),
    }
}

/// A real argon2 hash used only so missing-account logins take similar time.
pub(crate) fn dummy_password_hash() -> &'static str {
    DUMMY_PASSWORD_HASH.get_or_init(|| {
        hash_password("timing-equalization-dummy-password").expect("dummy password hash")
    })
}

/// Always run Argon2 so missing accounts cost similar to wrong passwords.
/// Passwordless accounts (NULL hash) still accept an empty password only.
pub(crate) fn verify_login_password(password_hash: Option<&str>, password: &str) -> bool {
    match password_hash {
        None | Some("") => {
            let _ = verify_password(dummy_password_hash(), password);
            password.is_empty()
        }
        Some(hash) => verify_password(hash, password),
    }
}

/// Reject passwords that are too short or too long.
pub(crate) fn validate_password_policy(password: &str) -> Result<(), ApiError> {
    if password.len() < MIN_PASSWORD_CHARS {
        return Err(ApiError::validation(format!(
            "password must be at least {MIN_PASSWORD_CHARS} characters"
        )));
    }
    if password.len() > MAX_PASSWORD_BYTES {
        return Err(ApiError::validation("password is too long"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Username validation
// ---------------------------------------------------------------------------

/// The username as stored: surrounding whitespace removed.
pub(crate) fn normalize_username(raw: &str) -> String {
    raw.trim().to_string()
}

/// True for 1 to 128 characters of letters, digits, `_`, `-`, or `.`.
pub(crate) fn is_valid_username(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() || s.len() > 128 {
        return false;
    }
    s.chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.')
}

/// The trimmed username, or the `validation-failed` every route that takes
/// one answers when it is malformed.
pub(crate) fn require_valid_username(raw: &str) -> Result<String, ApiError> {
    let username = normalize_username(raw);
    if !is_valid_username(&username) {
        return Err(ApiError::validation(
            "username must be 1–128 chars (alphanumeric, _, -, .)",
        ));
    }
    Ok(username)
}

/// Refuse a username another account already has.
///
/// # Errors
///
/// A `409` naming the username when it is taken. A failed lookup is a `500`.
pub(crate) async fn require_username_free(
    conn: &mut AnyConnection,
    username: &str,
) -> Result<(), ApiError> {
    if account_profile::lookup_account_by_username(conn, username)
        .await
        .map_err(ApiError::Internal)?
        .is_some()
    {
        return Err(ApiError::UsernameTaken(format!(
            "username already taken: {username}"
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Changing one's own password
// ---------------------------------------------------------------------------

/// Why a password change was refused.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ChangePasswordError {
    /// The presented current password does not match the stored hash.
    #[error("current password is incorrect")]
    IncorrectPassword,
    /// Database failure.
    #[error(transparent)]
    Db(#[from] anyhow::Error),
}

impl From<sqlx::Error> for ChangePasswordError {
    fn from(value: sqlx::Error) -> Self {
        Self::Db(value.into())
    }
}

impl From<ChangePasswordError> for ApiError {
    fn from(e: ChangePasswordError) -> Self {
        match e {
            err @ ChangePasswordError::IncorrectPassword => {
                Self::InvalidCredentials(err.to_string())
            }
            ChangePasswordError::Db(err) => Self::Internal(err),
        }
    }
}

/// Check the current password, store `new_hash`, drop named API tokens, and
/// issue a fresh session token. All of that happens in one database transaction
/// so a failure leaves the old credentials in place.
///
/// # Errors
///
/// [`ChangePasswordError::IncorrectPassword`] when the current password is
/// wrong; [`ChangePasswordError::Db`] when a database read or write fails.
pub(crate) async fn change_password_on_conn(
    conn: &mut AnyConnection,
    account_id: i64,
    current_password: &str,
    new_hash: &str,
) -> std::result::Result<String, ChangePasswordError> {
    let mut tx = conn.begin().await?;
    let current_hash = account_profile::load_password_hash(&mut tx, account_id).await?;
    if !passwords_match(current_hash.as_deref(), current_password) {
        return Err(ChangePasswordError::IncorrectPassword);
    }
    account_profile::update_password_hash(&mut tx, account_id, new_hash).await?;
    // Whatever brought the account here, it now carries a password its holder
    // chose, so the mark the vault owner set comes off in the same
    // transaction as the hash it refers to.
    account_profile::set_must_change_password(&mut tx, account_id, false).await?;
    api_tokens::delete_all_api_tokens(&mut tx, account_id).await?;
    let token = session_tokens::rotate_account_session_token(&mut tx, account_id).await?;
    tx.commit().await?;
    Ok(token)
}

#[cfg(test)]
mod tests;
