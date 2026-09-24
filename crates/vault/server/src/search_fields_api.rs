//! `GET /v1/search-fields/contacts` and `GET /v1/search-fields/conversations`:
//! the words the search language accepts on each list, so the web's
//! suggestions and the docs read the server's own table.
//!
//! Two fixed lists, so two paths: choosing which list to read is choosing a
//! resource, and a parameter only narrows one (`docs/architecture/http-api.md`,
//! "Naming a route").

use crate::extract::{Json, Query};
use serde::Deserialize;

use crate::paging::{DEFAULT_LIST_LIMIT, Page, page_of, page_params};
use crate::search::{FieldDoc, ListKind, describe};
use crate::server::{ApiError, FullAccess};

/// The paging a search-field list takes.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub(crate) struct ListSearchFieldsQuery {
    /// Page size, default 40, max 500.
    #[serde(default)]
    limit: Option<usize>,
    /// Page offset.
    #[serde(default)]
    offset: Option<usize>,
}

/// One list's words, paged.
fn search_fields(
    list: ListKind,
    query: &ListSearchFieldsQuery,
) -> Result<Page<FieldDoc>, ApiError> {
    let params = page_params(query.limit, query.offset, DEFAULT_LIST_LIMIT, None)?;
    Ok(page_of(describe(list), params))
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
        (status = 422, body = crate::problem::Problem),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem)
    )
)]
pub(crate) async fn list_contact_search_fields(
    FullAccess(_auth): FullAccess,
    Query(query): Query<ListSearchFieldsQuery>,
) -> Result<Json<Page<FieldDoc>>, ApiError> {
    Ok(Json(search_fields(ListKind::Contacts, &query)?))
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
        (status = 422, body = crate::problem::Problem),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem)
    )
)]
pub(crate) async fn list_conversation_search_fields(
    FullAccess(_auth): FullAccess,
    Query(query): Query<ListSearchFieldsQuery>,
) -> Result<Json<Page<FieldDoc>>, ApiError> {
    Ok(Json(search_fields(ListKind::Conversations, &query)?))
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use crate::test_support::{get_json, get_status, register_via_api, test_vault};

    /// The words a list's page names.
    fn words(body: &serde_json::Value) -> Vec<String> {
        body["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["word"].as_str().unwrap().to_string())
            .collect()
    }

    #[tokio::test]
    async fn each_list_is_its_own_path_with_its_own_words() {
        let vault = test_vault().await;
        let account = register_via_api(&vault.state, "alice", "hunter2hunter2").await;
        let contacts: serde_json::Value = get_json(
            &vault.state,
            "/v1/search-fields/contacts?limit=500",
            &account.token,
        )
        .await;
        let contact_words = words(&contacts);
        assert!(contact_words.iter().any(|w| w == "groups"));
        assert!(!contact_words.iter().any(|w| w == "with"));
        let first = &contacts["items"][0];
        assert!(first["help"].is_string() && first["example"].is_string());

        let conversations: serde_json::Value = get_json(
            &vault.state,
            "/v1/search-fields/conversations?limit=500",
            &account.token,
        )
        .await;
        let conversation_words = words(&conversations);
        assert!(conversation_words.iter().any(|w| w == "with"));
        assert!(!conversation_words.iter().any(|w| w == "groups"));

        // The list is the path now; the old parameter is refused, not obeyed.
        assert_eq!(
            get_status(
                &vault.state,
                "/v1/search-fields/contacts?list=conversations",
                &account.token
            )
            .await,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            get_status(
                &vault.state,
                "/v1/search-fields/conversations",
                "not-a-token"
            )
            .await,
            StatusCode::UNAUTHORIZED
        );
    }
}
