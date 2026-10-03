//! Shared content-addressed attachment naming.

use chrono::{TimeZone, Utc};

/// First 16 hex digits of a SHA-256 digest (content-addressed path prefix).
pub fn digest_prefix(digest_hex: &str) -> &str {
    &digest_hex[..16.min(digest_hex.len())]
}

/// `YYYYMMDD_HHMMSS` in UTC for attachment file names, or the raw seconds when
/// the time cannot be represented.
///
/// UTC and not the host's zone: the name is an identifier, so the same
/// attachment has to get the same name on every machine that exports it.
pub fn attachment_date_prefix(timestamp_secs: i64) -> String {
    Utc.timestamp_opt(timestamp_secs, 0).single().map_or_else(
        || timestamp_secs.to_string(),
        |t| t.format("%Y%m%d_%H%M%S").to_string(),
    )
}

/// Content-addressed attachment filename: `{utc-date}-{digest16}{ext}`.
pub fn attachment_dest_name(timestamp_secs: i64, digest_hex: &str, ext: &str) -> String {
    format!(
        "{}-{}{}",
        attachment_date_prefix(timestamp_secs),
        digest_prefix(digest_hex),
        ext
    )
}

#[cfg(test)]
mod tests;
