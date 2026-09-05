//! The one extension ↔ MIME table for attachment files.
//!
//! Every place that maps a file extension to a MIME type (exporters, the
//! export pipeline, the vault server) or a MIME type back to an extension
//! (asset uploads, EML attachment naming) goes through this module, so the
//! mappings cannot drift apart. [`classify`](crate::classify) reads the same
//! table, keeping "what is an image/video/audio file" and "what MIME does
//! this extension carry" consistent by construction.

/// Media kind of a recognized attachment extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Still image (convert target: `.jpg`).
    Image,
    /// Video (convert target: `.mp4`).
    Video,
    /// Audio (convert target: `.mp3`).
    Audio,
}

/// One row per extension: dotted lowercase extension, MIME type, media kind
/// (`None` for non-media formats the vault still wants MIME types for).
///
/// The **first** row carrying a given MIME type is the canonical extension
/// [`ext_for_mime`] returns for it.
const EXT_TABLE: &[(&str, &str, Option<Kind>)] = &[
    // Images
    (".jpg", "image/jpeg", Some(Kind::Image)),
    (".jpeg", "image/jpeg", Some(Kind::Image)),
    (".png", "image/png", Some(Kind::Image)),
    (".gif", "image/gif", Some(Kind::Image)),
    (".webp", "image/webp", Some(Kind::Image)),
    (".heic", "image/heic", Some(Kind::Image)),
    (".heif", "image/heic", Some(Kind::Image)),
    (".bmp", "image/bmp", Some(Kind::Image)),
    (".tif", "image/tiff", Some(Kind::Image)),
    (".tiff", "image/tiff", Some(Kind::Image)),
    // Video
    (".mp4", "video/mp4", Some(Kind::Video)),
    (".m4v", "video/mp4", Some(Kind::Video)),
    (".mov", "video/quicktime", Some(Kind::Video)),
    (".3gp", "video/3gpp", Some(Kind::Video)),
    (".3gpp", "video/3gpp", Some(Kind::Video)),
    (".3g2", "video/3gpp", Some(Kind::Video)),
    (".webm", "video/webm", Some(Kind::Video)),
    (".mkv", "video/x-matroska", Some(Kind::Video)),
    (".mpeg", "video/mpeg", Some(Kind::Video)),
    (".mpg", "video/mpeg", Some(Kind::Video)),
    (".avi", "video/x-msvideo", Some(Kind::Video)),
    // Audio
    (".mp3", "audio/mpeg", Some(Kind::Audio)),
    (".m4a", "audio/mp4", Some(Kind::Audio)),
    (".aac", "audio/mp4", Some(Kind::Audio)),
    (".caf", "audio/x-caf", Some(Kind::Audio)),
    (".amr", "audio/amr", Some(Kind::Audio)),
    (".wav", "audio/wav", Some(Kind::Audio)),
    (".ogg", "audio/ogg", Some(Kind::Audio)),
    (".oga", "audio/ogg", Some(Kind::Audio)),
    (".opus", "audio/opus", Some(Kind::Audio)),
    // Non-media formats phone backups carry (contact cards, scans).
    (".pdf", "application/pdf", None),
    (".vcf", "text/vcard", None),
];

/// MIME spellings seen in the wild mapped to the spelling [`EXT_TABLE`] uses.
const MIME_ALIASES: &[(&str, &str)] = &[
    ("image/jpg", "image/jpeg"),
    ("image/heif", "image/heic"),
    ("video/3gp", "video/3gpp"),
    ("audio/mp3", "audio/mpeg"),
    ("audio/aac", "audio/mp4"),
    ("audio/x-wav", "audio/wav"),
];

/// MIME type for a file extension, if known.
///
/// `ext` may be dotted or bare (`".jpg"` / `"jpg"`) and is matched
/// case-insensitively.
pub fn mime_for_ext(ext: &str) -> Option<&'static str> {
    let bare = ext.trim_start_matches('.');
    if bare.is_empty() {
        return None;
    }
    EXT_TABLE
        .iter()
        .find(|(e, _, _)| bare.eq_ignore_ascii_case(&e[1..]))
        .map(|(_, mime, _)| *mime)
}

/// Canonical dotted extension (`".jpg"`) for a MIME type, if known.
///
/// Parameters after `;` are ignored, matching is case-insensitive, and common
/// alias spellings (`image/jpg`, `audio/mp3`, `audio/x-wav`, …) resolve to the
/// same extension as their canonical MIME type.
pub fn ext_for_mime(mime: &str) -> Option<&'static str> {
    let base = mime
        .split(';')
        .next()
        .unwrap_or(mime)
        .trim()
        .to_ascii_lowercase();
    let base = MIME_ALIASES
        .iter()
        .find(|(alias, _)| *alias == base)
        .map_or(base.as_str(), |(_, canonical)| canonical);
    EXT_TABLE
        .iter()
        .find(|(_, m, _)| *m == base)
        .map(|(ext, _, _)| *ext)
}

/// Media kind for a MIME type by its top-level type: `image/*`, `video/*`,
/// `audio/*`; `None` for anything else. Parameters after `;` are ignored.
/// For a file stored without an extension, this is what the caller has.
pub fn kind_for_mime(mime: &str) -> Option<Kind> {
    let base = mime.split(';').next().unwrap_or("").trim();
    let top = base.split('/').next().unwrap_or("");
    match top.to_ascii_lowercase().as_str() {
        "image" => Some(Kind::Image),
        "video" => Some(Kind::Video),
        "audio" => Some(Kind::Audio),
        _ => None,
    }
}

/// Media kind for a file extension (dotted or bare, case-insensitive);
/// `None` for extensions the media pass does not process (including `.pdf`
/// and `.vcf`, which have MIME types but are not media).
pub fn kind_for_ext(ext: &str) -> Option<Kind> {
    let bare = ext.trim_start_matches('.');
    if bare.is_empty() {
        return None;
    }
    EXT_TABLE
        .iter()
        .find(|(e, _, _)| bare.eq_ignore_ascii_case(&e[1..]))
        .and_then(|(_, _, kind)| *kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_extension_maps_forward() {
        for (ext, mime, _) in EXT_TABLE {
            assert_eq!(mime_for_ext(ext), Some(*mime), "dotted {ext}");
            assert_eq!(mime_for_ext(&ext[1..]), Some(*mime), "bare {ext}");
            assert_eq!(
                mime_for_ext(&ext.to_ascii_uppercase()),
                Some(*mime),
                "uppercase {ext}"
            );
        }
    }

    #[test]
    fn forward_table_pins_current_call_site_outputs() {
        // Every mapping the pre-unification tables carried, verbatim.
        for (ext, mime) in [
            // media::mime_for_ext (exporters)
            ("jpg", "image/jpeg"),
            ("jpeg", "image/jpeg"),
            ("png", "image/png"),
            ("gif", "image/gif"),
            ("mp4", "video/mp4"),
            ("3gp", "video/3gpp"),
            ("amr", "audio/amr"),
            // io-core mime_for_rel extras
            ("webp", "image/webp"),
            ("m4v", "video/mp4"),
            ("mov", "video/quicktime"),
            ("mp3", "audio/mpeg"),
            ("m4a", "audio/mp4"),
            // server guess_mime extras
            ("heic", "image/heic"),
            ("heif", "image/heic"),
            ("bmp", "image/bmp"),
            ("tif", "image/tiff"),
            ("tiff", "image/tiff"),
            ("3gpp", "video/3gpp"),
            ("3g2", "video/3gpp"),
            ("webm", "video/webm"),
            ("mkv", "video/x-matroska"),
            ("mpeg", "video/mpeg"),
            ("mpg", "video/mpeg"),
            ("avi", "video/x-msvideo"),
            ("aac", "audio/mp4"),
            ("caf", "audio/x-caf"),
            ("wav", "audio/wav"),
            ("ogg", "audio/ogg"),
            ("oga", "audio/ogg"),
            ("pdf", "application/pdf"),
            ("vcf", "text/vcard"),
            // go-sms-pro chained entry
            ("wav", "audio/wav"),
            // New: classify recognized .opus but no table mapped it.
            ("opus", "audio/opus"),
        ] {
            assert_eq!(mime_for_ext(ext), Some(mime), "unexpected MIME for .{ext}");
        }
        assert_eq!(mime_for_ext("docx"), None);
        assert_eq!(mime_for_ext(""), None);
        assert_eq!(mime_for_ext("."), None);
    }

    #[test]
    fn reverse_table_pins_current_call_site_outputs() {
        for (mime, ext) in [
            // server asset_uploads ext_for_mime
            ("image/jpeg", ".jpg"),
            ("image/png", ".png"),
            ("image/gif", ".gif"),
            ("image/webp", ".webp"),
            ("image/heic", ".heic"),
            ("image/heif", ".heic"),
            ("video/mp4", ".mp4"),
            ("video/quicktime", ".mov"),
            ("video/webm", ".webm"),
            ("audio/mpeg", ".mp3"),
            ("audio/mp3", ".mp3"),
            ("audio/mp4", ".m4a"),
            ("audio/aac", ".m4a"),
            ("audio/wav", ".wav"),
            ("audio/x-wav", ".wav"),
            // sms-backup-plus extension_for
            ("image/jpg", ".jpg"),
            ("video/3gpp", ".3gp"),
            ("video/3gp", ".3gp"),
            ("audio/amr", ".amr"),
        ] {
            assert_eq!(ext_for_mime(mime), Some(ext), "unexpected ext for {mime}");
        }
        // Previously unmapped MIME types now resolve via the shared table.
        assert_eq!(ext_for_mime("video/x-matroska"), Some(".mkv"));
        assert_eq!(ext_for_mime("image/tiff"), Some(".tif"));
        assert_eq!(ext_for_mime("application/pdf"), Some(".pdf"));
        assert_eq!(ext_for_mime("application/octet-stream"), None);
        assert_eq!(ext_for_mime(""), None);
    }

    #[test]
    fn reverse_lookup_normalizes_params_and_case() {
        assert_eq!(ext_for_mime("image/jpeg; charset=binary"), Some(".jpg"));
        assert_eq!(ext_for_mime(" IMAGE/JPEG "), Some(".jpg"));
    }

    #[test]
    fn kinds_match_classify() {
        use std::path::Path;
        for (ext, _, kind) in EXT_TABLE {
            assert_eq!(kind_for_ext(ext), *kind, "{ext}");
            assert_eq!(
                crate::classify(&Path::new("f").with_extension(&ext[1..])),
                *kind,
                "classify disagrees with the table for {ext}"
            );
        }
        assert_eq!(kind_for_ext("pdf"), None);
        assert_eq!(kind_for_ext("vcf"), None);
        assert_eq!(kind_for_ext("opus"), Some(Kind::Audio));
        assert_eq!(kind_for_ext("3g2"), Some(Kind::Video));
        assert_eq!(kind_for_ext("oga"), Some(Kind::Audio));
    }
}
