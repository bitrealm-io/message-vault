//! Blocking HTTP client helpers and retry classification for the vault CLI
//! crates.
//!
//! `vault-push` and `vault-pull` both talk to the vault through one
//! [`HttpSession`] (built on [`build_client`]), log in through
//! [`auth_check`], share [`truncate`] for error snippets, and classify
//! retryable failures through `classify_retry` / `with_retries`.
//! [`AuthError`] and [`AuthInfo`] live here so both crates — and the desktop
//! app through their re-exports — share one auth surface, and [`ok_json`]
//! reads every vault answer, so the vault's `{error}` failure body is
//! understood in one place rather than in each client.

mod auth_error;
mod response;
mod retry;
mod session;

pub use auth_error::AuthError;
pub use response::{error_sentence, ok_json};
pub use retry::{RetryKind, VaultHttpError, classify_retry, with_retries};
pub use session::{HttpSession, auth_check, bearer_header, looks_like_html, trim_base_url};

use anyhow::{Context, Result};

/// Account id and username returned by a successful `GET /v1/session`.
#[derive(Debug, Clone)]
pub struct AuthInfo {
    /// The vault account id.
    pub account_id: i64,
    /// The display username for the account, if one is set.
    pub username: Option<String>,
}

/// Idle connections kept per vault host for worker threads.
const POOL_MAX_IDLE_PER_HOST: usize = 64;

/// Build the shared blocking reqwest client.
///
/// One client per `HttpSession`; the connection pool keeps
/// `POOL_MAX_IDLE_PER_HOST` idle connections per host for the worker threads.
///
/// # Errors
///
/// Returns an error when the reqwest client cannot be built.
pub fn build_client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .pool_max_idle_per_host(POOL_MAX_IDLE_PER_HOST)
        .build()
        .context("build HTTP client")
}

/// Copy `s`, cutting it to at most `max` bytes and adding an ellipsis when
/// longer.
///
/// Cuts on a char boundary, so multi-byte characters are never split.
pub fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..s.floor_char_boundary(max)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_leaves_short_and_exact_strings_alone() {
        assert_eq!(truncate("short", 200), "short");
        assert_eq!(truncate("exact", 5), "exact");
    }

    #[test]
    fn truncate_adds_ellipsis_and_cuts_on_a_char_boundary() {
        assert_eq!(truncate("123456", 5), "12345…");
        // 'h' is 1 byte, 'é' is 2: max=2 would split 'é' under the old code.
        assert_eq!(truncate("héllo", 2), "h…");
        assert_eq!(truncate("héllo", 3), "hé…");
    }

    #[test]
    fn truncate_survives_max_zero() {
        assert_eq!(truncate("héllo", 0), "…");
    }

    #[test]
    fn idle_pool_keeps_64_connections_per_host() {
        assert_eq!(POOL_MAX_IDLE_PER_HOST, 64);
    }
}
