//! Saved searches stored in `saved_searches`.
//!
//! Rows are addressed by id in the path rather than by name in the body, which
//! is how contact groups and message tags work. A saved search carries a name
//! and a query that are edited together, so name-addressing would use the
//! changing field as the key.

use crate::extract::{Json, Path};
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::db::saved_searches::{self, SavedSearch, SavedSearchKind};
use crate::server::{ApiError, AppState, Created, FullAccess};

/// A saved search's name and query.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct SavedSearchBody {
    name: String,
    query: String,
}

/// The account's saved searches, A–Z.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct SavedSearchesListResponse {
    items: Vec<SavedSearch>,
}

/// List the account's saved searches, A–Z.
#[utoipa::path(
    get,
    path = "/v1/saved-searches",
    tag = "Saved searches",
    security(("bearer" = [])),
    responses(
        (status = 200, body = SavedSearchesListResponse),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody)
    )
)]
pub(crate) async fn saved_searches_list_handler(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
) -> Result<Json<SavedSearchesListResponse>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let items = saved_searches::list(&mut conn, &auth.account_id).await?;
    Ok(Json(SavedSearchesListResponse { items }))
}

/// Create a saved search: `201 Created`, `Location: /v1/saved-searches/{id}`,
/// and the row.
#[utoipa::path(
    post,
    path = "/v1/saved-searches",
    tag = "Saved searches",
    security(("bearer" = [])),
    request_body = SavedSearchBody,
    responses(
        (
            status = 201,
            body = SavedSearch,
            headers(("Location" = String, description = "Path of the new saved search"))
        ),
        (status = 400, body = crate::server::ErrorBody),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody),
        (status = 409, body = crate::server::ErrorBody)
    )
)]
pub(crate) async fn saved_searches_create_handler(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    Json(body): Json<SavedSearchBody>,
) -> Result<Created<SavedSearch>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let row = saved_searches::create(
        &mut conn,
        &auth.account_id,
        &body.name,
        &body.query,
        SavedSearchKind::Manual,
    )
    .await?;
    Ok(Created {
        location: format!("/v1/saved-searches/{}", row.id),
        body: row,
    })
}

/// Replace a saved search's name and query, and return it.
#[utoipa::path(
    patch,
    path = "/v1/saved-searches/{id}",
    tag = "Saved searches",
    security(("bearer" = [])),
    params(("id" = i64, Path, description = "Saved search id")),
    request_body = SavedSearchBody,
    responses(
        (status = 200, body = SavedSearch),
        (status = 400, body = crate::server::ErrorBody),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody),
        (status = 404, body = crate::server::ErrorBody),
        (status = 409, body = crate::server::ErrorBody)
    )
)]
pub(crate) async fn saved_searches_update_handler(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    Path(id): Path<i64>,
    Json(body): Json<SavedSearchBody>,
) -> Result<Json<SavedSearch>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let row =
        saved_searches::update(&mut conn, &auth.account_id, id, &body.name, &body.query).await?;
    Ok(Json(row))
}

/// Delete a saved search.
///
/// Deleting an import-created saved search removes the shortcut only. The
/// `vault_imports` row it pointed at is the account's permanent record of that
/// run and is never touched here.
#[utoipa::path(
    delete,
    path = "/v1/saved-searches/{id}",
    tag = "Saved searches",
    security(("bearer" = [])),
    params(("id" = i64, Path, description = "Saved search id")),
    responses(
        (status = 204, description = "Saved search deleted"),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody),
        (status = 404, body = crate::server::ErrorBody)
    )
)]
pub(crate) async fn saved_searches_delete_handler(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let mut conn = state.db.acquire().await?;
    saved_searches::delete(&mut conn, &auth.account_id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use crate::test_support::{
        delete_status, get_json, patch_json, post_created_json, register_via_api, test_vault,
    };

    #[tokio::test]
    async fn saved_searches_list_as_items_and_each_write_answers_the_row_or_204() {
        let vault = test_vault().await;
        let state = vault.state.clone();
        let user = register_via_api(&state, "alice", "hunter2hunter2").await;

        let (location, created): (String, serde_json::Value) = post_created_json(
            &state,
            "/v1/saved-searches",
            &user.token,
            serde_json::json!({ "name": "Family", "query": "group:Family" }),
        )
        .await;
        assert_eq!(created["name"], "Family");
        assert!(created["id"].is_i64());
        assert!(created.get("savedSearch").is_none() && created.get("savedSearches").is_none());
        let id = created["id"].as_i64().unwrap();
        assert_eq!(location, format!("/v1/saved-searches/{id}"));

        let renamed: serde_json::Value = patch_json(
            &state,
            &format!("/v1/saved-searches/{id}"),
            &user.token,
            serde_json::json!({ "name": "Kin", "query": "group:Family" }),
        )
        .await;
        assert_eq!(renamed["name"], "Kin");

        let list: serde_json::Value = get_json(&state, "/v1/saved-searches", &user.token).await;
        assert_eq!(list["items"][0]["name"], "Kin");
        assert!(list.get("savedSearches").is_none());

        let status = delete_status(&state, &format!("/v1/saved-searches/{id}"), &user.token).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let list: serde_json::Value = get_json(&state, "/v1/saved-searches", &user.token).await;
        assert_eq!(list["items"].as_array().unwrap().len(), 0);
    }
}
