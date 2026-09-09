//! `GET /v1/search-fields`: the words the search language accepts on one
//! list, so the web's suggestions and the docs read the server's own table.

use crate::extract::{Json, Query};
use serde::Deserialize;

use crate::paging::{DEFAULT_LIST_LIMIT, Page, page_of, page_params};
use crate::search::{FieldDoc, ListKind, describe};
use crate::server::{ApiError, FullAccess};

/// Which list's words to describe.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub(crate) struct SearchFieldsQuery {
    /// `contacts`, `conversations`, or `messages`.
    list: ListKind,
    /// Page size, default 40, max 500.
    #[serde(default)]
    limit: Option<usize>,
    /// Page offset.
    #[serde(default)]
    offset: Option<usize>,
}

/// The search words one list accepts.
#[utoipa::path(
    get,
    path = "/v1/search-fields",
    tag = "Search",
    security(("session" = [])),
    params(SearchFieldsQuery),
    responses(
        (status = 200, body = crate::paging::Page<FieldDoc>),
        (status = 400, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem)
    )
)]
pub(crate) async fn search_fields_list(
    FullAccess(_auth): FullAccess,
    Query(query): Query<SearchFieldsQuery>,
) -> Result<Json<Page<FieldDoc>>, ApiError> {
    let params = page_params(query.limit, query.offset, DEFAULT_LIST_LIMIT, None)?;
    Ok(Json(page_of(describe(query.list), params)))
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use crate::test_support::{get_json, get_status, register_via_api, test_vault};

    #[tokio::test]
    async fn fields_are_served_per_list() {
        let vault = test_vault().await;
        let account = register_via_api(&vault.state, "alice", "hunter2hunter2").await;
        let body: serde_json::Value = get_json(
            &vault.state,
            "/v1/search-fields?list=contacts",
            &account.token,
        )
        .await;
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
        assert_eq!(
            get_status(&vault.state, "/v1/search-fields?list=nope", &account.token).await,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            get_status(
                &vault.state,
                "/v1/search-fields?list=messages",
                "not-a-token"
            )
            .await,
            StatusCode::UNAUTHORIZED
        );
    }
}
