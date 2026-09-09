//! `auth_check` is the desktop app's sign-in, and every branch in it produces a
//! different message for the person typing the URL: "that is the wrong host",
//! "that key is not valid", "the vault is rate limiting you". Nothing exercised
//! the function itself before — the unit tests reach the classifiers directly,
//! so the wiring between the response and the classifier was untested, and a
//! change that answered `invalid_key` to every failure would have passed them
//! all. These drive the real function over HTTP against a local mock.

use httpmock::prelude::*;
use vault_http::auth_check;

/// Serve one body and status at `GET /v1/session`, then call `auth_check`.
fn check_against(status: u16, body: &str) -> Result<vault_http::AuthInfo, vault_http::AuthError> {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/v1/session");
        then.status(status).body(body);
    });
    auth_check(&server.base_url(), "mv-user-testkey")
}

/// The vault answers, and the fields the clients read come back.
#[test]
fn a_session_body_becomes_the_account_it_names() {
    let info = check_against(200, r#"{"account_id": 42, "username": "alice"}"#)
        .expect("a well-formed session must be accepted");
    assert_eq!(info.account_id, 42);
    assert_eq!(info.username.as_deref(), Some("alice"));
}

/// A username is optional — an account that has not finished profile setup has
/// none — but an account id is not, because every later call is made on its
/// behalf.
#[test]
fn a_session_without_an_account_id_is_refused() {
    let info = check_against(200, r#"{"account_id": 7}"#).expect("a missing username is allowed");
    assert_eq!(info.account_id, 7);
    assert_eq!(info.username, None);

    let err = check_against(200, r#"{"username": "alice"}"#)
        .expect_err("a session with no account id is not a session");
    assert_eq!(err.kind(), "missing_account");
}

/// The wrong-host case, which is the most common mistake made at this screen:
/// the URL points at a web server or a proxy rather than at a vault, and the
/// answer is an HTML page. Reporting that as bad JSON tells the reader nothing.
///
/// The lowercase `<!doctype html>` is deliberate: it is what the HTML5
/// specification writes and what nginx and Cloudflare emit, and matching only
/// the uppercase spelling used to miss it.
#[test]
fn an_html_page_means_the_url_points_at_the_wrong_host() {
    for page in [
        "<!doctype html><html><body>502 Bad Gateway</body></html>",
        "<!DOCTYPE html><html><body>It works!</body></html>",
        "<html><head><title>nginx</title></head></html>",
    ] {
        let err = check_against(200, page).expect_err("an HTML body is not a session");
        assert_eq!(err.kind(), "wrong_host", "for page: {page}");
    }

    // An HTML page served with an error status is still the wrong host, and
    // must not be reported as that status instead.
    let err = check_against(502, "<!doctype html><html>bad gateway</html>")
        .expect_err("an HTML body is not a session");
    assert_eq!(err.kind(), "wrong_host");
}

/// Each status the vault can answer maps to its own error, because each one
/// asks the reader to do something different. Deleting any arm of that mapping
/// used to change nothing that any test could see.
#[test]
fn each_failing_status_keeps_its_own_meaning() {
    for (status, kind) in [
        (401, "invalid_key"),
        (403, "forbidden"),
        (404, "api_not_found"),
        (429, "rate_limited"),
        (500, "server_error"),
        (503, "server_error"),
        (418, "http_status"),
    ] {
        let err = check_against(status, r#"{"detail": "no"}"#)
            .expect_err("a failing status must not be accepted");
        assert_eq!(err.kind(), kind, "status {status}");
    }
}

/// A 200 that is neither HTML nor a session document is bad JSON, and the
/// message says so rather than blaming the key.
#[test]
fn a_body_that_is_not_json_is_reported_as_such() {
    let err = check_against(200, "not json at all").expect_err("garbage is not a session");
    assert_eq!(err.kind(), "bad_json");
}

/// A URL that cannot be parsed never reaches the network.
#[test]
fn an_unparsable_url_is_refused_before_the_request() {
    let err = auth_check("not a url", "mv-user-testkey").expect_err("that is not a URL");
    assert_eq!(err.kind(), "invalid_url");
}

/// Nothing is listening, so the failure is a network failure and not a
/// rejection by a vault. Port 1 refuses connections.
#[test]
fn an_unreachable_vault_is_a_network_failure() {
    let err = auth_check("http://127.0.0.1:1", "mv-user-testkey")
        .expect_err("nothing is listening on port 1");
    assert_eq!(err.kind(), "network");
}
