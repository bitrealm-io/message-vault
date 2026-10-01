//! Refuse a query parameter the route does not declare
//! (`docs/architecture/http-api.md`, "Lists").
//!
//! A typo (`limt=10`) or a guess at a convention the interface rejects
//! (`order=`, `fields=`, `year=`) would otherwise be answered as though it
//! had been obeyed. The parameters a route accepts are the ones its OpenAPI
//! operation declares, so the document the clients are generated from is
//! also the list this layer checks against: a route cannot take a parameter
//! the reference does not show, and the check needs no list of its own to
//! keep in step with the handlers.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use axum::extract::{MatchedPath, Request, State};
use axum::http::Method;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use utoipa::openapi::OpenApi;
use utoipa::openapi::path::{Operation, ParameterIn, PathItem};

use crate::server::ApiError;

/// The query parameters each `/v1` operation declares, by method and path
/// template (`/v1/conversations/{id}/messages`).
#[derive(Debug, Default)]
pub(crate) struct DeclaredQueries {
    routes: HashMap<(Method, String), BTreeSet<String>>,
}

impl DeclaredQueries {
    /// Read every operation's query parameters out of the assembled document.
    pub(crate) fn from_spec(spec: &OpenApi) -> Self {
        let mut routes = HashMap::new();
        for (path, item) in &spec.paths.paths {
            for (method, op) in operations(item) {
                let names = op
                    .parameters
                    .iter()
                    .flatten()
                    .chain(item.parameters.iter().flatten())
                    .filter(|p| matches!(p.parameter_in, ParameterIn::Query))
                    .map(|p| p.name.clone())
                    .collect();
                routes.insert((method, path.clone()), names);
            }
        }
        Self { routes }
    }

    /// The parameters `method` on `path` declares, or `None` for a route the
    /// document does not describe. `HEAD` falls back to `GET`, because Axum
    /// answers a `HEAD` with the `GET` handler when no `HEAD` is routed.
    fn declared(&self, method: &Method, path: &str) -> Option<&BTreeSet<String>> {
        self.routes
            .get(&(method.clone(), path.to_string()))
            .or_else(|| {
                (method == Method::HEAD)
                    .then(|| self.routes.get(&(Method::GET, path.to_string())))
                    .flatten()
            })
    }
}

/// The operations one path item holds, with their methods.
fn operations(item: &PathItem) -> impl Iterator<Item = (Method, &Operation)> {
    [
        (Method::GET, item.get.as_ref()),
        (Method::PUT, item.put.as_ref()),
        (Method::POST, item.post.as_ref()),
        (Method::DELETE, item.delete.as_ref()),
        (Method::HEAD, item.head.as_ref()),
        (Method::PATCH, item.patch.as_ref()),
    ]
    .into_iter()
    .filter_map(|(method, op)| op.map(|op| (method, op)))
}

/// The sentences for every parameter in `query` that `declared` lacks, one
/// each, naming what the route does take.
fn undeclared(query: &str, declared: &BTreeSet<String>) -> Vec<String> {
    let accepted = if declared.is_empty() {
        "this route takes no query parameters".to_string()
    } else {
        format!(
            "this route takes {}",
            declared
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let mut seen = BTreeSet::new();
    let uri: axum::http::Uri = format!("/?{query}").parse().unwrap_or_default();
    let pairs = axum::extract::Query::<Vec<(String, String)>>::try_from_uri(&uri)
        .map(|q| q.0)
        .unwrap_or_default();
    pairs
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| !declared.contains(name) && seen.insert(name.clone()))
        .map(|name| format!("unknown query parameter '{name}'; {accepted}"))
        .collect()
}

/// The layer: a `/v1` request carrying a query parameter its operation does
/// not declare answers `validation-failed`, before any extractor runs. A
/// route the document does not describe (the `/v1` fallbacks) is let
/// through to answer its own `404`.
pub(crate) async fn refuse_undeclared_query(
    State(declared): State<Arc<DeclaredQueries>>,
    matched: Option<MatchedPath>,
    request: Request,
    next: Next,
) -> Response {
    let (Some(matched), Some(query)) = (matched, request.uri().query()) else {
        return next.run(request).await;
    };
    if !matched.as_str().starts_with("/v1/") {
        return next.run(request).await;
    }
    let Some(names) = declared.declared(request.method(), matched.as_str()) else {
        return next.run(request).await;
    };
    let errors = undeclared(query, names);
    if errors.is_empty() {
        return next.run(request).await;
    }
    ApiError::ValidationFailed(errors).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> BTreeSet<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn a_declared_parameter_passes_and_an_undeclared_one_is_named_once() {
        let declared = names(&["limit", "offset"]);
        assert!(undeclared("limit=10&offset=0", &declared).is_empty());
        assert_eq!(
            undeclared("limt=10&limit=5&limt=3", &declared),
            ["unknown query parameter 'limt'; this route takes limit, offset"]
        );
        assert_eq!(
            undeclared("year=2024", &BTreeSet::new()),
            ["unknown query parameter 'year'; this route takes no query parameters"]
        );
    }
}
