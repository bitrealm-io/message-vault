//! What a logged-out browser is allowed to know about this vault, and the one
//! act it is allowed to perform: claiming an unclaimed vault.
//!
//! `GET /v1/vault` reports the vault's state as a single value rather than the
//! two facts behind it — whether an owner exists, and whether public
//! registration is on — so that the rule joining them is stated once, on the
//! server. A browser and a desktop app that each derived the entry screen from
//! raw fields would be two copies of one rule, free to drift apart.
//!
//! These are the vault's only unauthenticated routes besides logging in and
//! a stranger's `POST /v1/accounts`, and the first read routes that do not
//! require a session: the entry screen cannot have one yet, which is the
//! whole of the exception. See
//! `docs/adr/0008-the-vault-owner-holds-no-messages.md`.

use axum::extract::State;
use serde::{Deserialize, Serialize};

use crate::db::{account_profile, storage, vault_settings};
use crate::extract::Json;
use crate::server::{ApiError, AppState, Owner};

/// What state a vault is in, from outside.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum VaultState {
    /// Nobody owns this vault yet. The only thing to do is claim it.
    Unclaimed,
    /// Owned, and only the vault owner creates accounts.
    Closed,
    /// Owned, and anyone reaching the vault may create their own account.
    Open,
}

/// The vault's state, for the screen a logged-out person sees.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct VaultResponse {
    /// `unclaimed` shows Create Vault Owner alone; `closed` shows Login alone;
    /// `open` shows Login and Create Account.
    pub state: VaultState,
    /// This vault's Build: its Product Version, plus the commit it was built
    /// from unless it is a release. An app compares the Product Version with
    /// its own and says so when they differ; the vault serves it either way.
    pub version: String,
    /// The Schema Fingerprint, the number this vault stamps into its database.
    pub schema_fingerprint: i64,
}

/// Body for claiming a vault.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ClaimVaultRequest {
    /// Login username for the vault owner.
    pub username: String,
    /// Password for the vault owner. Must satisfy the vault's password policy.
    pub password: String,
}

/// Read the vault's state on an existing connection.
async fn state_on_conn(conn: &mut sqlx::AnyConnection) -> Result<VaultState, ApiError> {
    if !account_profile::vault_is_claimed(conn).await? {
        return Ok(VaultState::Unclaimed);
    }
    let settings = vault_settings::load(conn).await?;
    Ok(if settings.public_registration {
        VaultState::Open
    } else {
        VaultState::Closed
    })
}

/// Report whether this vault is unclaimed, closed, or open.
#[utoipa::path(
    get,
    path = "/v1/vault",
    tag = "Vault",
    operation_id = "vault_state",
    responses((status = 200, body = VaultResponse))
)]
pub async fn vault_state_handler(
    State(state): State<AppState>,
) -> Result<Json<VaultResponse>, ApiError> {
    let mut conn = state.db.acquire().await?;
    Ok(Json(VaultResponse {
        state: state_on_conn(&mut conn).await?,
        version: crate::BUILD.to_string(),
        schema_fingerprint: crate::db::schema::SCHEMA_FINGERPRINT,
    }))
}

/// Claim an unclaimed vault by creating its owner.
///
/// Unauthenticated, because a vault with no owner has no credential that
/// could authorize this. Whoever reaches an unclaimed vault first may claim
/// it: the vault is self-hosted, so its operator installs the software,
/// claims the vault, and publishes the port, in that order and at times of
/// their choosing. An unclaimed vault is also empty, so a lost race destroys
/// nothing and announces itself at once.
#[utoipa::path(
    post,
    path = "/v1/vault/claim",
    tag = "Vault",
    operation_id = "claim_vault",
    request_body = ClaimVaultRequest,
    responses(
        (status = 200, description = "Vault claimed; session issued", body = crate::session_api::SessionTokenResponse),
        (status = 400, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem),
        (status = 409, description = "Already claimed", body = crate::problem::Problem),
        (status = 429, body = crate::problem::Problem)
    )
)]
pub async fn claim_vault_handler(
    State(state): State<AppState>,
    Json(req): Json<ClaimVaultRequest>,
) -> Result<Json<crate::session_api::SessionTokenResponse>, ApiError> {
    let username = crate::credentials::require_valid_username(&req.username)?;
    crate::credentials::check_auth_rate_limit(&state.auth_rate_limits, "claim")?;
    let password_hash = crate::credentials::hash_owner_password(&req.password)?;

    let mut conn = state.db.acquire().await?;
    // The claim check and the insert share a transaction: two requests racing
    // for an unclaimed vault must not both believe they won it.
    let mut tx = sqlx::Connection::begin(&mut *conn).await?;
    if account_profile::vault_is_claimed(&mut tx).await? {
        return Err(ApiError::StateConflict(
            "this vault already has an owner".into(),
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

    Ok(Json(crate::session_api::SessionTokenResponse {
        token,
        account_id: account_profile::OWNER_ACCOUNT_ID,
        username,
    }))
}

/// The vault settings the owner controls.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct VaultSettingsResponse {
    /// Anyone reaching the vault may create their own account.
    pub public_registration: bool,
}

/// Body for changing the vault's settings. Omitted fields are left alone.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct PatchVaultSettingsRequest {
    /// Let anyone reaching the vault create their own account, or stop them.
    #[serde(default)]
    pub public_registration: Option<bool>,
}

/// Read the vault's settings.
#[utoipa::path(
    get,
    path = "/v1/vault/settings",
    tag = "Vault",
    operation_id = "vault_settings",
    security(("session" = ["owner"])),
    responses(
        (status = 200, body = VaultSettingsResponse),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem)
    )
)]
pub async fn vault_settings_handler(
    State(state): State<AppState>,
    Owner(_auth): Owner,
) -> Result<Json<VaultSettingsResponse>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let settings = vault_settings::load(&mut conn).await?;
    Ok(Json(VaultSettingsResponse {
        public_registration: settings.public_registration,
    }))
}

/// Change the vault's settings.
#[utoipa::path(
    patch,
    path = "/v1/vault/settings",
    tag = "Vault",
    operation_id = "patch_vault_settings",
    security(("session" = ["owner"])),
    request_body = PatchVaultSettingsRequest,
    responses(
        (status = 200, body = VaultSettingsResponse),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem)
    )
)]
pub async fn patch_vault_settings_handler(
    State(state): State<AppState>,
    Owner(_auth): Owner,
    Json(req): Json<PatchVaultSettingsRequest>,
) -> Result<Json<VaultSettingsResponse>, ApiError> {
    let mut conn = state.db.acquire().await?;
    if let Some(enabled) = req.public_registration {
        vault_settings::set_public_registration(&mut conn, enabled).await?;
    }
    let settings = vault_settings::load(&mut conn).await?;
    Ok(Json(VaultSettingsResponse {
        public_registration: settings.public_registration,
    }))
}

/// What the whole vault holds, summed over every account.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct VaultStorageResponse {
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
    /// vault. It is one shared structure, so there is no per-account figure.
    pub fts_bytes: i64,
    /// Every account, including ones with no messages: the owner first, then
    /// by username, as the User Accounts table lists them.
    pub accounts: Vec<AccountMessagesResponse>,
}

/// One account's share of the messages held: an id, a username and numbers.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct AccountMessagesResponse {
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

/// Read what the vault holds: the message, conversation, contact and
/// attachment counts and the attachment bytes, summed over every account;
/// what the database takes on disk, measured; and each account's estimated
/// share of message storage. Counts and totals only, never a name or a line
/// of text (`docs/adr/0008-the-vault-owner-holds-no-messages.md`, "What the
/// owner may see"). The owner's, because the owner administers the vault and
/// nobody else holds more than their own account.
#[utoipa::path(
    get,
    path = "/v1/vault/storage",
    tag = "Vault",
    operation_id = "vault_storage",
    security(("session" = ["owner"])),
    responses(
        (status = 200, body = VaultStorageResponse),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem)
    )
)]
pub async fn vault_storage_handler(
    State(state): State<AppState>,
    Owner(_auth): Owner,
) -> Result<Json<VaultStorageResponse>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let scope = storage::Scope::Vault;
    let messages_bytes = storage::messages_bytes(&mut conn).await?;
    let by_account = storage::text_by_account(&mut conn).await?;
    let shares = storage::split_by_text(messages_bytes, &by_account);
    let accounts = by_account
        .into_iter()
        .zip(shares)
        .map(
            |(account, estimated_message_bytes)| AccountMessagesResponse {
                account_id: account.account_id,
                username: account.username,
                message_count: account.message_count,
                text_bytes: account.text_bytes,
                estimated_message_bytes,
            },
        )
        .collect();
    Ok(Json(VaultStorageResponse {
        message_count: storage::message_count(&mut conn, scope).await?,
        conversation_count: storage::conversation_count(&mut conn, scope).await?,
        contact_count: storage::contact_count(&mut conn, scope).await?,
        attachment_count: storage::attachment_count(&mut conn, scope).await?,
        total_bytes: storage::attachment_bytes(&mut conn, scope).await?,
        database_bytes: storage::database_bytes(&mut conn).await?,
        messages_bytes,
        fts_bytes: storage::fts_bytes(&mut conn).await?,
        accounts,
    }))
}

#[cfg(test)]
mod tests;
