//! Typed retry classification for the vault HTTP paths.

use std::io;
use std::thread;
use std::time::Duration;

use anyhow::Result;

use crate::AuthError;

/// Whether a failure is likely to succeed on retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryKind {
    /// Worth retrying: network, timeout, 5xx, or anything unrecognized.
    Transient,
    /// Will fail the same way again, or already succeeded: auth, 4xx, a 2xx
    /// whose body could not be read, missing local files.
    Permanent,
}

/// An HTTP-status failure with its human-readable message.
///
/// `Display` prints only the message, so error text stays exactly what the
/// call site wrote; the status travels typed for [`classify_retry`].
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct VaultHttpError {
    status: u16,
    message: String,
}

impl VaultHttpError {
    /// Build a status-tagged error that displays `message` verbatim.
    pub fn new(status: u16, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

/// Classify an error for [`with_retries`].
///
/// Checks, in order: [`VaultHttpError`] (2xx and 4xx permanent), [`AuthError`]
/// (auth and 4xx permanent, transport transient), `reqwest::Error` status (4xx
/// permanent), `std::io::Error` kind (`NotFound` permanent). Anything
/// unrecognized is transient, matching the historical default.
///
/// A [`VaultHttpError`] with a 2xx status is an answer the client could not
/// read. The vault has already done the work, so sending the request again
/// would repeat a write it committed.
pub fn classify_retry(error: &anyhow::Error) -> RetryKind {
    if let Some(http) = error.downcast_ref::<VaultHttpError>() {
        return if (200..300).contains(&http.status) || (400..500).contains(&http.status) {
            RetryKind::Permanent
        } else {
            RetryKind::Transient
        };
    }
    if let Some(auth) = error.downcast_ref::<AuthError>() {
        return match auth {
            AuthError::InvalidKey
            | AuthError::Forbidden { .. }
            | AuthError::ApiNotFound { .. }
            | AuthError::RateLimited { .. }
            | AuthError::Rejected { .. } => RetryKind::Permanent,
            AuthError::HttpStatus { status, .. } if (400..500).contains(status) => {
                RetryKind::Permanent
            }
            _ => RetryKind::Transient,
        };
    }
    if let Some(reqwest) = error.downcast_ref::<reqwest::Error>() {
        return match reqwest.status() {
            Some(status) if (400..500).contains(&status.as_u16()) => RetryKind::Permanent,
            _ => RetryKind::Transient,
        };
    }
    if let Some(io) = error.downcast_ref::<io::Error>() {
        return if io.kind() == io::ErrorKind::NotFound {
            RetryKind::Permanent
        } else {
            RetryKind::Transient
        };
    }
    RetryKind::Transient
}

/// Run `op` again on transient failures, with backoff, up to `max_retries`
/// extra tries.
///
/// # Errors
///
/// Returns the last error from `op` when retries are exhausted or the error is
/// permanent.
pub fn with_retries<T, F>(max_retries: u32, mut op: F) -> Result<T>
where
    F: FnMut() -> Result<T>,
{
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        match op() {
            Ok(v) => return Ok(v),
            Err(e) => {
                if attempt > max_retries || classify_retry(&e) == RetryKind::Permanent {
                    return Err(e);
                }
                // Exponential backoff with jitter.
                let base_ms = 500u64 * 2u64.saturating_pow(attempt.saturating_sub(1));
                let jitter_ms = (base_ms / 4).min(5000);
                let wait_ms = base_ms + (jitter_ms / 2) + (jitter_ms as f64 * rand_factor()) as u64;
                thread::sleep(Duration::from_millis(wait_ms.min(30_000)));
            }
        }
    }
}

/// Deterministic pseudo-random factor in [0.0, 1.0) for retry jitter.
fn rand_factor() -> f64 {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    (nanos % 1000) as f64 / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    fn classified(kind: RetryKind) -> bool {
        kind == RetryKind::Permanent
    }

    #[test]
    fn http_status_errors_are_permanent_for_4xx() {
        let e = anyhow::Error::from(VaultHttpError::new(404, "asset HEAD failed (HTTP 404)"));
        assert!(classified(classify_retry(&e)));
        let e = anyhow::Error::from(VaultHttpError::new(413, "import rejected: HTTP 413"));
        assert!(classified(classify_retry(&e)));
        let e = anyhow::Error::from(VaultHttpError::new(401, "invalid vault key"));
        assert!(classified(classify_retry(&e)));
    }

    #[test]
    fn an_unreadable_2xx_is_permanent_because_the_vault_did_the_work() {
        let e = anyhow::Error::from(VaultHttpError::new(
            200,
            "could not read the vault's answer to import batch",
        ));
        assert!(classified(classify_retry(&e)));
    }

    #[test]
    fn http_status_errors_are_transient_for_5xx() {
        let e = anyhow::Error::from(VaultHttpError::new(503, "asset part 1 failed (HTTP 503)"));
        assert!(!classified(classify_retry(&e)));
    }

    #[test]
    fn auth_failures_are_permanent() {
        assert!(classified(classify_retry(&anyhow::Error::from(
            AuthError::InvalidKey
        ))));
        assert!(classified(classify_retry(&anyhow::Error::from(
            AuthError::Forbidden {
                status: 403,
                body: "username does not match vault key".into(),
            }
        ))));
        assert!(classified(classify_retry(&anyhow::Error::from(
            AuthError::RateLimited {
                status: 429,
                body: "slow down".into(),
            }
        ))));
        assert!(classified(classify_retry(&anyhow::Error::from(
            AuthError::ApiNotFound {
                status: 404,
                body: "missing".into(),
            }
        ))));
        assert!(classified(classify_retry(&anyhow::Error::from(
            AuthError::Rejected {
                message: "bad token".into(),
            }
        ))));
        assert!(classified(classify_retry(&anyhow::Error::from(
            AuthError::HttpStatus {
                status: 418,
                body: "teapot".into(),
            }
        ))));
    }

    #[test]
    fn auth_transport_failures_are_transient() {
        assert!(!classified(classify_retry(&anyhow::Error::from(
            AuthError::Network {
                url: "https://v".into(),
                detail: "dns".into(),
            }
        ))));
        assert!(!classified(classify_retry(&anyhow::Error::from(
            AuthError::ServerError {
                status: 503,
                body: "busy".into(),
            }
        ))));
        assert!(!classified(classify_retry(&anyhow::Error::from(
            AuthError::HttpStatus {
                status: 503,
                body: "busy".into(),
            }
        ))));
    }

    #[test]
    fn io_not_found_is_permanent_other_io_is_transient() {
        let e = anyhow::Error::from(io::Error::new(io::ErrorKind::NotFound, "no such file"));
        assert!(classified(classify_retry(&e)));
        let e = anyhow::Error::from(io::Error::new(io::ErrorKind::PermissionDenied, "denied"));
        assert!(!classified(classify_retry(&e)));
    }

    #[test]
    fn unrecognized_errors_are_transient() {
        assert!(!classified(classify_retry(&anyhow!("something odd"))));
    }

    #[test]
    fn with_retries_gives_up_on_permanent_immediately() {
        let mut calls = 0;
        let result = with_retries(3, || -> Result<u32> {
            calls += 1;
            Err(anyhow::Error::from(VaultHttpError::new(404, "gone")))
        });
        assert!(result.is_err());
        assert_eq!(calls, 1);
    }

    #[test]
    fn with_retries_retries_transient_then_succeeds() {
        let mut calls = 0;
        let result = with_retries(2, || -> Result<u32> {
            calls += 1;
            if calls < 2 {
                Err(anyhow::Error::from(io::Error::other("flaky")))
            } else {
                Ok(42)
            }
        });
        assert_eq!(result.unwrap(), 42);
        assert_eq!(calls, 2);
    }

    #[test]
    fn with_retries_gives_up_when_exhausted() {
        let mut calls = 0;
        let result = with_retries(0, || -> Result<u32> {
            calls += 1;
            Err(anyhow::Error::from(io::Error::other("flaky")))
        });
        assert!(result.is_err());
        assert_eq!(calls, 1);
    }
}
