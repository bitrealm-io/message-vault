//! Extract MIME attachment blobs from SMS Backup+ EML messages.

use crate::types::AttachmentBlob;
use mailparse::{MailHeaderMap, ParsedMail};
use message_vault_io_core::attachments::digest_prefix;
use regex::Regex;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::LazyLock;

use message_ir::valid_filename;

static SAFE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^\w.\-]+").expect("safe"));

/// File extension from the MIME type, falling back to the file name's own extension.
fn extension_for(ctype: &str, filename: Option<&str>) -> String {
    let ct = ctype.to_ascii_lowercase();
    if let Some(ext) = media::ext_for_mime(&ct) {
        return ext.into();
    }
    if let Some(valid) = filename.and_then(valid_filename)
        && let Some(ext) = Path::new(&valid).extension().and_then(|e| e.to_str())
    {
        return format!(".{}", ext.to_ascii_lowercase());
    }
    if ct.starts_with("image/") {
        ".jpg".into()
    } else if ct.starts_with("video/") {
        ".mp4".into()
    } else if ct.starts_with("audio/") {
        ".amr".into()
    } else {
        ".bin".into()
    }
}

/// Cap for the basename portion of generated attachment filenames. The name
/// is prefixed with ~46 bytes of file-key/timestamp/digest, so 160 keeps the
/// total well under ext4's 255-byte `NAME_MAX` and avoids ENAMETOOLONG.
const MAX_BASENAME_BYTES: usize = 160;

/// A file name with unsafe characters replaced by `_`, never empty.
fn safe_basename(name: &str) -> String {
    let cleaned = SAFE_RE.replace_all(name, "_");
    let trimmed = cleaned.trim_matches(|c| c == '.' || c == '_');
    if trimmed.is_empty() {
        return "attachment".into();
    }
    let mut base = trimmed.to_string();
    if base.len() > MAX_BASENAME_BYTES {
        // Keep a short extension, then truncate the stem on a char boundary
        // so multi-byte UTF-8 names don't end with a cut character.
        let (stem, ext) = match base.rfind('.') {
            Some(dot) if dot > 0 => (base[..dot].to_string(), base[dot..].to_string()),
            _ => (base.clone(), String::new()),
        };
        let budget = MAX_BASENAME_BYTES.saturating_sub(ext.len());
        let mut end = budget.min(stem.len());
        while end > 0 && !stem.is_char_boundary(end) {
            end -= 1;
        }
        base = format!("{}{}", &stem[..end], ext);
    }
    base
}

/// Collect every leaf MIME part.
fn walk_parts<'a>(mail: &'a ParsedMail<'a>, out: &mut Vec<&'a ParsedMail<'a>>) {
    if mail.subparts.is_empty() {
        out.push(mail);
    } else {
        for part in &mail.subparts {
            walk_parts(part, out);
        }
    }
}

/// Decode non-text MIME parts into attachment blobs.
pub(crate) fn extract_attachments(
    mail: &ParsedMail<'_>,
    timestamp_ms: f64,
    file_key: Option<&str>,
) -> Vec<AttachmentBlob> {
    // Filename prefix only — fall back to epoch rather than panic on bad stamps.
    let date_prefix = crate::identity::local_datetime_from_secs((timestamp_ms / 1000.0) as i64)
        .unwrap_or_else(|| chrono::DateTime::UNIX_EPOCH.with_timezone(&chrono::Local))
        .format("%Y%m%d_%H%M%S")
        .to_string();
    let name_prefix = file_key.map(|k| format!("{k}_")).unwrap_or_default();

    let mut parts = Vec::new();
    walk_parts(mail, &mut parts);

    let mut out = Vec::new();
    let mut seq = 0u32;
    for part in parts {
        let ctype = part.ctype.mimetype.to_ascii_lowercase();
        if ctype.starts_with("multipart/") || ctype.starts_with("text/") {
            continue;
        }
        let payload = match part.get_body_raw() {
            Ok(p) if !p.is_empty() => p,
            _ => continue,
        };
        seq += 1;
        let filename = part
            .get_content_disposition()
            .params
            .get("filename")
            .cloned()
            .or_else(|| part.headers.get_first_value("Content-Type").and(None));
        // Prefer Content-Disposition filename
        let original = filename.as_deref().and_then(valid_filename).or_else(|| {
            part.get_content_disposition()
                .params
                .get("name")
                .and_then(|n| valid_filename(n))
        });
        let ext = extension_for(&ctype, original.as_deref());
        // Content-addressed prefix: re-exports with different bytes get a new path
        // instead of leaving stale attachment files under the old name.
        let digest_hex = hex::encode(Sha256::digest(&payload));
        let digest_prefix = digest_prefix(&digest_hex);
        let out_name = if let Some(ref orig) = original {
            format!(
                "{name_prefix}{date_prefix}_{digest_prefix}_{}",
                safe_basename(orig)
            )
        } else {
            format!("{name_prefix}{date_prefix}_{digest_prefix}_{seq}{ext}")
        };
        out.push(AttachmentBlob {
            filename: out_name,
            original_name: original,
            mime_type: media::mime_for_ext(&ext)
                .map(|s| s.to_string())
                .or(if ctype.is_empty() { None } else { Some(ctype) }),
            digest_hex,
            data: payload,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_basename_caps_length_and_keeps_extension() {
        let base = safe_basename(&format!("{}.jpg", "a".repeat(400)));
        assert!(base.len() <= MAX_BASENAME_BYTES);
        assert!(base.ends_with(".jpg"));
        assert!(base.len() > 100);
    }

    #[test]
    fn safe_basename_truncates_on_char_boundary() {
        // 60 CJK chars = 180 bytes; truncation must not split a character.
        let name = "中".repeat(60);
        let base = safe_basename(&name);
        assert!(base.len() <= MAX_BASENAME_BYTES);
        assert!(
            base.len().is_multiple_of(3),
            "char boundary truncation failed"
        );
        assert!(base.chars().all(|c| c == '中'));
    }

    #[test]
    fn safe_basename_short_names_unchanged() {
        assert_eq!(safe_basename("photo.jpg"), "photo.jpg");
        assert_eq!(safe_basename("a b/c"), "a_b_c");
    }
}
