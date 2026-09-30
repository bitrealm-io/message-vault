//! Import-time media rewrite before the content-addressed asset store.
//!
//! The modes and the conversions are the `media` crate's, the same ones the
//! desktop export applies, so an attachment converted on import and one
//! converted on export come out as the same bytes.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use media::{CompressOptions, Kind, MediaMode, TranscodeOutcome};

/// The file to store for one attachment, after the mode's transformation.
#[derive(Debug)]
pub struct ResolvedMedia {
    /// Path of the file to store in the vault.
    pub path: PathBuf,
    /// MIME type of the attachment, when known.
    pub mime_type: Option<String>,
}

impl ResolvedMedia {
    /// The source file, stored as it is.
    fn as_is(source_path: &Path, mime: Option<&str>) -> Self {
        Self {
            path: source_path.to_path_buf(),
            mime_type: mime.map(str::to_string),
        }
    }
}

/// Resolve the file bytes to store for one attachment.
///
/// Returns `Ok(None)` when the attachment should be omitted
/// ([`MediaMode::Disabled`]).
///
/// # Errors
///
/// Returns an error when a conversion fails, or when the mode needs ffmpeg
/// and the `media` crate cannot find it.
pub fn resolve_for_store(
    source_path: &Path,
    mime: Option<&str>,
    mode: MediaMode,
    work_dir: &Path,
) -> Result<Option<ResolvedMedia>> {
    match mode {
        MediaMode::Disabled => Ok(None),
        MediaMode::Clone => Ok(Some(ResolvedMedia::as_is(source_path, mime))),
        MediaMode::Convert | MediaMode::Compress => transform(source_path, mime, mode, work_dir),
    }
}

/// Convert or compress one media file by kind; anything else is stored as it is.
fn transform(
    source_path: &Path,
    mime: Option<&str>,
    mode: MediaMode,
    work_dir: &Path,
) -> Result<Option<ResolvedMedia>> {
    if !source_path.is_file() {
        return Ok(Some(ResolvedMedia::as_is(source_path, mime)));
    }
    let Some(kind) = media::kind_of(source_path, mime, &[]) else {
        return Ok(Some(ResolvedMedia::as_is(source_path, mime)));
    };
    let (tag, target) = match kind {
        Kind::Image => ("img", "jpg"),
        Kind::Video => ("vid", "mp4"),
        Kind::Audio => ("aud", "mp3"),
    };
    let out = work_dir.join(format!("{tag}-{}.{target}", stem_token(source_path)));
    let outcome =
        media::transcode_file_as(source_path, kind, &out, mode, &CompressOptions::default())
            .with_context(|| format!("convert {}", source_path.display()))?;
    Ok(Some(match outcome {
        TranscodeOutcome::Produced => ResolvedMedia {
            path: out,
            mime_type: media::mime_for_ext(target).map(str::to_string),
        },
        TranscodeOutcome::Skipped => ResolvedMedia {
            path: source_path.to_path_buf(),
            mime_type: mime.map(str::to_string).or_else(|| {
                source_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .and_then(media::mime_for_ext)
                    .map(str::to_string)
            }),
        },
    }))
}

/// A short, filesystem-safe token from the file stem, for temp file names.
fn stem_token(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("media")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .take(24)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stem_token_keeps_alphanumerics_and_caps_the_length() {
        assert_eq!(stem_token(Path::new("a/b/IMG_0001.HEIC")), "IMG_0001");
        assert_eq!(
            stem_token(Path::new("voice note (2).m4a")),
            "voice_note__2_"
        );
        assert_eq!(
            stem_token(Path::new("abcdefghijklmnopqrstuvwxyz0123456789.mp4")),
            "abcdefghijklmnopqrstuvwx"
        );
        assert_eq!(stem_token(Path::new("")), "media");
    }

    #[test]
    fn as_is_keeps_the_path_and_the_declared_mime() {
        let media = ResolvedMedia::as_is(Path::new("x/photo.heic"), Some("image/heic"));
        assert_eq!(media.path, PathBuf::from("x/photo.heic"));
        assert_eq!(media.mime_type.as_deref(), Some("image/heic"));
        let unknown = ResolvedMedia::as_is(Path::new("x/blob"), None);
        assert_eq!(unknown.mime_type, None);
    }

    #[test]
    fn disabled_stores_nothing() {
        let work = tempfile::tempdir().unwrap();
        let resolved = resolve_for_store(
            Path::new("x/photo.heic"),
            Some("image/heic"),
            MediaMode::Disabled,
            work.path(),
        )
        .unwrap();
        assert!(resolved.is_none());
    }

    #[test]
    fn clone_stores_the_source_as_it_is() {
        let work = tempfile::tempdir().unwrap();
        let resolved = resolve_for_store(
            Path::new("x/photo.heic"),
            Some("image/heic"),
            MediaMode::Clone,
            work.path(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(resolved.path, PathBuf::from("x/photo.heic"));
        assert_eq!(resolved.mime_type.as_deref(), Some("image/heic"));
    }

    #[test]
    fn convert_stores_a_missing_source_as_it_is() {
        let work = tempfile::tempdir().unwrap();
        let missing = work.path().join("gone.mp4");
        let resolved =
            resolve_for_store(&missing, Some("video/mp4"), MediaMode::Convert, work.path())
                .unwrap()
                .unwrap();
        assert_eq!(resolved.path, missing);
        assert_eq!(resolved.mime_type.as_deref(), Some("video/mp4"));
    }

    #[test]
    fn convert_stores_a_file_that_is_not_media_as_it_is() {
        let work = tempfile::tempdir().unwrap();
        let card = work.path().join("contact.vcf");
        std::fs::write(&card, b"BEGIN:VCARD\nEND:VCARD\n").unwrap();
        let resolved =
            resolve_for_store(&card, Some("text/vcard"), MediaMode::Compress, work.path())
                .unwrap()
                .unwrap();
        assert_eq!(resolved.path, card);
        assert_eq!(resolved.mime_type.as_deref(), Some("text/vcard"));
    }

    #[test]
    fn convert_stores_a_gif_as_it_is_because_animations_are_never_converted() {
        let work = tempfile::tempdir().unwrap();
        let gif = work.path().join("sticker.gif");
        std::fs::write(&gif, b"GIF89a").unwrap();
        let resolved = resolve_for_store(&gif, None, MediaMode::Convert, work.path())
            .unwrap()
            .unwrap();
        assert_eq!(resolved.path, gif);
        assert_eq!(resolved.mime_type, None);
    }
}
