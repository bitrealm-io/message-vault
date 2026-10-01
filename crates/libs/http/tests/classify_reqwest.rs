//! reqwest-status classification needs a real response: build an error via
//! `error_for_status()` against a local mock server — or a refused connection
//! for a statusless transport error.

use httpmock::prelude::*;
use message_crate_http::{RetryKind, classify_retry};

#[test]
fn reqwest_4xx_status_is_permanent_5xx_is_transient() {
    let server = MockServer::start();
    let mock_404 = server.mock(|when, then| {
        when.method(GET).path("/missing");
        then.status(404);
    });
    let mock_500 = server.mock(|when, then| {
        when.method(GET).path("/broken");
        then.status(500);
    });

    let client = reqwest::blocking::Client::new();
    let e404 = client
        .get(server.url("/missing"))
        .send()
        .and_then(|r| r.error_for_status())
        .unwrap_err();
    assert_eq!(
        classify_retry(&anyhow::Error::new(e404)),
        RetryKind::Permanent
    );

    let e500 = client
        .get(server.url("/broken"))
        .send()
        .and_then(|r| r.error_for_status())
        .unwrap_err();
    assert_eq!(
        classify_retry(&anyhow::Error::new(e500)),
        RetryKind::Transient
    );

    mock_404.assert();
    mock_500.assert();
}

#[test]
fn statusless_connect_error_is_transient() {
    // Port 1 refuses connections, so the send fails with a transport error
    // that carries no HTTP status — the statusless reqwest path, which
    // classify_retry treats as transient.
    let client = reqwest::blocking::Client::new();
    let e = client.get("http://127.0.0.1:1/").send().unwrap_err();
    assert!(e.status().is_none());
    assert_eq!(classify_retry(&anyhow::Error::new(e)), RetryKind::Transient);
}
