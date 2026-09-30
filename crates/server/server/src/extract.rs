//! Axum's `Query`, `Path`, and `Json`, answering as problem documents.
//!
//! Axum's extractors reject a bad request with a plain-text body. Every other
//! failure on this interface is a problem document (`docs/architecture/http-api.md`), so these three
//! wrappers turn each rejection into the [`ApiError`] on the right side of one
//! line: a request that cannot be read is `malformed-body`, one that parsed
//! and then broke a rule is `validation-failed`. Handlers use these names in
//! place of Axum's.

use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequest, FromRequestParts, OptionalFromRequest, Request};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::server::ApiError;

/// Axum's `Query`, rejecting as `validation-failed`: the string was read and
/// a value in it does not fit the parameter.
#[derive(Debug, Clone, Copy, Default)]
pub struct Query<T>(pub T);

impl<T, S> FromRequestParts<S> for Query<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match axum::extract::Query::<T>::from_request_parts(parts, state).await {
            Ok(axum::extract::Query(value)) => Ok(Query(value)),
            Err(rejection) => Err(ApiError::validation(rejection.body_text())),
        }
    }
}

/// Axum's `Path`, rejecting as `validation-failed`: a segment that is not
/// the number the route expects.
#[derive(Debug, Clone, Copy, Default)]
pub struct Path<T>(pub T);

impl<T, S> FromRequestParts<S> for Path<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match axum::extract::Path::<T>::from_request_parts(parts, state).await {
            Ok(axum::extract::Path(value)) => Ok(Path(value)),
            Err(rejection) => Err(ApiError::validation(rejection.body_text())),
        }
    }
}

/// Axum's `Json`, rejecting as a problem and answering as JSON.
#[derive(Debug, Clone, Copy, Default)]
pub struct Json<T>(pub T);

/// Axum's rejection as the problem on the right side of the line: well-formed
/// JSON that does not fit the target type parsed and then broke a rule;
/// everything else could not be read.
fn json_rejection(rejection: JsonRejection) -> ApiError {
    match rejection {
        JsonRejection::JsonDataError(e) => ApiError::validation(e.body_text()),
        JsonRejection::JsonSyntaxError(e) => ApiError::MalformedBody(e.body_text()),
        JsonRejection::MissingJsonContentType(e) => ApiError::UnsupportedMediaType(e.body_text()),
        rejection if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            ApiError::PayloadTooLarge(rejection.body_text())
        }
        rejection => ApiError::MalformedBody(rejection.body_text()),
    }
}

impl<T, S> FromRequest<S> for Json<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match <axum::Json<T> as FromRequest<S>>::from_request(req, state).await {
            Ok(axum::Json(value)) => Ok(Json(value)),
            Err(rejection) => Err(json_rejection(rejection)),
        }
    }
}

/// `body: Option<Json<T>>`, for a route one caller sends a body to and
/// another does not: `None` when the request carries no `Content-Type`, the
/// parsed body when it does, and the same rejections as the required form
/// when what it carries is not JSON.
impl<T, S> OptionalFromRequest<S> for Json<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Option<Self>, Self::Rejection> {
        match <axum::Json<T> as OptionalFromRequest<S>>::from_request(req, state).await {
            Ok(Some(axum::Json(value))) => Ok(Some(Json(value))),
            Ok(None) => Ok(None),
            Err(rejection) => Err(json_rejection(rejection)),
        }
    }
}

impl<T: Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        axum::Json(self.0).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::Json;
    use crate::problem::ProblemType;
    use crate::server::ApiError;
    use crate::test_support::{
        PASSWORD, delete_raw, expect_problem, get_raw, post_raw, test_vault, vault_with_account,
    };
    use axum::extract::FromRequest;
    use axum::http::{Request, StatusCode, header};

    #[tokio::test]
    async fn a_query_parameter_of_the_wrong_type_is_a_validation_422() {
        let (vault, user) = vault_with_account().await;
        let (status, text) =
            get_raw(&vault.state, "/v1/conversations?limit=ten", &user.token).await;
        let problem = expect_problem(status, &text, ProblemType::ValidationFailed);
        assert!(problem.errors.unwrap()[0].contains("limit"), "{text}");
    }

    #[tokio::test]
    async fn a_path_id_that_is_not_a_number_is_a_validation_422() {
        let (vault, user) = vault_with_account().await;
        let (status, text) =
            get_raw(&vault.state, "/v1/conversations/abc/sources", &user.token).await;
        let problem = expect_problem(status, &text, ProblemType::ValidationFailed);
        assert!(!problem.errors.unwrap().is_empty(), "{text}");
    }

    #[tokio::test]
    async fn a_json_body_missing_a_field_is_a_json_422() {
        let (vault, user) = vault_with_account().await;
        let (status, text) = post_raw(
            &vault.state,
            "/v1/saved-searches",
            &user.token,
            "application/json",
            r#"{"name": "only a name"}"#,
        )
        .await;
        // Well-formed JSON that fails to deserialize into the target type is
        // Axum's `JsonDataError`, which carries `422` — a different rejection
        // from malformed JSON syntax (`400`).
        let problem = expect_problem(status, &text, ProblemType::ValidationFailed);
        assert!(problem.errors.unwrap()[0].contains("query"), "{text}");
    }

    #[tokio::test]
    async fn a_json_body_with_the_wrong_content_type_is_a_json_415() {
        let (vault, user) = vault_with_account().await;
        let (status, text) = post_raw(
            &vault.state,
            "/v1/saved-searches",
            &user.token,
            "text/plain",
            r#"{"name": "only a name", "query": "hi"}"#,
        )
        .await;
        expect_problem(status, &text, ProblemType::UnsupportedMediaType);
    }

    /// A JSON body over `[server] max_body_bytes` on an ordinary route answers
    /// the `payload-too-large` problem, while a body under the cap that is
    /// not JSON still answers `malformed-body`: the cap never turns a syntax
    /// error into a 413, and a 413 is never a 400. Sent with a
    /// `Content-Length`, so `RequestBodyLimitLayer` answers before the
    /// extractor runs and `json_body_limit_response` rewrites its plain
    /// text; the extractor's own arm is tested below without HTTP.
    #[tokio::test]
    async fn a_json_body_over_the_body_cap_is_a_json_413_and_a_syntax_error_a_400() {
        let (vault, user) = vault_with_account().await;
        let mut state = vault.state.clone();
        state.max_body_bytes = 1024;

        let padding = "a".repeat(4096);
        let body = serde_json::json!({ "name": padding, "query": "hi" }).to_string();
        let (status, text) = post_raw(
            &state,
            "/v1/saved-searches",
            &user.token,
            "application/json",
            body,
        )
        .await;
        expect_problem(status, &text, ProblemType::PayloadTooLarge);

        let (status, text) = post_raw(
            &state,
            "/v1/saved-searches",
            &user.token,
            "application/json",
            r#"{"name": "unterminated"#,
        )
        .await;
        expect_problem(status, &text, ProblemType::MalformedBody);
    }

    /// A request to the extractor itself, so the body reaches Axum's `Json`
    /// with no `Content-Length` for the limit layer to answer from.
    fn json_request(body: axum::body::Body) -> Request<axum::body::Body> {
        Request::post("/")
            .header(header::CONTENT_TYPE, "application/json")
            .body(body)
            .unwrap()
    }

    /// A body the extractor had to read to discover it was too long (no
    /// `Content-Length`, so the limit layer cannot answer early) is Axum's
    /// `413` rejection, and `json_rejection` keeps that status as the
    /// `payload-too-large` problem rather than folding it into `400`. Axum's
    /// own default cap is 2 MiB; the body streams in past it.
    #[tokio::test]
    async fn a_streamed_json_body_over_the_limit_is_payload_too_large() {
        let chunk = axum::body::Bytes::from(vec![b'a'; 1024 * 1024]);
        let chunks: Vec<Result<axum::body::Bytes, std::io::Error>> =
            std::iter::once(axum::body::Bytes::from_static(b"\""))
                .chain(std::iter::repeat_n(chunk, 3))
                .map(Ok)
                .collect();
        let body = axum::body::Body::from_stream(futures_util::stream::iter(chunks));

        let error = Json::<serde_json::Value>::from_request(json_request(body), &())
            .await
            .unwrap_err();

        assert!(
            matches!(error, ApiError::PayloadTooLarge(_)),
            "a body over the limit is payload-too-large, got {error:?}"
        );
        assert_eq!(error.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    /// A body that fails while being read is the other rejection on the
    /// buffering path, and it is not over any limit: it is `malformed-body`,
    /// never `payload-too-large`. This is what keeps the status check in
    /// `json_rejection` honest instead of treating every read failure as a
    /// 413.
    #[tokio::test]
    async fn a_json_body_that_fails_midway_is_malformed_not_too_large() {
        let chunks: Vec<Result<axum::body::Bytes, std::io::Error>> = vec![
            Ok(axum::body::Bytes::from_static(b"{\"name\": ")),
            Err(std::io::Error::other("connection reset")),
        ];
        let body = axum::body::Body::from_stream(futures_util::stream::iter(chunks));

        let error = Json::<serde_json::Value>::from_request(json_request(body), &())
            .await
            .unwrap_err();

        assert!(
            matches!(error, ApiError::MalformedBody(_)),
            "a read failure is malformed-body, got {error:?}"
        );
        assert_eq!(error.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn a_json_body_over_the_auth_router_body_limit_is_a_json_413() {
        let vault = test_vault().await;
        // The auth router caps request bodies at 32 KiB (server.rs,
        // `limited_auth_router`); pad well past it with a valid JSON string.
        let padding = "a".repeat(64 * 1024);
        let body = serde_json::json!({ "username": padding, "password": PASSWORD }).to_string();
        let (status, text) = post_raw(
            &vault.state,
            "/v1/session",
            "unused-token",
            "application/json",
            body,
        )
        .await;
        expect_problem(status, &text, ProblemType::PayloadTooLarge);
    }

    #[tokio::test]
    async fn an_unknown_api_path_is_a_json_404_and_a_wrong_method_a_json_405() {
        let (vault, user) = vault_with_account().await;
        let (status, text) = get_raw(&vault.state, "/v1/no-such-thing", &user.token).await;
        let problem = expect_problem(status, &text, ProblemType::NotFound);
        assert_eq!(
            problem.detail.as_deref(),
            Some("no route at /v1/no-such-thing")
        );

        let (status, text) = delete_raw(&vault.state, "/v1/conversations", &user.token).await;
        let problem = expect_problem(status, &text, ProblemType::MethodNotAllowed);
        assert_eq!(
            problem.detail.as_deref(),
            Some("DELETE is not allowed at /v1/conversations")
        );
    }

    #[tokio::test]
    async fn bare_v1_and_v1_slash_are_a_json_404() {
        let vault = test_vault().await;
        for path in ["/v1", "/v1/"] {
            let (status, text) = get_raw(&vault.state, path, "unused-token").await;
            expect_problem(status, &text, ProblemType::NotFound);
        }
    }
}
