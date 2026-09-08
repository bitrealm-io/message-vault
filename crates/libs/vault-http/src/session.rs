//! The shared blocking HTTP session and the `GET /v1/session` login call.
//!
//! `vault-push` and `vault-pull` both talk to the vault through one
//! [`HttpSession`]. The session owns base-URL trimming and bearer-header
//! construction so no caller formats `Authorization` by hand.

use std::time::Duration;

use anyhow::Result;
use reqwest::Method;
use reqwest::blocking::{Client, RequestBuilder};
use serde::Deserialize;

use crate::{AuthError, AuthInfo, truncate};

/// Blocking HTTP client shared by every vault call in one run.
#[derive(Debug, Clone)]
pub struct HttpSession {
    client: Client,
}

/// `Authorization` header value for a vault API key.
pub fn bearer_header(key: &str) -> String {
    format!("Bearer {}", key.trim())
}

/// `base_url` with surrounding whitespace and trailing slashes removed.
pub fn trim_base_url(base_url: &str) -> &str {
    base_url.trim().trim_end_matches('/')
}

impl HttpSession {
    /// Blocking HTTP client with a connection pool for worker threads.
    ///
    /// # Errors
    ///
    /// Returns an error when the reqwest client cannot be built.
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: crate::build_client()?,
        })
    }

    /// Start a request to `{base_url}{path}` with the bearer header set.
    ///
    /// `path` must start with `/`. Append query parameters with
    /// [`RequestBuilder::query`], which percent-encodes values.
    pub fn vault_request(
        &self,
        method: Method,
        base_url: &str,
        path: &str,
        key: &str,
    ) -> RequestBuilder {
        let base = trim_base_url(base_url);
        self.client
            .request(method, format!("{base}{path}"))
            .header("Authorization", bearer_header(key))
    }

    /// Start a request to an already-built URL with the bearer header set.
    pub fn request_url(&self, method: Method, url: reqwest::Url, key: &str) -> RequestBuilder {
        self.client
            .request(method, url)
            .header("Authorization", bearer_header(key))
    }

    /// Call `GET /v1/session` and return the account id on success.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError`] when the URL is invalid, the host is unreachable,
    /// or the key is rejected.
    pub fn auth_check(
        &self,
        base_url: &str,
        key: &str,
    ) -> std::result::Result<AuthInfo, AuthError> {
        // The token alone names the account; the reply carries the username.
        let base = trim_base_url(base_url);
        let parsed_base = match reqwest::Url::parse(base) {
            Ok(parsed) => parsed,
            Err(error) => {
                return Err(AuthError::InvalidUrl {
                    url: base.to_string(),
                    detail: error.to_string(),
                });
            }
        };
        let url = format!("{base}/v1/session");
        let response = self
            .vault_request(Method::GET, base, "/v1/session", key)
            .timeout(Duration::from_secs(15))
            .send()
            .map_err(|error| classify_auth_transport_error(&url, error))?;
        let status = response.status();
        let status_code = status.as_u16();
        // After redirects, reqwest reports the final URL. http→https drops Authorization.
        let final_url = response.url().clone();
        let text = response.text().map_err(|error| AuthError::ReadResponse {
            detail: error.to_string(),
        })?;
        if looks_like_html(&text) {
            return Err(AuthError::WrongHostHtml {
                url,
                status: status_code,
            });
        }
        if status_code == 401 {
            return Err(classify_unauthorized(base, &parsed_base, &final_url));
        }
        if !status.is_success() {
            return Err(classify_auth_http_status(status_code, text));
        }
        let parsed: SessionResponse =
            serde_json::from_str(&text).map_err(|_| AuthError::BadJson {
                url: url.clone(),
                status: status_code,
                snippet: truncate(&text, 200),
            })?;
        let account_id = parsed
            .account_id
            .filter(|s| !s.is_empty())
            .ok_or(AuthError::MissingAccountId)?;
        Ok(AuthInfo {
            account_id,
            username: parsed.username,
        })
    }
}

/// The fields of `GET /v1/session` the clients read.
#[derive(Debug, Deserialize)]
struct SessionResponse {
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    username: Option<String>,
}

/// True when the body looks like an HTML error page instead of JSON.
pub fn looks_like_html(body: &str) -> bool {
    let t = body.trim_start();
    t.starts_with("<!DOCTYPE") || t.starts_with("<html") || t.starts_with("<HTML")
}

/// Map HTTP 401. When `http://` was redirected to `https://`, the API key was
/// dropped with the Authorization header — tell the user to use https.
fn classify_unauthorized(
    requested_base: &str,
    requested_url: &reqwest::Url,
    final_url: &reqwest::Url,
) -> AuthError {
    if requested_url.scheme() == "http" && final_url.scheme() == "https" {
        AuthError::HttpsRequired {
            url: requested_base.to_string(),
        }
    } else {
        AuthError::InvalidKey
    }
}

/// Map a reqwest transport failure (timeout, connect, TLS) onto [`AuthError`].
fn classify_auth_transport_error(url: &str, error: reqwest::Error) -> AuthError {
    let url = url.to_string();
    let detail = error.to_string();
    if error.is_timeout() {
        AuthError::Timeout { url, detail }
    } else if error.is_builder() {
        AuthError::InvalidUrl { url, detail }
    } else {
        // Connection refused, DNS failure, and anything else unrecognized all
        // mean "could not reach the vault".
        AuthError::Network { url, detail }
    }
}

/// Map a non-success HTTP status from `GET /v1/session` onto [`AuthError`].
fn classify_auth_http_status(status: u16, body: String) -> AuthError {
    match status {
        403 => AuthError::Forbidden { status, body },
        404 => AuthError::ApiNotFound { status, body },
        429 => AuthError::RateLimited { status, body },
        500..=599 => AuthError::ServerError { status, body },
        _ => AuthError::HttpStatus { status, body },
    }
}

/// Build a session and call [`HttpSession::auth_check`].
///
/// # Errors
///
/// Returns [`AuthError`] when the client cannot be built or login fails.
pub fn auth_check(base_url: &str, key: &str) -> std::result::Result<AuthInfo, AuthError> {
    let session = HttpSession::new().map_err(|error| AuthError::Client {
        detail: format!("{error:#}"),
    })?;
    session.auth_check(base_url, key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unauthorized_http_to_https_redirect_asks_for_https() {
        let requested = reqwest::Url::parse("http://app.bitrealm.io").unwrap();
        let final_url = reqwest::Url::parse("https://app.bitrealm.io/v1/session").unwrap();
        let err = classify_unauthorized("http://app.bitrealm.io", &requested, &final_url);
        assert_eq!(err.kind(), "https_required");
        assert!(err.user_message().contains("https://"));
        assert!(err.detail().contains("Authorization"));
    }

    #[test]
    fn unauthorized_same_scheme_is_invalid_key() {
        let requested = reqwest::Url::parse("https://app.bitrealm.io").unwrap();
        let final_url = reqwest::Url::parse("https://app.bitrealm.io/v1/session").unwrap();
        let err = classify_unauthorized("https://app.bitrealm.io", &requested, &final_url);
        assert_eq!(err.kind(), "invalid_key");
    }

    #[test]
    fn unauthorized_local_http_is_invalid_key() {
        let requested = reqwest::Url::parse("http://127.0.0.1:8080").unwrap();
        let final_url = reqwest::Url::parse("http://127.0.0.1:8080/v1/session").unwrap();
        let err = classify_unauthorized("http://127.0.0.1:8080", &requested, &final_url);
        assert_eq!(err.kind(), "invalid_key");
    }

    #[test]
    fn bearer_header_trims_the_key() {
        assert_eq!(bearer_header("  mv_key \n"), "Bearer mv_key");
    }

    #[test]
    fn trim_base_url_removes_whitespace_and_trailing_slashes() {
        assert_eq!(
            trim_base_url("  http://127.0.0.1:8080// "),
            "http://127.0.0.1:8080"
        );
    }
}
