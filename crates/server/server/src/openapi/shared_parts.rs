//! The parts of the reference no handler writes by hand
//! (`docs/architecture/http-api.md`, "The reference").
//!
//! An operation's error responses are built here from what it takes: the
//! credential brings `401` and `403`, a body brings `400`, `413`, `415` and
//! `422`, an id in the path brings `404` and `422`, and every `/v1` route
//! answers `422` to a query parameter it does not declare. A handler names
//! only the problem types that are its own, through
//! [`crate::problem::openapi`], and this files each under its status. Every
//! failure then has one response, declared as `application/problem+json`,
//! whose description names the types it can carry.
//!
//! utoipa makes the first paragraph of a handler's doc comment the summary;
//! the rule is the first sentence, so the rest of that paragraph moves to the
//! front of the description here.

use std::collections::BTreeMap;

use serde_json::Value;
use utoipa::openapi::extensions::ExtensionsBuilder;
use utoipa::openapi::path::{Operation, ParameterIn, PathItem};
use utoipa::openapi::{Content, OpenApi, Ref, RefOr, Response, ResponseBuilder};

use crate::problem::openapi::KEY_PREFIX;
use crate::problem::{Problem, ProblemType};

/// The response extension listing the `type` URLs a failure can carry.
pub(crate) const PROBLEM_TYPES: &str = "x-problem-types";

/// Finish every operation of the assembled document.
///
/// # Panics
///
/// When a handler writes an error status out by hand, or names a problem
/// type the registry does not have. Both are mistakes in the source, found
/// the first time the document is assembled, which every test run does.
pub(crate) fn apply(spec: &mut OpenApi) {
    for (path, item) in &mut spec.paths.paths {
        for op in operations_mut(item) {
            one_sentence_summary(op);
            failures(path, op);
        }
    }
}

fn operations_mut(item: &mut PathItem) -> impl Iterator<Item = &mut Operation> {
    [
        &mut item.get,
        &mut item.put,
        &mut item.post,
        &mut item.delete,
        &mut item.options,
        &mut item.head,
        &mut item.patch,
        &mut item.trace,
    ]
    .into_iter()
    .filter_map(Option::as_mut)
}

/// Cut the summary to its first sentence and put the rest in front of the
/// description.
fn one_sentence_summary(op: &mut Operation) {
    let Some(summary) = op.summary.take() else {
        return;
    };
    let flat = summary.split_whitespace().collect::<Vec<_>>().join(" ");
    let (first, rest) = split_first_sentence(&flat);
    op.summary = Some(first.to_string());
    let description = op.description.take().filter(|d| !d.trim().is_empty());
    op.description = match (rest.is_empty(), description) {
        (true, description) => description,
        (false, None) => Some(rest.to_string()),
        (false, Some(description)) => Some(format!("{rest}\n\n{description}")),
    };
}

/// The first sentence of `text` and what follows it. A sentence ends at a
/// full stop followed by a space, outside backticks, so a path or a file name
/// in code (`docs/adr/0008-….md`) never ends one.
pub(crate) fn split_first_sentence(text: &str) -> (&str, &str) {
    let mut in_code = false;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        match c {
            '`' => in_code = !in_code,
            '.' if !in_code && chars.peek().is_some_and(|(_, next)| next.is_whitespace()) => {
                return (&text[..=i], text[i + 1..].trim_start());
            }
            _ => {}
        }
    }
    (text, "")
}

/// Replace the operation's failures with the ones its shape and its handler
/// give it, one problem response per status.
fn failures(path: &str, op: &mut Operation) {
    let responses = &mut op.responses.responses;
    let mut kinds: Vec<ProblemType> = Vec::new();
    let named: Vec<String> = responses
        .keys()
        .filter(|key| key.starts_with(KEY_PREFIX))
        .cloned()
        .collect();
    for key in named {
        responses.remove(&key);
        let slug = &key[KEY_PREFIX.len()..];
        let kind = ProblemType::ALL
            .into_iter()
            .find(|kind| kind.slug() == slug)
            .unwrap_or_else(|| panic!("{path} names an unregistered problem type {slug}"));
        kinds.push(kind);
    }
    if let Some(status) = responses
        .keys()
        .find(|status| status.parse::<u16>().is_ok_and(|s| s >= 400))
    {
        panic!(
            "{path} writes its {status} out by hand; name the problem type with \
             crate::problem::openapi instead, and let the shared parts bring the rest"
        );
    }

    if path.starts_with("/v1/") {
        // A query parameter the route does not declare.
        kinds.push(ProblemType::ValidationFailed);
    }
    let security = serde_json::to_value(&op.security).unwrap_or(Value::Null);
    let requirements = security.as_array().map(Vec::as_slice).unwrap_or_default();
    if requirements
        .iter()
        .any(|r| r.as_object().is_some_and(|r| !r.is_empty()))
    {
        kinds.extend([
            ProblemType::AuthenticationRequired,
            ProblemType::InsufficientScope,
            ProblemType::AccountDisabled,
        ]);
        if requirements.iter().any(|r| {
            r["session"]
                .as_array()
                .is_some_and(|s| s.iter().any(|s| s == "owner"))
        }) {
            kinds.push(ProblemType::NotTheOwner);
        }
    }
    if op.request_body.is_some() {
        kinds.extend([
            ProblemType::MalformedBody,
            ProblemType::PayloadTooLarge,
            ProblemType::UnsupportedMediaType,
            ProblemType::ValidationFailed,
        ]);
    }
    if op
        .parameters
        .iter()
        .flatten()
        .any(|p| matches!(p.parameter_in, ParameterIn::Path))
    {
        kinds.extend([ProblemType::NotFound, ProblemType::ValidationFailed]);
    }

    let mut by_status: BTreeMap<u16, Vec<ProblemType>> = BTreeMap::new();
    for kind in ProblemType::ALL {
        if kinds.contains(&kind) {
            by_status
                .entry(kind.status().as_u16())
                .or_default()
                .push(kind);
        }
    }
    for (status, kinds) in by_status {
        responses.insert(status.to_string(), RefOr::T(problem_response(&kinds)));
    }
}

/// The one failure response: a problem document of one of `kinds`, each
/// named and linked to its page in the description.
fn problem_response(kinds: &[ProblemType]) -> Response {
    let description = kinds
        .iter()
        .map(|kind| {
            let page = kind.page();
            let first_paragraph = page.split("\n\n").next().unwrap_or_default();
            let flat = first_paragraph
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            format!(
                "[`{}`]({}): {}",
                kind.slug(),
                kind.url(),
                split_first_sentence(&flat).0
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let urls: Vec<String> = kinds.iter().map(|kind| kind.url()).collect();
    ResponseBuilder::new()
        .description(description)
        .content(
            Problem::CONTENT_TYPE,
            Content::new(Some(RefOr::Ref(Ref::from_schema_name("Problem")))),
        )
        .extensions(Some(
            ExtensionsBuilder::new()
                .add(PROBLEM_TYPES, Value::from(urls))
                .build(),
        ))
        .build()
}

#[cfg(test)]
mod tests {
    use super::split_first_sentence;

    #[test]
    fn a_sentence_ends_at_a_full_stop_and_a_space_outside_code() {
        assert_eq!(
            split_first_sentence("Start a run. Finish it at `POST /v1/x.y` later."),
            ("Start a run.", "Finish it at `POST /v1/x.y` later.")
        );
        assert_eq!(
            split_first_sentence("See `a. b` first. Then more."),
            ("See `a. b` first.", "Then more.")
        );
        assert_eq!(split_first_sentence("One only."), ("One only.", ""));
    }
}
