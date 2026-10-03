//! Reading the server's answer: the value on success, the server's own sentence
//! on failure.
//!
//! Every route the server serves answers a failure with an RFC 7807 problem
//! document (`docs/architecture/http-api.md`), and its `detail` is written for the person to read.
//! Both client crates were reading it themselves — `message-crate-push` with an
//! `ok_json` helper, `message-crate-pull` with an `error_sentence` one — over two
//! private copies of the same struct. One copy of the reading lives here, over
//! the shared [`Problem`] type, so a change to the server's failure shape is
//! one edit rather than a hunt.

use anyhow::Result;
use message_crate_api_types::Problem;
use serde::de::DeserializeOwned;

use crate::SESSION_REFUSED;
use crate::retry::HttpError;
use crate::truncate;

/// Longest failure body repeated back to the person. A server sentence is far
/// shorter; a proxy's HTML error page is not, and none of it helps.
const MAX_BODY_SNIPPET: usize = 300;

/// The sentence to show for a failed response: the problem's `detail` (or its
/// `errors`) when the body is one, followed by the request id so a person can
/// quote it, otherwise the body itself, clipped. The status is the caller's
/// to report — [`ok_json`] does, once — so a body that carries no sentence
/// does not end up naming the status twice.
#[must_use]
pub fn error_sentence(body: &str) -> String {
    match serde_json::from_str::<Problem>(body) {
        Ok(problem) => match &problem.request_id {
            Some(id) => format!("{} (request id {id})", problem.sentence()),
            None => problem.sentence(),
        },
        Err(_) => truncate(body, MAX_BODY_SNIPPET),
    }
}

/// Read a response's status and body, for [`ok_json`] to judge.
///
/// A body that cannot be read after a 2xx status is a [`HttpError`] with that
/// status, which [`crate::classify_retry`] treats as permanent: the server
/// committed the work before the connection dropped, and sending the request
/// again would repeat it. A body that cannot be read after any other status
/// keeps the transport error, which stays transient.
///
/// # Errors
///
/// Returns an error when the body cannot be read.
pub fn read_body(
    what: &str,
    response: reqwest::blocking::Response,
) -> Result<(reqwest::StatusCode, String)> {
    let status = response.status();
    match response.text() {
        Ok(body) => Ok((status, body)),
        Err(e) if status.is_success() => Err(HttpError::new(
            status.as_u16(),
            format!("could not read the server's answer to {what} ({e})"),
        )
        .into()),
        Err(e) => Err(anyhow::Error::from(e).context(format!("read the answer to {what}"))),
    }
}

/// Parse a server JSON response body, or fail with what the server said went
/// wrong.
///
/// A 2xx status is a success and the body is `T`. Anything else is a
/// [`HttpError`] carrying the status, so the retry rules can classify it,
/// the problem document when the body is one, so a caller can read a member
/// such as `line`, and a sentence naming `what` was being asked for — "import batch",
/// "export messages" — because a status alone does not tell the person which
/// part of a long run stopped.
///
/// # Errors
///
/// Returns an error for any non-2xx status, and for a 2xx body that is not the
/// JSON `T` expects. The second kind is never retried: the server already did
/// the work, and sending the request again would repeat it.
pub fn ok_json<T: DeserializeOwned>(
    what: &str,
    status: reqwest::StatusCode,
    body: &str,
) -> Result<T> {
    if status.is_success() {
        return serde_json::from_str::<T>(body).map_err(|e| {
            HttpError::new(
                status.as_u16(),
                format!("could not read the server's answer to {what} ({e}): {body}"),
            )
            .into()
        });
    }
    // A 401 mid-run means the session expired or was ended since login, so
    // the message says to log in again rather than what the server wrote.
    let message = if status == reqwest::StatusCode::UNAUTHORIZED {
        format!("{what} failed. {SESSION_REFUSED}")
    } else {
        format!("{what} failed (HTTP {status}): {}", error_sentence(body))
    };
    let error = HttpError::new(status.as_u16(), message);
    Err(match serde_json::from_str::<Problem>(body) {
        Ok(problem) => error.with_problem(problem),
        Err(_) => error,
    }
    .into())
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[derive(Debug, Deserialize)]
    struct Answer {
        #[serde(default)]
        ok: bool,
    }

    #[test]
    fn the_servers_own_sentence_is_what_the_person_sees() {
        let err = ok_json::<Answer>(
            "asset upload",
            reqwest::StatusCode::BAD_REQUEST,
            r#"{"type":"https://messagecrate.app/docs/developer/reference/errors/asset-upload-invalid","title":"Asset upload invalid","status":400,"detail":"sha256 mismatch: claimed abc, got def","request_id":"3f2b1c0e-8d4a-4b6e-9f21-5c7d8e9a0b1c"}"#,
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "asset upload failed (HTTP 400 Bad Request): sha256 mismatch: claimed abc, got def (request id 3f2b1c0e-8d4a-4b6e-9f21-5c7d8e9a0b1c)"
        );
    }

    #[test]
    fn a_401_says_to_log_in_again_and_stays_permanent() {
        let err = ok_json::<Answer>(
            "import batch",
            reqwest::StatusCode::UNAUTHORIZED,
            r#"{"type":"https://messagecrate.app/docs/developer/reference/errors/authentication-required","title":"Authentication required","status":401,"detail":"Authentication required."}"#,
        )
        .unwrap_err();
        let message = err.to_string();
        assert!(message.starts_with("import batch failed."), "got {message}");
        assert!(message.contains("Log in again"), "got {message}");
        assert_eq!(
            crate::classify_retry(&err),
            crate::RetryKind::Permanent,
            "a refused session is not retried"
        );
    }

    #[test]
    fn a_body_with_no_error_sentence_falls_back_to_status_and_body() {
        let err =
            ok_json::<Answer>("import batch", reqwest::StatusCode::BAD_GATEWAY, "{}").unwrap_err();
        assert_eq!(
            err.to_string(),
            "import batch failed (HTTP 502 Bad Gateway): {}"
        );
        let err = ok_json::<Answer>("import batch", reqwest::StatusCode::BAD_GATEWAY, "gateway")
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "import batch failed (HTTP 502 Bad Gateway): gateway"
        );
    }

    #[test]
    fn a_proxys_error_page_is_clipped_rather_than_repeated_whole() {
        let page = "x".repeat(MAX_BODY_SNIPPET * 2);
        let sentence = error_sentence(&page);
        assert!(
            sentence.len() < page.len(),
            "a long body must be clipped: {} bytes",
            sentence.len()
        );
        assert!(sentence.ends_with('…'));
    }

    #[test]
    fn the_status_decides_success_not_a_field_in_the_body() {
        let parsed: Answer = ok_json("asset upload", reqwest::StatusCode::OK, r#"{"ok":true}"#)
            .expect("2xx is a success");
        assert!(parsed.ok);
        assert!(
            ok_json::<Answer>(
                "asset upload",
                reqwest::StatusCode::INTERNAL_SERVER_ERROR,
                "{}"
            )
            .is_err(),
            "a 500 is a failure even when the body parses"
        );
    }

    #[test]
    fn a_2xx_body_that_is_not_json_names_what_was_being_read() {
        let err =
            ok_json::<Answer>("export messages", reqwest::StatusCode::OK, "not json").unwrap_err();
        assert!(
            err.to_string()
                .contains("could not read the server's answer to export messages"),
            "{err}"
        );
    }

    /// The status has to survive into the error, or a run would give up on a
    /// 503 the server meant the client to come back from.
    #[test]
    fn a_failure_carries_the_status_so_retries_can_classify_it() {
        let err = ok_json::<Answer>(
            "import batch",
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
            "{}",
        )
        .unwrap_err();
        assert_eq!(
            crate::classify_retry(&err),
            crate::RetryKind::Transient,
            "a 503 must still read as transient: {err}"
        );
        let err =
            ok_json::<Answer>("import batch", reqwest::StatusCode::BAD_REQUEST, "{}").unwrap_err();
        assert_ne!(
            crate::classify_retry(&err),
            crate::RetryKind::Transient,
            "a 400 must not: {err}"
        );
    }
}
