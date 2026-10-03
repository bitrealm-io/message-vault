//! The Session singleton: `POST`, `GET` and `DELETE /v1/session`.
//!
//! A Session is one per logged-in account or owner. Logging in creates it,
//! reading it says which account the bearer token names, and logging out
//! ends it. The password rules it applies live in `credentials`.

use axum::extract::State;
use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use sqlx::{Connection, SqliteConnection, SqlitePool};

use crate::credentials::{
    MAX_PASSWORD_BYTES, check_auth_rate_limit, dummy_password_hash, normalize_username,
    password_bucket, unknown_username_bucket, verify_login_password, verify_password,
};
use crate::db::audit_trail::{self, AuditReason};
use crate::db::session_tokens::ConnectingApp;
use crate::db::{account_profile, api_tokens, schema, session_tokens};
use crate::dedupe;
use crate::extract::Json;
use crate::server::{ApiError, AppState, AuthIdentity, Created};

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
pub struct CreateSessionResponse {
    /// Session token to send as `Authorization: Bearer …`.
    pub token: String,
    /// Account id the session belongs to.
    pub account_id: i64,
    /// Account username (falls back to the account id).
    pub username: String,
}

impl CreateSessionResponse {
    /// Open the Session for an existing account, replacing the one it had,
    /// and record the login. Uses the account id when the row has no username.
    async fn for_existing_account(
        conn: &mut SqliteConnection,
        account_id: i64,
        app: Option<&ConnectingApp>,
    ) -> anyhow::Result<CreateSessionResponse> {
        let username = account_profile::username_for_account(conn, account_id)
            .await?
            .unwrap_or_else(|| account_id.to_string());
        let mut tx = conn.begin().await?;
        let token = session_tokens::open_session(&mut tx, account_id, &username, app).await?;
        account_profile::record_login(&mut tx, account_id).await?;
        tx.commit().await?;
        Ok(CreateSessionResponse {
            token,
            account_id,
            username,
        })
    }
}

/// The logged-in credential's account, username, and import sources.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct Session {
    sources: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    account_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<String>,
}

/// The Session the bearer token names: its account, username, and import
/// sources. A session token and an API token both answer, because a program
/// checking its token needs the same facts as a browser restoring a login.
#[utoipa::path(
    get,
    path = "/v1/session",
    tag = "Session",
    security(("session" = []), ("api-token" = [])),
    responses(
        (status = 200, body = Session),
    )
)]
pub(crate) async fn get_session(
    State(state): State<AppState>,
    auth: AuthIdentity,
) -> Result<Json<Session>, ApiError> {
    let account_id = auth.account_id;
    let username = load_username(&state.db, account_id).await?;
    let sources = list_account_sources(&state.db, account_id).await?;
    Ok(Json(Session {
        sources,
        account_id: Some(account_id),
        username,
    }))
}

/// Source ids this account has imported, oldest first.
async fn list_account_sources(pool: &SqlitePool, account_id: i64) -> Result<Vec<String>, ApiError> {
    // Read-only: do not run ensure_schema (avoids write locks on auth).
    let mut conn = pool.acquire().await?;
    Ok(dedupe::source_priority_from_db(&mut conn, account_id).await?)
}

/// Username for an account id, when the account has one.
async fn load_username(pool: &SqlitePool, account_id: i64) -> Result<Option<String>, ApiError> {
    let mut conn = pool.acquire().await?;
    Ok(account_profile::username_for_account(&mut conn, account_id).await?)
}

/// Log in: verify a local username and password and answer the Session, a
/// `201 Created` whose `Location` is the singleton itself.
#[utoipa::path(
    post,
    path = "/v1/session",
    tag = "Session",
    request_body = CreateSessionRequest,
    responses(
        (
            status = 201,
            description = "Logged in; the Session exists",
            body = CreateSessionResponse,
            headers(("Location" = String, description = "`/v1/session`"))
        ),
        crate::problem::openapi::InvalidCredentials,
        crate::problem::openapi::AccountDisabled,
        crate::problem::openapi::RateLimited
    )
)]
pub async fn create_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateSessionRequest>,
) -> Result<Created<CreateSessionResponse>, ApiError> {
    let username = normalize_username(&req.username);
    if username.is_empty() {
        return Err(ApiError::validation("username is required"));
    }
    let app = crate::server::connecting_app(&headers);
    let app = app.as_ref();
    let mut conn = state.db.acquire().await?;
    let account_id = account_profile::lookup_account_by_username(&mut conn, &username).await?;
    // Counted per account, not per username as typed: the lookup ignores
    // case, so every spelling of a username guesses at the same password.
    let bucket = match account_id {
        Some(id) => password_bucket(id),
        None => unknown_username_bucket(&username),
    };
    // A rate-limited attempt is turned away before anything is checked, and
    // the Audit Trail records nothing of it.
    check_auth_rate_limit(&state.auth_rate_limits, &bucket)?;
    if req.password.len() > MAX_PASSWORD_BYTES {
        return Err(ApiError::validation("password is too long"));
    }
    // Refused logins for usernames nobody holds are kept for a fixed time,
    // and a login is when the old ones go.
    audit_trail::trim_refused_logins(&mut conn).await?;

    let password = req.password.clone();

    let Some(account_id) = account_id else {
        let _ = verify_password(dummy_password_hash(), &password);
        audit_trail::record_refused_login(
            &mut conn,
            &username,
            None,
            AuditReason::UnknownUsername,
            app,
        )
        .await?;
        return Err(ApiError::InvalidCredentials(
            "invalid username or password".into(),
        ));
    };

    // The Demo Account cannot be entered while it is being built: until the
    // build ends it holds part of its data, and a failed build removes it.
    // The login card is told there is no Demo Account, so the answer here is
    // the one for a username that does not exist.
    if account_id == account_profile::DEMO_ACCOUNT_ID && state.demo_build.is_building() {
        return Err(ApiError::InvalidCredentials(
            "invalid username or password".into(),
        ));
    }

    let stored_username = account_profile::username_for_account(&mut conn, account_id)
        .await?
        .unwrap_or_else(|| username.clone());
    let account = Some((account_id, stored_username.as_str()));
    let password_hash = account_profile::load_password_hash(&mut conn, account_id).await?;
    if !verify_login_password(password_hash.as_deref(), &password) {
        audit_trail::record_refused_login(
            &mut conn,
            &username,
            account,
            AuditReason::WrongPassword,
            app,
        )
        .await?;
        return Err(ApiError::InvalidCredentials(
            "invalid username or password".into(),
        ));
    }

    let auth = account_profile::load_account_auth(&mut conn, account_id)
        .await?
        .ok_or_else(|| ApiError::InvalidCredentials("invalid username or password".into()))?;
    if auth.disabled {
        audit_trail::record_refused_login(
            &mut conn,
            &username,
            account,
            AuditReason::AccountDisabled,
            app,
        )
        .await?;
        return Err(ApiError::AccountDisabled("this account is disabled".into()));
    }

    let body = CreateSessionResponse::for_existing_account(&mut conn, account_id, app).await?;

    Ok(Created {
        location: "/v1/session".to_string(),
        body,
    })
}

/// Revoke the session token. Returns whether it named a Session.
async fn logout_on_conn(conn: &mut SqliteConnection, token: &str) -> anyhow::Result<bool> {
    session_tokens::revoke_session_token(conn, token).await
}

/// Log out: revoke the presented session token, ending the Session.
///
/// It takes the bearer token itself rather than a guard, so a disabled
/// account can still end its own Session. An API token is not a Session and
/// is refused; a token that names nothing is a `401`.
#[utoipa::path(
    delete,
    path = "/v1/session",
    tag = "Session",
    security(("session" = [])),
    responses(
        (status = 204, description = "Logged out"),
    )
)]
pub async fn delete_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<axum::http::StatusCode, ApiError> {
    let token = crate::server::bearer_token(&headers)?;
    let mut conn = state.db.acquire().await?;
    schema::ensure_accounts_schema(&mut conn).await?;
    if logout_on_conn(&mut conn, &token).await? {
        return Ok(axum::http::StatusCode::NO_CONTENT);
    }
    if api_tokens::lookup_account_for_api_token(&mut conn, &token)
        .await?
        .is_some()
    {
        return Err(ApiError::InsufficientScope(
            "an API token is not a session and cannot log out; delete it under Settings instead"
                .into(),
        ));
    }
    Err(ApiError::AuthenticationRequired(
        "this token names no session".into(),
    ))
}

#[cfg(test)]
mod tests;
