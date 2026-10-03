//! An account's named API tokens: `/v1/accounts/{id}/api-tokens`.
//!
//! The account itself, and nobody else. The owner has no tokens and
//! does not manage other people's: a token is a program's credential into
//! one account's messages, and the owner never reaches those. A logged-in
//! session is required; a token cannot mint, rename or revoke tokens.

use crate::extract::{Json, Path, Query};
use crate::paging::{DEFAULT_LIST_LIMIT, Page, PageQuery, page_of, page_params};
use axum::extract::State;
use serde::{Deserialize, Serialize};

use crate::accounts_api::{Admits, require_account_reach};
use crate::db::api_tokens;
use crate::db::permissions::Permissions;
use crate::db::schema;
use crate::server::{ApiError, AppState, Created, FullAccess};

/// Admit only the account whose tokens the path names. The refusal reads the
/// same whether or not the other account exists.
const HOLDER_ONLY: Admits =
    Admits::NobodyElse("API tokens are managed by the account that holds them");

/// One named API token as shown in Settings: label, permissions, and masked secret.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ApiToken {
    /// Token id (the secret itself is stored hashed).
    pub id: i64,
    /// User-chosen label shown in Settings.
    pub label: String,
    /// May call the import endpoints.
    pub can_import: bool,
    /// May call the export endpoints.
    pub can_export: bool,
    /// Masked secret for Settings (e.g. `mc-api-Sd..mE`).
    pub token_hint: String,
    /// Creation time as a Unix-seconds string.
    pub created_at: String,
    /// Unix-seconds string of last use; absent when never used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_accessed_at: Option<String>,
    /// Unix-seconds expiry; absent means no expiry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    /// True when the token is disabled and rejects requests.
    pub disabled: bool,
}

impl From<api_tokens::ApiTokenRow> for ApiToken {
    fn from(row: api_tokens::ApiTokenRow) -> Self {
        Self {
            id: row.id,
            label: row.label,
            can_import: row.permissions.import,
            can_export: row.permissions.export,
            token_hint: row.token_hint,
            created_at: row.created_at,
            last_accessed_at: row.last_accessed_at,
            expires_at: row.expires_at,
            disabled: row.disabled,
        }
    }
}

/// Label validation rejections are the caller's fault; anything else is a server error.
fn map_label_error(e: crate::db::api_tokens::ApiTokenMutationError) -> ApiError {
    use crate::db::api_tokens::ApiTokenMutationError;
    match e {
        ApiTokenMutationError::InvalidLabel(err) => ApiError::validation(err.to_string()),
        ApiTokenMutationError::Other(err) => ApiError::Internal(err),
    }
}

/// Body for creating a token: label, permissions, optional expiry. A token
/// carries `import` and `export` only, so a body naming `can_delete` is
/// refused rather than ignored.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateApiTokenRequest {
    /// User-chosen label shown in Settings.
    pub label: String,
    /// May call the import endpoints. Default true.
    #[serde(default = "default_true")]
    pub can_import: bool,
    /// May call the export endpoints. Default true.
    #[serde(default = "default_true")]
    pub can_export: bool,
    /// Days until expiry. Omit for the default (365 days). Pass `0` for no expiry.
    #[serde(default)]
    pub expires_in_days: Option<u64>,
}

const fn default_true() -> bool {
    true
}

/// The created token, including its plaintext secret (returned once).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct CreateApiTokenResponse {
    /// Token id.
    pub id: i64,
    /// User-chosen label.
    pub label: String,
    /// May call the import endpoints.
    pub can_import: bool,
    /// May call the export endpoints.
    pub can_export: bool,
    /// Creation time as a Unix-seconds string.
    pub created_at: String,
    /// Unix-seconds expiry; absent means no expiry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    /// Plaintext secret — returned once at creation.
    pub token: String,
    /// Masked form for the Settings list (also persisted).
    pub token_hint: String,
}

/// Body for renaming a token.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateApiTokenRequest {
    /// Replacement label.
    pub label: String,
}

/// The renamed token's id and stored label.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct UpdateApiTokenResponse {
    /// Token id that was renamed.
    pub id: i64,
    /// Stored label after the rename.
    pub label: String,
}

/// List the account's named API tokens with their permissions and masked secrets.
/// Each token's permissions are capped by the account's as they are now.
#[utoipa::path(
    get,
    path = "/v1/accounts/{id}/api-tokens",
    tag = "Accounts",
    security(("session" = [])),
    params(
        ("id" = i64, Path, description = "Account id; must be the caller's own"),
        ("limit" = Option<usize>, Query, description = "Page size, default 40, max 500"),
        ("offset" = Option<usize>, Query, description = "Page offset")
    ),
    responses(
        (status = 200, body = crate::paging::Page<ApiToken>),
    )
)]
pub async fn list_api_tokens(
    State(state): State<AppState>,
    Path(account_id): Path<i64>,
    FullAccess(auth): FullAccess,
    Query(query): Query<PageQuery>,
) -> Result<Json<Page<ApiToken>>, ApiError> {
    let mut conn = state.db.acquire().await?;
    require_account_reach(&mut conn, &auth, account_id, HOLDER_ONLY).await?;
    let params = page_params(query.limit, query.offset, DEFAULT_LIST_LIMIT, None)?;

    schema::ensure_accounts_schema(&mut conn).await?;
    let rows = api_tokens::list_api_tokens(&mut conn, account_id).await?;
    let account_permissions = auth.permissions();
    let items: Vec<ApiToken> = rows
        .into_iter()
        .map(|row| shown(row, account_permissions))
        .collect();

    Ok(Json(page_of(items, params)))
}

/// A token as it is shown: what it may do now, its stored scopes capped by
/// the account's permissions as they are on this request, so a permission
/// the owner turned off after the token was made shows as off.
fn shown(row: api_tokens::ApiTokenRow, account_permissions: Permissions) -> ApiToken {
    ApiToken::from(api_tokens::ApiTokenRow {
        permissions: row.permissions.intersect(account_permissions),
        ..row
    })
}

/// Read one named API token as the list shows it: label, permissions, masked
/// secret and last use. The secret itself is never answered again.
#[utoipa::path(
    get,
    path = "/v1/accounts/{id}/api-tokens/{token_id}",
    tag = "Accounts",
    security(("session" = [])),
    params(
        ("id" = i64, Path, description = "Account id; must be the caller's own"),
        ("token_id" = i64, Path, description = "API token id")
    ),
    responses(
        (status = 200, body = ApiToken),
    )
)]
pub async fn get_api_token(
    State(state): State<AppState>,
    Path((account_id, id)): Path<(i64, i64)>,
    FullAccess(auth): FullAccess,
) -> Result<Json<ApiToken>, ApiError> {
    let mut conn = state.db.acquire().await?;
    require_account_reach(&mut conn, &auth, account_id, HOLDER_ONLY).await?;
    schema::ensure_accounts_schema(&mut conn).await?;
    let row = api_tokens::get_api_token(&mut conn, account_id, id)
        .await?
        .ok_or_else(|| ApiError::NotFound("API token not found".into()))?;
    Ok(Json(shown(row, auth.permissions())))
}

/// Create a named API token. Returns the plaintext secret once, at creation;
/// it is never returned again. The token's permissions are those the request
/// asks for and the account holds.
#[utoipa::path(
    post,
    path = "/v1/accounts/{id}/api-tokens",
    tag = "Accounts",
    security(("session" = [])),
    params(("id" = i64, Path, description = "Account id; must be the caller's own")),
    request_body = CreateApiTokenRequest,
    responses(
        (
            status = 201,
            body = CreateApiTokenResponse,
            headers(("Location" = String, description = "Path of the new token"))
        ),
    )
)]
pub async fn create_api_token(
    State(state): State<AppState>,
    Path(account_id): Path<i64>,
    FullAccess(auth): FullAccess,
    Json(req): Json<CreateApiTokenRequest>,
) -> Result<Created<CreateApiTokenResponse>, ApiError> {
    let mut conn = state.db.acquire().await?;
    require_account_reach(&mut conn, &auth, account_id, HOLDER_ONLY).await?;
    let label = req.label;
    // A token can narrow its account's permissions, never widen them, so
    // what it stores is what the request asked and the account holds.
    let permissions =
        Permissions::token(req.can_import, req.can_export).intersect(auth.permissions());
    let expires_in_days = req.expires_in_days;

    schema::ensure_accounts_schema(&mut conn).await?;
    let created =
        api_tokens::create_api_token(&mut conn, account_id, &label, permissions, expires_in_days)
            .await
            .map_err(map_label_error)?;

    Ok(Created {
        location: format!("/v1/accounts/{account_id}/api-tokens/{}", created.id),
        body: CreateApiTokenResponse {
            id: created.id,
            label: created.label,
            can_import: created.permissions.import,
            can_export: created.permissions.export,
            created_at: created.created_at,
            expires_at: created.expires_at,
            token_hint: api_tokens::mask_api_token(&created.token),
            token: created.token,
        },
    })
}

/// Delete one named API token. Requests using it start failing on the next call.
#[utoipa::path(
    delete,
    path = "/v1/accounts/{id}/api-tokens/{token_id}",
    tag = "Accounts",
    security(("session" = [])),
    params(
        ("id" = i64, Path, description = "Account id; must be the caller's own"),
        ("token_id" = i64, Path, description = "API token id")
    ),
    responses(
        (status = 204, description = "Token deleted"),
    )
)]
pub async fn delete_api_token(
    State(state): State<AppState>,
    Path((account_id, id)): Path<(i64, i64)>,
    FullAccess(auth): FullAccess,
) -> Result<axum::http::StatusCode, ApiError> {
    let mut conn = state.db.acquire().await?;
    require_account_reach(&mut conn, &auth, account_id, HOLDER_ONLY).await?;
    schema::ensure_accounts_schema(&mut conn).await?;
    let deleted = api_tokens::delete_api_token(&mut conn, account_id, id).await?;

    if !deleted {
        return Err(ApiError::NotFound("API token not found".into()));
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Rename one named API token. The label is trimmed before storing.
#[utoipa::path(
    patch,
    path = "/v1/accounts/{id}/api-tokens/{token_id}",
    tag = "Accounts",
    security(("session" = [])),
    params(
        ("id" = i64, Path, description = "Account id; must be the caller's own"),
        ("token_id" = i64, Path, description = "API token id")
    ),
    request_body = UpdateApiTokenRequest,
    responses(
        (status = 200, body = UpdateApiTokenResponse),
    )
)]
pub async fn update_api_token(
    State(state): State<AppState>,
    Path((account_id, id)): Path<(i64, i64)>,
    FullAccess(auth): FullAccess,
    Json(req): Json<UpdateApiTokenRequest>,
) -> Result<Json<UpdateApiTokenResponse>, ApiError> {
    let mut conn = state.db.acquire().await?;
    require_account_reach(&mut conn, &auth, account_id, HOLDER_ONLY).await?;
    let label = req.label;

    schema::ensure_accounts_schema(&mut conn).await?;
    let trimmed = label.trim().to_string();
    let ok = api_tokens::update_api_token_label(&mut conn, account_id, id, &trimmed)
        .await
        .map_err(map_label_error)?;

    if !ok {
        return Err(ApiError::NotFound("API token not found".into()));
    }
    Ok(Json(UpdateApiTokenResponse { id, label: trimmed }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::api_tokens::{ApiTokenLabelError, ApiTokenMutationError};

    /// The `Location` of a new token names it, and a `GET` there answers the
    /// token as the list shows it, with its masked secret and never the
    /// secret itself.
    #[tokio::test]
    async fn a_new_tokens_location_answers_the_token_as_the_list_shows_it() {
        use crate::test_support::{fixture_with_account, get_json, get_raw, post_created_json};

        let (fixture, alice) = fixture_with_account().await;
        let state = fixture.state.clone();
        let tokens = format!("/v1/accounts/{}/api-tokens", alice.account_id);
        let (location, created): (String, serde_json::Value) = post_created_json(
            &state,
            &tokens,
            &alice.token,
            serde_json::json!({ "label": "pull", "can_import": false }),
        )
        .await;

        let token: serde_json::Value = get_json(&state, &location, &alice.token).await;
        let listed: serde_json::Value = get_json(&state, &tokens, &alice.token).await;
        assert_eq!(token, listed["items"][0], "{token}");
        assert_eq!(token["token_hint"], created["token_hint"], "{token}");
        assert!(token.get("token").is_none(), "{token}");

        let (status, text) = get_raw(&state, &format!("{tokens}/999999"), &alice.token).await;
        crate::test_support::expect_problem(status, &text, crate::problem::ProblemType::NotFound);
    }

    #[test]
    fn label_errors_map_to_validation_failed_with_the_same_message() {
        let err = map_label_error(ApiTokenMutationError::InvalidLabel(
            ApiTokenLabelError::Required,
        ));
        match err {
            ApiError::ValidationFailed(msg) => assert_eq!(msg, ["label is required"]),
            other => panic!("expected ValidationFailed, got {other:?}"),
        }

        let err = map_label_error(ApiTokenMutationError::InvalidLabel(
            ApiTokenLabelError::TooLong,
        ));
        match err {
            ApiError::ValidationFailed(msg) => {
                assert_eq!(msg, ["label must be at most 120 characters"]);
            }
            other => panic!("expected ValidationFailed, got {other:?}"),
        }
    }

    #[test]
    fn other_errors_map_to_internal() {
        let err = map_label_error(ApiTokenMutationError::Other(anyhow::anyhow!("boom")));
        match err {
            ApiError::Internal(err) => assert_eq!(err.to_string(), "boom"),
            other => panic!("expected Internal, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn deleting_a_token_that_does_not_exist_is_not_found() {
        let (fixture, account) = crate::test_support::fixture_with_account().await;

        let (status, text) = crate::test_support::delete_raw(
            &fixture.state,
            &format!("/v1/accounts/{}/api-tokens/999999", account.account_id),
            &account.token,
        )
        .await;

        crate::test_support::expect_problem(status, &text, crate::problem::ProblemType::NotFound);
    }

    /// A token never carries `delete`, so a create-token body asking for it
    /// is refused, not quietly stripped: a caller that believes it holds a
    /// delete token would otherwise find out only when a delete fails.
    #[tokio::test]
    async fn create_token_asking_for_delete_is_refused() {
        let fixture = crate::test_support::test_fixture().await;
        let state = fixture.state.clone();
        let account =
            crate::test_support::register_via_api(&state, "token-owner", "hunter2hunter2").await;

        let collection = format!("/v1/accounts/{}/api-tokens", account.account_id);
        let (status, text) = crate::test_support::post_raw(
            &state,
            &collection,
            &account.token,
            "application/json",
            r#"{"label": "cli token", "can_delete": true}"#,
        )
        .await;
        crate::test_support::expect_problem(
            status,
            &text,
            crate::problem::ProblemType::ValidationFailed,
        );

        let mut conn = state.db.acquire().await.unwrap();
        let rows: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM account_api_tokens WHERE account_id = $1")
                .bind(account.account_id)
                .fetch_one(&mut *conn)
                .await
                .unwrap();
        assert_eq!(rows, 0, "a refused create stores no token");
    }

    /// Tokens belong to the account that holds them: another account is
    /// refused on every token route, and so is the owner, who has none.
    #[tokio::test]
    async fn only_the_account_itself_reaches_its_tokens() {
        use crate::test_support::{
            claim_as_owner, delete_status, get_status, patch_status, post_status, register_via_api,
            test_fixture,
        };
        use axum::http::StatusCode;

        let fixture = test_fixture().await;
        let state = fixture.state.clone();
        let owner = claim_as_owner(&state, "keeper", "hunter2hunter2").await;
        let alice = register_via_api(&state, "alice", "hunter2hunter2").await;
        let bob = register_via_api(&state, "bob", "hunter2hunter2").await;
        let alices = format!("/v1/accounts/{}/api-tokens", alice.account_id);

        assert_eq!(
            get_status(&state, &alices, &alice.token).await,
            StatusCode::OK
        );
        for (who, token) in [("bob", &bob.token), ("the owner", &owner.token)] {
            assert_eq!(
                get_status(&state, &alices, token).await,
                StatusCode::FORBIDDEN,
                "{who} must not list alice's tokens"
            );
            assert_eq!(
                post_status(&state, &alices, token, serde_json::json!({ "label": "x" })).await,
                StatusCode::FORBIDDEN,
                "{who} must not mint a token for alice"
            );
            assert_eq!(
                patch_status(
                    &state,
                    &format!("{alices}/1"),
                    token,
                    serde_json::json!({ "label": "y" })
                )
                .await,
                StatusCode::FORBIDDEN,
                "{who} must not rename alice's token"
            );
            assert_eq!(
                delete_status(&state, &format!("{alices}/1"), token).await,
                StatusCode::FORBIDDEN,
                "{who} must not revoke alice's token"
            );
        }
        // The owner's own row has no tokens either: the route takes an
        // ordinary session, and the owner's is not one.
        assert_eq!(
            get_status(
                &state,
                &format!("/v1/accounts/{}/api-tokens", owner.account_id),
                &owner.token
            )
            .await,
            StatusCode::FORBIDDEN
        );
    }

    /// A rename answers the new label, trimmed, and the list shows it; an id
    /// the account does not hold is a 404.
    #[tokio::test]
    async fn renaming_a_token_answers_and_stores_the_new_label() {
        use crate::test_support::{
            fixture_with_account, get_json, patch_json, patch_status, post_created_json,
        };
        use axum::http::StatusCode;

        let (fixture, alice) = fixture_with_account().await;
        let state = fixture.state.clone();
        let collection = format!("/v1/accounts/{}/api-tokens", alice.account_id);
        let (_, created): (String, serde_json::Value) = post_created_json(
            &state,
            &collection,
            &alice.token,
            serde_json::json!({ "label": "old name" }),
        )
        .await;
        let id = created["id"].as_i64().unwrap();

        let renamed: serde_json::Value = patch_json(
            &state,
            &format!("{collection}/{id}"),
            &alice.token,
            serde_json::json!({ "label": "  new name  " }),
        )
        .await;
        assert_eq!(
            renamed,
            serde_json::json!({ "id": id, "label": "new name" })
        );
        let listed: serde_json::Value = get_json(&state, &collection, &alice.token).await;
        assert_eq!(listed["items"][0]["label"], "new name", "{listed}");

        assert_eq!(
            patch_status(
                &state,
                &format!("{collection}/{}", id + 1000),
                &alice.token,
                serde_json::json!({ "label": "nobody" })
            )
            .await,
            StatusCode::NOT_FOUND
        );
    }

    /// A token created without naming `can_export` may export: the default is
    /// on, and the export route admits it.
    #[tokio::test]
    async fn a_token_created_without_can_export_may_export() {
        use crate::test_support::{fixture_with_account, get_status, post_created_json};
        use axum::http::StatusCode;

        let (fixture, alice) = fixture_with_account().await;
        let state = fixture.state.clone();
        let (_, created): (String, serde_json::Value) = post_created_json(
            &state,
            &format!("/v1/accounts/{}/api-tokens", alice.account_id),
            &alice.token,
            serde_json::json!({ "label": "pull" }),
        )
        .await;
        assert_eq!(created["can_export"], true);

        let token = created["token"].as_str().unwrap();
        assert_eq!(
            get_status(&state, "/v1/exports", token).await,
            StatusCode::OK
        );
    }

    /// Turn `flags` on `account_id` as the owner would.
    async fn set_flags(
        fixture: &crate::test_support::TestFixture,
        account_id: i64,
        flags: crate::db::account_profile::AccountFlags,
    ) {
        let mut conn = fixture.conn().await;
        crate::db::account_profile::set_account_flags(&mut conn, account_id, flags)
            .await
            .unwrap();
    }

    /// A token made by an account that may not import does not report
    /// `can_import`, in the answer or the list, whatever the request asked.
    #[tokio::test]
    async fn a_new_token_is_capped_by_its_accounts_permissions() {
        use crate::db::account_profile::AccountFlags;
        use crate::test_support::{fixture_with_account, get_json, post_created_json};

        let (fixture, alice) = fixture_with_account().await;
        set_flags(
            &fixture,
            alice.account_id,
            AccountFlags {
                can_import: Some(false),
                ..Default::default()
            },
        )
        .await;
        let collection = format!("/v1/accounts/{}/api-tokens", alice.account_id);

        let (_, created): (String, serde_json::Value) = post_created_json(
            &fixture.state,
            &collection,
            &alice.token,
            serde_json::json!({ "label": "x", "can_import": true }),
        )
        .await;
        assert_eq!(created["can_import"], false, "{created}");
        assert_eq!(created["can_export"], true, "{created}");

        let listed: serde_json::Value = get_json(&fixture.state, &collection, &alice.token).await;
        assert_eq!(listed["items"][0]["can_import"], false, "{listed}");
        assert_eq!(listed["items"][0]["can_export"], true, "{listed}");
    }

    /// A permission the owner turns off after a token is made shows as off
    /// in the list.
    #[tokio::test]
    async fn the_token_list_follows_the_accounts_permissions_as_they_are_now() {
        use crate::db::account_profile::AccountFlags;
        use crate::test_support::{fixture_with_account, get_json, post_created_json};

        let (fixture, alice) = fixture_with_account().await;
        let collection = format!("/v1/accounts/{}/api-tokens", alice.account_id);
        let (_, created): (String, serde_json::Value) = post_created_json(
            &fixture.state,
            &collection,
            &alice.token,
            serde_json::json!({ "label": "pull" }),
        )
        .await;
        assert_eq!(created["can_export"], true, "{created}");

        set_flags(
            &fixture,
            alice.account_id,
            AccountFlags {
                can_export: Some(false),
                ..Default::default()
            },
        )
        .await;

        let listed: serde_json::Value = get_json(&fixture.state, &collection, &alice.token).await;
        assert_eq!(listed["items"][0]["can_export"], false, "{listed}");
        assert_eq!(listed["items"][0]["can_import"], true, "{listed}");
    }
}
