//! One id per request, made by the server, carried on every response and
//! repeated in every problem body (`docs/agents/http-api-rules.md`).
//!
//! The id is set by [`layer`], the outermost layer on the router, and read by
//! [`current`] wherever a problem document is built. It travels as a task
//! local rather than a request extension because the code that builds the
//! failure body is `IntoResponse for ApiError`, which sees no request at all.
//!
//! An `x-request-id` a client sends is dropped, never kept: none of the
//! vault's own clients send one, and an operator grepping the log needs ids
//! they know the server made.

use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;

/// The header the id travels in, both ways.
pub const HEADER: HeaderName = HeaderName::from_static("x-request-id");

tokio::task_local! {
    static REQUEST_ID: String;
}

/// The id of the request being served, or `None` outside one (a handler
/// called directly from a test, a CLI command).
#[must_use]
pub fn current() -> Option<String> {
    REQUEST_ID.try_with(Clone::clone).ok()
}

/// Make the id, put it on the request so the trace span and any handler can
/// see it, serve the request under it, and put it on the response.
pub async fn layer(mut request: Request, next: Next) -> Response {
    let id = uuid::Uuid::new_v4().to_string();
    let value = HeaderValue::from_str(&id).expect("a UUID is a valid header value");
    request.headers_mut().insert(HEADER, value.clone());
    let mut response = REQUEST_ID.scope(id, next.run(request)).await;
    response.headers_mut().insert(HEADER, value);
    response
}
