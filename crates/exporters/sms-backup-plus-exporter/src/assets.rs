//! Read the text and attachment blobs of SMS Backup+ EML messages.

use crate::types::AttachmentBlob;
use mailparse::{MailHeaderMap, ParsedMail};
use message_crate_core::attachments::{attachment_date_prefix, digest_prefix};
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

/// The message text and attachments a mail's parts make.
#[derive(Debug, Default)]
pub(crate) struct MailBody {
    /// The `text/plain` parts joined with a newline.
    pub text: String,
    /// Every other part with content.
    pub attachments: Vec<AttachmentBlob>,
    /// Parts dropped because their content could not be decoded.
    pub unreadable_parts: u64,
}

/// What one leaf part holds, decoded once.
enum Payload {
    None,
    Text(String),
    Bytes(Vec<u8>),
    Unreadable,
}

/// One leaf part's content: the decoded text of a `text/plain` part, with
/// newlines normalized to `\n`, and the bytes of any other part.
fn payload(part: &ParsedMail<'_>) -> Payload {
    if mms_parts::is_text(&part.ctype.mimetype) {
        return match part.get_body() {
            Ok(body) => Payload::Text(body.replace("\r\n", "\n").replace('\r', "\n")),
            Err(_) => Payload::Unreadable,
        };
    }
    match part.get_body_raw() {
        Ok(bytes) if bytes.is_empty() => Payload::None,
        Ok(bytes) => Payload::Bytes(bytes),
        Err(_) => Payload::Unreadable,
    }
}

/// Every key a SMIL part may name a leaf part by: its file name, its name,
/// its Content-ID and its Content-Location.
fn part_keys(part: &ParsedMail<'_>) -> Vec<String> {
    let disposition = part.get_content_disposition();
    [
        disposition.params.get("filename").cloned(),
        disposition.params.get("name").cloned(),
        part.ctype.params.get("name").cloned(),
        part.headers.get_first_value("Content-ID"),
        part.headers.get_first_value("Content-Location"),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The text and attachments of a mail, by the rules of [`mms_parts`]: every
/// `text/plain` part is text, and every other part with content, a contact
/// card included, is an attachment.
///
/// SMS Backup+ writes each part of an MMS as one MIME part, and the message
/// body as `text/plain` on every mail: zero of 20,000 sampled carry a
/// `text/html` part.
pub(crate) fn extract_body(
    mail: &ParsedMail<'_>,
    timestamp_ms: f64,
    file_key: Option<&str>,
) -> MailBody {
    let mut leaves = Vec::new();
    walk_parts(mail, &mut leaves);
    leaves.retain(|part| {
        !part
            .ctype
            .mimetype
            .to_ascii_lowercase()
            .starts_with("multipart/")
    });
    let payloads: Vec<Payload> = leaves.iter().map(|part| payload(part)).collect();
    let keys: Vec<Vec<String>> = leaves.iter().map(|part| part_keys(part)).collect();
    let shaped: Vec<mms_parts::Part<'_>> = leaves
        .iter()
        .zip(&payloads)
        .zip(&keys)
        .map(|((part, payload), keys)| mms_parts::Part {
            content_type: &part.ctype.mimetype,
            keys: keys.iter().map(String::as_str).collect(),
            content: match payload {
                Payload::None => mms_parts::Content::None,
                Payload::Text(text) => mms_parts::Content::Text(text.clone()),
                Payload::Bytes(bytes) => mms_parts::Content::Bytes(bytes),
                Payload::Unreadable => mms_parts::Content::Unreadable,
            },
        })
        .collect();
    let body = mms_parts::body_of(&shaped);

    // UTC, so the name is the same on every machine that exports this mail.
    let date_prefix = attachment_date_prefix((timestamp_ms / 1000.0) as i64);
    let name_prefix = file_key.map(|k| format!("{k}_")).unwrap_or_default();
    let prefix = format!("{name_prefix}{date_prefix}");
    let attachments = body
        .attachments
        .iter()
        .zip(1u32..)
        .filter_map(|(&index, seq)| {
            let data = match &payloads[index] {
                Payload::Bytes(bytes) => bytes.clone(),
                Payload::Text(text) => text.clone().into_bytes(),
                Payload::None | Payload::Unreadable => return None,
            };
            Some(attachment_blob(leaves[index], data, &prefix, seq))
        })
        .collect();
    MailBody {
        text: body.text,
        attachments,
        unreadable_parts: body.unreadable.len() as u64,
    }
}

/// One attachment blob. `prefix` is the file key and the date, and `seq` the
/// attachment's position, which names a part that has no file name.
fn attachment_blob(part: &ParsedMail<'_>, data: Vec<u8>, prefix: &str, seq: u32) -> AttachmentBlob {
    let ctype = part.ctype.mimetype.to_ascii_lowercase();
    let disposition = part.get_content_disposition();
    let original = disposition
        .params
        .get("filename")
        .and_then(|n| valid_filename(n))
        .or_else(|| {
            disposition
                .params
                .get("name")
                .and_then(|n| valid_filename(n))
        })
        .or_else(|| {
            part.ctype
                .params
                .get("name")
                .and_then(|n| valid_filename(n))
        });
    let ext = extension_for(&ctype, original.as_deref());
    // Content-addressed prefix: re-exports with different bytes get a new path
    // instead of leaving stale attachment files under the old name.
    let digest_hex = hex::encode(Sha256::digest(&data));
    let digest_prefix = digest_prefix(&digest_hex);
    let filename = match &original {
        Some(orig) => format!("{prefix}_{digest_prefix}_{}", safe_basename(orig)),
        None => format!("{prefix}_{digest_prefix}_{seq}{ext}"),
    };
    AttachmentBlob {
        filename,
        original_name: original,
        mime_type: media::mime_for_ext(&ext)
            .map(|s| s.to_string())
            .or(if ctype.is_empty() { None } else { Some(ctype) }),
        digest_hex,
        data,
    }
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

    /// The date in the filename is the message's instant in UTC, not the
    /// exporting machine's clock: 2024-03-15 23:59:59 UTC stays the 15th
    /// even on a machine already into the 16th.
    #[test]
    fn attachment_filenames_carry_the_utc_date() {
        let raw = concat!(
            "Content-Type: multipart/mixed; boundary=\"b\"\r\n",
            "\r\n",
            "--b\r\n",
            "Content-Type: image/jpeg\r\n",
            "Content-Disposition: attachment; filename=\"photo.jpg\"\r\n",
            "Content-Transfer-Encoding: base64\r\n",
            "\r\n",
            "/9j/4AAQ\r\n",
            "--b--\r\n",
        );
        let mail = mailparse::parse_mail(raw.as_bytes()).expect("parse");
        let blobs = extract_body(&mail, 1_710_547_199_000.0, Some("abc")).attachments;
        assert_eq!(blobs.len(), 1);
        assert!(
            blobs[0].filename.starts_with("abc_20240315_235959_"),
            "got {}",
            blobs[0].filename
        );
        assert!(blobs[0].filename.ends_with("_photo.jpg"));
    }
}
