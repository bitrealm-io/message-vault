//! Every rule of `docs/architecture/http-api.md` that can be checked by
//! walking the OpenAPI document, checked over every operation in it
//! ("The reference": a rule checked one route at a time is checked on the
//! routes someone remembered).
//!
//! Part of it reads the document: the page shape and paging parameters of
//! every list, a `Location` on every `201`, a `404` on every path with an id,
//! no body on the success of a `HEAD`,
//! one-sentence summaries, declared tags, kebab-case paths and the nesting
//! depth. The rest calls every operation through the router, on the
//! credential matrix's fixture: with no credential it must answer `401`,
//! with a query parameter it does not declare `422`, with a body that has no
//! `Content-Type` or one the route does not take `415`, with a JSON body
//! that is not JSON `400`, and a list with
//! `limit`, or an `offset` past the ceiling its description states, out of
//! range `422`. Each answer must be a problem document carrying its
//! `request_id`, of a status and type the operation's document lists. An
//! `Accept` that names nothing JSON must answer `406` exactly where the
//! document lists it; a `GET` that succeeds must answer a media type its
//! document declares; and a `201` must name in its `Location` a resource the
//! same credential can `GET`.
//!
//! The failures an operation's shape brings are written into the document
//! by `shared_parts`, so a check that reads them back from the document
//! passes whatever `shared_parts` does. Calling the operation is what shows
//! the document says what the server answers. Like the matrix, it walks the
//! in-process document, so a new route is covered the moment it is
//! registered.

use std::collections::BTreeSet;

use axum::http::StatusCode;
use serde_json::Value;

use super::credential_matrix::{self, Operation, Shared, World};
use super::dump_openapi_json;
use super::shared_parts::{PROBLEM_TYPES, split_first_sentence};
use crate::paging::MAX_LIST_OFFSET;
use crate::problem::{Problem, ProblemType};

/// The one route nested three deep (`docs/architecture/http-api.md`,
/// "Naming a route").
const MULTIPART_PART: &str = "/v1/assets/{sha256}/uploads/{upload_id}/parts/{part}";

/// The creations whose `Location` names a path with no `GET` yet (#1486).
/// The list empties when that issue is fixed, and nothing is added to it.
const UNREADABLE_LOCATIONS: [&str; 3] = [
    "/v1/contact-groups",
    "/v1/message-tags",
    "/v1/saved-searches",
];

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

    // Each operation gets accounts and rows of its own, so a call the server
    // wrongly accepts (a delete, a logout) cannot change what the next
    // operation sees.
    let shared = Shared::build().await;
    for (n, op) in operations.iter().enumerate() {
        let world = World::build(&shared, n).await;
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
    // A failure keeps its problem document's media type, as every failure
    // does, and the HEAD answer carries the header without the body.
    if op.method == "head" {
        for (status, response) in &responses {
            if status.starts_with('2') && !response["content"].is_null() {
                broken.push(format!(
                    "a HEAD answer has no body, and its {status} declares one"
                ));
            }
        }
    }
    if responses.contains_key("201") && spec["responses"]["201"]["headers"]["Location"].is_null() {
        broken.push("201 without a Location header".to_string());
    }
    // `shared_parts` gives `404` to an operation that declares a path
    // parameter, so a handler that leaves its id out of `params(...)` gets
    // none, though the server answers `404` for an id that is not there.
    // This reads the path itself, which `shared_parts` does not.
    if op.path.contains('{') && !responses.contains_key("404") {
        broken.push("no 404, which an id in the path brings".to_string());
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

/// The rules one operation breaks when called. Each call puts the operation
/// in a condition the shared parts claim a failure for, and the status and
/// problem type the server answers must be ones the document lists for the
/// operation. Reading the document alone cannot show that, because the
/// document and any reading of it come from the same code in `shared_parts`.
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
    let fixture_body = credential_matrix::body_for(op, 0);
    let sent = || {
        fixture_body
            .clone()
            .map(|(content_type, body)| (Some(content_type), body))
    };

    let answer = call(world, op, &with("no_such_parameter=1"), None, sent()).await;
    broken.extend(answer.problem_rule(
        op,
        spec,
        ProblemType::ValidationFailed,
        "an unknown parameter",
    ));

    if takes_a_credential_only(op) {
        let answer = call(world, op, &path, None, sent()).await;
        broken.extend(answer.problem_rule(
            op,
            spec,
            ProblemType::AuthenticationRequired,
            "no credential",
        ));
    }

    // Every call below carries a credential the operation admits, so the
    // guard lets it through to the body and the query.
    let token = op.admitted().map(|c| world.token(c).to_string());
    let token = token.as_deref();
    if !spec["requestBody"].is_null() {
        let Some((content_type, body)) = &fixture_body else {
            broken.push("takes a body, and credential_matrix::body_for has none for it".into());
            return broken;
        };
        // An asset's bytes are sent with the asset's own media type, so only
        // a missing one is wrong there. Elsewhere `text/plain` is wrong, and
        // so is a missing type, unless the body is optional: there a body
        // with no type is read as no body.
        let wrong_types = if *content_type == "application/octet-stream" {
            vec![None]
        } else if spec["requestBody"]["required"] == true {
            vec![None, Some("text/plain")]
        } else {
            vec![Some("text/plain")]
        };
        for wrong in wrong_types {
            let answer = call(world, op, &path, token, Some((wrong, body.clone()))).await;
            let asked = match wrong {
                None => "a body with no Content-Type".to_string(),
                Some(wrong) => format!("a body sent as {wrong}"),
            };
            broken.extend(answer.problem_rule(op, spec, ProblemType::UnsupportedMediaType, &asked));
        }
        // A JSON body, or an import's JSON Lines, that is not JSON.
        if ["application/json", "application/x-ndjson"].contains(content_type) {
            let not_json = (Some(*content_type), b"not json\n".to_vec());
            let answer = call(world, op, &path, token, Some(not_json)).await;
            broken.extend(answer.problem_rule(
                op,
                spec,
                ProblemType::MalformedBody,
                "a body that is not JSON",
            ));
        }
    }

    // An `Accept` that names nothing JSON. A route that answers JSON refuses
    // it with `406`, which its document must list; a route that answers
    // bytes ignores it, and its document must not claim a `406` it never
    // gives.
    let answer = call_with(world, op, &path, token, sent(), &[("accept", "text/html")]).await;
    if answer.status == StatusCode::NOT_ACCEPTABLE || !spec["responses"]["406"].is_null() {
        broken.extend(answer.problem_rule(
            op,
            spec,
            ProblemType::NotAcceptable,
            "Accept: text/html",
        ));
    }

    // A read that succeeds answers in a media type its document declares.
    if op.method == "get" {
        let answer = call(world, op, &path, token, None).await;
        if answer.status == StatusCode::OK {
            broken.extend(answer.declared_media_type_rule(spec));
        }
    }

    if op.method == "get" && page_schema_named(spec).is_some() {
        let mut out_of_range = vec!["limit=0", "limit=501"];
        // A browse list says its offset ceiling in the parameter's own
        // description, and must keep to it.
        if offset_description(spec).contains(&MAX_LIST_OFFSET.to_string()) {
            out_of_range.push("offset=50001");
        }
        for query in out_of_range {
            let answer = call(world, op, &with(query), token, None).await;
            broken.extend(answer.problem_rule(op, spec, ProblemType::ValidationFailed, query));
        }
    }

    // A creation names the new resource in `Location`, and the credential
    // that made it can read it there ("Status codes"). Last, because it
    // makes something.
    if !spec["responses"]["201"].is_null()
        && token.is_some()
        && !UNREADABLE_LOCATIONS.contains(&op.path.as_str())
    {
        let answer = call(world, op, &path, token, sent()).await;
        if answer.status == StatusCode::CREATED {
            match &answer.location {
                None => broken.push("a 201 with no Location".to_string()),
                Some(location) => {
                    let read = Operation {
                        method: "get".to_string(),
                        path: location.clone(),
                        security: None,
                    };
                    let followed = call(world, &read, location, token, None).await;
                    if followed.status != StatusCode::OK {
                        broken.push(format!(
                            "the 201's Location {location} answered GET with {}: {}",
                            followed.status.as_u16(),
                            followed.text
                        ));
                    }
                }
            }
        }
    }
    broken
}

/// A response, read whole.
struct Answer {
    status: StatusCode,
    content_type: String,
    location: Option<String>,
    text: String,
}

impl Answer {
    /// What is wrong with this answer as the problem `kind`, if anything:
    /// another status or type, a body that is not a problem document, or a
    /// status and type the operation's document does not list.
    fn problem_rule(
        &self,
        op: &Operation,
        spec: &Value,
        kind: ProblemType,
        asked: &str,
    ) -> Option<String> {
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
        if op.method != "head" {
            if self.content_type != Problem::CONTENT_TYPE {
                return Some(format!("{asked} answered as {}", self.content_type));
            }
            match serde_json::from_str::<Problem>(&self.text) {
                Err(e) => {
                    return Some(format!(
                        "{asked} answered a body that is not a problem ({e})"
                    ));
                }
                Ok(problem) if problem.kind != kind.url() => {
                    return Some(format!("{asked} answered the type {}", problem.kind));
                }
                Ok(problem) if problem.request_id.is_none() => {
                    return Some(format!("{asked} answered a problem with no request_id"));
                }
                Ok(_) => {}
            }
        }
        let status = kind.status().as_u16().to_string();
        let response = &spec["responses"][&status];
        if response.is_null() {
            return Some(format!(
                "{asked} answered {status} {}, a status the document does not list",
                kind.slug()
            ));
        }
        let listed = response[PROBLEM_TYPES]
            .as_array()
            .is_some_and(|types| types.iter().any(|t| *t == kind.url()));
        if !listed {
            return Some(format!(
                "{asked} answered {status} {}, a type the document does not list under {status}",
                kind.slug()
            ));
        }
        None
    }
}

impl Answer {
    /// What is wrong with a `200` answer's media type, if anything: one the
    /// operation's document does not declare for its `200`. A declared
    /// range (`*/*`, `image/*`) covers every type in it.
    fn declared_media_type_rule(&self, spec: &Value) -> Option<String> {
        let answered = self
            .content_type
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        let declared: Vec<&String> = spec["responses"]["200"]["content"]
            .as_object()
            .map(|content| content.keys().collect())
            .unwrap_or_default();
        let covered = declared.iter().any(|range| match range.split_once('/') {
            Some(("*", "*")) => true,
            Some((kind, "*")) => answered.split('/').next() == Some(kind),
            _ => **range == answered,
        });
        (!covered).then(|| {
            format!("a 200 answered as {answered}, and the document declares {declared:?}")
        })
    }
}

/// Call `op` at `path` with `token`, or no credential, sending `body` with
/// its `Content-Type`, or with none, or no body.
async fn call(
    world: &World<'_>,
    op: &Operation,
    path: &str,
    token: Option<&str>,
    body: Option<(Option<&str>, Vec<u8>)>,
) -> Answer {
    call_with(world, op, path, token, body, &[]).await
}

/// [`call`], with `headers` added to the request.
async fn call_with(
    world: &World<'_>,
    op: &Operation,
    path: &str,
    token: Option<&str>,
    body: Option<(Option<&str>, Vec<u8>)>,
    headers: &[(&str, &str)],
) -> Answer {
    let method = reqwest::Method::from_bytes(op.method.to_uppercase().as_bytes()).unwrap();
    let mut request = reqwest::Client::new().request(method, world.url(path));
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    if let Some((content_type, body)) = body {
        if let Some(content_type) = content_type {
            request = request.header(reqwest::header::CONTENT_TYPE, content_type);
        }
        request = request.body(body);
    }
    let response = request.send().await.unwrap();
    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let text = response.text().await.unwrap_or_default();
    Answer {
        status,
        content_type,
        location,
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
