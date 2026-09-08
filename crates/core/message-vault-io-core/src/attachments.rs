//! Shared content-addressed attachment naming and idempotent file writes.

use anyhow::{Context, Result};
use chrono::{Local, TimeZone};
use std::path::Path;

/// First 16 hex digits of a SHA-256 digest (content-addressed path prefix).
pub fn digest_prefix(digest_hex: &str) -> &str {
    &digest_hex[..16.min(digest_hex.len())]
}

/// `YYYYMMDD_HHMMSS` in local time for attachment file names, or the raw seconds when the time cannot be represented.
fn date_prefix(timestamp_secs: i64) -> String {
    Local.timestamp_opt(timestamp_secs, 0).single().map_or_else(
        || timestamp_secs.to_string(),
        |t| t.format("%Y%m%d_%H%M%S").to_string(),
    )
}

/// Content-addressed attachment filename: `{local-date}-{digest16}{ext}`.
pub fn attachment_dest_name(timestamp_secs: i64, digest_hex: &str, ext: &str) -> String {
    format!(
        "{}-{}{}",
        date_prefix(timestamp_secs),
        digest_prefix(digest_hex),
        ext
    )
}

/// Write `bytes` to `path` only when the file does not exist.
///
/// Returns `true` when the file was written.
///
/// # Errors
///
/// Returns an error when the write fails.
pub fn write_if_missing(path: &Path, bytes: &[u8]) -> Result<bool> {
    if path.exists() {
        return Ok(false);
    }
    std::fs::write(path, bytes)?;
    Ok(true)
}

/// Copy `src` to `dest` only when `dest` does not exist.
///
/// Returns `true` when the copy happened.
///
/// # Errors
///
/// Returns an error when the copy fails.
pub fn copy_if_missing(src: &Path, dest: &Path) -> Result<bool> {
    if dest.exists() {
        return Ok(false);
    }
    std::fs::copy(src, dest)
        .with_context(|| format!("copy {} to {}", src.display(), dest.display()))?;
    Ok(true)
}

#[cfg(test)]
mod tests;
