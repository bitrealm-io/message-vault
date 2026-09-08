//! HTTP helpers for an Export Run: creating it, paging its messages, closing
//! it, and downloading attachments.
//!
//! Calls are blocking so they can run on worker threads without an async
//! runtime. The session type is [`vault_http::HttpSession`].
//!
//! The shapes are `vault-api-types`, the same definitions the vault
//! serializes from. This file used to mirror them by hand, and three defects
//! shipped because the mirror and the vault drifted apart with nothing to
//! notice.

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::Method;
use serde::Deserialize;
use vault_http::{VaultHttpError, error_sentence, ok_json, trim_base_url};

use vault_api_types::{ExportRun, ExportScope, Message};

pub use vault_http::HttpSession;

/// One page from `GET /v1/exports/{id}/messages`: `{items, total, limit, offset}`.
#[derive(Debug, Deserialize)]
pub struct ExportMessagesPage {
    #[serde(default)]
    pub items: Vec<Message>,
    #[serde(default)]
    pub total: u64,
}

/// The body of `POST /v1/exports`.
#[derive(Debug, serde::Serialize)]
struct CreateExportBody<'a> {
    scope: &'a ExportScope,
    tool: &'a str,
}

/// `POST /v1/exports`: record a run for `scope` and return it with the
/// counts the vault computed.
///
/// # Errors
///
/// Returns an error when the request fails, the vault refuses the scope, or
/// the body is not a run.
pub fn create_export(
    http: &HttpSession,
    base_url: &str,
    key: &str,
    scope: &ExportScope,
    tool: &str,
) -> Result<ExportRun> {
    let body = serde_json::to_vec(&CreateExportBody { scope, tool })?;
    let response = http
        .vault_request(Method::POST, base_url, "/v1/exports", key)
        .header("Content-Type", "application/json")
        .body(body)
        .timeout(Duration::from_secs(120))
        .send()
        .context("POST /v1/exports")?;
    let status = response.status();
    let text = response.text().unwrap_or_default();
    ok_json("create export", status, &text)
}

/// Arguments for [`export_messages`].
pub(crate) struct ExportMessagesArgs<'a> {
    pub base_url: &'a str,
    pub key: &'a str,
    /// The run whose messages are paged.
    pub export_id: i64,
    pub limit: usize,
    pub offset: usize,
}

/// Fetch one page of messages from `GET /v1/exports/{id}/messages`.
///
/// # Errors
///
/// Returns an error when the request fails or the body is not valid JSON.
pub fn export_messages(
    http: &HttpSession,
    args: ExportMessagesArgs<'_>,
) -> Result<ExportMessagesPage> {
    let ExportMessagesArgs {
        base_url,
        key,
        export_id,
        limit,
        offset,
    } = args;
    let path = format!("/v1/exports/{export_id}/messages");
    let response = http
        .vault_request(Method::GET, base_url, &path, key)
        .query(&[("limit", limit.to_string()), ("offset", offset.to_string())])
        .timeout(Duration::from_secs(120))
        .send()
        .with_context(|| format!("GET {path}"))?;

    let status = response.status();
    let body = response.text().unwrap_or_default();
    ok_json("export messages", status, &body)
}

/// `POST /v1/exports/{id}/complete` or `.../cancel`: close the run and
/// return it as it now stands. `action` is `complete` or `cancel`.
///
/// # Errors
///
/// Returns an error when the request fails, the run is already closed, or
/// the body is not a run.
pub fn close_export(
    http: &HttpSession,
    base_url: &str,
    key: &str,
    export_id: i64,
    action: &str,
) -> Result<ExportRun> {
    let path = format!("/v1/exports/{export_id}/{action}");
    let response = http
        .vault_request(Method::POST, base_url, &path, key)
        .timeout(Duration::from_secs(120))
        .send()
        .with_context(|| format!("POST {path}"))?;
    let status = response.status();
    let text = response.text().unwrap_or_default();
    ok_json(&format!("{action} export"), status, &text)
}

/// Download one attachment by SHA-256 fingerprint to `dest`.
///
/// Bytes are written to a `.part` file first, then renamed, so a crash does
/// not leave a truncated file at the destination.
///
/// # Errors
///
/// Returns an error when the fingerprint is not 64 hex characters, the vault
/// returns 404 or another failure, or the file cannot be written.
pub fn download_asset(
    http: &HttpSession,
    base_url: &str,
    key: &str,
    account: &str,
    source: &str,
    sha256: &str,
    dest: &Path,
) -> Result<()> {
    // Validate sha256 is a 64-char hex string before putting it in the URL.
    let sha_clean = sha256.trim();
    if sha_clean.len() != 64 || !sha_clean.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("invalid SHA-256 digest for asset download: {sha256}");
    }
    let base = trim_base_url(base_url);
    let mut url = reqwest::Url::parse(&format!("{base}/v1/assets/{sha_clean}"))
        .with_context(|| format!("invalid vault URL {base}"))?;
    {
        let mut qp = url.query_pairs_mut();
        qp.append_pair("source", source);
        let account = account.trim();
        if !account.is_empty() {
            qp.append_pair("account", account);
        }
    }

    let mut response = http
        .request_url(Method::GET, url, key)
        .timeout(Duration::from_secs(300))
        .send()
        .context("GET /v1/assets")?;

    let status = response.status();
    if status.as_u16() == 404 {
        return Err(VaultHttpError::new(
            404,
            format!("asset not found: {sha256} (source={source})"),
        )
        .into());
    }
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        return Err(VaultHttpError::new(
            status.as_u16(),
            format!(
                "asset download failed (HTTP {status}): {}",
                error_sentence(&body)
            ),
        )
        .into());
    }

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    // Write to a temp file then rename, so a partial download (crash, cancel,
    // network drop) never leaves a truncated file at the destination path.
    let tmp = dest.with_extension("part");
    let mut file = File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
    std::io::copy(&mut response, &mut file).with_context(|| format!("write {}", tmp.display()))?;
    file.flush()?;
    std::fs::rename(&tmp, dest)
        .with_context(|| format!("rename {} -> {}", tmp.display(), dest.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_create_body_carries_the_scope_as_given_and_the_tool() {
        let scope = ExportScope::Query {
            q: "from:me".into(),
        };
        let body = serde_json::to_value(CreateExportBody {
            scope: &scope,
            tool: "vault-pull",
        })
        .unwrap();
        assert_eq!(
            body,
            serde_json::json!({ "scope": { "kind": "query", "q": "from:me" }, "tool": "vault-pull" })
        );
    }

    #[test]
    fn a_page_parses_without_an_ok_flag_and_a_failure_body_yields_its_sentence() {
        let page: ExportMessagesPage =
            serde_json::from_str(r#"{"items":[],"total":7,"limit":500,"offset":0}"#).unwrap();
        assert_eq!((page.items.len(), page.total), (0, 7));
        assert_eq!(
            error_sentence(
                r#"{"type":"https://bitrealm.io/vault/developer/reference/errors/validation-failed","title":"Validation failed","status":422,"errors":["limit exceeds maximum of 500"]}"#
            ),
            "limit exceeds maximum of 500"
        );
        assert_eq!(error_sentence("<html>"), "<html>");
    }
}
