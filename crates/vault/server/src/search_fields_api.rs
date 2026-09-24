//! `GET /v1/search-fields/{contacts,conversations,messages}`: the words the
//! search language accepts on one list, so the web's suggestions and the docs
//! read the server's own table.
//!
//! Each list is a route of its own rather than one route read with `?list=`,
//! because choosing which list to read is choosing a resource, and the path
//! does that (`docs/architecture/http-api.md`, "Naming a route").

use crate::extract::{Json, Query};
use serde::Deserialize;

use crate::paging::{DEFAULT_LIST_LIMIT, Page, page_of, page_params};
use crate::search::{FieldDoc, ListKind, describe};
use crate::server::{ApiError, FullAccess};

/// The paging of a search-field list.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub(crate) struct ListSearchFieldsQuery {
    /// Page size, default 40, max 500.
    #[serde(default)]
    limit: Option<usize>,
    /// Page offset.
    #[serde(default)]
    offset: Option<usize>,
}

/// One page of `list`'s search words.
fn search_fields(
    list: ListKind,
    query: &ListSearchFieldsQuery,
) -> Result<Json<Page<FieldDoc>>, ApiError> {
    let params = page_params(query.limit, query.offset, DEFAULT_LIST_LIMIT, None)?;
    Ok(Json(page_of(describe(list), params)))
}

/// The search words the Contacts list accepts.
#[utoipa::path(
    get,
    path = "/v1/search-fields/contacts",
    tag = "Search",
    security(("session" = [])),
    params(ListSearchFieldsQuery),
    responses(
        (status = 200, body = crate::paging::Page<FieldDoc>),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem)
    )
)]
pub(crate) async fn list_contact_search_fields(
    FullAccess(_auth): FullAccess,
    Query(query): Query<ListSearchFieldsQuery>,
) -> Result<Json<Page<FieldDoc>>, ApiError> {
    search_fields(ListKind::Contacts, &query)
}

/// The search words the Conversations list accepts.
#[utoipa::path(
    get,
    path = "/v1/search-fields/conversations",
    tag = "Search",
    security(("session" = [])),
    params(ListSearchFieldsQuery),
    responses(
        (status = 200, body = crate::paging::Page<FieldDoc>),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem)
    )
)]
pub(crate) async fn list_conversation_search_fields(
    FullAccess(_auth): FullAccess,
    Query(query): Query<ListSearchFieldsQuery>,
) -> Result<Json<Page<FieldDoc>>, ApiError> {
    search_fields(ListKind::Conversations, &query)
}

/// The search words the Messages list accepts.
#[utoipa::path(
    get,
    path = "/v1/search-fields/messages",
    tag = "Search",
    security(("session" = [])),
    params(ListSearchFieldsQuery),
    responses(
        (status = 200, body = crate::paging::Page<FieldDoc>),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem)
    )
)]
pub(crate) async fn list_message_search_fields(
    FullAccess(_auth): FullAccess,
    Query(query): Query<ListSearchFieldsQuery>,
) -> Result<Json<Page<FieldDoc>>, ApiError> {
    search_fields(ListKind::Messages, &query)
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use crate::test_support::{get_json, get_status, register_via_api, test_vault};

    #[tokio::test]
    async fn fields_are_served_per_list() {
        let vault = test_vault().await;
        let account = register_via_api(&vault.state, "alice", "hunter2hunter2").await;
        let body: serde_json::Value =
            get_json(&vault.state, "/v1/search-fields/contacts", &account.token).await;
        let words: Vec<&str> = body["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["word"].as_str().unwrap())
            .collect();
        assert!(words.contains(&"groups"));
        assert!(!words.contains(&"from"));
        let first = &body["items"][0];
        assert!(first["help"].is_string() && first["example"].is_string());
        for list in ["conversations", "messages"] {
            let body: serde_json::Value = get_json(
                &vault.state,
                &format!("/v1/search-fields/{list}"),
                &account.token,
            )
            .await;
            assert!(body["total"].as_u64().unwrap() > 0, "{list}: {body}");
        }
        // Which list to read is a path segment, not a parameter, so the old
        // shape is no route at all.
        assert_eq!(
            get_status(
                &vault.state,
                "/v1/search-fields?list=contacts",
                &account.token
            )
            .await,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            get_status(&vault.state, "/v1/search-fields/nope", &account.token).await,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            get_status(&vault.state, "/v1/search-fields/messages", "not-a-token").await,
            StatusCode::UNAUTHORIZED
        );
    }
}
