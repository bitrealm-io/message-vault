//! The accounts collection: `/v1/accounts` and everything under a member
//! except API tokens, which `api_tokens_api` serves.
//!
//! One collection serves the vault owner and every account. Who may call a
//! route is decided here, per handler, never by the path: the owner reaches
//! every row, an account reaches its own, and a stranger may create one while
//! the vault is open. `docs/architecture/http-api.md` records the rule and the
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
    change_password_on_conn, check_auth_rate_limit, hash_owner_password, hash_user_password,
    passwords_match, require_username_free, require_valid_username,
};
use crate::db::dialect::{begin_immediate_sql, engine_of};
use crate::db::handles::{self, Identity};
use crate::db::storage::{self, Scope};
use crate::db::{account_profile, session_tokens, vault_imports, vault_settings};
use crate::extract::{Json, Path, Query};
use crate::paging::{DEFAULT_LIST_LIMIT, Page, PageQuery, page_of, page_params};
use crate::server::{ApiError, AppState, AuthIdentity, Created, LoggedIn, Owner};

// ---------------------------------------------------------------------------
// The account as every caller sees it
// ---------------------------------------------------------------------------

/// One account: who it is, what it may do, and how much it holds. The owner
/// and the account itself both read the whole struct; nothing in it is a
/// message.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Account {
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
    /// May not log in.
    pub disabled: bool,
    /// The account holder has not set up their profile yet, so profile setup
    /// is owed before the account can be used. The vault decides this, not the
    /// client: the same answer reaches every app, and it survives cleared site
    /// data and a second browser.
    pub must_set_up_profile: bool,
    /// When the account last logged in (RFC 3339, UTC), or `null` if it never
    /// has. Logging in, claiming the vault and registering all count; a
    /// password change does not.
    pub last_login_at: Option<String>,
    /// Which app last used the account's session, the desktop app or the
    /// website, or `null` when the account has no session or no request on it
    /// has named an app.
    pub app: Option<crate::db::session_tokens::AppKind>,
    /// The Build that app reported, such as `0.9.0+343fe0d8`. Present exactly
    /// when `app` is.
    pub app_version: Option<String>,
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

/// Load one account's row. `None` when the account does not exist.
async fn load_account(
    conn: &mut AnyConnection,
    account_id: i64,
) -> Result<Option<Account>, ApiError> {
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
    let message_count = storage::message_count(conn, Scope::Account(account_id)).await?;
    let storage_bytes = storage::attachment_bytes(conn, Scope::Account(account_id)).await?;
    let last_login_at = account_profile::load_last_login(conn, account_id).await?;
    let app = crate::db::session_tokens::connecting_app_for_account(conn, account_id).await?;
    Ok(Some(Account {
        account_id,
        username,
        preferred_name,
        time_zone,
        phones: profile.phones,
        emails: profile.emails,
        is_demo: account_profile::is_demo_account(account_id),
        is_owner: account_profile::is_vault_owner(account_id),
        disabled: auth.disabled,
        must_set_up_profile: auth.must_set_up_profile,
        last_login_at,
        app: app.as_ref().map(|app| app.kind),
        app_version: app.map(|app| app.build),
        can_import: auth.permissions.import,
        can_export: auth.permissions.export,
        can_delete: auth.permissions.delete,
        message_count,
        storage_bytes,
    }))
}

/// Load one account's row, or `404 Not Found`.
async fn require_account(conn: &mut AnyConnection, account_id: i64) -> Result<Account, ApiError> {
    load_account(conn, account_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("account {account_id} not found")))
}

// ---------------------------------------------------------------------------
// Who the caller is to the addressed row
// ---------------------------------------------------------------------------

/// What the caller is to the account a member route addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reach {
    /// The vault owner, acting on an account that is not its own.
    Owner,
    /// The vault owner, on its own row.
    OwnersOwn,
    /// An ordinary account, on its own row.
    Own,
}

impl Reach {
    /// True when the row is the caller's own, the owner's included.
    pub(crate) fn is_own(self) -> bool {
        matches!(self, Self::Own | Self::OwnersOwn)
    }
}

/// Who a member route admits besides the account itself.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Admits {
    /// The vault owner too, on any account.
    Owner,
    /// Nobody else. The sentence is the refusal everyone else gets.
    NobodyElse(&'static str),
}

/// The one answer to "is this row the caller's?" for the routes under
/// `/v1/accounts/{id}`: admit the account itself, and the vault owner when
/// `admits` says so, and say which.
///
/// A caller addressing a row it may not reach answers `403`, whether or not
/// the row exists, so the refusal says nothing about the vault's accounts.
/// The owner alone learns that an id is absent.
pub(crate) async fn require_account_reach(
    conn: &mut AnyConnection,
    auth: &AuthIdentity,
    target: i64,
    admits: Admits,
) -> Result<Reach, ApiError> {
    if auth.account_id == target {
        return Ok(if auth.is_owner() {
            Reach::OwnersOwn
        } else {
            Reach::Own
        });
    }
    match admits {
        Admits::NobodyElse(refusal) => Err(ApiError::InsufficientScope(refusal.into())),
        Admits::Owner if !auth.is_owner() => Err(ApiError::NotTheOwner(format!(
            "account {target} is not yours, and only the vault owner reaches other accounts"
        ))),
        Admits::Owner => {
            if account_profile::username_for_account(conn, target)
                .await?
                .is_none()
            {
                return Err(ApiError::NotFound(format!("account {target} not found")));
            }
            Ok(Reach::Owner)
        }
    }
}

// ---------------------------------------------------------------------------
// The collection
// ---------------------------------------------------------------------------

/// List the accounts this vault holds, with their flags, message count, and
/// storage use. The owner's own account comes first, then the rest by
/// username: the owner is an account of this vault too, and reaches its own
/// settings from the same list as everyone else's.
#[utoipa::path(
    get,
    path = "/v1/accounts",
    tag = "Accounts",
    security(("session" = ["owner"])),
    params(
        ("limit" = Option<usize>, Query, description = "Page size, default 40, max 500"),
        ("offset" = Option<usize>, Query, description = "Page offset")
    ),
    responses(
        (status = 200, body = crate::paging::Page<Account>),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem)
    )
)]
pub async fn list_accounts(
    State(state): State<AppState>,
    Owner(_auth): Owner,
    Query(query): Query<PageQuery>,
) -> Result<Json<Page<Account>>, ApiError> {
    let page = page_params(query.limit, query.offset, DEFAULT_LIST_LIMIT, None)?;
    let mut conn = state.db.acquire().await?;
    let total = account_profile::count_accounts(&mut conn).await?;
    let ids = account_profile::account_ids_page(&mut conn, page.limit, page.offset).await?;

    let mut items = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(account) = load_account(&mut conn, id).await? {
            items.push(account);
        }
    }
    Ok(Json(Page {
        items,
        total: total.max(0) as u64,
        limit: page.limit,
        offset: page.offset,
    }))
}

/// Body for creating an account, by the vault owner or by a stranger.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateAccountRequest {
    /// Login username.
    pub username: String,
    /// Local password, of any length. Absent or empty opens an account with
    /// no password.
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
pub struct CreateAccountResponse {
    /// The new account.
    #[serde(flatten)]
    pub account: Account,
    /// Session token to send as `Authorization: Bearer …`. Present only when
    /// a stranger registered, because they are logged in on creation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

/// Create an account.
///
/// The vault owner may always: the owner picks the first password and the
/// account holder replaces it at first login, so the owner's choice survives
/// one session and no longer. A stranger with no credential may while the
/// vault is open, and is logged in on creation. Registering is the vault's
/// only self-service door, shut unless the owner has opened it; an unclaimed
/// vault is shut too, because its first act is being claimed, not being
/// joined.
#[utoipa::path(
    post,
    path = "/v1/accounts",
    tag = "Accounts",
    security((), ("session" = ["owner"])),
    request_body = CreateAccountRequest,
    responses(
        (
            status = 201,
            body = CreateAccountResponse,
            headers(("Location" = String, description = "Path of the new account"))
        ),
        (status = 400, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem),
        (status = 403, description = "The vault is closed, or the credential is not the owner's", body = crate::problem::Problem),
        (status = 409, description = "Username taken", body = crate::problem::Problem),
        (status = 429, description = "Rate limited", body = crate::problem::Problem)
    )
)]
pub async fn create_account(
    State(state): State<AppState>,
    auth: Option<AuthIdentity>,
    Json(req): Json<CreateAccountRequest>,
) -> Result<Created<CreateAccountResponse>, ApiError> {
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

    let password_hash = hash_user_password(req.password.as_deref().unwrap_or(""))?;
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
    // chose, which is the one thing the forced change exists to prevent. On
    // SQLite the transaction takes the write lock before the username check
    // (`begin_immediate_sql`, as imports and exports begin), so two
    // registrations of one name cannot both pass the check and then race to
    // the insert.
    let engine = engine_of(&conn);
    let mut tx = conn.begin_with(begin_immediate_sql(engine)).await?;
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
    let token = if by_owner {
        None
    } else {
        // Registering opens a Session, so it is the account's first login.
        let token = session_tokens::insert_account_session_token(&mut tx, account_id)
            .await
            .map_err(ApiError::Internal)?;
        account_profile::record_login(&mut tx, account_id).await?;
        Some(token)
    };
    tx.commit().await?;

    let account = load_account(&mut conn, account_id).await?.ok_or_else(|| {
        ApiError::Internal(anyhow::anyhow!("account vanished immediately after insert"))
    })?;
    Ok(Created {
        location: format!("/v1/accounts/{account_id}"),
        body: CreateAccountResponse { account, token },
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
    security(("session" = [])),
    params(("id" = i64, Path, description = "Account id")),
    responses(
        (status = 200, body = Account),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem)
    )
)]
pub async fn get_account(
    State(state): State<AppState>,
    Path(target): Path<i64>,
    LoggedIn(auth): LoggedIn,
) -> Result<Json<Account>, ApiError> {
    let mut conn = state.db.acquire().await?;
    require_account_reach(&mut conn, &auth, target, Admits::Owner).await?;
    Ok(Json(require_account(&mut conn, target).await?))
}

/// One handle to link or unlink, with its platform service.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct AccountIdentityRequest {
    /// Raw handle value, e.g. `+15555550100` or `alex@example.com`.
    pub handle: String,
    /// Platform the handle belongs to: `phone`, `email`, or `whatsapp`.
    pub service: String,
}

/// Body for changing an account. Omitted fields are left alone. The name,
/// zone and handles are set by the account or by the vault owner; the
/// disabled flag and the three permissions are the vault owner's alone.
#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
pub struct UpdateAccountRequest {
    /// Display name to set; `None` (or empty) leaves the current name unchanged.
    #[serde(default)]
    pub preferred_name: Option<String>,
    /// IANA time zone to set, for example `America/New_York`; `None` leaves
    /// the current zone unchanged. An unknown name is a 422.
    #[serde(default)]
    pub time_zone: Option<String>,
    /// Handles to add/link onto the account profile.
    #[serde(default)]
    pub handles: Vec<AccountIdentityRequest>,
    /// Handles to unlink from the account profile.
    #[serde(default)]
    pub remove_handles: Vec<AccountIdentityRequest>,
    /// Disable or re-enable login.
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

impl UpdateAccountRequest {
    /// True when the body names the display name, the time zone or a handle.
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
    handles: &[AccountIdentityRequest],
    remove_handles: &[AccountIdentityRequest],
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
        account_profile::set_preferred_name(conn, account_id, stored_name).await?;
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
    req: &UpdateAccountRequest,
    completes_setup: bool,
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
    // An account saving its own profile is what profile setup is, so it no
    // longer owes one. Cleared in the same transaction as the change it
    // describes, so the flag cannot outlive the fact it stands for. The vault
    // owner filling a profile in ahead of time is not the holder's setup.
    if completes_setup {
        account_profile::set_must_set_up_profile(&mut tx, account_id, false).await?;
    }
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
    req: &UpdateAccountRequest,
) -> Result<(), ApiError> {
    let flags = account_profile::AccountFlags {
        disabled: req.disabled,
        can_import: req.can_import,
        can_export: req.can_export,
        can_delete: req.can_delete,
    };
    account_profile::set_account_flags(conn, account_id, flags).await?;
    Ok(())
}

/// Change an account. Its display name, time zone and handles are set by
/// the account itself or by the vault owner; only the vault owner sets an
/// account's disabled flag and its import, export and delete permissions. A
/// field the caller may not set answers `403 Forbidden`, and the reloaded
/// account is the answer.
#[utoipa::path(
    patch,
    path = "/v1/accounts/{id}",
    tag = "Accounts",
    security(("session" = [])),
    params(("id" = i64, Path, description = "Account id to change")),
    request_body = UpdateAccountRequest,
    responses(
        (status = 200, body = Account),
        (status = 400, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem)
    )
)]
pub async fn update_account(
    State(state): State<AppState>,
    Path(target): Path<i64>,
    LoggedIn(auth): LoggedIn,
    Json(req): Json<UpdateAccountRequest>,
) -> Result<Json<Account>, ApiError> {
    let mut conn = state.db.acquire().await?;
    match require_account_reach(&mut conn, &auth, target, Admits::Owner).await? {
        reach @ (Reach::Own | Reach::OwnersOwn) => {
            if req.touches_flags() {
                return Err(if reach == Reach::OwnersOwn {
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
            update_profile_on_conn(&mut conn, target, &req, true).await?;
        }
        Reach::Owner => {
            // The owner sets up an account for its holder: the name, zone and
            // handles as well as the flags.
            if req.touches_profile() {
                update_profile_on_conn(&mut conn, target, &req, false).await?;
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
    security(("session" = [])),
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
pub async fn delete_account(
    State(state): State<AppState>,
    Path(target): Path<i64>,
    LoggedIn(auth): LoggedIn,
    body: Option<Json<DeleteAccountRequest>>,
) -> Result<StatusCode, ApiError> {
    let mut conn = state.db.acquire().await?;
    let reach = require_account_reach(&mut conn, &auth, target, Admits::Owner).await?;
    if account_profile::is_vault_owner(target) {
        return Err(ApiError::validation("the vault owner cannot be deleted"));
    }
    if reach.is_own() {
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
                    "Current password is required to delete this account.",
                ));
            };
            if !passwords_match(password_hash.as_deref(), pw) {
                return Err(ApiError::InvalidCredentials(
                    "Current password is incorrect.".into(),
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

/// The new password.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ReplaceAccountPasswordRequest {
    /// The new password. Empty clears a user account's password; the vault
    /// owner's must be one character or more.
    pub password: String,
    /// The new password typed a second time. The vault, not the screen,
    /// refuses a pair that differs, so the checks run in one fixed order:
    /// current password, then the pair, then that the new one differs from the
    /// current one.
    pub password_confirmation: String,
    /// The password being replaced. Required when the vault owner changes its
    /// own; nobody else sends it.
    #[serde(default)]
    pub current_password: Option<String>,
}

/// Fresh session token issued after an account changed its own password.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ReplaceAccountPasswordResponse {
    /// Replacement session token (the previous one is revoked).
    pub token: String,
}

/// Set an account's password.
///
/// For a user account the session is the credential, and the current
/// password is not asked for. The vault owner changing its own must send
/// `current_password`: that account reaches every other, so a session left
/// open on a shared machine must not be enough to take it over.
/// An account changing its own has its API tokens revoked and gets
/// `200` with a rotated session token. The vault owner setting another
/// account's answers `204`. That is the whole of it: the account's sessions carry on,
/// and its holder keeps the new password until they change it themselves.
#[utoipa::path(
    put,
    path = "/v1/accounts/{id}/password",
    tag = "Accounts",
    security(("session" = [])),
    params(("id" = i64, Path, description = "Account id whose password is set")),
    request_body = ReplaceAccountPasswordRequest,
    responses(
        (status = 200, description = "Own password changed; the rotated session token", body = ReplaceAccountPasswordResponse),
        (status = 204, description = "Password set by the vault owner"),
        (status = 400, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem)
    )
)]
pub async fn replace_account_password(
    State(state): State<AppState>,
    Path(target): Path<i64>,
    LoggedIn(auth): LoggedIn,
    Json(req): Json<ReplaceAccountPasswordRequest>,
) -> Result<Response, ApiError> {
    let mut conn = state.db.acquire().await?;
    let reach = require_account_reach(&mut conn, &auth, target, Admits::Owner).await?;

    // The checks run in a fixed order so the first thing a user is told is the
    // first thing they typed wrong: the current password, then the pair, then
    // that the new one is actually new.
    if reach == Reach::OwnersOwn {
        let Some(current) = req.current_password.as_deref() else {
            return Err(ApiError::validation(
                "Current password is required to change the vault owner's password.",
            ));
        };
        let password_hash = account_profile::load_password_hash(&mut conn, target).await?;
        if !passwords_match(password_hash.as_deref(), current) {
            return Err(ApiError::InvalidCredentials(
                "Current password is incorrect.".into(),
            ));
        }
        if req.password != req.password_confirmation {
            return Err(ApiError::validation("New passwords do not match."));
        }
        if req.password == current {
            return Err(ApiError::validation(
                "New password must be different from the current password.",
            ));
        }
    } else if req.password != req.password_confirmation {
        return Err(ApiError::validation("New passwords do not match."));
    }

    // The owner must have a password; a user account may have none.
    let new_hash = if account_profile::is_vault_owner(target) {
        Some(hash_owner_password(&req.password)?)
    } else {
        hash_user_password(&req.password)?
    };
    let new_hash = new_hash.as_deref();

    match reach {
        Reach::Own | Reach::OwnersOwn => {
            let token = change_password_on_conn(&mut conn, target, new_hash).await?;
            Ok(Json(ReplaceAccountPasswordResponse { token }).into_response())
        }
        Reach::Owner => {
            account_profile::update_password_hash(&mut conn, target, new_hash).await?;
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
/// session that carries the `delete` permission, and confirms in the body.
/// An API token is refused whatever its scopes: permanent deletion is a
/// person's act (`docs/architecture/http-api.md`, "Credentials and reach").
#[utoipa::path(
    delete,
    path = "/v1/accounts/{id}/messages",
    tag = "Accounts",
    security(
        ("session" = ["owner"]),
        ("session" = ["delete"])
    ),
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
pub async fn delete_account_messages(
    State(state): State<AppState>,
    Path(target): Path<i64>,
    LoggedIn(auth): LoggedIn,
    body: Option<Json<DeleteMessagesRequest>>,
) -> Result<Json<DeleteMessagesResponse>, ApiError> {
    let mut conn = state.db.acquire().await?;
    if require_account_reach(&mut conn, &auth, target, Admits::Owner)
        .await?
        .is_own()
    {
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

/// What an account holds: counts, attachment bytes and the largest files.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct AccountStorage {
    /// Attachment bytes, by original file size.
    pub total_bytes: i64,
    /// Attachment rows.
    pub attachment_count: i64,
    /// Conversations. A count and never a title: how many an account has is
    /// a measure of the vault, and who they are with is the holder's.
    pub conversation_count: i64,
    /// Contacts, on the same terms as `conversation_count`.
    pub contact_count: i64,
    pub top_attachments: Vec<vault_imports::TopAttachment>,
}

/// What an account holds: attachment bytes, the attachment, conversation and
/// contact counts, and the 100 largest files. The owner reads any account's;
/// an account reads its own. The owner is told each file's name, type and
/// size and not the conversation it is in, which says who the account talks
/// to (`docs/adr/0008-the-vault-owner-holds-no-messages.md`).
#[utoipa::path(
    get,
    path = "/v1/accounts/{id}/storage",
    tag = "Accounts",
    security(("session" = [])),
    params(("id" = i64, Path, description = "Account id")),
    responses(
        (status = 200, body = AccountStorage),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem)
    )
)]
pub(crate) async fn get_account_storage(
    State(state): State<AppState>,
    Path(target): Path<i64>,
    LoggedIn(auth): LoggedIn,
) -> Result<Json<AccountStorage>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let reach = require_account_reach(&mut conn, &auth, target, Admits::Owner).await?;
    let scope = Scope::Account(target);
    let total_bytes = storage::attachment_bytes(&mut conn, scope).await?;
    let attachment_count = storage::attachment_count(&mut conn, scope).await?;
    let conversation_count = storage::conversation_count(&mut conn, scope).await?;
    let contact_count = storage::contact_count(&mut conn, scope).await?;
    let mut top_attachments =
        vault_imports::top_attachments_by_size(&mut conn, target, 100).await?;
    if matches!(reach, Reach::Owner) {
        top_attachments = top_attachments
            .into_iter()
            .map(vault_imports::TopAttachment::without_conversation)
            .collect();
    }
    Ok(Json(AccountStorage {
        total_bytes,
        attachment_count,
        conversation_count,
        contact_count,
        top_attachments,
    }))
}

// ---------------------------------------------------------------------------
// Identities and the messages held at them
// ---------------------------------------------------------------------------

/// An account's identities, each with the messages held at it. The
/// owner reads any account's; an account reads its own.
#[utoipa::path(
    get,
    path = "/v1/accounts/{id}/identities",
    tag = "Accounts",
    security(("session" = [])),
    params(
        ("id" = i64, Path, description = "Account id"),
        ("limit" = Option<usize>, Query, description = "Page size, default 40, at most 500"),
        ("offset" = Option<usize>, Query, description = "Rows to skip")
    ),
    responses(
        (status = 200, body = Page<Identity>),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem)
    )
)]
pub(crate) async fn list_account_identities(
    State(state): State<AppState>,
    Path(target): Path<i64>,
    LoggedIn(auth): LoggedIn,
    Query(query): Query<PageQuery>,
) -> Result<Json<Page<Identity>>, ApiError> {
    let params = page_params(query.limit, query.offset, DEFAULT_LIST_LIMIT, None)?;
    let mut conn = state.db.acquire().await?;
    require_account_reach(&mut conn, &auth, target, Admits::Owner).await?;
    let rows = handles::identities(&mut conn, handles::IdentitiesOf::Account(target)).await?;
    Ok(Json(page_of(rows, params)))
}

// ---------------------------------------------------------------------------
// Import and export history
// ---------------------------------------------------------------------------
//
// An account's history is metadata about it, so the owner reads it as well as
// the account (`docs/adr/0008-the-vault-owner-holds-no-messages.md`, "What the
// owner may see"). `/v1/imports` and `/v1/exports` are the import and export
// pipelines' own routes and ask for a permission the owner's session never
// carries; these ask only who is calling. Which contacts a run created is the
// holder's address book, so `/v1/imports/{id}/contacts` has no twin here: the
// run's detail carries the counts.

/// An account's Import Runs as a page, newest first unless `sort` says
/// otherwise. The owner reads any account's; an account reads its own.
#[utoipa::path(
    get,
    path = "/v1/accounts/{id}/imports",
    tag = "Accounts",
    security(("session" = [])),
    params(
        ("id" = i64, Path, description = "Account id"),
        ("status" = Option<String>, Query, description = "One of running, completed, completed_with_issues, failed, cancelled"),
        ("limit" = Option<usize>, Query, description = "Page size, default 40, at most 500"),
        ("offset" = Option<usize>, Query, description = "Rows to skip, at most 50000"),
        ("sort" = Option<String>, Query, description = "`started_at` or `-started_at`. Default `-started_at`, newest first.")
    ),
    responses(
        (status = 200, body = Page<vault_imports::ImportSummary>),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem)
    )
)]
pub(crate) async fn list_account_imports(
    State(state): State<AppState>,
    Path(target): Path<i64>,
    LoggedIn(auth): LoggedIn,
    Query(query): Query<crate::imports_api::ListImportsQuery>,
) -> Result<Json<Page<vault_imports::ImportSummary>>, ApiError> {
    require_reach(&state, &auth, target).await?;
    crate::imports_api::imports_page(&state, target, query).await
}

/// One of an account's Import Runs: status, timings, counts and issues. A run
/// that is another account's is a 404.
#[utoipa::path(
    get,
    path = "/v1/accounts/{id}/imports/{import_id}",
    tag = "Accounts",
    security(("session" = [])),
    params(
        ("id" = i64, Path, description = "Account id"),
        ("import_id" = i64, Path, description = "Import Run id")
    ),
    responses(
        (status = 200, body = crate::imports_api::ImportRun),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem)
    )
)]
pub(crate) async fn get_account_import(
    State(state): State<AppState>,
    Path((target, import_id)): Path<(i64, i64)>,
    LoggedIn(auth): LoggedIn,
) -> Result<Json<crate::imports_api::ImportRun>, ApiError> {
    require_reach(&state, &auth, target).await?;
    crate::imports_api::import_detail(&state, target, import_id).await
}

/// An account's Export Runs as a page, newest first unless `sort` says
/// otherwise. The owner reads any account's; an account reads its own.
#[utoipa::path(
    get,
    path = "/v1/accounts/{id}/exports",
    tag = "Accounts",
    security(("session" = [])),
    params(
        ("id" = i64, Path, description = "Account id"),
        ("status" = Option<String>, Query, description = "One of running, completed, failed, cancelled"),
        ("limit" = Option<usize>, Query, description = "Page size, default 40, at most 500"),
        ("offset" = Option<usize>, Query, description = "Rows to skip, at most 50000"),
        ("sort" = Option<String>, Query, description = "`started_at` or `-started_at`. Default `-started_at`, newest first.")
    ),
    responses(
        (status = 200, body = Page<vault_api_types::ExportRun>),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem)
    )
)]
pub(crate) async fn list_account_exports(
    State(state): State<AppState>,
    Path(target): Path<i64>,
    LoggedIn(auth): LoggedIn,
    Query(query): Query<crate::exports_api::ListExportsQuery>,
) -> Result<Json<Page<vault_api_types::ExportRun>>, ApiError> {
    require_reach(&state, &auth, target).await?;
    crate::exports_api::exports_page(&state, target, query).await
}

/// [`require_account_reach`], admitting the owner, on a connection of its
/// own, for a handler whose work then runs on another.
async fn require_reach(
    state: &AppState,
    auth: &AuthIdentity,
    target: i64,
) -> Result<Reach, ApiError> {
    let mut conn = state.db.acquire().await?;
    require_account_reach(&mut conn, auth, target, Admits::Owner).await
}

#[cfg(test)]
mod tests;
