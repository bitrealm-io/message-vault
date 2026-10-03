//! HTTP helpers for attachment upload and JSON Lines message import.
//!
//! JSON Lines means one JSON object per line. Calls are blocking so they can
//! run on worker threads without an async runtime. Login lives in
//! [`message_crate_http::auth_check`]; the session type here is
//! [`message_crate_http::HttpSession`].

use message_crate_api_types::ImportMode;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use message_crate_http::{
    HttpError, error_sentence, looks_like_html, ok_json, read_body, trim_base_url,
};
use reqwest::Method;
use serde::Deserialize;

pub use message_crate_http::HttpSession;

use crate::run::Session;

/// The server's `Asset`: the answer to `HEAD` or `PUT /v1/assets/{sha256}`
/// and to completing a multipart upload. Only `already_present` is read.
#[derive(Debug, Deserialize)]
pub struct Asset {
    #[serde(default)]
    pub already_present: bool,
}

/// The answer to `POST /v1/imports/{id}/batches`. Only the counts the
/// report needs are read.
#[derive(Debug, Deserialize)]
pub struct CreateImportBatchResponse {
    #[serde(default)]
    pub messages: u64,
    #[serde(default)]
    pub messages_appended: u64,
    #[serde(default)]
    pub messages_deduped: u64,
}

/// One attachment to upload: where it is on disk, what it is, and the size
/// above which it goes up in parts.
pub(crate) struct AssetUpload<'a> {
    pub source: &'a str,
    pub sha256: &'a str,
    pub file: &'a Path,
    pub mime: Option<&'a str>,
    /// Files larger than this use multipart upload (typically [`crate::run::MAX_PROXY_BODY_BYTES`]).
    pub multipart_threshold: usize,
}

/// How an Import Run ended, for `/v1/imports/{id}/complete`.
pub(crate) struct ImportOutcome<'a> {
    /// `completed`, `completed_with_issues`, or `failed`.
    pub status: &'a str,
    pub bytes_uploaded: u64,
}

/// The answer to `POST /v1/imports`: the new Import Run's id.
#[derive(Debug, Deserialize)]
struct CreateImportResponse {
    id: i64,
}

/// The Import Run `POST /v1/imports/{id}/complete` answers. Only `id` is read: it
/// names the run the server closed.
#[derive(Debug, Deserialize)]
struct ImportRun {
    id: i64,
}

/// The answer to `POST /v1/assets/{sha256}/uploads`.
#[derive(Debug, Deserialize)]
struct CreateAssetUploadResponse {
    #[serde(default)]
    upload_id: Option<String>,
    #[serde(default)]
    part_size: Option<usize>,
    #[serde(default)]
    already_present: bool,
}

/// True for HTTP 413 or a proxy HTML page that says the body was too large.
fn looks_like_payload_too_large(status: reqwest::StatusCode, body: &str) -> bool {
    status.as_u16() == 413
        || body.contains("413 Payload Too Large")
        || body.contains("413 Request Entity Too Large")
        || (looks_like_html(body) && body.to_ascii_lowercase().contains("payload too large"))
}

/// Human-readable 413 error that names the request kind and optional byte size.
fn payload_too_large_message(kind: &str, bytes: Option<usize>) -> String {
    let size = bytes
        .map(|n| format!(" (request was {n} bytes)"))
        .unwrap_or_default();
    format!(
        "{kind} rejected: HTTP 413 Payload Too Large{size}. \
         Cloudflare Free/Pro caps proxied uploads at ~100 MB. \
         message-crate-push chunks message imports under 64 MiB and large assets via multipart; \
         if this still fails, raise nginx client_max_body_size for /v1 (need ≥100m for 64 MiB parts) \
         or tunnel to the server on :8080."
    )
}

/// Build `{base}/v1/assets/...` with extra path segments (percent-encoded)
/// and the `source=` query every asset route takes. The account is not a
/// parameter: the API key names it.
fn asset_url(base_url: &str, segments: &[&str], source: &str) -> Result<reqwest::Url> {
    let base = trim_base_url(base_url);
    let mut url =
        reqwest::Url::parse(base).with_context(|| format!("invalid server address {base}"))?;
    url.path_segments_mut()
        .map_err(|()| anyhow!("invalid server address {base}"))?
        .pop_if_empty()
        .extend(["v1", "assets"].into_iter().chain(segments.iter().copied()));
    url.query_pairs_mut().append_pair("source", source);
    Ok(url)
}

impl Session {
    /// `/v1/assets/...` URL under this session's account.
    fn asset_url(&self, source: &str, segments: &[&str]) -> Result<reqwest::Url> {
        asset_url(&self.url, segments, source)
    }

    /// Whether the server already holds the attachment with this digest:
    /// `true` for a 2xx, `false` for a 404. A HEAD reply carries no body, so
    /// the status is the whole answer: the problem type the server chose
    /// cannot be read, and each error message names every cause the
    /// reference lists for its status.
    ///
    /// # Errors
    ///
    /// Returns an error when the server does not accept the credential
    /// (`401 Unauthorized`: unknown or expired), refuses the account
    /// (`403 Forbidden`: disabled, or neither import nor export), or the
    /// request fails in any other way.
    pub(crate) fn head_asset(&self, source: &str, sha256: &str) -> Result<bool> {
        let url = self.asset_url(source, &[sha256])?;
        let response = self
            .http
            .request_url(Method::HEAD, url.clone(), &self.key)
            .timeout(Duration::from_secs(15))
            .send()
            .with_context(|| format!("HEAD {url}"))?;
        let status = response.status();
        match status.as_u16() {
            404 => return Ok(false),
            401 => {
                return Err(HttpError::new(
                    401,
                    "The server did not accept this credential (401 Unauthorized): \
                     it is unknown or has expired.",
                )
                .into());
            }
            403 => {
                return Err(HttpError::new(
                    403,
                    "The server refused this account access to attachments (403 Forbidden): \
                     the account is disabled, or it may neither import nor export.",
                )
                .into());
            }
            _ => {}
        }
        if !status.is_success() {
            let text = response.text().unwrap_or_default();
            return Err(HttpError::new(
                status.as_u16(),
                format!(
                    "asset HEAD failed (HTTP {status}): {}",
                    error_sentence(&text)
                ),
            )
            .into());
        }
        Ok(true)
    }

    /// Upload one attachment: in one PUT, or in parts when the file is larger
    /// than its threshold.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read or the server rejects it;
    /// a 413 says how large a body the server accepts.
    pub(crate) fn put_asset(&self, asset: &AssetUpload<'_>) -> Result<Asset> {
        let file_len = std::fs::metadata(asset.file)
            .with_context(|| format!("stat {}", asset.file.display()))?
            .len();
        if file_len > asset.multipart_threshold as u64 {
            return self.put_asset_multipart(asset, file_len);
        }

        let url = self.asset_url(asset.source, &[asset.sha256])?;
        let bytes =
            std::fs::read(asset.file).with_context(|| format!("read {}", asset.file.display()))?;
        let content_type = asset
            .mime
            .filter(|mime| !mime.is_empty())
            .unwrap_or("application/octet-stream");
        let response = self
            .http
            .request_url(Method::PUT, url.clone(), &self.key)
            .timeout(Duration::from_secs(600))
            .header("Content-Type", content_type)
            .body(bytes)
            .send()
            .with_context(|| format!("PUT {url}"))?;
        let (status, text) = read_body("asset upload", response)?;
        if looks_like_payload_too_large(status, &text) {
            return Err(HttpError::new(
                413,
                payload_too_large_message("asset upload", Some(file_len as usize)),
            )
            .into());
        }
        ok_json::<Asset>("asset upload", status, &text)
    }

    /// Upload in parts: open a multipart upload, send each part, complete
    /// it. A part or completion that fails aborts the upload on the server.
    fn put_asset_multipart(&self, asset: &AssetUpload<'_>, file_len: u64) -> Result<Asset> {
        let Some(upload) = MultipartUpload::start(self, asset, file_len)? else {
            return Ok(Asset {
                already_present: true,
            });
        };
        let mut file =
            File::open(asset.file).with_context(|| format!("open {}", asset.file.display()))?;
        let mut part: u32 = 1;
        let mut remaining = file_len;
        while remaining > 0 {
            let this_len = remaining.min(upload.part_size as u64) as usize;
            let mut buf = vec![0u8; this_len];
            file.read_exact(&mut buf)
                .with_context(|| format!("read part {part} from {}", asset.file.display()))?;
            if let Err(error) = upload.send_part(part, buf) {
                upload.abort();
                return Err(error);
            }
            remaining -= this_len as u64;
            part += 1;
        }
        let completed = upload.complete();
        if completed.is_err() {
            upload.abort();
        }
        completed
    }

    /// POST one JSON Lines batch into the Import Run at
    /// `/v1/imports/{id}/batches`. The run's row says the source, the mode
    /// and whether to dedupe; the request carries only the body.
    ///
    /// # Errors
    ///
    /// Returns a 413 before sending when the body is over the proxy limit,
    /// and the server's error otherwise.
    pub(crate) fn post_import(
        &self,
        import_id: i64,
        ndjson: Vec<u8>,
    ) -> Result<CreateImportBatchResponse> {
        let body_len = ndjson.len();
        if body_len > crate::run::MAX_PROXY_BODY_BYTES {
            return Err(
                HttpError::new(413, payload_too_large_message("import", Some(body_len))).into(),
            );
        }
        let path = format!("/v1/imports/{import_id}/batches");
        let response = self
            .http
            .server_request(Method::POST, &self.url, &path, &self.key)
            .timeout(Duration::from_secs(600))
            .header("Content-Type", "application/jsonl")
            .body(ndjson)
            .send()
            .with_context(|| format!("POST {path}"))?;
        let (status, text) = read_body("import batch", response)?;
        if looks_like_payload_too_large(status, &text) {
            return Err(
                HttpError::new(413, payload_too_large_message("import", Some(body_len))).into(),
            );
        }
        ok_json::<CreateImportBatchResponse>("import batch", status, &text)
    }

    /// Create an Import Run on the server and return its id. Every batch is
    /// posted into it; the bearer token names the account.
    ///
    /// # Errors
    ///
    /// Returns an error when the server refuses, which includes an account
    /// that already has a running Import Run.
    pub(crate) fn start_import(
        &self,
        source: &str,
        mode: ImportMode,
        tool: Option<&str>,
    ) -> Result<i64> {
        let mut body = serde_json::json!({
            "source": source,
            "mode": mode,
        });
        if let Some(tool) = tool {
            body["tool"] = serde_json::Value::String(tool.to_string());
        }
        let response = self
            .http
            .server_request(Method::POST, &self.url, "/v1/imports", &self.key)
            .timeout(Duration::from_secs(60))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .context("POST /v1/imports")?;
        let (status, text) = read_body("import run", response)?;
        let parsed: CreateImportResponse = ok_json("import run", status, &text)?;
        Ok(parsed.id)
    }

    /// Record how the Import Run ended.
    ///
    /// # Errors
    ///
    /// Returns an error when the server refuses.
    pub(crate) fn complete_import(
        &self,
        import_id: i64,
        outcome: &ImportOutcome<'_>,
    ) -> Result<()> {
        let body = serde_json::json!({
            "status": outcome.status,
            "bytes_uploaded": outcome.bytes_uploaded,
        });
        let response = self
            .http
            .server_request(
                Method::POST,
                &self.url,
                &format!("/v1/imports/{import_id}/complete"),
                &self.key,
            )
            .timeout(Duration::from_secs(60))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .with_context(|| format!("POST /v1/imports/{import_id}/complete"))?;
        let (status, text) = read_body("import run complete", response)?;
        let closed: ImportRun = ok_json("import run complete", status, &text)?;
        if closed.id != import_id {
            return Err(anyhow!(
                "import run complete: the server closed run {} for run {import_id}",
                closed.id
            ));
        }
        Ok(())
    }
}

/// A multipart upload the server has opened for one attachment.
struct MultipartUpload<'a> {
    session: &'a Session,
    source: &'a str,
    sha256: &'a str,
    upload_id: String,
    /// Bytes per part, as the server asked.
    part_size: usize,
}

impl<'a> MultipartUpload<'a> {
    /// Open the upload. `None` when the server says it already has the file.
    ///
    /// # Errors
    ///
    /// Returns an error when the server refuses or its reply lacks an upload
    /// id or part size.
    fn start(session: &'a Session, asset: &AssetUpload<'a>, file_len: u64) -> Result<Option<Self>> {
        let start_url = session.asset_url(asset.source, &[asset.sha256, "uploads"])?;
        let mut start_body = serde_json::json!({ "bytes": file_len });
        if let Some(mime) = asset.mime.filter(|m| !m.is_empty()) {
            start_body["mime"] = serde_json::Value::String(mime.to_string());
        }
        let response = session
            .http
            .request_url(Method::POST, start_url.clone(), &session.key)
            .timeout(Duration::from_secs(30))
            .header("Content-Type", "application/json")
            .json(&start_body)
            .send()
            .with_context(|| format!("POST {start_url}"))?;
        let (status, text) = read_body("asset upload start", response)?;
        if looks_like_payload_too_large(status, &text) {
            return Err(
                HttpError::new(413, payload_too_large_message("asset upload start", None)).into(),
            );
        }
        let started: CreateAssetUploadResponse = ok_json("asset upload start", status, &text)?;
        if started.already_present {
            return Ok(None);
        }
        let upload_id = started
            .upload_id
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("upload start missing upload_id"))?;
        let part_size = started
            .part_size
            .filter(|&n| n > 0)
            .ok_or_else(|| anyhow!("upload start missing part_size"))?;
        Ok(Some(Self {
            session,
            source: asset.source,
            sha256: asset.sha256,
            upload_id,
            part_size,
        }))
    }

    /// This upload's URL with `tail` appended: `parts/N`, `complete`, or nothing.
    fn url(&self, tail: &[&str]) -> Result<reqwest::Url> {
        let mut segments = vec![self.sha256, "uploads", self.upload_id.as_str()];
        segments.extend_from_slice(tail);
        self.session.asset_url(self.source, &segments)
    }

    /// PUT one part.
    ///
    /// # Errors
    ///
    /// Names the part and, for a 413, its size.
    fn send_part(&self, part: u32, buf: Vec<u8>) -> Result<()> {
        let part_len = buf.len();
        let part_url = self.url(&["parts", &part.to_string()])?;
        let response = self
            .session
            .http
            .request_url(Method::PUT, part_url.clone(), &self.session.key)
            .timeout(Duration::from_secs(600))
            .header("Content-Type", "application/octet-stream")
            .body(buf)
            .send()
            .with_context(|| format!("PUT {part_url}"))?;
        let status = response.status();
        let text = response.text().unwrap_or_default();
        if looks_like_payload_too_large(status, &text) {
            return Err(HttpError::new(
                413,
                payload_too_large_message("asset upload part", Some(part_len)),
            )
            .into());
        }
        if !status.is_success() {
            return Err(HttpError::new(
                status.as_u16(),
                format!(
                    "asset part {part} failed (HTTP {status}): {}",
                    error_sentence(&text)
                ),
            )
            .into());
        }
        Ok(())
    }

    /// Tell the server every part is in and read its reply.
    fn complete(&self) -> Result<Asset> {
        let complete_url = self.url(&["complete"])?;
        let response = self
            .session
            .http
            .request_url(Method::POST, complete_url.clone(), &self.session.key)
            .timeout(Duration::from_secs(600))
            .send()
            .with_context(|| format!("POST {complete_url}"))?;
        let (status, text) = read_body("asset upload complete", response)?;
        ok_json::<Asset>("asset upload complete", status, &text)
    }

    /// Drop the upload on the server. Best effort: a failed abort only leaves
    /// a stale upload for the server to expire.
    fn abort(&self) {
        let Ok(url) = self.url(&[]) else {
            return;
        };
        let _ = self
            .session
            .http
            .request_url(Method::DELETE, url, &self.session.key)
            .timeout(Duration::from_secs(30))
            .send();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use message_crate_http::AuthInfo;

    const DIGEST: &str = "aa11";

    fn session(url: String) -> Session {
        Session {
            http: HttpSession::new().unwrap(),
            url,
            key: "mc-user-test".into(),
            username: "alice".into(),
            auth: AuthInfo {
                account_id: 1,
                username: Some("alice".into()),
            },
        }
    }

    /// The message `head_asset` gives when the server answers `HEAD` with `status`.
    fn head_asset_error(status: u16) -> String {
        let server = MockServer::start();
        let _head = server.mock(|when, then| {
            when.method("HEAD").path(format!("/v1/assets/{DIGEST}"));
            then.status(status);
        });
        session(server.base_url())
            .head_asset("sms-backup-restore", DIGEST)
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn head_asset_403_names_the_documented_causes_and_no_username() {
        let message = head_asset_error(403);
        assert!(message.contains("403 Forbidden"), "got {message}");
        assert!(message.contains("disabled"), "got {message}");
        assert!(
            message.contains("neither import nor export"),
            "got {message}"
        );
        assert!(!message.contains("username"), "got {message}");
        assert!(!message.contains("API key"), "got {message}");
    }

    #[test]
    fn head_asset_401_does_not_name_an_api_key() {
        let message = head_asset_error(401);
        assert!(message.contains("401 Unauthorized"), "got {message}");
        assert!(!message.contains("API key"), "got {message}");
    }

    #[test]
    fn payload_too_large_mentions_64_mib_import_chunks() {
        let msg = payload_too_large_message("import", Some(10));
        assert!(
            msg.contains("imports under 64 MiB"),
            "413 help must name the import chunk size, got {msg}"
        );
    }

    #[test]
    fn asset_url_encodes_segments_and_query() {
        let url = asset_url(
            "http://127.0.0.1:8080/",
            &["abc123", "uploads", "up 1"],
            "sms backup",
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "http://127.0.0.1:8080/v1/assets/abc123/uploads/up%201?source=sms+backup"
        );
    }
}
