//! Every rule of `docs/architecture/http-api.md` that can be checked by
//! walking the OpenAPI document, checked over every operation in it
//! ("The reference": a rule checked one route at a time is checked on the
//! routes someone remembered).
//!
//! Part of it reads the document: the page shape and paging parameters of
//! every list, a `Location` on every `201`, a problem response on every
//! failure, the shared failures each operation's shape brings, one-sentence
//! summaries, declared tags, kebab-case paths and the nesting depth. The rest
//! calls every operation through the router, on the credential matrix's
//! fixture: with no credential it must answer `401`, with a query parameter
//! it does not declare `422`, and a list with `limit`, or an `offset` past
//! the ceiling its description states, out of range `422`,
//! each as a problem document carrying its `request_id`. Like the matrix,
//! it walks the in-process document, so a new route is covered the moment
//! it is registered.

use std::collections::BTreeSet;

use axum::http::StatusCode;
use serde_json::Value;

use super::credential_matrix::{self, Credential, Operation, Shared, World};
use super::dump_openapi_json;
use super::shared_parts::{PROBLEM_TYPES, split_first_sentence};
use crate::paging::MAX_LIST_OFFSET;
use crate::problem::{Problem, ProblemType};

/// The one route nested three deep (`docs/architecture/http-api.md`,
/// "Naming a route").
const MULTIPART_PART: &str = "/v1/assets/{sha256}/uploads/{upload_id}/parts/{part}";

/// The four keys of every page.
const PAGE_KEYS: [&str; 4] = ["items", "total", "limit", "offset"];

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_operation_keeps_the_rules_the_document_can_show() {
    let doc: Value = serde_json::from_str(&dump_openapi_json()).unwrap();
    let operations = credential_matrix::operations();
    let mut broken: Vec<String> = Vec::new();
    for op in &operations {
        let spec = &doc["paths"][&op.path][&op.method];
        for rule in read_rules(&doc, op, spec) {
            broken.push(format!("{}: {rule}", op.label()));
        }
    }

    let shared = Shared::build().await;
    let world = World::build(&shared, 0).await;
    for op in &operations {
        let spec = &doc["paths"][&op.path][&op.method];
        for rule in called_rules(&world, op, spec).await {
            broken.push(format!("{}: {rule}", op.label()));
        }
    }

    assert!(
        operations.len() >= 80,
        "walked only {} operations; is the document whole?",
        operations.len()
    );
    assert!(
        broken.is_empty(),
        "{} rules broken:\n{}",
        broken.len(),
        broken.join("\n")
    );
}

/// The rules one operation breaks on paper.
fn read_rules(doc: &Value, op: &Operation, spec: &Value) -> Vec<String> {
    let mut broken = Vec::new();

    let segments: Vec<&str> = op.path.trim_start_matches('/').split('/').collect();
    for segment in &segments {
        if !segment.starts_with('{') && !is_kebab(segment) {
            broken.push(format!("path segment {segment} is not kebab-case"));
        }
    }
    let depth = segments.iter().filter(|s| s.starts_with('{')).count();
    if depth > 2 && op.path != MULTIPART_PART {
        broken.push(format!("nested {depth} deep; the rule is at most two"));
    }

    let summary = spec["summary"].as_str().unwrap_or_default();
    if summary.is_empty() {
        broken.push("no summary".to_string());
    } else if !split_first_sentence(summary).1.is_empty() {
        broken.push(format!("summary is more than one sentence: {summary}"));
    }
    let declared_tags: BTreeSet<&str> = doc["tags"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t["name"].as_str())
        .collect();
    for tag in spec["tags"].as_array().into_iter().flatten() {
        let tag = tag.as_str().unwrap_or_default();
        if !declared_tags.contains(tag) {
            broken.push(format!("tag {tag} is not declared"));
        }
    }

    let responses = spec["responses"].as_object().cloned().unwrap_or_default();
    if responses.contains_key("201") && spec["responses"]["201"]["headers"]["Location"].is_null() {
        broken.push("201 without a Location header".to_string());
    }
    for (status, response) in &responses {
        let Ok(code) = status.parse::<u16>() else {
            broken.push(format!("response {status} is not a status"));
            continue;
        };
        if code >= 400 {
            broken.extend(failure_rules(code, response));
        }
    }

    let has = |status: &str| responses.contains_key(status);
    let mut needs: Vec<(&str, &str)> = Vec::new();
    if takes_a_credential_only(op) {
        needs.push(("401", "a credential"));
    }
    if op.path.starts_with("/v1/") {
        needs.push(("422", "a query parameter it does not declare"));
    }
    if !spec["requestBody"].is_null() {
        needs.extend([("415", "a body"), ("422", "a body")]);
    }
    if op.path.contains('{') {
        needs.extend([("404", "an id in the path"), ("422", "an id in the path")]);
    }
    for (status, why) in needs {
        if !has(status) {
            broken.push(format!("no {status}, which {why} brings"));
        }
    }

    if let Some(page) = page_schema(doc, spec) {
        let required: BTreeSet<&str> = page["required"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        for key in PAGE_KEYS {
            if !required.contains(key) {
                broken.push(format!("the page it answers has no required {key}"));
            }
        }
        // A POST that reads the rows its body names answers the whole body
        // as one page, and takes no paging ("Lists").
        if op.method == "get" {
            let query = query_parameters(spec);
            for key in ["limit", "offset"] {
                if !query.contains(key) {
                    broken.push(format!("a list that does not take {key}"));
                }
            }
        }
    }
    broken
}

/// The rules one failure response breaks: a problem document, named types
/// registered for this status, and a description.
fn failure_rules(code: u16, response: &Value) -> Vec<String> {
    let mut broken = Vec::new();
    let content: Vec<&String> = response["content"]
        .as_object()
        .map(|c| c.keys().collect())
        .unwrap_or_default();
    if content != [Problem::CONTENT_TYPE] {
        broken.push(format!("{code} is served as {content:?}, not a problem"));
    }
    if response["content"][Problem::CONTENT_TYPE]["schema"]["$ref"]
        != "#/components/schemas/Problem"
    {
        broken.push(format!("{code} does not describe its body as Problem"));
    }
    if response["description"]
        .as_str()
        .unwrap_or_default()
        .is_empty()
    {
        broken.push(format!("{code} has no description"));
    }
    let types: Vec<&str> = response[PROBLEM_TYPES]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if types.is_empty() {
        broken.push(format!("{code} names no problem type"));
    }
    for url in types {
        match ProblemType::ALL.into_iter().find(|t| t.url() == url) {
            None => broken.push(format!("{code} names an unregistered type {url}")),
            Some(kind) if kind.status().as_u16() != code => broken.push(format!(
                "{code} names {}, which answers {}",
                kind.slug(),
                kind.status().as_u16()
            )),
            Some(_) => {}
        }
    }
    broken
}

/// The rules one operation breaks when called.
async fn called_rules(world: &World<'_>, op: &Operation, spec: &Value) -> Vec<String> {
    let mut broken = Vec::new();
    if !op.path.starts_with("/v1/") {
        return broken;
    }
    let path = world.path_for(op);
    let with = |query: &str| {
        let joiner = if path.contains('?') { '&' } else { '?' };
        format!("{path}{joiner}{query}")
    };

    let answer = call(world, op, &with("no_such_parameter=1"), None).await;
    broken.extend(answer.problem_rule(op, ProblemType::ValidationFailed, "an unknown parameter"));

    if takes_a_credential_only(op) {
        let answer = call(world, op, &path, None).await;
        broken.extend(answer.problem_rule(
            op,
            ProblemType::AuthenticationRequired,
            "no credential",
        ));
    }

    if op.method == "get" && page_schema_named(spec).is_some() {
        let credential = if op.names_owner() {
            Credential::Owner
        } else {
            Credential::Session
        };
        let token = world.token(credential).to_string();
        let mut out_of_range = vec!["limit=0", "limit=501"];
        // A browse list says its offset ceiling in the parameter's own
        // description, and must keep to it.
        if offset_description(spec).contains(&MAX_LIST_OFFSET.to_string()) {
            out_of_range.push("offset=50001");
        }
        for query in out_of_range {
            let answer = call(world, op, &with(query), Some(&token)).await;
            broken.extend(answer.problem_rule(op, ProblemType::ValidationFailed, query));
        }
    }
    broken
}

/// A response, read whole.
struct Answer {
    status: StatusCode,
    content_type: String,
    text: String,
}

impl Answer {
    /// What is wrong with this answer as the problem `kind`, if anything.
    fn problem_rule(&self, op: &Operation, kind: ProblemType, asked: &str) -> Option<String> {
        if self.status != kind.status() {
            return Some(format!(
                "{asked} answered {}, not {} {}: {}",
                self.status.as_u16(),
                kind.status().as_u16(),
                kind.slug(),
                self.text
            ));
        }
        // A HEAD answer has no body to be a problem document.
        if op.method == "head" {
            return None;
        }
        if self.content_type != Problem::CONTENT_TYPE {
            return Some(format!("{asked} answered as {}", self.content_type));
        }
        match serde_json::from_str::<Problem>(&self.text) {
            Err(e) => Some(format!(
                "{asked} answered a body that is not a problem ({e})"
            )),
            Ok(problem) if problem.kind != kind.url() => {
                Some(format!("{asked} answered the type {}", problem.kind))
            }
            Ok(problem) if problem.request_id.is_none() => {
                Some(format!("{asked} answered a problem with no request_id"))
            }
            Ok(_) => None,
        }
    }
}

/// Call `op` at `path` with `token`, or no credential, sending the body
/// that gets it past its own validation.
async fn call(world: &World<'_>, op: &Operation, path: &str, token: Option<&str>) -> Answer {
    let method = reqwest::Method::from_bytes(op.method.to_uppercase().as_bytes()).unwrap();
    let mut request = reqwest::Client::new().request(method, world.url(path));
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    if let Some((content_type, body)) = credential_matrix::body_for(op, 0) {
        request = request
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .body(body);
    }
    let response = request.send().await.unwrap();
    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let text = response.text().await.unwrap_or_default();
    Answer {
        status,
        content_type,
        text,
    }
}

/// Whether the operation takes a credential and admits no request without
/// one. `POST /v1/accounts` admits a stranger, so it has no such refusal.
fn takes_a_credential_only(op: &Operation) -> bool {
    op.security.as_ref().is_some_and(|requirements| {
        !requirements.is_empty()
            && requirements
                .iter()
                .all(|r| r.as_object().is_some_and(|r| !r.is_empty()))
    })
}

/// Lowercase words of letters and digits joined by single hyphens.
fn is_kebab(segment: &str) -> bool {
    !segment.is_empty()
        && segment.split('-').all(|word| {
            !word.is_empty()
                && word
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

/// The name of the page schema a `200` answers, if it answers a page.
fn page_schema_named(spec: &Value) -> Option<&str> {
    spec["responses"]["200"]["content"]["application/json"]["schema"]["$ref"]
        .as_str()
        .and_then(|r| r.strip_prefix("#/components/schemas/"))
        .filter(|name| name.starts_with("Page_"))
}

/// The page schema a `200` answers, if it answers a page.
fn page_schema<'d>(doc: &'d Value, spec: &Value) -> Option<&'d Value> {
    page_schema_named(spec).map(|name| &doc["components"]["schemas"][name])
}

/// What the operation says about its `offset`.
fn offset_description(spec: &Value) -> &str {
    spec["parameters"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|p| p["in"] == "query" && p["name"] == "offset")
        .and_then(|p| p["description"].as_str())
        .unwrap_or_default()
}

/// The names of the query parameters an operation declares.
fn query_parameters(spec: &Value) -> BTreeSet<&str> {
    spec["parameters"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["in"] == "query")
        .filter_map(|p| p["name"].as_str())
        .collect()
}
