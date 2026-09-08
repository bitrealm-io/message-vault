//! CRUD for named CLI API tokens.

use crate::extract::{Json, Path as AxumPath};
use axum::extract::State;
use serde::{Deserialize, Serialize};

use crate::db::api_tokens;
use crate::db::permissions::Permissions;
use crate::db::schema;
use crate::server::{ApiError, AppState, Created, FullAccess};

/// One named API token as shown in Settings: label, permissions, and masked secret.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ApiTokenItem {
    /// Token id (the secret itself is stored hashed).
    pub id: String,
    /// User-chosen label shown in Settings.
    pub label: String,
    /// May call the import endpoints.
    pub can_import: bool,
    /// May call the export endpoints.
    pub can_export: bool,
    /// May destroy message data.
    pub can_delete: bool,
    /// Masked secret for Settings (e.g. `mv-api-Sd..mE`).
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

impl From<api_tokens::ApiTokenRow> for ApiTokenItem {
    fn from(row: api_tokens::ApiTokenRow) -> Self {
        Self {
            id: row.id,
            label: row.label,
            can_import: row.permissions.import,
            can_export: row.permissions.export,
            can_delete: row.permissions.delete,
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

/// The account's named API tokens.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ListApiTokensResponse {
    /// The account's tokens.
    pub items: Vec<ApiTokenItem>,
}

/// Body for creating a token: label, permissions, optional expiry.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateApiTokenRequest {
    /// User-chosen label shown in Settings.
    pub label: String,
    /// May call the import endpoints. Default true.
    #[serde(default = "default_true")]
    pub can_import: bool,
    /// May call the export endpoints. Default true.
    #[serde(default = "default_true")]
    pub can_export: bool,
    /// May destroy message data. Default false — asked for, never inherited.
    #[serde(default)]
    pub can_delete: bool,
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
    pub id: String,
    /// User-chosen label.
    pub label: String,
    /// May call the import endpoints.
    pub can_import: bool,
    /// May call the export endpoints.
    pub can_export: bool,
    /// May destroy message data.
    pub can_delete: bool,
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
pub struct RenameApiTokenRequest {
    /// Replacement label.
    pub label: String,
}

/// The renamed token's id and stored label.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct RenameApiTokenResponse {
    /// Token id that was renamed.
    pub id: String,
    /// Stored label after the rename.
    pub label: String,
}

/// List the account's named API tokens with their permissions and masked secrets.
#[utoipa::path(
    get,
    path = "/v1/account/api-tokens",
    tag = "Account",
    security(("bearer" = [])),
    responses(
        (status = 200, body = ListApiTokensResponse),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem)
    )
)]
pub async fn list_api_tokens_handler(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
) -> Result<Json<ListApiTokensResponse>, ApiError> {
    let account_id = auth.account_id;

    let mut conn = state.db.acquire().await?;
    schema::ensure_accounts_schema(&mut conn).await?;
    let rows = api_tokens::list_api_tokens(&mut conn, &account_id).await?;
    let items = rows.into_iter().map(ApiTokenItem::from).collect();

    Ok(Json(ListApiTokensResponse { items }))
}

/// Create a named API token. Returns the plaintext secret once, at creation;
/// it is never returned again.
#[utoipa::path(
    post,
    path = "/v1/account/api-tokens",
    tag = "Account",
    security(("bearer" = [])),
    request_body = CreateApiTokenRequest,
    responses(
        (
            status = 201,
            body = CreateApiTokenResponse,
            headers(("Location" = String, description = "Path of the new token"))
        ),
        (status = 400, body = crate::problem::Problem),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem)
    )
)]
pub async fn create_api_token_handler(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    Json(req): Json<CreateApiTokenRequest>,
) -> Result<Created<CreateApiTokenResponse>, ApiError> {
    let account_id = auth.account_id;
    let label = req.label;
    let permissions = Permissions {
        import: req.can_import,
        export: req.can_export,
        delete: req.can_delete,
    };
    let expires_in_days = req.expires_in_days;

    let mut conn = state.db.acquire().await?;
    schema::ensure_accounts_schema(&mut conn).await?;
    let created =
        api_tokens::create_api_token(&mut conn, &account_id, &label, permissions, expires_in_days)
            .await
            .map_err(map_label_error)?;

    Ok(Created {
        location: format!("/v1/account/api-tokens/{}", created.id),
        body: CreateApiTokenResponse {
            id: created.id,
            label: created.label,
            can_import: created.permissions.import,
            can_export: created.permissions.export,
            can_delete: created.permissions.delete,
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
    path = "/v1/account/api-tokens/{id}",
    tag = "Account",
    security(("bearer" = [])),
    params(("id" = String, Path, description = "API token id")),
    responses(
        (status = 204, description = "Token deleted"),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem)
    )
)]
pub async fn delete_api_token_handler(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    AxumPath(id): AxumPath<String>,
) -> Result<axum::http::StatusCode, ApiError> {
    let account_id = auth.account_id;

    let mut conn = state.db.acquire().await?;
    schema::ensure_accounts_schema(&mut conn).await?;
    let deleted = api_tokens::delete_api_token(&mut conn, &account_id, &id).await?;

    if !deleted {
        return Err(ApiError::NotFound("API token not found".into()));
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Rename one named API token. The label is trimmed before storing.
#[utoipa::path(
    patch,
    path = "/v1/account/api-tokens/{id}",
    tag = "Account",
    security(("bearer" = [])),
    params(("id" = String, Path, description = "API token id")),
    request_body = RenameApiTokenRequest,
    responses(
        (status = 200, body = RenameApiTokenResponse),
        (status = 400, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem)
    )
)]
pub async fn rename_api_token_handler(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<RenameApiTokenRequest>,
) -> Result<Json<RenameApiTokenResponse>, ApiError> {
    let account_id = auth.account_id;
    let label = req.label;
    let id_for_resp = id.clone();

    let mut conn = state.db.acquire().await?;
    schema::ensure_accounts_schema(&mut conn).await?;
    let trimmed = label.trim().to_string();
    let ok = api_tokens::update_api_token_label(&mut conn, &account_id, &id, &trimmed)
        .await
        .map_err(map_label_error)?;

    if !ok {
        return Err(ApiError::NotFound("API token not found".into()));
    }
    Ok(Json(RenameApiTokenResponse {
        id: id_for_resp,
        label: trimmed,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::api_tokens::{ApiTokenLabelError, ApiTokenMutationError};

    #[test]
    fn label_errors_map_to_bad_request_with_the_same_message() {
        let err = map_label_error(ApiTokenMutationError::InvalidLabel(
            ApiTokenLabelError::Required,
        ));
        match err {
            ApiError::ValidationFailed(msg) => assert_eq!(msg, ["label is required"]),
            other => panic!("expected BadRequest, got {other:?}"),
        }

        let err = map_label_error(ApiTokenMutationError::InvalidLabel(
            ApiTokenLabelError::TooLong,
        ));
        match err {
            ApiError::ValidationFailed(msg) => {
                assert_eq!(msg, ["label must be at most 120 characters"]);
            }
            other => panic!("expected BadRequest, got {other:?}"),
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

    /// A create-token body that omits `can_delete` entirely (as a CLI or
    /// script caller might) must default to `false` — delete is opt-in, not
    /// inherited. This exercises the real JSON deserialization path (a bare
    /// `#[serde(default)]`), which a struct built in Rust would not catch if
    /// the attribute regressed to `default_true`.
    #[tokio::test]
    async fn create_token_without_can_delete_field_defaults_to_false() {
        let vault = crate::test_support::test_vault().await;
        let state = vault.state.clone();
        let account =
            crate::test_support::register_via_api(&state, "token-owner", "hunter2hunter2").await;

        let (location, body): (String, serde_json::Value) = crate::test_support::post_created_json(
            &state,
            "/v1/account/api-tokens",
            &account.token,
            serde_json::json!({ "label": "cli token" }),
        )
        .await;
        assert_eq!(
            location,
            format!("/v1/account/api-tokens/{}", body["id"].as_str().unwrap())
        );

        assert_eq!(
            body["can_delete"],
            serde_json::json!(false),
            "a create-token body omitting can_delete must not grant delete"
        );

        // Confirm the stored row agrees, not just the immediate response.
        let mut conn = state.db.acquire().await.unwrap();
        let can_delete: i64 = sqlx::query_scalar(
            "SELECT can_delete FROM account_api_tokens WHERE account_id = $1 AND label = $2",
        )
        .bind(&account.account_id)
        .bind("cli token")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
        assert_eq!(
            can_delete, 0,
            "stored token row must not have can_delete set"
        );
    }
}
