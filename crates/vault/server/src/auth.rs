//! Registration, and the Session singleton: `POST`, `GET` and `DELETE /v1/session`.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::extract::Json;
use anyhow::Result;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::extract::State;
use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use sqlx::Connection;
use sqlx::{AnyConnection, AnyPool};

use crate::db::{account_profile, api_tokens, schema, session_tokens};
use crate::dedupe;
use crate::server::{ApiError, AppState, AuthIdentity, Created};

/// Max password bytes accepted before hashing (registration / login / change).
pub(crate) const MAX_PASSWORD_BYTES: usize = 1024;
const MIN_PASSWORD_CHARS: usize = 8;
/// Sliding window for unauthenticated auth endpoints.
pub(crate) const AUTH_RATE_WINDOW: Duration = Duration::from_secs(60);
pub(crate) const AUTH_RATE_MAX: usize = 20;

static DUMMY_PASSWORD_HASH: OnceLock<String> = OnceLock::new();

/// Sliding-window hit counts for the unauthenticated auth endpoints, keyed by
/// bucket (`register:<username>`, `session:<username>`).
///
/// This lives on [`AppState`] rather than in a process-global static: a served
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
// Request / response types
// ---------------------------------------------------------------------------

/// Body for local account registration.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RegisterRequest {
    /// Login username.
    pub username: String,
    /// Local password; absent or empty registers an account without one.
    #[serde(default)]
    pub password: Option<String>,
    /// Display name shown in the vault.
    #[serde(default)]
    pub preferred_name: Option<String>,
    /// Phone number linked to the account.
    #[serde(default)]
    pub phone: Option<String>,
}

/// Username and password, the body of `POST /v1/session`.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateSessionRequest {
    /// Login username.
    pub username: String,
    /// Login password.
    #[serde(default)]
    pub password: String,
}

/// Session token plus the account id and username it belongs to.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SessionTokenResponse {
    /// Session token to send as `Authorization: Bearer …`.
    pub token: String,
    /// Account id the session belongs to.
    pub account_id: String,
    /// Account username (falls back to the account id).
    pub username: String,
}

impl SessionTokenResponse {
    /// Issue (or reuse) the session token for an existing account. Uses the
    /// account id when the row has no username.
    async fn for_existing_account(
        conn: &mut AnyConnection,
        account_id: String,
    ) -> Result<SessionTokenResponse> {
        let token = session_tokens::get_or_create_session_token(conn, &account_id).await?;
        let username = account_profile::username_for_account(conn, &account_id)
            .await?
            .unwrap_or_else(|| account_id.clone());
        Ok(SessionTokenResponse {
            token,
            account_id,
            username,
        })
    }
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
fn verify_password(hash: &str, password: &str) -> bool {
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
fn dummy_password_hash() -> &'static str {
    DUMMY_PASSWORD_HASH.get_or_init(|| {
        hash_password("timing-equalization-dummy-password").expect("dummy password hash")
    })
}

/// Always run Argon2 so missing accounts cost similar to wrong passwords.
/// Passwordless accounts (NULL hash) still accept an empty password only.
fn verify_login_password(password_hash: Option<&str>, password: &str) -> bool {
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

/// The signed-in credential's account, username, and import sources.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct SessionResponse {
    sources: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<String>,
}

/// The Session the bearer token names: its account, username, and import
/// sources. A session token and an API token both answer, because a program
/// checking its token needs the same facts as a browser restoring a sign-in.
#[utoipa::path(
    get,
    path = "/v1/session",
    tag = "Session",
    security(("bearer" = [])),
    responses(
        (status = 200, body = SessionResponse),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem)
    )
)]
pub(crate) async fn get_session_handler(
    State(state): State<AppState>,
    auth: AuthIdentity,
) -> Result<Json<SessionResponse>, ApiError> {
    let account_id = auth.account_id;
    let username = load_username(&state.db, &account_id).await?;
    let sources = list_account_sources(&state.db, &account_id).await?;
    Ok(Json(SessionResponse {
        sources,
        account_id: Some(account_id),
        username,
    }))
}

/// Source ids this account has imported, oldest first.
async fn list_account_sources(pool: &AnyPool, account_id: &str) -> Result<Vec<String>, ApiError> {
    let account_id = account_id.to_string();
    // Read-only: do not run ensure_vault_schema (avoids write locks on auth).
    let mut conn = pool.acquire().await?;
    Ok(dedupe::source_priority_from_db(&mut conn, &account_id).await?)
}

/// Username for an account id, when the account has one.
async fn load_username(pool: &AnyPool, account_id: &str) -> Result<Option<String>, ApiError> {
    let account_id = account_id.to_string();
    let mut conn = pool.acquire().await?;
    Ok(account_profile::username_for_account(&mut conn, &account_id).await?)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Refuse a username another account already has.
///
/// # Errors
///
/// A 400 naming the username when it is taken. A failed lookup is a 400 too,
/// as it always was here.
pub(crate) async fn require_username_free(
    conn: &mut AnyConnection,
    username: &str,
) -> Result<(), ApiError> {
    if account_profile::lookup_account_ref(conn, username)
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

/// Create a local vault account and return its session token.
#[utoipa::path(
    post,
    path = "/v1/auth/register",
    tag = "Auth",
    request_body = RegisterRequest,
    responses(
        (status = 200, description = "Session issued", body = SessionTokenResponse),
        (status = 400, description = "Invalid input", body = crate::problem::Problem),
        (status = 403, description = "Public registration is off", body = crate::problem::Problem),
        (status = 429, description = "Rate limited", body = crate::problem::Problem)
    )
)]
pub async fn register_handler(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<SessionTokenResponse>, ApiError> {
    let username = normalize_username(&req.username);
    if !is_valid_username(&username) {
        return Err(ApiError::validation(
            "username must be 1–128 chars (alphanumeric, _, -, .)",
        ));
    }
    check_auth_rate_limit(&state.auth_rate_limits, &format!("register:{username}"))?;

    // Registering is the vault's only self-service door, and it is shut
    // unless the vault owner has opened it. An unclaimed vault is shut too:
    // its first act is being claimed, not being joined.
    {
        let mut conn = state.db.acquire().await?;
        if !crate::db::vault_settings::load(&mut conn)
            .await?
            .public_registration
        {
            return Err(ApiError::NotTheOwner(
                "this vault does not accept new accounts; ask its owner for one".into(),
            ));
        }
    }

    let password_plain = req.password.as_deref().unwrap_or("").to_string();
    if !password_plain.is_empty() {
        validate_password_policy(&password_plain)?;
    }
    let password_hash: Option<String> = if password_plain.is_empty() {
        None
    } else {
        Some(hash_password(&password_plain)?)
    };

    let preferred_name = req.preferred_name.as_deref().and_then(message_ir::nonempty);
    let phone = req.phone.as_deref().and_then(message_ir::nonempty);

    let account_id = uuid::Uuid::new_v4().to_string();

    let mut conn = state.db.acquire().await?;
    let mut tx = conn.begin().await?;

    require_username_free(&mut tx, &username).await?;

    account_profile::insert_account(
        &mut tx,
        &account_id,
        &username,
        password_hash.as_deref(),
        preferred_name.as_deref(),
    )
    .await
    .map_err(ApiError::Internal)?;

    if let Some(ref phone) = phone {
        account_profile::upsert_account_phone(&mut tx, &account_id, phone)
            .await
            .map_err(ApiError::Internal)?;
    }

    // A registration that named nothing leaves an account with no display
    // name and no handles, and that account owes profile setup. Decided once,
    // here, and recorded — rather than re-derived from an empty-looking
    // profile by each client that reads it.
    if preferred_name.is_none() && phone.is_none() {
        account_profile::set_must_set_up_profile(&mut tx, &account_id, true)
            .await
            .map_err(ApiError::Internal)?;
    }

    let token = session_tokens::insert_account_session_token(&mut tx, &account_id)
        .await
        .map_err(ApiError::Internal)?;
    tx.commit()
        .await
        .map_err(|e| ApiError::Internal(e.into()))?;
    Ok(Json(SessionTokenResponse {
        token,
        account_id,
        username,
    }))
}

/// Sign in: verify a local username and password and answer the Session, a
/// `201 Created` whose `Location` is the singleton itself.
#[utoipa::path(
    post,
    path = "/v1/session",
    tag = "Session",
    request_body = CreateSessionRequest,
    responses(
        (
            status = 201,
            description = "Signed in; the Session exists",
            body = SessionTokenResponse,
            headers(("Location" = String, description = "`/v1/session`"))
        ),
        (status = 400, description = "Invalid input", body = crate::problem::Problem),
        (status = 401, description = "Invalid credentials", body = crate::problem::Problem),
        (status = 403, description = "Account is disabled", body = crate::problem::Problem),
        (status = 429, description = "Rate limited", body = crate::problem::Problem)
    )
)]
pub async fn create_session_handler(
    State(state): State<AppState>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<Created<SessionTokenResponse>, ApiError> {
    let username = normalize_username(&req.username);
    if username.is_empty() {
        return Err(ApiError::validation("username is required"));
    }
    check_auth_rate_limit(&state.auth_rate_limits, &format!("session:{username}"))?;
    if req.password.len() > MAX_PASSWORD_BYTES {
        return Err(ApiError::validation("password is too long"));
    }

    let password = req.password.clone();

    let mut conn = state.db.acquire().await?;
    let Some(account_id) = account_profile::lookup_account_ref(&mut conn, &username).await? else {
        let _ = verify_password(dummy_password_hash(), &password);
        return Err(ApiError::InvalidCredentials(
            "invalid username or password".into(),
        ));
    };

    let password_hash = account_profile::load_password_hash(&mut conn, &account_id).await?;
    if !verify_login_password(password_hash.as_deref(), &password) {
        return Err(ApiError::InvalidCredentials(
            "invalid username or password".into(),
        ));
    }

    let auth = account_profile::load_account_auth(&mut conn, &account_id)
        .await?
        .ok_or_else(|| ApiError::InvalidCredentials("invalid username or password".into()))?;
    if auth.disabled {
        return Err(ApiError::AccountDisabled("this account is disabled".into()));
    }

    let body = SessionTokenResponse::for_existing_account(&mut conn, account_id).await?;

    Ok(Created {
        location: "/v1/session".to_string(),
        body,
    })
}

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
    account_id: &str,
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

// ---------------------------------------------------------------------------
// Change-password and sign-out handlers
// ---------------------------------------------------------------------------

/// Revoke the session token.
async fn logout_on_conn(conn: &mut AnyConnection, token: &str) -> Result<()> {
    let _ = session_tokens::revoke_session_token(conn, token).await?;
    Ok(())
}

/// Sign out: revoke the presented session token, ending the Session.
#[utoipa::path(
    delete,
    path = "/v1/session",
    tag = "Session",
    security(("bearer" = [])),
    responses(
        (status = 204, description = "Signed out"),
        (status = 401, body = crate::problem::Problem)
    )
)]
pub async fn delete_session_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<axum::http::StatusCode, ApiError> {
    let token = crate::server::bearer_token(&headers)?;
    let mut conn = state.db.acquire().await?;
    schema::ensure_accounts_schema(&mut conn).await?;
    logout_on_conn(&mut conn, &token).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests;
