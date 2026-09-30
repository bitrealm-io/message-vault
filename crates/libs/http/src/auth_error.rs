//! Typed login failures from `GET /v1/session`.
//!
//! Each variant has a stable `kind()` string for tests and a short
//! `user_message()` for the desktop app banner.

/// Failure from `GET /v1/session`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    /// The server URL could not be parsed as a valid HTTP URL.
    #[error("invalid server address {url}: {detail}")]
    InvalidUrl {
        /// The server URL that failed to parse.
        url: String,
        /// Why the URL was rejected.
        detail: String,
    },
    /// The HTTP client could not be built (for example, TLS setup failed).
    #[error("build HTTP client: {detail}")]
    Client {
        /// The underlying client-build error.
        detail: String,
    },
    /// The server could not be reached over the network.
    #[error("GET {url}: {detail}")]
    Network {
        /// The endpoint that could not be reached.
        url: String,
        /// The underlying network error.
        detail: String,
    },
    /// The server did not respond within the request timeout.
    #[error("GET {url}: {detail}")]
    Timeout {
        /// The endpoint that timed out.
        url: String,
        /// The underlying timeout error.
        detail: String,
    },
    /// Connected, but the response body could not be read.
    #[error("read session body: {detail}")]
    ReadResponse {
        /// The underlying read error.
        detail: String,
    },
    /// The endpoint returned HTML instead of the Message Crate API.
    #[error(
        "GET /v1/session returned HTML from {url} (HTTP {status}). The server address must point at the Message Crate server (TLS site or port 8080), not the Next.js browse UI alone (port 3000)"
    )]
    WrongHostHtml {
        /// The endpoint that returned HTML.
        url: String,
        /// The HTTP status code returned.
        status: u16,
    },
    /// Requested `http://…` but the server redirected to `https://…` (auth header dropped).
    #[error(
        "server address {url} redirected from http to https; use https:// so the API key is sent (http redirects drop Authorization)"
    )]
    HttpsRequired {
        /// The `http://` URL that the server redirected to `https://`.
        url: String,
    },
    /// The API key was rejected as invalid.
    #[error("invalid API key")]
    InvalidKey,
    /// The API key does not have permission for this Message Crate.
    #[error("session check failed (HTTP {status}): {body}")]
    Forbidden {
        /// The HTTP status code returned.
        status: u16,
        /// The response body from the server.
        body: String,
    },
    /// The Message Crate API was not found at this URL.
    #[error("session check failed (HTTP {status}): {body}")]
    ApiNotFound {
        /// The HTTP status code returned.
        status: u16,
        /// The response body from the server.
        body: String,
    },
    /// The server rejected the request because it was rate limited.
    #[error("session check failed (HTTP {status}): {body}")]
    RateLimited {
        /// The HTTP status code returned.
        status: u16,
        /// The response body from the server.
        body: String,
    },
    /// The server failed while verifying the credentials.
    #[error("session check failed (HTTP {status}): {body}")]
    ServerError {
        /// The HTTP status code returned.
        status: u16,
        /// The response body from the server.
        body: String,
    },
    /// The server returned an unexpected HTTP status.
    #[error("session check failed (HTTP {status}): {body}")]
    HttpStatus {
        /// The HTTP status code returned.
        status: u16,
        /// The response body from the server.
        body: String,
    },
    /// The response body was not recognizable JSON.
    #[error("parse session JSON from {url} (HTTP {status}): {snippet}")]
    BadJson {
        /// The endpoint whose response could not be parsed.
        url: String,
        /// The HTTP status code returned.
        status: u16,
        /// A short excerpt of the unparseable response body.
        snippet: String,
    },
    /// The server rejected the supplied credentials.
    #[error("session check rejected: {message}")]
    Rejected {
        /// The rejection message from the server.
        message: String,
    },
    /// The server did not return an account id.
    #[error("session check did not return account_id")]
    MissingAccountId,
}

impl AuthError {
    /// Stable machine-readable kind for tests and mapping.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::InvalidUrl { .. } => "invalid_url",
            Self::Client { .. } => "client",
            Self::Network { .. } => "network",
            Self::Timeout { .. } => "timeout",
            Self::ReadResponse { .. } => "read_response",
            Self::WrongHostHtml { .. } => "wrong_host",
            Self::HttpsRequired { .. } => "https_required",
            Self::InvalidKey => "invalid_key",
            Self::Forbidden { .. } => "forbidden",
            Self::ApiNotFound { .. } => "api_not_found",
            Self::RateLimited { .. } => "rate_limited",
            Self::ServerError { .. } => "server_error",
            Self::HttpStatus { .. } => "http_status",
            Self::BadJson { .. } => "bad_json",
            Self::Rejected { .. } => "rejected",
            Self::MissingAccountId => "missing_account",
        }
    }

    /// Short message for the GUI error banner (no transport internals).
    pub fn user_message(&self) -> String {
        match self {
            Self::InvalidUrl { .. } => {
                "This server address is not valid. Enter the full URL, including `https://`.".into()
            }
            Self::Timeout { .. } => {
                "The server did not respond within 15 seconds. Check the URL and try again.".into()
            }
            Self::Network { .. } => {
                "Could not connect to the server. Check the URL, your network connection, and whether the server is running.".into()
            }
            Self::Client { .. } => {
                "Could not start a secure connection to the server. Restart the app and try again."
                    .into()
            }
            Self::ReadResponse { .. } => {
                "Connected to the server, but could not read its response. Try again.".into()
            }
            Self::WrongHostHtml { .. } => {
                "This URL points to the Message Crate website, not the API. Use the server address (the TLS host or port 8080, not port 3000).".into()
            }
            Self::HttpsRequired { .. } => {
                "This server requires https:// but http:// was specified.".into()
            }
            Self::InvalidKey => {
                "This API key is not valid for this server. Paste a valid key and try again."
                    .into()
            }
            Self::Forbidden { .. } => {
                "This API key does not have permission to access this server.".into()
            }
            Self::ApiNotFound { .. } => {
                "The API was not found at this URL. Enter the server’s base URL without `/v1/session`.".into()
            }
            Self::RateLimited { .. } => {
                "Too many verification attempts. Wait a moment, then try again.".into()
            }
            Self::ServerError { status, .. } => {
                format!(
                    "The server could not verify your credentials right now (HTTP {status}). Try again later."
                )
            }
            Self::HttpStatus { status, .. } => {
                format!(
                    "The server rejected the verification request (HTTP {status}). Open the Log tab for details."
                )
            }
            Self::BadJson { .. } => {
                "Connected to the server, but its response was not recognized. Confirm that the server is compatible with this app.".into()
            }
            Self::Rejected { .. } => {
                "The server rejected these credentials. Check the server address and API key.".into()
            }
            Self::MissingAccountId => {
                "The API key was accepted, but the server did not return an account. Contact the owner of this Message Crate.".into()
            }
        }
    }

    /// Technical detail for the Log tab / CLI.
    pub fn detail(&self) -> String {
        self.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_and_user_messages_cover_all_variants() {
        let cases: Vec<(AuthError, &str)> = vec![
            (
                AuthError::InvalidUrl {
                    url: "notaurl".into(),
                    detail: "relative URL without a base".into(),
                },
                "invalid_url",
            ),
            (
                AuthError::Client {
                    detail: "tls".into(),
                },
                "client",
            ),
            (
                AuthError::Network {
                    url: "https://v/v1/session".into(),
                    detail: "dns".into(),
                },
                "network",
            ),
            (
                AuthError::Timeout {
                    url: "https://v/v1/session".into(),
                    detail: "timed out".into(),
                },
                "timeout",
            ),
            (
                AuthError::ReadResponse {
                    detail: "reset".into(),
                },
                "read_response",
            ),
            (
                AuthError::WrongHostHtml {
                    url: "https://app/v1/session".into(),
                    status: 200,
                },
                "wrong_host",
            ),
            (
                AuthError::HttpsRequired {
                    url: "http://app.example".into(),
                },
                "https_required",
            ),
            (AuthError::InvalidKey, "invalid_key"),
            (
                AuthError::Forbidden {
                    status: 403,
                    body: "nope".into(),
                },
                "forbidden",
            ),
            (
                AuthError::ApiNotFound {
                    status: 404,
                    body: "missing".into(),
                },
                "api_not_found",
            ),
            (
                AuthError::RateLimited {
                    status: 429,
                    body: "slow down".into(),
                },
                "rate_limited",
            ),
            (
                AuthError::ServerError {
                    status: 503,
                    body: "busy".into(),
                },
                "server_error",
            ),
            (
                AuthError::HttpStatus {
                    status: 418,
                    body: "teapot".into(),
                },
                "http_status",
            ),
            (
                AuthError::BadJson {
                    url: "https://v/v1/session".into(),
                    status: 200,
                    snippet: "{".into(),
                },
                "bad_json",
            ),
            (
                AuthError::Rejected {
                    message: "bad token".into(),
                },
                "rejected",
            ),
            (AuthError::MissingAccountId, "missing_account"),
        ];

        for (error, kind) in cases {
            assert_eq!(error.kind(), kind);
            let user = error.user_message();
            assert!(!user.is_empty(), "{kind} user message empty");
            // Banner copy must stay free of transport / body dumps.
            assert!(
                !user.contains("dns")
                    && !user.contains("teapot")
                    && !user.contains("busy")
                    && !user.contains("nope")
                    && !user.contains("missing")
                    && !user.contains("slow down")
                    && !user.contains("bad token")
                    && !user.contains("relative URL")
                    && !user.contains("GET https"),
                "{kind} user message leaked detail: {user}"
            );
            let detail = error.detail();
            assert!(!detail.is_empty(), "{kind} detail empty");
            match &error {
                AuthError::InvalidKey | AuthError::MissingAccountId => {}
                AuthError::HttpsRequired { url } => {
                    assert!(detail.contains(url));
                    assert!(detail.contains("https"));
                    assert!(user.contains("https://"));
                }
                AuthError::WrongHostHtml { .. } => {
                    assert!(detail.contains("HTML") || detail.contains("html"));
                }
                AuthError::ServerError { status, body, .. }
                | AuthError::HttpStatus { status, body, .. }
                | AuthError::Forbidden { status, body, .. }
                | AuthError::ApiNotFound { status, body, .. }
                | AuthError::RateLimited { status, body, .. } => {
                    assert!(detail.contains(&status.to_string()));
                    assert!(detail.contains(body));
                }
                AuthError::BadJson { snippet, .. } => assert!(detail.contains(snippet)),
                AuthError::Rejected { message } => assert!(detail.contains(message)),
                AuthError::Network { detail: d, .. }
                | AuthError::Timeout { detail: d, .. }
                | AuthError::Client { detail: d }
                | AuthError::ReadResponse { detail: d }
                | AuthError::InvalidUrl { detail: d, .. } => {
                    assert!(detail.contains(d));
                }
            }
        }
    }

    #[test]
    fn status_messages_include_http_code() {
        assert!(
            AuthError::ServerError {
                status: 502,
                body: "x".into()
            }
            .user_message()
            .contains("HTTP 502")
        );
        assert!(
            AuthError::HttpStatus {
                status: 418,
                body: "x".into()
            }
            .user_message()
            .contains("HTTP 418")
        );
    }
}
