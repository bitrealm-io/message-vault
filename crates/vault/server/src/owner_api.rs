//! Account management for the vault owner. Every route requires the owner's
//! session; no API token ever resolves to the owner, so none can reach here.
//!
//! Responses carry account metadata, counts, and storage sizes — never
//! message content. Multitenancy stays inviolable here: the owner manages
//! accounts, not the contents of other people's vaults, so nothing in this
//! module reads `messages.body`, `attachments.transcription`, or any other
//! content column. See `docs/adr/0008-the-vault-owner-holds-no-messages.md`.

use crate::extract::{Json, Path};
use axum::extract::State;
use serde::{Deserialize, Serialize};
use sqlx::{AnyConnection, Connection};

use crate::db::account_profile;
use crate::server::{ApiError, AppState, Owner};

/// One account as the vault owner sees it: who it is and what it holds, never
/// what it says.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ManagedAccount {
    /// Account id.
    pub account_id: String,
    /// Login username.
    pub username: String,
    /// May not sign in.
    pub disabled: bool,
    /// Still carries the password the owner chose, and must replace it.
    pub must_change_password: bool,
    /// May call the import endpoints.
    pub can_import: bool,
    /// May call the export endpoints.
    pub can_export: bool,
    /// May destroy message data.
    pub can_delete: bool,
    /// Messages this account owns.
    pub message_count: i64,
    /// Attachment bytes this account owns.
    pub storage_bytes: i64,
}

/// Every account in the vault except the owner's own.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ListAccountsResponse {
    /// One row per account.
    pub items: Vec<ManagedAccount>,
}

/// Body for creating an account as the vault owner.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateAccountRequest {
    /// Login username.
    pub username: String,
    /// Initial password. Must satisfy the vault's password policy. The
    /// account holder is made to replace it at first sign-in, so it survives
    /// exactly one session.
    pub password: String,
}

/// Body for changing an account's flags. Omitted fields are left alone.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct PatchAccountRequest {
    /// Disable or re-enable sign-in.
    #[serde(default)]
    pub disabled: Option<bool>,
    /// Allow or forbid import.
    #[serde(default)]
    pub can_import: Option<bool>,
    /// Allow or forbid export.
    #[serde(default)]
    pub can_export: Option<bool>,
    /// Allow or forbid deleting message data.
    #[serde(default)]
    pub can_delete: Option<bool>,
}

/// Body for the vault owner setting someone's password.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SetPasswordRequest {
    /// The new password. Must satisfy the vault's password policy.
    pub password: String,
}

/// Number of messages an account owns. Never touches message content.
async fn account_message_count(
    conn: &mut AnyConnection,
    account_id: &str,
) -> Result<i64, ApiError> {
    Ok(
        sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE account_id = $1")
            .bind(account_id)
            .fetch_one(&mut *conn)
            .await?,
    )
}

/// Load one account's owner-facing row: flags, message count, storage bytes.
/// `None` when the account no longer exists.
async fn load_managed_account(
    conn: &mut AnyConnection,
    account_id: &str,
) -> Result<Option<ManagedAccount>, ApiError> {
    let row: Option<(String, i64, i64, i64, i64, i64)> = sqlx::query_as(
        "SELECT username, disabled, must_change_password, can_import, can_export, can_delete
         FROM accounts WHERE id = $1",
    )
    .bind(account_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((username, disabled, must_change, import, export, delete)) = row else {
        return Ok(None);
    };
    let message_count = account_message_count(conn, account_id).await?;
    let storage_bytes =
        crate::db::vault_imports::account_attachment_bytes(conn, account_id).await?;
    Ok(Some(ManagedAccount {
        account_id: account_id.to_string(),
        username,
        disabled: disabled != 0,
        must_change_password: must_change != 0,
        can_import: import != 0,
        can_export: export != 0,
        can_delete: delete != 0,
        message_count,
        storage_bytes,
    }))
}

/// Return `404 Not Found` unless an account with this id exists and is one
/// the owner manages.
///
/// The owner's own id is reported as absent rather than forbidden. It is
/// absent from the list for the same reason: the list holds the users of this
/// vault, and the owner is not one of them.
async fn require_managed_account(
    conn: &mut AnyConnection,
    account_id: &str,
) -> Result<(), ApiError> {
    if account_profile::is_vault_owner(account_id)
        || account_profile::username_for_account(conn, account_id)
            .await?
            .is_none()
    {
        return Err(ApiError::NotFound(format!(
            "account {account_id} not found"
        )));
    }
    Ok(())
}

/// List the accounts this vault holds, with their flags, message count, and
/// storage use. The owner's own account is not among them.
#[utoipa::path(
    get,
    path = "/v1/owner/accounts",
    tag = "Owner",
    operation_id = "owner_list_accounts",
    security(("bearer" = [])),
    responses(
        (status = 200, body = ListAccountsResponse),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody)
    )
)]
pub async fn list_accounts_handler(
    State(state): State<AppState>,
    Owner(_auth): Owner,
) -> Result<Json<ListAccountsResponse>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let ids: Vec<String> =
        sqlx::query_scalar("SELECT id FROM accounts WHERE id != $1 ORDER BY username")
            .bind(account_profile::OWNER_ACCOUNT_ID)
            .fetch_all(&mut *conn)
            .await?;

    let mut items = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(account) = load_managed_account(&mut conn, &id).await? {
            items.push(account);
        }
    }
    Ok(Json(ListAccountsResponse { items }))
}

/// Create an account. The owner picks the first password and the account
/// holder replaces it at first sign-in, so the owner's choice survives one
/// session and no longer.
#[utoipa::path(
    post,
    path = "/v1/owner/accounts",
    tag = "Owner",
    operation_id = "owner_create_account",
    security(("bearer" = [])),
    request_body = CreateAccountRequest,
    responses(
        (status = 200, body = ManagedAccount),
        (status = 400, body = crate::server::ErrorBody),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody)
    )
)]
pub async fn create_account_handler(
    State(state): State<AppState>,
    Owner(_auth): Owner,
    Json(req): Json<CreateAccountRequest>,
) -> Result<Json<ManagedAccount>, ApiError> {
    let username = crate::auth::normalize_username(&req.username);
    if !crate::auth::is_valid_username(&username) {
        return Err(ApiError::BadRequest(
            "username must be 1–128 chars (alphanumeric, _, -, .)".into(),
        ));
    }
    crate::auth::validate_password_policy(&req.password)?;
    let password_hash = crate::auth::hash_password(&req.password)?;

    let mut conn = state.db.acquire().await?;
    // The insert and the forced-change mark must land together: a failure
    // between them would leave an account whose holder keeps the password the
    // owner chose, which is the one thing this pair exists to prevent.
    let mut tx = conn.begin().await?;
    crate::auth::require_username_free(&mut tx, &username).await?;

    let account_id = uuid::Uuid::new_v4().to_string();
    account_profile::insert_account(&mut tx, &account_id, &username, Some(&password_hash), None)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    account_profile::set_must_change_password(&mut tx, &account_id, true).await?;
    // The owner names a username and a password and nothing else, so the
    // account arrives with no display name and no handles. Its holder sets
    // that up themselves, after replacing the password.
    account_profile::set_must_set_up_profile(&mut tx, &account_id, true).await?;
    tx.commit().await?;

    let account = load_managed_account(&mut conn, &account_id)
        .await?
        .ok_or_else(|| {
            ApiError::Internal(anyhow::anyhow!("account vanished immediately after insert"))
        })?;
    Ok(Json(account))
}

/// Change an account's disabled flag or its import, export and delete
/// permissions.
///
/// Clearing `can_import` or `can_export` also narrows every API token that
/// account has already issued, because a token's permissions are intersected
/// with its account's on every request. The owner restrains the account and
/// the tokens follow, without ever seeing one.
#[utoipa::path(
    patch,
    path = "/v1/owner/accounts/{id}",
    tag = "Owner",
    operation_id = "owner_patch_account",
    security(("bearer" = [])),
    params(("id" = String, Path, description = "Account id to modify")),
    request_body = PatchAccountRequest,
    responses(
        (status = 200, body = ManagedAccount),
        (status = 400, body = crate::server::ErrorBody),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody),
        (status = 404, body = crate::server::ErrorBody)
    )
)]
pub async fn patch_account_handler(
    State(state): State<AppState>,
    Path(target): Path<String>,
    Owner(_auth): Owner,
    Json(req): Json<PatchAccountRequest>,
) -> Result<Json<ManagedAccount>, ApiError> {
    let mut conn = state.db.acquire().await?;
    require_managed_account(&mut conn, &target).await?;

    // Column names come from this compile-time array, never from the
    // request, so formatting them into the SQL is safe; values stay bound.
    let flags = [
        ("disabled", req.disabled),
        ("can_import", req.can_import),
        ("can_export", req.can_export),
        ("can_delete", req.can_delete),
    ];
    for (column, value) in flags {
        let Some(value) = value else { continue };
        sqlx::query(&format!("UPDATE accounts SET {column} = $1 WHERE id = $2"))
            .bind(i32::from(value))
            .bind(&target)
            .execute(&mut *conn)
            .await?;
    }

    let account = load_managed_account(&mut conn, &target)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("account {target} not found")))?;
    Ok(Json(account))
}

/// Set an account's password. Invalidates that account's existing session
/// (unlike a self-service password change, which leaves other sessions alone)
/// — after this call the account holder must sign in again with the new
/// password, and replace it once they do.
#[utoipa::path(
    put,
    path = "/v1/owner/accounts/{id}/password",
    tag = "Owner",
    operation_id = "owner_set_account_password",
    security(("bearer" = [])),
    params(("id" = String, Path, description = "Account id whose password is set")),
    request_body = SetPasswordRequest,
    responses(
        (status = 204, description = "Password set"),
        (status = 400, body = crate::server::ErrorBody),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody),
        (status = 404, body = crate::server::ErrorBody)
    )
)]
pub async fn set_account_password_handler(
    State(state): State<AppState>,
    Path(target): Path<String>,
    Owner(_auth): Owner,
    Json(req): Json<SetPasswordRequest>,
) -> Result<axum::http::StatusCode, ApiError> {
    crate::auth::validate_password_policy(&req.password)?;
    let hash = crate::auth::hash_password(&req.password)?;

    let mut conn = state.db.acquire().await?;
    require_managed_account(&mut conn, &target).await?;
    account_profile::update_password_hash(&mut conn, &target, &hash).await?;
    account_profile::set_must_change_password(&mut conn, &target, true).await?;
    crate::db::session_tokens::revoke_account_sessions(&mut conn, &target).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Destroy one account's conversations, messages, and attachments. The
/// account itself, its contacts, and its login survive.
#[utoipa::path(
    delete,
    path = "/v1/owner/accounts/{id}/messages",
    tag = "Owner",
    operation_id = "owner_delete_account_messages",
    security(("bearer" = [])),
    params(("id" = String, Path, description = "Account whose messages are destroyed")),
    responses(
        (status = 200, body = crate::profile::DeleteMessagesResponse),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody),
        (status = 404, body = crate::server::ErrorBody)
    )
)]
pub async fn delete_account_messages_handler(
    State(state): State<AppState>,
    Path(target): Path<String>,
    Owner(_auth): Owner,
) -> Result<Json<crate::profile::DeleteMessagesResponse>, ApiError> {
    let mut conn = state.db.acquire().await?;
    require_managed_account(&mut conn, &target).await?;

    let stats = account_profile::delete_all_messages_for_account(&mut conn, &target).await?;
    crate::profile::remove_account_asset_trees(
        &state.cfg.paths.data_dir,
        &target,
        &state.cfg.paths.assets_dir,
        &state.cfg.paths.assets_converted_dir,
    )?;

    Ok(Json(crate::profile::DeleteMessagesResponse {
        conversations: stats.conversations,
        attachments: stats.attachments,
    }))
}

/// Permanently delete an account: login, profile, contacts, and every message
/// it owns. The demo account is deleted like any other, which is how a demo
/// vault is cleared into a real one.
#[utoipa::path(
    delete,
    path = "/v1/owner/accounts/{id}",
    tag = "Owner",
    operation_id = "owner_delete_account",
    security(("bearer" = [])),
    params(("id" = String, Path, description = "Account id to delete")),
    responses(
        (status = 204, description = "Account deleted"),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody),
        (status = 404, body = crate::server::ErrorBody)
    )
)]
pub async fn delete_account_handler(
    State(state): State<AppState>,
    Path(target): Path<String>,
    Owner(_auth): Owner,
) -> Result<axum::http::StatusCode, ApiError> {
    let mut conn = state.db.acquire().await?;
    require_managed_account(&mut conn, &target).await?;

    account_profile::delete_account(&mut conn, &target).await?;
    let account_root = state.cfg.paths.data_dir.join(&target);
    if account_root.exists() {
        tokio::task::spawn_blocking(move || std::fs::remove_dir_all(&account_root))
            .await
            .map_err(|e| ApiError::Internal(e.into()))?
            .map_err(|e| ApiError::Internal(e.into()))?;
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests;
