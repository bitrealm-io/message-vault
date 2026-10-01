//! What a logged-out browser is allowed to know about this Message Crate, and the one
//! act it is allowed to perform: claiming an unclaimed Message Crate.
//!
//! `GET /v1/server` reports this Message Crate's state as a single value rather than the
//! two facts behind it — whether an owner exists, and whether public
//! registration is on — so that the rule joining them is stated once, on the
//! server. A browser and a desktop app that each derived the entry screen from
//! raw fields would be two copies of one rule, free to drift apart.
//!
//! These are the server's only unauthenticated routes besides logging in and
//! a stranger's `POST /v1/accounts`, and the first read routes that do not
//! require a session: the entry screen cannot have one yet, which is the
//! whole of the exception. See
//! `docs/adr/0008-the-owner-holds-no-messages.md`.

use axum::extract::State;
use serde::{Deserialize, Serialize};

use crate::db::{account_profile, server_settings, storage};
use crate::extract::Json;
use crate::server::{ApiError, AppState, Created, Owner};

/// What state a Message Crate is in, from outside.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ServerState {
    /// Nobody owns this Message Crate yet. The only thing to do is claim it.
    Unclaimed,
    /// Owned, and only the owner creates accounts.
    Closed,
    /// Owned, and anyone reaching the server may create their own account.
    Open,
}

/// The state of this Message Crate, for the screen a logged-out person sees.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ServerInfo {
    /// `unclaimed` shows Create Owner alone; `closed` shows Login alone;
    /// `open` shows Login and Create Account.
    pub state: ServerState,
    /// Whether the Demo Account exists. While it does, the screen offers a
    /// way into it beside whatever `state` shows: it has no password, so
    /// there is nothing to type.
    pub demo_account: bool,
    /// The server's Build: its Product Version, plus the commit it was built
    /// from unless it is a release. An app compares the Product Version with
    /// its own and says so when they differ; the server serves it either way.
    pub version: String,
    /// The Schema Fingerprint, the number this server stamps into its database.
    pub schema_fingerprint: i64,
}

/// Body for claiming a Message Crate.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ClaimRequest {
    /// Login username for the owner.
    pub username: String,
    /// Password for the owner. Must satisfy the server's password policy.
    pub password: String,
}

/// Read this Message Crate's state on an existing connection.
async fn state_on_conn(conn: &mut sqlx::AnyConnection) -> Result<ServerState, ApiError> {
    if !account_profile::is_claimed(conn).await? {
        return Ok(ServerState::Unclaimed);
    }
    let settings = server_settings::load(conn).await?;
    Ok(if settings.public_registration {
        ServerState::Open
    } else {
        ServerState::Closed
    })
}

/// Report whether this Message Crate is unclaimed, closed, or open.
#[utoipa::path(
    get,
    path = "/v1/server",
    tag = "Server",
    responses((status = 200, body = ServerInfo))
)]
pub async fn get_server(State(state): State<AppState>) -> Result<Json<ServerInfo>, ApiError> {
    let mut conn = state.db.acquire().await?;
    Ok(Json(ServerInfo {
        state: state_on_conn(&mut conn).await?,
        demo_account: account_profile::username_for_account(
            &mut conn,
            account_profile::DEMO_ACCOUNT_ID,
        )
        .await?
        .is_some(),
        version: crate::BUILD.to_string(),
        schema_fingerprint: crate::db::schema::SCHEMA_FINGERPRINT,
    }))
}

/// Claim an unclaimed Message Crate by creating its owner.
///
/// Unauthenticated, because a Message Crate with no owner has no credential
/// that could authorize this. Whoever reaches an unclaimed one first may
/// claim it: Message Crate is self-hosted, so its operator installs the
/// software, claims it, and publishes the port, in that order and at times
/// of their choosing. An unclaimed one is also empty, so a lost race
/// destroys nothing and announces itself at once.
#[utoipa::path(
    post,
    path = "/v1/server/claim",
    tag = "Server",
    request_body = ClaimRequest,
    responses(
        (
            status = 201,
            description = "Claimed; the owner's Session is made",
            body = crate::session_api::CreateSessionResponse,
            headers(("Location" = String, description = "`/v1/session`, the Session the claim made"))
        ),
        crate::problem::openapi::StateConflict,
        crate::problem::openapi::RateLimited
    )
)]
pub async fn claim_server(
    State(state): State<AppState>,
    Json(req): Json<ClaimRequest>,
) -> Result<Created<crate::session_api::CreateSessionResponse>, ApiError> {
    let username = crate::credentials::require_valid_username(&req.username)?;
    crate::credentials::check_auth_rate_limit(&state.auth_rate_limits, "claim")?;
    let password_hash = crate::credentials::hash_owner_password(&req.password)?;

    let mut conn = state.db.acquire().await?;
    // The claim check and the insert share a transaction: two requests racing
    // for an unclaimed Message Crate must not both believe they won it.
    let mut tx = sqlx::Connection::begin(&mut *conn).await?;
    if account_profile::is_claimed(&mut tx).await? {
        return Err(ApiError::StateConflict(
            "this Message Crate already has an owner".into(),
        ));
    }
    crate::credentials::require_username_free(&mut tx, &username).await?;
    account_profile::insert_account_at(
        &mut tx,
        account_profile::OWNER_ACCOUNT_ID,
        &username,
        Some(&password_hash),
        None,
    )
    .await
    .map_err(ApiError::Internal)?;
    let token = crate::db::session_tokens::insert_account_session_token(
        &mut tx,
        account_profile::OWNER_ACCOUNT_ID,
    )
    .await
    .map_err(ApiError::Internal)?;
    account_profile::record_login(&mut tx, account_profile::OWNER_ACCOUNT_ID).await?;
    tx.commit().await?;

    // The claim makes the owner's Session, and that is the resource it names.
    Ok(Created {
        location: "/v1/session".to_string(),
        body: crate::session_api::CreateSessionResponse {
            token,
            account_id: account_profile::OWNER_ACCOUNT_ID,
            username,
        },
    })
}

/// The server settings the owner controls.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ServerSettings {
    /// Anyone reaching the server may create their own account.
    pub public_registration: bool,
}

/// Body for changing the server settings. Omitted fields are left alone.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateServerSettingsRequest {
    /// Let anyone reaching the server create their own account, or stop them.
    #[serde(default)]
    pub public_registration: Option<bool>,
}

/// Read the server settings.
#[utoipa::path(
    get,
    path = "/v1/server/settings",
    tag = "Server",
    security(("session" = ["owner"])),
    responses(
        (status = 200, body = ServerSettings),
    )
)]
pub async fn get_server_settings(
    State(state): State<AppState>,
    Owner(_auth): Owner,
) -> Result<Json<ServerSettings>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let settings = server_settings::load(&mut conn).await?;
    Ok(Json(ServerSettings {
        public_registration: settings.public_registration,
    }))
}

/// Change the server settings.
#[utoipa::path(
    patch,
    path = "/v1/server/settings",
    tag = "Server",
    security(("session" = ["owner"])),
    request_body = UpdateServerSettingsRequest,
    responses(
        (status = 200, body = ServerSettings),
    )
)]
pub async fn update_server_settings(
    State(state): State<AppState>,
    Owner(_auth): Owner,
    Json(req): Json<UpdateServerSettingsRequest>,
) -> Result<Json<ServerSettings>, ApiError> {
    let mut conn = state.db.acquire().await?;
    if let Some(enabled) = req.public_registration {
        server_settings::set_public_registration(&mut conn, enabled).await?;
    }
    let settings = server_settings::load(&mut conn).await?;
    Ok(Json(ServerSettings {
        public_registration: settings.public_registration,
    }))
}

/// What the whole database holds, summed over every account.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ServerStorage {
    /// Messages across every account.
    pub message_count: i64,
    /// Conversations across every account.
    pub conversation_count: i64,
    /// Contacts across every account.
    pub contact_count: i64,
    /// Attachment rows across every account.
    pub attachment_count: i64,
    /// Attachment bytes across every account, by original file size.
    pub total_bytes: i64,
    /// Bytes the database takes on disk, measured. Attachment files are not
    /// in it; `total_bytes` has those.
    pub database_bytes: i64,
    /// Bytes the messages table and its indexes take, measured, without the
    /// full-text search index.
    pub messages_bytes: i64,
    /// Bytes the full-text search index takes, measured, for the whole
    /// database. It is one shared structure, so there is no per-account figure.
    pub fts_bytes: i64,
    /// Every account, including ones with no messages: the owner first, then
    /// by username, as the User Accounts table lists them.
    pub accounts: Vec<AccountMessages>,
}

/// One account's share of the messages held: an id, a username and numbers.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct AccountMessages {
    pub account_id: i64,
    pub username: String,
    /// Messages the account holds.
    pub message_count: i64,
    /// Bytes of message text the account holds: every body and subject, added up.
    pub text_bytes: i64,
    /// The account's estimated share of `messages_bytes`, split by its share
    /// of all text. The shares add up to `messages_bytes` exactly.
    pub estimated_message_bytes: i64,
}

/// Read what the database holds. The counts and the attachment bytes are summed
/// over every account. The database, messages and full-text search sizes
/// are measured on disk. Each account's share of message storage is an
/// estimate from its share of text. Counts and totals only, never a name or
/// a line of text (`docs/adr/0008-the-owner-holds-no-messages.md`,
/// "What the owner may see"). The owner's, because the owner administers this
/// Message Crate and nobody else holds more than their own account.
#[utoipa::path(
    get,
    path = "/v1/server/storage",
    tag = "Server",
    security(("session" = ["owner"])),
    responses(
        (status = 200, body = ServerStorage),
    )
)]
pub async fn get_server_storage(
    State(state): State<AppState>,
    Owner(_auth): Owner,
) -> Result<Json<ServerStorage>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let scope = storage::Scope::AllAccounts;
    let fts_bytes = storage::fts_bytes(&mut conn).await?;
    let messages_bytes = storage::messages_bytes(&mut conn, fts_bytes).await?;
    let by_account = storage::text_by_account(&mut conn).await?;
    let shares = storage::split_by_text(messages_bytes, &by_account);
    let accounts = by_account
        .into_iter()
        .zip(shares)
        .map(|(account, estimated_message_bytes)| AccountMessages {
            account_id: account.account_id,
            username: account.username,
            message_count: account.message_count,
            text_bytes: account.text_bytes,
            estimated_message_bytes,
        })
        .collect();
    Ok(Json(ServerStorage {
        message_count: storage::message_count(&mut conn, scope).await?,
        conversation_count: storage::conversation_count(&mut conn, scope).await?,
        contact_count: storage::contact_count(&mut conn, scope).await?,
        attachment_count: storage::attachment_count(&mut conn, scope).await?,
        total_bytes: storage::attachment_bytes(&mut conn, scope).await?,
        database_bytes: storage::database_bytes(&mut conn).await?,
        messages_bytes,
        fts_bytes,
        accounts,
    }))
}

#[cfg(test)]
mod tests;
