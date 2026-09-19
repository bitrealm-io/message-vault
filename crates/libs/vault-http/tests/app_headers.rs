//! Once the desktop app has said which Build it is, every vault request
//! carries that Build and names the desktop app.
//!
//! This sits in a test binary of its own because `identify_desktop_app` sets a
//! value for the whole process.

use httpmock::prelude::*;
use reqwest::Method;
use vault_http::HttpSession;

#[test]
fn every_request_names_the_desktop_app_and_its_build() {
    vault_http::identify_desktop_app("0.9.0+343fe0d8");

    let server = MockServer::start();
    let named = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/session")
            .header("x-message-vault-app", "desktop")
            .header("x-message-vault-version", "0.9.0+343fe0d8");
        then.status(200);
    });

    let session = HttpSession::new().unwrap();
    session
        .vault_request(Method::GET, &server.base_url(), "/v1/session", "mv-user-k")
        .send()
        .unwrap();

    named.assert();
}
