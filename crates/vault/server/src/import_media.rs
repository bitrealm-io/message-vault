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
    let Some(kind) = kind_of(source_path, mime) else {
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

/// Media kind from the file's extension, else from the declared MIME type.
/// GIFs are animations and are never converted, so they answer `None`.
fn kind_of(source_path: &Path, mime: Option<&str>) -> Option<Kind> {
    let gif_name = source_path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("gif"));
    if gif_name || mime == Some("image/gif") {
        return None;
    }
    media::classify(source_path).or_else(|| mime.and_then(media::kind_for_mime))
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
