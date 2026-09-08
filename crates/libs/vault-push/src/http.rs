//! HTTP helpers for attachment upload and JSON Lines message import.
//!
//! JSON Lines means one JSON object per line. Calls are blocking so they can
//! run on worker threads without an async runtime. Login lives in
//! [`vault_http::auth_check`]; the session type here is
//! [`vault_http::HttpSession`].

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::time::Duration;
use vault_api_types::ImportMode;

use anyhow::{Context, Result, anyhow};
use reqwest::Method;
use serde::Deserialize;
use vault_http::{VaultHttpError, error_sentence, looks_like_html, ok_json, trim_base_url};

pub use vault_http::HttpSession;

use crate::run::Session;

#[derive(Debug, Deserialize)]
/// Vault reply after HEAD or PUT of one attachment.
pub struct AssetPutResponse {
    #[serde(default)]
    pub already_present: bool,
}

#[derive(Debug, Deserialize)]
/// Vault reply after posting one JSON Lines import batch.
pub struct ImportResponse {
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
    pub ok: bool,
    /// `completed`, `completed_with_issues`, or `failed`.
    pub status: &'a str,
    pub message_count: u64,
    pub attachment_count: u64,
    pub bytes_uploaded: u64,
}

/// Body of the `/v1/imports` and `/v1/imports/{id}/complete` replies.
#[derive(Debug, Deserialize)]
struct ImportRunResponse {
    id: i64,
}

#[derive(Debug, Deserialize)]
struct UploadStartResponse {
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
         vault-push chunks message imports under 64 MiB and large assets via multipart; \
         if this still fails, raise nginx client_max_body_size for /v1 (need ≥100m for 64 MiB parts) \
         or tunnel to vault :8080."
    )
}

/// Build `{base}/v1/assets/...` with extra path segments (percent-encoded)
/// and the `source=` query every asset route takes. The account is not a
/// parameter: the API key names it.
fn asset_url(base_url: &str, segments: &[&str], source: &str) -> Result<reqwest::Url> {
    let base = trim_base_url(base_url);
    let mut url = reqwest::Url::parse(base).with_context(|| format!("invalid vault URL {base}"))?;
    url.path_segments_mut()
        .map_err(|()| anyhow!("invalid vault URL {base}"))?
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

    /// Whether the vault already holds the attachment with this digest.
    /// `None` means it does not (404); `Some` carries the vault's reply.
    ///
    /// # Errors
    ///
    /// Returns an error for a bad key (401), a username that does not match
    /// the key (403), or any other failure.
    pub(crate) fn head_asset(
        &self,
        source: &str,
        sha256: &str,
    ) -> Result<Option<AssetPutResponse>> {
        let url = self.asset_url(source, &[sha256])?;
        let response = self
            .http
            .request_url(Method::HEAD, url.clone(), &self.key)
            .timeout(Duration::from_secs(15))
            .send()
            .with_context(|| format!("HEAD {url}"))?;
        let status = response.status();
        match status.as_u16() {
            404 => return Ok(None),
            401 => return Err(VaultHttpError::new(401, "invalid vault key").into()),
            403 => {
                return Err(VaultHttpError::new(403, "username does not match vault key").into());
            }
            _ => {}
        }
        if !status.is_success() {
            let text = response.text().unwrap_or_default();
            return Err(VaultHttpError::new(
                status.as_u16(),
                format!(
                    "asset HEAD failed (HTTP {status}): {}",
                    error_sentence(&text)
                ),
            )
            .into());
        }
        let assumed_present = AssetPutResponse {
            already_present: true,
        };
        let text = response.text().unwrap_or_default();
        if text.trim().is_empty() {
            return Ok(Some(assumed_present));
        }
        let Ok(parsed) = serde_json::from_str::<AssetPutResponse>(&text) else {
            return Ok(Some(assumed_present));
        };
        if !parsed.already_present {
            return Ok(None);
        }
        Ok(Some(parsed))
    }

    /// Upload one attachment: in one PUT, or in parts when the file is larger
    /// than its threshold.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read or the vault rejects it;
    /// a 413 says how large a body the vault accepts.
    pub(crate) fn put_asset(&self, asset: &AssetUpload<'_>) -> Result<AssetPutResponse> {
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
        let status = response.status();
        let text = response.text().context("read asset response")?;
        if looks_like_payload_too_large(status, &text) {
            return Err(VaultHttpError::new(
                413,
                payload_too_large_message("asset upload", Some(file_len as usize)),
            )
            .into());
        }
        ok_json::<AssetPutResponse>("asset upload", status, &text)
    }

    /// Upload in parts: open a multipart upload, send each part, complete
    /// it. A part or completion that fails aborts the upload on the vault.
    fn put_asset_multipart(
        &self,
        asset: &AssetUpload<'_>,
        file_len: u64,
    ) -> Result<AssetPutResponse> {
        let Some(upload) = MultipartUpload::start(self, asset, file_len)? else {
            return Ok(AssetPutResponse {
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
    /// and the vault's error otherwise.
    pub(crate) fn post_import(&self, import_id: i64, ndjson: Vec<u8>) -> Result<ImportResponse> {
        let body_len = ndjson.len();
        if body_len > crate::run::MAX_PROXY_BODY_BYTES {
            return Err(VaultHttpError::new(
                413,
                payload_too_large_message("import", Some(body_len)),
            )
            .into());
        }
        let path = format!("/v1/imports/{import_id}/batches");
        let response = self
            .http
            .vault_request(Method::POST, &self.url, &path, &self.key)
            .timeout(Duration::from_secs(600))
            .header("Content-Type", "application/jsonl")
            .body(ndjson)
            .send()
            .with_context(|| format!("POST {path}"))?;
        let status = response.status();
        let text = response.text().context("read import response")?;
        if looks_like_payload_too_large(status, &text) {
            return Err(VaultHttpError::new(
                413,
                payload_too_large_message("import", Some(body_len)),
            )
            .into());
        }
        ok_json::<ImportResponse>("import batch", status, &text)
    }

    /// Create an Import Run on the vault and return its id. Every batch is
    /// posted into it; the bearer token names the account.
    ///
    /// # Errors
    ///
    /// Returns an error when the vault refuses, which includes an account
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
            .vault_request(Method::POST, &self.url, "/v1/imports", &self.key)
            .timeout(Duration::from_secs(60))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .context("POST /v1/imports")?;
        let status = response.status();
        let text = response.text().context("read start-import response")?;
        let parsed: ImportRunResponse = ok_json("import run", status, &text)?;
        Ok(parsed.id)
    }

    /// Record how the Import Run ended.
    ///
    /// # Errors
    ///
    /// Returns an error when the vault refuses.
    pub(crate) fn complete_import(
        &self,
        import_id: i64,
        outcome: &ImportOutcome<'_>,
    ) -> Result<()> {
        let body = serde_json::json!({
            "ok": outcome.ok,
            "status": outcome.status,
            "message_count": outcome.message_count,
            "attachment_count": outcome.attachment_count,
            "bytes_uploaded": outcome.bytes_uploaded,
        });
        let response = self
            .http
            .vault_request(
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
        let status = response.status();
        let text = response.text().context("read complete-import response")?;
        let _: ImportRunResponse = ok_json("import run complete", status, &text)?;
        Ok(())
    }
}

/// A multipart upload the vault has opened for one attachment.
struct MultipartUpload<'a> {
    session: &'a Session,
    source: &'a str,
    sha256: &'a str,
    upload_id: String,
    /// Bytes per part, as the vault asked.
    part_size: usize,
}

impl<'a> MultipartUpload<'a> {
    /// Open the upload. `None` when the vault says it already has the file.
    ///
    /// # Errors
    ///
    /// Returns an error when the vault refuses or its reply lacks an upload
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
        let status = response.status();
        let text = response.text().context("read upload start response")?;
        if looks_like_payload_too_large(status, &text) {
            return Err(VaultHttpError::new(
                413,
                payload_too_large_message("asset upload start", None),
            )
            .into());
        }
        let started: UploadStartResponse = ok_json("asset upload start", status, &text)?;
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
            return Err(VaultHttpError::new(
                413,
                payload_too_large_message("asset upload part", Some(part_len)),
            )
            .into());
        }
        if !status.is_success() {
            return Err(VaultHttpError::new(
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

    /// Tell the vault every part is in and read its reply.
    fn complete(&self) -> Result<AssetPutResponse> {
        let complete_url = self.url(&["complete"])?;
        let response = self
            .session
            .http
            .request_url(Method::POST, complete_url.clone(), &self.session.key)
            .timeout(Duration::from_secs(600))
            .send()
            .with_context(|| format!("POST {complete_url}"))?;
        let status = response.status();
        let text = response.text().context("read upload complete response")?;
        ok_json::<AssetPutResponse>("asset upload complete", status, &text)
    }

    /// Drop the upload on the vault. Best effort: a failed abort only leaves
    /// a stale upload for the vault to expire.
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
