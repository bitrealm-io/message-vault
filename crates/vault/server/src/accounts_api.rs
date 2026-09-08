//! The accounts collection: `/v1/accounts` and everything under a member
//! except API tokens, which `api_tokens_api` serves.
//!
//! One collection serves the vault owner and every account. Who may call a
//! route is decided here, per handler, never by the path: the owner reaches
//! every row, an account reaches its own, and a stranger may create one while
//! the vault is open. `docs/agents/http-api-rules.md` records the rule and the
//! role prefix it replaced.
//!
//! The owner manages accounts, not the contents of other people's vaults, so
//! nothing here reads `messages.body`, `attachments.transcription`, or any
//! other content column: a row carries who an account is, what it may do and
//! how much it holds, never what it says.
//! See `docs/adr/0008-the-vault-owner-holds-no-messages.md`.

use anyhow::Context;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use message_ir::HandleType;
use serde::{Deserialize, Serialize};
use sqlx::{AnyConnection, Connection};

use crate::credentials::{
    MAX_PASSWORD_BYTES, change_password_on_conn, check_auth_rate_limit, hash_password,
    passwords_match, require_username_free, require_valid_username, validate_password_policy,
};
use crate::db::{account_profile, session_tokens, vault_imports, vault_settings};
use crate::extract::{Json, Path};
use crate::server::{ApiError, AppState, AuthIdentity, Created, Owner, SignedIn};

// ---------------------------------------------------------------------------
// The account as every caller sees it
// ---------------------------------------------------------------------------

/// One account: who it is, what it may do, and how much it holds. The owner
/// and the account itself both read the whole struct; nothing in it is a
/// message.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct AccountResponse {
    /// Account id.
    pub account_id: i64,
    /// Login username.
    pub username: String,
    /// Display name, when set.
    pub preferred_name: Option<String>,
    /// IANA time zone every message time, day and year is shown in, for
    /// example `America/New_York`. Chosen at profile setup.
    pub time_zone: String,
    /// Phone handles linked to the account.
    pub phones: Vec<String>,
    /// Email addresses linked to the account.
    pub emails: Vec<String>,
    /// True for the seeded demo account (cannot be deleted).
    pub is_demo: bool,
    /// True for the vault owner: manages accounts, holds no messages.
    pub is_owner: bool,
    /// May not sign in.
    pub disabled: bool,
    /// The vault owner chose this password; it must be replaced before the
    /// account can be used.
    pub must_change_password: bool,
    /// The account holder has not set up their profile yet, so profile setup
    /// is owed before the account can be used. The vault decides this, not the
    /// client: the same answer reaches every app, and it survives cleared site
    /// data and a second browser.
    pub must_set_up_profile: bool,
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
    pub items: Vec<AccountResponse>,
}

/// Number of messages an account owns. Never touches message content.
async fn account_message_count(conn: &mut AnyConnection, account_id: i64) -> Result<i64, ApiError> {
    Ok(
        sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE account_id = $1")
            .bind(account_id)
            .fetch_one(&mut *conn)
            .await?,
    )
}

/// Load one account's row. `None` when the account does not exist.
async fn load_account(
    conn: &mut AnyConnection,
    account_id: i64,
) -> Result<Option<AccountResponse>, ApiError> {
    let Some(username) = account_profile::username_for_account(conn, account_id).await? else {
        return Ok(None);
    };
    let Some(auth) = account_profile::load_account_auth(conn, account_id).await? else {
        return Ok(None);
    };
    let preferred_name = account_profile::load_preferred_name(conn, account_id).await?;
    let time_zone = account_profile::load_time_zone(conn, account_id)
        .await?
        .name()
        .to_string();
    let profile = account_profile::load_account_profile(conn, account_id).await?;
    let message_count = account_message_count(conn, account_id).await?;
    let storage_bytes = vault_imports::account_attachment_bytes(conn, account_id).await?;
    Ok(Some(AccountResponse {
        account_id,
        username,
        preferred_name,
        time_zone,
        phones: profile.phones,
        emails: profile.emails,
        is_demo: account_profile::is_demo_account(account_id),
        is_owner: account_profile::is_vault_owner(account_id),
        disabled: auth.disabled,
        must_change_password: auth.must_change_password,
        must_set_up_profile: auth.must_set_up_profile,
        can_import: auth.permissions.import,
        can_export: auth.permissions.export,
        can_delete: auth.permissions.delete,
        message_count,
        storage_bytes,
    }))
}

/// Load one account's row, or `404 Not Found`.
async fn require_account(
    conn: &mut AnyConnection,
    account_id: i64,
) -> Result<AccountResponse, ApiError> {
    load_account(conn, account_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("account {account_id} not found")))
}

// ---------------------------------------------------------------------------
// Who the caller is to the addressed row
// ---------------------------------------------------------------------------

/// What the caller is to the account a member route addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// The vault owner, acting on an account that is not its own.
    Owner,
    /// The account itself, the owner's own row included.
    Own,
}

/// Admit the vault owner to any row and an account to its own, and say
/// which. A caller addressing a row that is neither answers `403`, whether
/// or not the row exists, so the refusal says nothing about the vault's
/// accounts. The owner alone learns that an id is absent.
async fn require_owner_or_self(
    conn: &mut AnyConnection,
    auth: &AuthIdentity,
    target: i64,
) -> Result<Reach, ApiError> {
    if auth.account_id == target {
        return Ok(Reach::Own);
    }
    if !auth.is_owner() {
        return Err(ApiError::NotTheOwner(format!(
            "account {target} is not yours, and only the vault owner reaches other accounts"
        )));
    }
    if account_profile::username_for_account(conn, target)
        .await?
        .is_none()
    {
        return Err(ApiError::NotFound(format!("account {target} not found")));
    }
    Ok(Reach::Owner)
}

// ---------------------------------------------------------------------------
// The collection
// ---------------------------------------------------------------------------

/// List the accounts this vault holds, with their flags, message count, and
/// storage use. The owner's own account is not among them: the list holds
/// the users of this vault, and the owner is not one of them.
#[utoipa::path(
    get,
    path = "/v1/accounts",
    tag = "Accounts",
    operation_id = "list_accounts",
    security(("bearer" = [])),
    responses(
        (status = 200, body = ListAccountsResponse),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem)
    )
)]
pub async fn list_accounts_handler(
    State(state): State<AppState>,
    Owner(_auth): Owner,
) -> Result<Json<ListAccountsResponse>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let ids: Vec<i64> =
        sqlx::query_scalar("SELECT id FROM accounts WHERE id != $1 ORDER BY username")
            .bind(account_profile::OWNER_ACCOUNT_ID)
            .fetch_all(&mut *conn)
            .await?;

    let mut items = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(account) = load_account(&mut conn, id).await? {
            items.push(account);
        }
    }
    Ok(Json(ListAccountsResponse { items }))
}

/// Body for creating an account, by the vault owner or by a stranger.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateAccountRequest {
    /// Login username.
    pub username: String,
    /// Local password. The owner must give one, and the account holder
    /// replaces it at first sign-in. A stranger may leave it absent or empty
    /// to open an account with no password.
    #[serde(default)]
    pub password: Option<String>,
    /// Display name shown in the vault.
    #[serde(default)]
    pub preferred_name: Option<String>,
    /// Phone number linked to the account.
    #[serde(default)]
    pub phone: Option<String>,
}

/// The account that was created, and the Session a stranger's registration
/// opens on it. The owner's creation opens no session, so `token` is absent.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CreatedAccountResponse {
    /// The new account.
    #[serde(flatten)]
    pub account: AccountResponse,
    /// Session token to send as `Authorization: Bearer …`. Present only when
    /// a stranger registered, because they are signed in on creation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

/// Create an account.
///
/// The vault owner may always: the owner picks the first password and the
/// account holder replaces it at first sign-in, so the owner's choice survives
/// one session and no longer. A stranger with no credential may while the
/// vault is open, and is signed in on creation. Registering is the vault's
/// only self-service door, shut unless the owner has opened it; an unclaimed
/// vault is shut too, because its first act is being claimed, not being
/// joined.
#[utoipa::path(
    post,
    path = "/v1/accounts",
    tag = "Accounts",
    operation_id = "create_account",
    security((), ("bearer" = [])),
    request_body = CreateAccountRequest,
    responses(
        (
            status = 201,
            body = CreatedAccountResponse,
            headers(("Location" = String, description = "Path of the new account"))
        ),
        (status = 400, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem),
        (status = 403, description = "The vault is closed, or the credential is not the owner's", body = crate::problem::Problem),
        (status = 409, description = "Username taken", body = crate::problem::Problem),
        (status = 429, description = "Rate limited", body = crate::problem::Problem)
    )
)]
pub async fn create_account_handler(
    State(state): State<AppState>,
    auth: Option<AuthIdentity>,
    Json(req): Json<CreateAccountRequest>,
) -> Result<Created<CreatedAccountResponse>, ApiError> {
    let username = require_valid_username(&req.username)?;
    let by_owner = match &auth {
        Some(auth) if auth.is_owner() => true,
        Some(_) => {
            return Err(ApiError::NotTheOwner(
                "only the vault owner creates accounts for others; a stranger creates their own with no credential while the vault is open".into(),
            ));
        }
        None => {
            check_auth_rate_limit(&state.auth_rate_limits, &format!("register:{username}"))?;
            false
        }
    };

    let password = req.password.as_deref().unwrap_or("");
    let password_hash = if by_owner || !password.is_empty() {
        validate_password_policy(password)?;
        Some(hash_password(password)?)
    } else {
        None
    };
    let preferred_name = req.preferred_name.as_deref().and_then(message_ir::nonempty);
    let phone = req.phone.as_deref().and_then(message_ir::nonempty);

    let mut conn = state.db.acquire().await?;
    if !by_owner && !vault_settings::load(&mut conn).await?.public_registration {
        return Err(ApiError::NotTheOwner(
            "this vault does not accept new accounts; ask its owner for one".into(),
        ));
    }

    // The insert and the marks on the row land together: a failure between
    // them would leave an account whose holder keeps the password the owner
    // chose, which is the one thing the forced change exists to prevent.
    let mut tx = conn.begin().await?;
    require_username_free(&mut tx, &username).await?;
    let account_id = account_profile::insert_account(
        &mut tx,
        &username,
        password_hash.as_deref(),
        preferred_name.as_deref(),
    )
    .await
    .map_err(ApiError::Internal)?;
    if let Some(phone) = phone.as_deref() {
        account_profile::upsert_account_phone(&mut tx, account_id, phone)
            .await
            .map_err(ApiError::Internal)?;
    }
    // A creation that named nothing leaves an account with no display name
    // and no handles, and that account owes profile setup. Decided once, here,
    // and recorded, rather than re-derived from an empty-looking profile by
    // each client that reads it.
    if preferred_name.is_none() && phone.is_none() {
        account_profile::set_must_set_up_profile(&mut tx, account_id, true).await?;
    }
    if by_owner {
        account_profile::set_must_change_password(&mut tx, account_id, true).await?;
    }
    let token = if by_owner {
        None
    } else {
        Some(
            session_tokens::insert_account_session_token(&mut tx, account_id)
                .await
                .map_err(ApiError::Internal)?,
        )
    };
    tx.commit().await?;

    let account = load_account(&mut conn, account_id).await?.ok_or_else(|| {
        ApiError::Internal(anyhow::anyhow!("account vanished immediately after insert"))
    })?;
    Ok(Created {
        location: format!("/v1/accounts/{account_id}"),
        body: CreatedAccountResponse { account, token },
    })
}

// ---------------------------------------------------------------------------
// One account
// ---------------------------------------------------------------------------

/// Read one account: the owner reads any, an account reads its own.
#[utoipa::path(
    get,
    path = "/v1/accounts/{id}",
    tag = "Accounts",
    operation_id = "get_account",
    security(("bearer" = [])),
    params(("id" = i64, Path, description = "Account id")),
    responses(
        (status = 200, body = AccountResponse),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem)
    )
)]
pub async fn get_account_handler(
    State(state): State<AppState>,
    Path(target): Path<i64>,
    SignedIn(auth): SignedIn,
) -> Result<Json<AccountResponse>, ApiError> {
    let mut conn = state.db.acquire().await?;
    require_owner_or_self(&mut conn, &auth, target).await?;
    Ok(Json(require_account(&mut conn, target).await?))
}

/// One handle to link or unlink, with its platform service.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ProfileHandleInput {
    /// Raw handle value, e.g. `+15555550100` or `alex@example.com`.
    pub handle: String,
    /// Platform the handle belongs to: `phone`, `email`, or `whatsapp`.
    pub service: String,
}

/// Body for changing an account. Omitted fields are left alone. The name,
/// zone and handles are the account's own to set; the disabled flag and the
/// three permissions are the vault owner's.
#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
pub struct PatchAccountRequest {
    /// Display name to set; `None` (or empty) leaves the current name unchanged.
    #[serde(default)]
    pub preferred_name: Option<String>,
    /// IANA time zone to set, for example `America/New_York`; `None` leaves
    /// the current zone unchanged. An unknown name is a 422.
    #[serde(default)]
    pub time_zone: Option<String>,
    /// Handles to add/link onto the account profile.
    #[serde(default)]
    pub handles: Vec<ProfileHandleInput>,
    /// Handles to unlink from the account profile.
    #[serde(default)]
    pub remove_handles: Vec<ProfileHandleInput>,
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

impl PatchAccountRequest {
    /// True when the body names a field only the account itself may set.
    fn touches_profile(&self) -> bool {
        self.preferred_name.is_some()
            || self.time_zone.is_some()
            || !self.handles.is_empty()
            || !self.remove_handles.is_empty()
    }

    /// True when the body names a field only the vault owner may set.
    fn touches_flags(&self) -> bool {
        self.disabled.is_some()
            || self.can_import.is_some()
            || self.can_export.is_some()
            || self.can_delete.is_some()
    }
}

/// Why a profile update was refused.
#[derive(Debug, thiserror::Error)]
enum ProfileUpdateError {
    /// The client named a handle service the profile does not support.
    #[error("unsupported handle service: {0}")]
    UnsupportedService(String),
    /// The client named a time zone chrono-tz does not know.
    #[error("unknown time zone: {0}; use an IANA name such as America/New_York")]
    UnknownTimeZone(String),
    /// Database failure.
    #[error(transparent)]
    Db(#[from] anyhow::Error),
}

impl From<sqlx::Error> for ProfileUpdateError {
    fn from(value: sqlx::Error) -> Self {
        Self::Db(value.into())
    }
}

impl From<ProfileUpdateError> for ApiError {
    fn from(e: ProfileUpdateError) -> Self {
        match e {
            err @ (ProfileUpdateError::UnsupportedService(_)
            | ProfileUpdateError::UnknownTimeZone(_)) => Self::validation(err.to_string()),
            ProfileUpdateError::Db(err) => Self::Internal(err),
        }
    }
}

enum ProfileHandleKind {
    Phone,
    Email,
    Whatsapp,
}

/// Map a client `service` string to a handle kind.
fn parse_profile_service(
    service: &str,
) -> std::result::Result<ProfileHandleKind, ProfileUpdateError> {
    match service.trim().to_ascii_lowercase().as_str() {
        "phone" => Ok(ProfileHandleKind::Phone),
        "email" => Ok(ProfileHandleKind::Email),
        "whatsapp" => Ok(ProfileHandleKind::Whatsapp),
        other => Err(ProfileUpdateError::UnsupportedService(other.to_string())),
    }
}

/// Apply name, zone and handle changes on an open connection.
async fn apply_profile_update(
    conn: &mut AnyConnection,
    account_id: i64,
    preferred_name: Option<&str>,
    time_zone: Option<&str>,
    handles: &[ProfileHandleInput],
    remove_handles: &[ProfileHandleInput],
) -> std::result::Result<(), ProfileUpdateError> {
    if let Some(name) = time_zone.map(str::trim).filter(|n| !n.is_empty()) {
        let zone: chrono_tz::Tz = name
            .parse()
            .map_err(|_| ProfileUpdateError::UnknownTimeZone(name.to_string()))?;
        account_profile::set_time_zone(conn, account_id, zone).await?;
    }
    if let Some(name) = preferred_name {
        let name = name.trim();
        let stored_name = if name.is_empty() {
            None::<&str>
        } else {
            Some(name)
        };
        sqlx::query("UPDATE accounts SET preferred_name = $1 WHERE id = $2")
            .bind(stored_name)
            .bind(account_id)
            .execute(&mut *conn)
            .await?;
    }

    for entry in remove_handles {
        let raw = entry.handle.trim();
        if raw.is_empty() {
            continue;
        }
        match parse_profile_service(&entry.service)? {
            ProfileHandleKind::Phone | ProfileHandleKind::Whatsapp => {
                account_profile::unlink_account_handle(conn, account_id, raw, HandleType::Phone)
                    .await?;
            }
            ProfileHandleKind::Email => {
                account_profile::unlink_account_handle(conn, account_id, raw, HandleType::Email)
                    .await?;
            }
        }
    }

    for entry in handles {
        let raw = entry.handle.trim();
        if raw.is_empty() {
            continue;
        }
        match parse_profile_service(&entry.service)? {
            ProfileHandleKind::Phone => {
                account_profile::link_account_handle(conn, account_id, raw, HandleType::Phone)
                    .await?;
            }
            ProfileHandleKind::Email => {
                account_profile::link_account_handle(conn, account_id, raw, HandleType::Email)
                    .await?;
                account_profile::upsert_account_email(
                    conn,
                    account_id,
                    &raw.to_ascii_lowercase(),
                    false,
                )
                .await?;
            }
            ProfileHandleKind::Whatsapp => {
                account_profile::link_account_handle_with_service(
                    conn,
                    account_id,
                    raw,
                    HandleType::Phone,
                    Some("whatsapp"),
                )
                .await?;
            }
        }
    }

    Ok(())
}

/// Apply a profile update in one transaction.
async fn update_profile_on_conn(
    conn: &mut AnyConnection,
    account_id: i64,
    req: &PatchAccountRequest,
) -> std::result::Result<(), ProfileUpdateError> {
    let mut tx = conn.begin().await?;
    apply_profile_update(
        &mut tx,
        account_id,
        req.preferred_name.as_deref(),
        req.time_zone.as_deref(),
        &req.handles,
        &req.remove_handles,
    )
    .await?;
    // Saving a profile is what profile setup is, so the account no longer owes
    // one. Cleared in the same transaction as the change it describes, the way
    // changing a password clears `must_change_password`, so the flag cannot
    // outlive the fact it stands for.
    account_profile::set_must_set_up_profile(&mut tx, account_id, false).await?;
    tx.commit().await?;
    Ok(())
}

/// Set the owner's flags on an account.
///
/// Clearing `can_import` or `can_export` also narrows every API token that
/// account has already issued, because a token's permissions are intersected
/// with its account's on every request. The owner restrains the account and
/// the tokens follow, without ever seeing one.
async fn apply_flags(
    conn: &mut AnyConnection,
    account_id: i64,
    req: &PatchAccountRequest,
) -> Result<(), ApiError> {
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
            .bind(account_id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

/// Change an account. The account itself sets its display name, time zone
/// and handles; the vault owner sets another account's disabled flag and
/// its import, export and delete permissions. A field the caller may not
/// set answers `403 Forbidden`, and the reloaded account is the answer.
#[utoipa::path(
    patch,
    path = "/v1/accounts/{id}",
    tag = "Accounts",
    operation_id = "patch_account",
    security(("bearer" = [])),
    params(("id" = i64, Path, description = "Account id to change")),
    request_body = PatchAccountRequest,
    responses(
        (status = 200, body = AccountResponse),
        (status = 400, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem)
    )
)]
pub async fn patch_account_handler(
    State(state): State<AppState>,
    Path(target): Path<i64>,
    SignedIn(auth): SignedIn,
    Json(req): Json<PatchAccountRequest>,
) -> Result<Json<AccountResponse>, ApiError> {
    let mut conn = state.db.acquire().await?;
    match require_owner_or_self(&mut conn, &auth, target).await? {
        Reach::Own => {
            if req.touches_flags() {
                return Err(if auth.is_owner() {
                    // The owner holds no messages, so its permissions mean
                    // nothing, and it cannot lock itself out.
                    ApiError::validation("the vault owner cannot be disabled or given permissions")
                } else {
                    ApiError::InsufficientScope(
                        "only the vault owner sets disabled, can_import, can_export and can_delete"
                            .into(),
                    )
                });
            }
            update_profile_on_conn(&mut conn, target, &req).await?;
        }
        Reach::Owner => {
            if req.touches_profile() {
                return Err(ApiError::InsufficientScope(
                    "an account's name, time zone and handles are its own to set".into(),
                ));
            }
            apply_flags(&mut conn, target, &req).await?;
        }
    }
    Ok(Json(require_account(&mut conn, target).await?))
}

/// Confirmation flag and the current password when one is set: the body an
/// account sends to delete itself. The owner sends none.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct DeleteAccountRequest {
    /// Must be `true`; anything else is rejected.
    pub confirm: bool,
    /// Required when the account has a local password.
    #[serde(default)]
    pub current_password: Option<String>,
}

/// Permanently delete an account: login, profile, contacts, and every
/// message it owns, with its data directory.
///
/// The vault owner deletes any account outright, the demo account included,
/// which is how a demo vault is cleared into a real one. An account deletes
/// itself with a body carrying the confirmation and its current password: a
/// credential belongs in a body, not in a URL or a header of the vault's own
/// invention, and a DELETE body has no defined meaning in RFC 9110 but is not
/// forbidden. The demo account refuses its own deletion, and nobody deletes
/// the owner.
#[utoipa::path(
    delete,
    path = "/v1/accounts/{id}",
    tag = "Accounts",
    operation_id = "delete_account",
    security(("bearer" = [])),
    params(("id" = i64, Path, description = "Account id to delete")),
    request_body(content = Option<DeleteAccountRequest>, description = "Sent by an account deleting itself; the owner sends no body"),
    responses(
        (status = 204, description = "Account deleted"),
        (status = 400, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem)
    )
)]
pub async fn delete_account_handler(
    State(state): State<AppState>,
    Path(target): Path<i64>,
    SignedIn(auth): SignedIn,
    body: Option<Json<DeleteAccountRequest>>,
) -> Result<StatusCode, ApiError> {
    let mut conn = state.db.acquire().await?;
    let reach = require_owner_or_self(&mut conn, &auth, target).await?;
    if account_profile::is_vault_owner(target) {
        return Err(ApiError::validation("the vault owner cannot be deleted"));
    }
    if reach == Reach::Own {
        if account_profile::is_demo_account(target) {
            return Err(ApiError::DemoAccountProtected(
                "the demo account cannot be deleted; use reset-demo to restore it".into(),
            ));
        }
        let Some(Json(req)) = body else {
            return Err(ApiError::MissingParameter(
                "deleting your own account takes a body with confirm and current_password".into(),
            ));
        };
        if !req.confirm {
            return Err(ApiError::validation("confirmation flag must be true"));
        }
        let password_hash = account_profile::load_password_hash(&mut conn, target).await?;
        let has_local_password = matches!(password_hash.as_deref(), Some(hash) if !hash.is_empty());
        if has_local_password {
            let Some(pw) = req.current_password.as_deref() else {
                return Err(ApiError::validation(
                    "current password is required to delete this account",
                ));
            };
            if !passwords_match(password_hash.as_deref(), pw) {
                return Err(ApiError::InvalidCredentials(
                    "current password is incorrect".into(),
                ));
            }
        }
    }

    account_profile::delete_account(&mut conn, target).await?;
    let account_root = state.cfg.paths.data_dir.join(target.to_string());
    if account_root.exists() {
        let root = account_root.clone();
        tokio::task::spawn_blocking(move || std::fs::remove_dir_all(&root))
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("remove account data dir task: {e}")))?
            .with_context(|| format!("remove account data dir {}", account_root.display()))?;
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Password
// ---------------------------------------------------------------------------

/// The new password, and the current one when an account changes its own.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SetPasswordRequest {
    /// The new password. Must satisfy the vault's password policy.
    pub password: String,
    /// The account's current password. Required when an account changes its
    /// own; ignored when the vault owner sets another account's.
    #[serde(default)]
    pub current_password: Option<String>,
}

/// Fresh session token issued after an account changed its own password.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SetPasswordResponse {
    /// Replacement session token (the previous one is revoked).
    pub token: String,
}

/// Set an account's password.
///
/// An account changing its own must supply the current one; the change
/// revokes its API tokens and answers `200` with a rotated session token.
/// The vault owner sets another account's without the current one and
/// answers `204`: that account's sessions end, and its holder signs in with
/// the new password and is made to replace it.
#[utoipa::path(
    put,
    path = "/v1/accounts/{id}/password",
    tag = "Accounts",
    operation_id = "set_account_password",
    security(("bearer" = [])),
    params(("id" = i64, Path, description = "Account id whose password is set")),
    request_body = SetPasswordRequest,
    responses(
        (status = 200, description = "Own password changed; the rotated session token", body = SetPasswordResponse),
        (status = 204, description = "Password set by the vault owner; the account's sessions are ended"),
        (status = 400, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem)
    )
)]
pub async fn set_password_handler(
    State(state): State<AppState>,
    Path(target): Path<i64>,
    SignedIn(auth): SignedIn,
    Json(req): Json<SetPasswordRequest>,
) -> Result<Response, ApiError> {
    validate_password_policy(&req.password)?;
    let new_hash = hash_password(&req.password)?;

    let mut conn = state.db.acquire().await?;
    match require_owner_or_self(&mut conn, &auth, target).await? {
        Reach::Own => {
            let Some(current) = req.current_password.as_deref() else {
                return Err(ApiError::validation(
                    "current_password is required to change your own password",
                ));
            };
            if current.len() > MAX_PASSWORD_BYTES {
                return Err(ApiError::validation("password is too long"));
            }
            let token = change_password_on_conn(&mut conn, target, current, &new_hash).await?;
            Ok(Json(SetPasswordResponse { token }).into_response())
        }
        Reach::Owner => {
            account_profile::update_password_hash(&mut conn, target, &new_hash).await?;
            account_profile::set_must_change_password(&mut conn, target, true).await?;
            session_tokens::revoke_account_sessions(&mut conn, target).await?;
            Ok(StatusCode::NO_CONTENT.into_response())
        }
    }
}

// ---------------------------------------------------------------------------
// Messages and storage
// ---------------------------------------------------------------------------

/// Confirmation flag: the body an account sends to delete its own messages.
/// The owner sends none.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct DeleteMessagesRequest {
    /// Must be `true`; anything else is rejected.
    pub confirm: bool,
}

/// Counts of deleted conversations and attachment rows.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DeleteMessagesResponse {
    /// Conversations deleted.
    pub conversations: u64,
    /// Attachment rows deleted (on-disk files are removed too).
    pub attachments: u64,
}

/// Delete on-disk attachment trees for every source under this account.
fn remove_account_asset_trees(
    data_dir: &std::path::Path,
    account_id: i64,
    assets_name: &str,
    converted_name: &str,
) -> anyhow::Result<()> {
    let account_root = data_dir.join(account_id.to_string());
    if !account_root.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(&account_root)
        .with_context(|| format!("read {}", account_root.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let source_root = entry.path();
        for name in [assets_name, converted_name] {
            let dir = source_root.join(name);
            if dir.exists() {
                std::fs::remove_dir_all(&dir)
                    .with_context(|| format!("remove {}", dir.display()))?;
            }
        }
    }
    Ok(())
}

/// Destroy one account's conversations, messages, and attachments. The
/// account itself, its contacts, and its login survive.
///
/// The vault owner may, on any account. The account itself may with a
/// credential that carries the `delete` scope, session or API token, and
/// confirms in the body.
#[utoipa::path(
    delete,
    path = "/v1/accounts/{id}/messages",
    tag = "Accounts",
    operation_id = "delete_account_messages",
    security(("bearer" = [])),
    params(("id" = i64, Path, description = "Account whose messages are destroyed")),
    request_body(content = Option<DeleteMessagesRequest>, description = "Sent by an account deleting its own messages; the owner sends no body"),
    responses(
        (status = 200, body = DeleteMessagesResponse),
        (status = 400, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem)
    )
)]
pub async fn delete_messages_handler(
    State(state): State<AppState>,
    Path(target): Path<i64>,
    auth: AuthIdentity,
    body: Option<Json<DeleteMessagesRequest>>,
) -> Result<Json<DeleteMessagesResponse>, ApiError> {
    let mut conn = state.db.acquire().await?;
    if require_owner_or_self(&mut conn, &auth, target).await? == Reach::Own {
        crate::server::require_delete_access(&auth)?;
        if !body.is_some_and(|Json(req)| req.confirm) {
            return Err(ApiError::validation("confirmation flag must be true"));
        }
    }

    let stats = account_profile::delete_all_messages_for_account(&mut conn, target).await?;
    remove_account_asset_trees(
        &state.cfg.paths.data_dir,
        target,
        &state.cfg.paths.assets_dir,
        &state.cfg.paths.assets_converted_dir,
    )?;

    Ok(Json(DeleteMessagesResponse {
        conversations: stats.conversations,
        attachments: stats.attachments,
    }))
}

/// Attachment usage and the largest files.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct AccountStorageResponse {
    pub total_bytes: i64,
    pub attachment_count: i64,
    pub top_attachments: Vec<vault_imports::TopAttachment>,
}

/// Attachment storage usage for an account: total bytes, count, and the 100
/// largest files. The owner reads any account's; an account reads its own.
#[utoipa::path(
    get,
    path = "/v1/accounts/{id}/storage",
    tag = "Accounts",
    operation_id = "get_account_storage",
    security(("bearer" = [])),
    params(("id" = i64, Path, description = "Account id")),
    responses(
        (status = 200, body = AccountStorageResponse),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem)
    )
)]
pub(crate) async fn account_storage_handler(
    State(state): State<AppState>,
    Path(target): Path<i64>,
    SignedIn(auth): SignedIn,
) -> Result<Json<AccountStorageResponse>, ApiError> {
    let mut conn = state.db.acquire().await?;
    require_owner_or_self(&mut conn, &auth, target).await?;
    let total_bytes = vault_imports::account_attachment_bytes(&mut conn, target).await?;
    let attachment_count = vault_imports::account_attachment_count(&mut conn, target).await?;
    let top_attachments = vault_imports::top_attachments_by_size(&mut conn, target, 100).await?;
    Ok(Json(AccountStorageResponse {
        total_bytes,
        attachment_count,
        top_attachments,
    }))
}

#[cfg(test)]
mod tests;
