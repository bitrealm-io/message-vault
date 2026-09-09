//! The Session singleton: `POST`, `GET` and `DELETE /v1/session`.
//!
//! A Session is one per signed-in account or owner. Signing in creates it,
//! reading it says which account the bearer token names, and signing out
//! ends it. The password rules it applies live in `credentials`.

use axum::extract::State;
use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use sqlx::{AnyConnection, AnyPool};

use crate::credentials::{
    MAX_PASSWORD_BYTES, check_auth_rate_limit, dummy_password_hash, normalize_username,
    verify_login_password, verify_password,
};
use crate::db::{account_profile, schema, session_tokens};
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
pub struct SessionTokenResponse {
    /// Session token to send as `Authorization: Bearer …`.
    pub token: String,
    /// Account id the session belongs to.
    pub account_id: i64,
    /// Account username (falls back to the account id).
    pub username: String,
}

impl SessionTokenResponse {
    /// Issue (or reuse) the session token for an existing account. Uses the
    /// account id when the row has no username.
    async fn for_existing_account(
        conn: &mut AnyConnection,
        account_id: i64,
    ) -> anyhow::Result<SessionTokenResponse> {
        let token = session_tokens::get_or_create_session_token(conn, account_id).await?;
        let username = account_profile::username_for_account(conn, account_id)
            .await?
            .unwrap_or_else(|| account_id.to_string());
        Ok(SessionTokenResponse {
            token,
            account_id,
            username,
        })
    }
}

/// The signed-in credential's account, username, and import sources.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct SessionResponse {
    sources: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    account_id: Option<i64>,
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
    security(("session" = []), ("api-token" = [])),
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
    let username = load_username(&state.db, account_id).await?;
    let sources = list_account_sources(&state.db, account_id).await?;
    Ok(Json(SessionResponse {
        sources,
        account_id: Some(account_id),
        username,
    }))
}

/// Source ids this account has imported, oldest first.
async fn list_account_sources(pool: &AnyPool, account_id: i64) -> Result<Vec<String>, ApiError> {
    // Read-only: do not run ensure_vault_schema (avoids write locks on auth).
    let mut conn = pool.acquire().await?;
    Ok(dedupe::source_priority_from_db(&mut conn, account_id).await?)
}

/// Username for an account id, when the account has one.
async fn load_username(pool: &AnyPool, account_id: i64) -> Result<Option<String>, ApiError> {
    let mut conn = pool.acquire().await?;
    Ok(account_profile::username_for_account(&mut conn, account_id).await?)
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
    let Some(account_id) =
        account_profile::lookup_account_by_username(&mut conn, &username).await?
    else {
        let _ = verify_password(dummy_password_hash(), &password);
        return Err(ApiError::InvalidCredentials(
            "invalid username or password".into(),
        ));
    };

    let password_hash = account_profile::load_password_hash(&mut conn, account_id).await?;
    if !verify_login_password(password_hash.as_deref(), &password) {
        return Err(ApiError::InvalidCredentials(
            "invalid username or password".into(),
        ));
    }

    let auth = account_profile::load_account_auth(&mut conn, account_id)
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

/// Revoke the session token.
async fn logout_on_conn(conn: &mut AnyConnection, token: &str) -> anyhow::Result<()> {
    let _ = session_tokens::revoke_session_token(conn, token).await?;
    Ok(())
}

/// Sign out: revoke the presented session token, ending the Session.
#[utoipa::path(
    delete,
    path = "/v1/session",
    tag = "Session",
    security(("session" = [])),
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
