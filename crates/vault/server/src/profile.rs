//! Account profile read + update handlers.

use crate::extract::Json;
use anyhow::{Context, Result};
use axum::extract::State;
use message_ir::HandleType;
use serde::{Deserialize, Serialize};
use sqlx::AnyConnection;
use sqlx::Connection;

use crate::db::account_profile;
use crate::server::{ApiError, AppState, DeleteAccess, FullAccess, SignedIn};

/// The signed-in account's profile.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct AccountProfileResponse {
    /// The signed-in account id.
    pub account_id: String,
    /// Account username (falls back to the account id).
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
}

/// Load the profile JSON for `account_id`.
async fn load_response(
    conn: &mut AnyConnection,
    account_id: &str,
) -> Result<AccountProfileResponse> {
    let username = account_profile::username_for_account(conn, account_id)
        .await?
        .unwrap_or_else(|| account_id.to_string());
    let preferred_name = account_profile::load_preferred_name(conn, account_id).await?;
    let time_zone = account_profile::load_time_zone(conn, account_id)
        .await?
        .name()
        .to_string();
    let profile = account_profile::load_account_profile(conn, account_id).await?;
    let auth = account_profile::load_account_auth(conn, account_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("account no longer exists"))?;
    Ok(AccountProfileResponse {
        account_id: account_id.to_string(),
        username,
        preferred_name,
        time_zone,
        phones: profile.phones,
        emails: profile.emails,
        is_demo: account_profile::is_demo_account(account_id),
        is_owner: account_profile::is_vault_owner(account_id),
        must_change_password: auth.must_change_password,
        must_set_up_profile: auth.must_set_up_profile,
        can_import: auth.permissions.import,
        can_export: auth.permissions.export,
        can_delete: auth.permissions.delete,
    })
}

/// Load the signed-in account's profile: username, display name, linked
/// handles, and the demo flag.
#[utoipa::path(
    get,
    path = "/v1/account/profile",
    tag = "Account",
    security(("bearer" = [])),
    responses(
        (status = 200, body = AccountProfileResponse),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody)
    )
)]
pub async fn account_profile_handler(
    State(state): State<AppState>,
    SignedIn(auth): SignedIn,
) -> Result<Json<AccountProfileResponse>, ApiError> {
    let account_id = auth.account_id;

    let mut conn = state.db.acquire().await?;
    let result = load_response(&mut conn, &account_id).await?;

    Ok(Json(result))
}

/// One handle to link or unlink, with its platform service.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ProfileHandleInput {
    /// Raw handle value, e.g. `+15555550100` or `alex@example.com`.
    pub handle: String,
    /// Platform the handle belongs to: `phone`, `email`, or `whatsapp`.
    pub service: String,
}

/// Display name and handle changes.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct AccountProfileUpdateRequest {
    /// Display name to set; `None` (or empty) leaves the current name unchanged.
    #[serde(default)]
    pub preferred_name: Option<String>,
    /// IANA time zone to set, for example `America/New_York`; `None` leaves
    /// the current zone unchanged. An unknown name is a 400.
    #[serde(default)]
    pub time_zone: Option<String>,
    /// Handles to add/link onto the account profile.
    #[serde(default)]
    pub handles: Vec<ProfileHandleInput>,
    /// Handles to unlink from the account profile.
    #[serde(default)]
    pub remove_handles: Vec<ProfileHandleInput>,
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
            | ProfileUpdateError::UnknownTimeZone(_)) => Self::BadRequest(err.to_string()),
            ProfileUpdateError::Db(err) => Self::Internal(err),
        }
    }
}

/// Apply name and handle changes on an open connection.
async fn apply_profile_update(
    conn: &mut AnyConnection,
    account_id: &str,
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

/// Apply a profile update in one transaction, then reload the response.
async fn update_profile_on_conn(
    conn: &mut AnyConnection,
    account_id: &str,
    req: &AccountProfileUpdateRequest,
) -> std::result::Result<AccountProfileResponse, ProfileUpdateError> {
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
    Ok(load_response(conn, account_id).await?)
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

/// Update the account's display name and linked handles, then return the
/// reloaded profile.
#[utoipa::path(
    post,
    path = "/v1/account/profile",
    tag = "Account",
    security(("bearer" = [])),
    request_body = AccountProfileUpdateRequest,
    responses(
        (status = 200, body = AccountProfileResponse),
        (status = 400, body = crate::server::ErrorBody),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody)
    )
)]
pub async fn account_profile_update_handler(
    State(state): State<AppState>,
    SignedIn(auth): SignedIn,
    Json(req): Json<AccountProfileUpdateRequest>,
) -> Result<Json<AccountProfileResponse>, ApiError> {
    let account_id = auth.account_id;

    let mut conn = state.db.acquire().await?;
    let result = update_profile_on_conn(&mut conn, &account_id, &req).await?;

    Ok(Json(result))
}

/// Confirmation flag for deleting all messages.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct DeleteMessagesRequest {
    /// Must be `true`; anything else is rejected with a 400.
    pub confirm: bool,
}

/// Counts of deleted conversations and attachment rows.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DeleteMessagesResponse {
    /// Conversations deleted.
    pub conversations: u64,
    /// Attachment rows deleted (on-disk files are removed too).
    pub attachments: u64,
}

/// Delete on-disk attachment trees for every source under this account.
pub(crate) fn remove_account_asset_trees(
    data_dir: &std::path::Path,
    account_id: &str,
    assets_name: &str,
    converted_name: &str,
) -> Result<()> {
    let account_root = data_dir.join(account_id);
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

/// Delete every conversation, message, and attachment for the account.
/// Contacts and the account login survive.
#[utoipa::path(
    post,
    path = "/v1/account/delete-messages",
    tag = "Account",
    security(("bearer" = [])),
    request_body = DeleteMessagesRequest,
    responses(
        (status = 200, body = DeleteMessagesResponse),
        (status = 400, body = crate::server::ErrorBody),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody)
    )
)]
pub async fn delete_messages_handler(
    State(state): State<AppState>,
    DeleteAccess(auth): DeleteAccess,
    Json(req): Json<DeleteMessagesRequest>,
) -> Result<Json<DeleteMessagesResponse>, ApiError> {
    if !req.confirm {
        return Err(ApiError::BadRequest(
            "confirmation flag must be true".into(),
        ));
    }
    let account_id = auth.account_id;
    let data_dir = state.cfg.paths.data_dir.clone();
    let assets_name = state.cfg.paths.assets_dir.clone();
    let converted_name = state.cfg.paths.assets_converted_dir.clone();

    let mut conn = state.db.acquire().await?;
    let stats = account_profile::delete_all_messages_for_account(&mut conn, &account_id).await?;
    remove_account_asset_trees(&data_dir, &account_id, &assets_name, &converted_name)?;

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
    pub top_attachments: Vec<crate::db::vault_imports::TopAttachment>,
}

/// Attachment storage usage for the account: total bytes, count, and the 100
/// largest files.
#[utoipa::path(
    get,
    path = "/v1/account/storage",
    tag = "Account",
    security(("bearer" = [])),
    responses(
        (status = 200, body = AccountStorageResponse),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody)
    )
)]
pub(crate) async fn account_storage_handler(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
) -> Result<Json<AccountStorageResponse>, ApiError> {
    let account_id = auth.account_id;
    let mut conn = state.db.acquire().await?;
    let total_bytes =
        crate::db::vault_imports::account_attachment_bytes(&mut conn, &account_id).await?;
    let attachment_count =
        crate::db::vault_imports::account_attachment_count(&mut conn, &account_id).await?;
    let top_attachments =
        crate::db::vault_imports::top_attachments_by_size(&mut conn, &account_id, 100).await?;
    let result = AccountStorageResponse {
        total_bytes,
        attachment_count,
        top_attachments,
    };

    Ok(Json(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::db::api_tokens;
    use crate::db::permissions::Permissions;
    use crate::test_support::*;
    use axum::http::StatusCode;

    #[tokio::test]
    async fn apply_profile_update_sets_name_and_handles() {
        let vault = test_vault().await;
        let account_id = vault
            .account_with_id("00000000-0000-4000-8000-000000000001", "alice")
            .await;
        let mut conn = vault.conn().await;
        apply_profile_update(
            &mut conn,
            &account_id,
            Some("Alex"),
            None,
            &[
                ProfileHandleInput {
                    handle: "+1 (555) 555-0100".into(),
                    service: "phone".into(),
                },
                ProfileHandleInput {
                    handle: "Alex@Example.com".into(),
                    service: "email".into(),
                },
                ProfileHandleInput {
                    handle: "+15555550199".into(),
                    service: "whatsapp".into(),
                },
            ],
            &[],
        )
        .await
        .unwrap();

        let loaded = load_response(&mut conn, &account_id).await.unwrap();
        assert_eq!(loaded.preferred_name.as_deref(), Some("Alex"));
        assert!(loaded.phones.iter().any(|p| p == "+15555550100"));
        assert!(loaded.phones.iter().any(|p| p == "+15555550199"));
        assert!(loaded.emails.iter().any(|e| e == "alex@example.com"));

        let wa_service: String = sqlx::query_scalar(
            "SELECT service FROM handles WHERE account_id = $1 AND normalized = $2",
        )
        .bind(&account_id)
        .bind("+15555550199")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
        assert_eq!(wa_service, "whatsapp");
    }

    /// Saving a profile is what profile setup is, so the vault stops asking
    /// for one. The flag is the answer every client reads, so it has to move
    /// when the fact behind it does.
    #[tokio::test]
    async fn saving_a_profile_clears_the_setup_owed_flag() {
        let vault = test_vault().await;
        let account_id = vault
            .account_with_id("00000000-0000-4000-8000-000000000001", "alice")
            .await;
        let mut conn = vault.conn().await;
        account_profile::set_must_set_up_profile(&mut conn, &account_id, true)
            .await
            .unwrap();
        assert!(
            load_response(&mut conn, &account_id)
                .await
                .unwrap()
                .must_set_up_profile
        );

        let reloaded = update_profile_on_conn(
            &mut conn,
            &account_id,
            &AccountProfileUpdateRequest {
                preferred_name: Some("Alex".into()),
                time_zone: None,
                handles: Vec::new(),
                remove_handles: Vec::new(),
            },
        )
        .await
        .unwrap();

        assert!(!reloaded.must_set_up_profile);
        let auth = account_profile::load_account_auth(&mut conn, &account_id)
            .await
            .unwrap()
            .unwrap();
        assert!(
            !auth.must_set_up_profile,
            "the cleared flag is written, not just reported"
        );
    }

    /// An account that owes nothing is not asked again, whatever its profile
    /// happens to hold. The empty-looking profile used to be the whole rule.
    #[tokio::test]
    async fn an_empty_profile_is_not_by_itself_setup_owed() {
        let vault = test_vault().await;
        let account_id = vault
            .account_with_id("00000000-0000-4000-8000-000000000002", "bare")
            .await;
        let mut conn = vault.conn().await;

        let loaded = load_response(&mut conn, &account_id).await.unwrap();
        assert_eq!(loaded.preferred_name, None);
        assert!(loaded.phones.is_empty());
        assert!(loaded.emails.is_empty());
        assert!(
            !loaded.must_set_up_profile,
            "the flag says what is owed; an empty profile does not"
        );
    }

    #[tokio::test]
    async fn apply_profile_update_removes_handles() {
        let vault = test_vault().await;
        let account_id = vault
            .account_with_id("00000000-0000-4000-8000-000000000001", "alice")
            .await;
        let mut conn = vault.conn().await;
        apply_profile_update(
            &mut conn,
            &account_id,
            None,
            None,
            &[
                ProfileHandleInput {
                    handle: "+15555550100".into(),
                    service: "phone".into(),
                },
                ProfileHandleInput {
                    handle: "alex@example.com".into(),
                    service: "email".into(),
                },
            ],
            &[],
        )
        .await
        .unwrap();

        apply_profile_update(
            &mut conn,
            &account_id,
            None,
            None,
            &[],
            &[
                ProfileHandleInput {
                    handle: "+15555550100".into(),
                    service: "phone".into(),
                },
                ProfileHandleInput {
                    handle: "alex@example.com".into(),
                    service: "email".into(),
                },
            ],
        )
        .await
        .unwrap();

        let loaded = load_response(&mut conn, &account_id).await.unwrap();
        assert!(loaded.phones.is_empty());
        assert!(loaded.emails.is_empty());
    }

    #[tokio::test]
    async fn profile_update_rolls_back_when_a_handle_service_is_unsupported() {
        let vault = test_vault().await;
        let account_id = vault
            .account_with_id("00000000-0000-4000-8000-000000000001", "alice")
            .await;
        let mut conn = vault.conn().await;

        let result = update_profile_on_conn(
            &mut conn,
            &account_id,
            &AccountProfileUpdateRequest {
                preferred_name: Some("Changed Name".into()),
                time_zone: None,
                handles: vec![ProfileHandleInput {
                    handle: "alice@example.com".into(),
                    service: "unsupported".into(),
                }],
                remove_handles: vec![],
            },
        )
        .await;

        assert!(result.is_err());
        assert_eq!(
            account_profile::load_preferred_name(&mut conn, &account_id)
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn delete_messages_needs_the_delete_permission() {
        let vault = test_vault().await;
        let state = vault.state.clone();
        let created = register_via_api(&state, "alice", "hunter2hunter2").await;

        let mut conn = state.db.acquire().await.unwrap();
        sqlx::query("UPDATE accounts SET can_delete = 0 WHERE id = $1")
            .bind(&created.account_id)
            .execute(&mut *conn)
            .await
            .unwrap();

        let status = post_status(
            &state,
            "/v1/account/delete-messages",
            &created.token,
            serde_json::json!({ "confirm": true }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn a_token_with_delete_may_delete_but_may_not_close_the_account() {
        let vault = test_vault().await;
        let state = vault.state.clone();
        let created = register_via_api(&state, "alice", "hunter2hunter2").await;
        let mut conn = state.db.acquire().await.unwrap();
        let token = api_tokens::create_api_token(
            &mut conn,
            &created.account_id,
            "tool",
            Permissions::all(),
            None,
        )
        .await
        .unwrap()
        .token;

        let deleted = post_status(
            &state,
            "/v1/account/delete-messages",
            &token,
            serde_json::json!({ "confirm": true }),
        )
        .await;
        assert_eq!(deleted, StatusCode::OK);

        let closed = post_status(
            &state,
            "/v1/auth/delete-account",
            &token,
            serde_json::json!({ "confirm": true }),
        )
        .await;
        assert_eq!(
            closed,
            StatusCode::FORBIDDEN,
            "closing the account stays session-only"
        );
    }

    /// The zone is chosen at profile setup and read back on the profile; an
    /// unknown name is refused before anything is written.
    #[tokio::test]
    async fn the_profile_carries_a_time_zone_and_refuses_an_unknown_one() {
        let vault = crate::test_support::test_vault().await;
        let account =
            crate::test_support::register_via_api(&vault.state, "alice", "hunter2hunter2").await;
        let mut conn = vault.conn().await;
        let before = load_response(&mut conn, &account.account_id).await.unwrap();
        assert_eq!(before.time_zone, "UTC", "a new account starts in UTC");

        update_profile_on_conn(
            &mut conn,
            &account.account_id,
            &AccountProfileUpdateRequest {
                preferred_name: None,
                time_zone: Some("America/New_York".into()),
                handles: vec![],
                remove_handles: vec![],
            },
        )
        .await
        .unwrap();
        let after = load_response(&mut conn, &account.account_id).await.unwrap();
        assert_eq!(after.time_zone, "America/New_York");

        let err = update_profile_on_conn(
            &mut conn,
            &account.account_id,
            &AccountProfileUpdateRequest {
                preferred_name: None,
                time_zone: Some("Mars/Olympus_Mons".into()),
                handles: vec![],
                remove_handles: vec![],
            },
        )
        .await
        .unwrap_err();
        assert!(
            matches!(err, ProfileUpdateError::UnknownTimeZone(_)),
            "{err}"
        );
        assert!(matches!(ApiError::from(err), ApiError::BadRequest(_)));
        let unchanged = load_response(&mut conn, &account.account_id).await.unwrap();
        assert_eq!(unchanged.time_zone, "America/New_York");
    }
}
