//! Every operation in the OpenAPI document refuses a query parameter it does
//! not declare, and names the ones it does.
//!
//! `docs/architecture/http-api.md` ("Lists"): a typo such as `limt=10`, or a
//! guess at a convention the rules turn down (`order=`, `fields=`, `year=`),
//! would otherwise be answered as though it had been obeyed. The check walks
//! the in-process document, so a new route is covered the moment it is
//! registered, and it calls each route with no credential, because the
//! parameters are checked before anything else about the request.

use axum::http::StatusCode;
use serde_json::Value;

use crate::problem::ProblemType;

/// One operation: its method, its path, and the query parameters it declares.
struct Operation {
    method: String,
    path: String,
    declared: Vec<String>,
}

fn operations() -> Vec<Operation> {
    let doc: Value = serde_json::from_str(&super::dump_openapi_json()).unwrap();
    let mut operations = Vec::new();
    for (path, item) in doc["paths"].as_object().unwrap() {
        for (method, op) in item.as_object().unwrap() {
            if !["get", "put", "post", "delete", "patch", "head"].contains(&method.as_str()) {
                continue;
            }
            let declared = op["parameters"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|p| p["in"] == "query")
                .filter_map(|p| p["name"].as_str().map(str::to_string))
                .collect();
            operations.push(Operation {
                method: method.clone(),
                path: path.clone(),
                declared,
            });
        }
    }
    operations
}

/// The path with every `{segment}` filled with a value of the right shape.
/// The row need not exist: the parameters are refused before it is looked up.
fn concrete(path: &str) -> String {
    let mut out = String::new();
    let mut rest = path;
    while let Some(open) = rest.find('{') {
        let close = rest[open..].find('}').unwrap() + open;
        out.push_str(&rest[..open]);
        out.push_str(match &rest[open + 1..close] {
            "sha256" => "0000000000000000000000000000000000000000000000000000000000000000",
            "upload_id" => "upload",
            _ => "1",
        });
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out
}

#[tokio::test]
async fn every_route_refuses_a_query_parameter_it_does_not_declare() {
    let vault = crate::test_support::test_vault().await;
    let server = crate::test_support::serve(&vault.state).await;
    let client = reqwest::Client::new();
    let operations = operations();
    let mut mismatches = Vec::new();
    for op in &operations {
        let method = reqwest::Method::from_bytes(op.method.to_uppercase().as_bytes()).unwrap();
        let url = format!("{}{}?not_a_parameter=1", server.base(), concrete(&op.path));
        let response = client.request(method, url).send().await.unwrap();
        let status = response.status();
        let text = response.text().await.unwrap();
        let label = format!("{} {}", op.method.to_uppercase(), op.path);
        if status != StatusCode::UNPROCESSABLE_ENTITY {
            mismatches.push(format!("{label}: answered {status}"));
            continue;
        }
        // A HEAD answer has no body to read.
        if op.method == "head" {
            continue;
        }
        let problem = crate::test_support::problem(&text);
        let errors = problem.errors.unwrap_or_default().join(" ");
        if problem.kind != ProblemType::ValidationFailed.url()
            || !errors.contains("not_a_parameter")
            || !op
                .declared
                .iter()
                .all(|name| errors.contains(name.as_str()))
        {
            mismatches.push(format!(
                "{label}: does not refuse naming {:?}: {text}",
                op.declared
            ));
        }
    }
    assert!(
        operations.len() >= 80,
        "walked only {} operations; is the document whole?",
        operations.len()
    );
    assert!(
        mismatches.is_empty(),
        "{} of {} operations accept a parameter they do not declare:\n{}",
        mismatches.len(),
        operations.len(),
        mismatches.join("\n")
    );
}
