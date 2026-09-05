//! Convert or compress attachment media under a converter export directory.
//!
//! Modes:
//! - **Disabled** — do not copy or write attachment files (CLI exporters)
//! - **Clone** — leave exported files as-is (a no-op after export)
//! - **Convert** — rewrite images to `.jpg`, videos to `.mp4`, audio to `.mp3`
//! - **Compress** — re-encode to shrink files, with optional video settings
//!
//! Convert and compress need `ffmpeg` / `ffprobe` beside the running binary,
//! in `MESSAGE_VAULT_IO_BIN`, or on `PATH`.

#![warn(missing_docs)]

mod estimate;
mod mime;
mod probe;
mod process;
mod size;
mod tools;

pub use estimate::{SizeVerdict, classify_probed, estimate_bytes, needs_probe};
pub use mime::{Kind, ext_for_mime, kind_for_ext, kind_for_mime, mime_for_ext};
pub use probe::{MediaProbe, probe_media};
pub use process::{
    MediaReport, TranscodeOutcome, classify, collect_media_files, derivative_name,
    derivative_name_for_missing, format_bytes, process_attachment_files, transcode_file,
    transcode_file_as,
};
use size::parse_size;
pub use tools::{FfmpegToolsProbe, ffmpeg_available, probe_ffmpeg_tools, set_tools_dir, tools_dir};

use std::fmt;
use std::str::FromStr;

/// Attachment media handling after export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MediaMode {
    /// Do not write attachment files during export.
    Disabled,
    /// Copy attachment files through unchanged; the default (a no-op after export).
    #[default]
    Clone,
    /// Rewrite images to `.jpg`, videos to `.mp4`, audio to `.mp3`.
    Convert,
    /// Re-encode attachments to shrink them per `CompressOptions`.
    Compress,
}

impl MediaMode {
    /// Canonical lowercase CLI string (`disabled` / `clone` / `convert` / `compress`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Clone => "clone",
            Self::Convert => "convert",
            Self::Compress => "compress",
        }
    }

    /// Parse a CLI string (case- and whitespace-insensitive); `None` for unknown input.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "disabled" | "none" | "skip" => Some(Self::Disabled),
            "clone" | "copy" => Some(Self::Clone),
            "convert" => Some(Self::Convert),
            "compress" => Some(Self::Compress),
            _ => None,
        }
    }

    /// True when the mode requires ffmpeg/ffprobe (Convert or Compress).
    pub fn needs_tools(self) -> bool {
        matches!(self, Self::Convert | Self::Compress)
    }

    /// Whether exporters should write bytes under `attachments/`.
    pub fn copies_attachments(self) -> bool {
        !matches!(self, Self::Disabled)
    }
}

impl fmt::Display for MediaMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for MediaMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or_else(|| {
            format!("invalid media-mode '{s}' (expected disabled, clone, convert, or compress)")
        })
    }
}

/// Max long-edge cap for video compress (no upscale).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MaxResolution {
    /// Cap the video long edge at 1280 px.
    P720,
    /// Cap the video long edge at 1920 px; the default.
    #[default]
    P1080,
    /// Cap the video long edge at 3840 px.
    P4k,
}

impl MaxResolution {
    /// Canonical string (`720p` / `1080p` / `4k`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::P720 => "720p",
            Self::P1080 => "1080p",
            Self::P4k => "4k",
        }
    }

    /// Pixel length of the long-edge cap.
    pub fn max_long_edge(self) -> u32 {
        match self {
            Self::P720 => 1280,
            Self::P1080 => 1920,
            Self::P4k => 3840,
        }
    }

    /// Parse `720p`/`1080p`/`4k` (or bare numbers); `None` for unknown input.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "720p" | "720" => Some(Self::P720),
            "1080p" | "1080" => Some(Self::P1080),
            "4k" | "2160p" | "2160" => Some(Self::P4k),
            _ => None,
        }
    }
}

impl fmt::Display for MaxResolution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for MaxResolution {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or_else(|| {
            format!("invalid media-max-resolution '{s}' (expected 720p, 1080p, or 4k)")
        })
    }
}

/// Build compress options from CLI-style fields (`min_size` like `20M`).
///
/// # Errors
///
/// Returns an error when `min_size` is not a parseable size (like `20M`).
pub fn compress_options_from_cli(
    max_resolution: MaxResolution,
    max_fps: f32,
    min_size: &str,
    skip_efficient: bool,
) -> anyhow::Result<CompressOptions> {
    Ok(CompressOptions {
        max_resolution,
        max_fps,
        min_size_bytes: parse_size(min_size)?,
        skip_efficient,
    })
}

/// Options applied only when [`MediaMode::Compress`].
#[derive(Debug, Clone, PartialEq)]
pub struct CompressOptions {
    /// Long-edge cap applied when compressing video.
    pub max_resolution: MaxResolution,
    /// Target frame rate for video compression.
    pub max_fps: f32,
    /// Videos smaller than this are not compressed.
    pub min_size_bytes: u64,
    /// Skip already-efficient (HEVC, low bitrate) videos.
    pub skip_efficient: bool,
}

impl Default for CompressOptions {
    fn default() -> Self {
        Self {
            max_resolution: MaxResolution::P1080,
            max_fps: 30.0,
            min_size_bytes: 20 * 1024 * 1024,
            skip_efficient: true,
        }
    }
}

/// Stream a file through SHA-256 in 64 KB chunks (no full read into memory).
///
/// Returns 64 lowercase hex digits. Thin wrapper over
/// [`message_ir::file_sha256`], kept so media callers do not need their own
/// `message-ir` dependency edge.
///
/// # Errors
///
/// Returns an error when the file cannot be opened or read.
pub fn file_sha256(path: &std::path::Path) -> anyhow::Result<String> {
    Ok(message_ir::file_sha256(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mode_and_resolution() {
        assert_eq!(MediaMode::parse("Convert"), Some(MediaMode::Convert));
        assert_eq!(MaxResolution::parse("4k"), Some(MaxResolution::P4k));
        assert_eq!(MaxResolution::P720.max_long_edge(), 1280);
    }

    #[test]
    fn parse_size_units() {
        assert_eq!(parse_size("20M").unwrap(), 20 * 1024 * 1024);
        assert_eq!(parse_size("512k").unwrap(), 512 * 1024);
        assert_eq!(parse_size("100").unwrap(), 100);
    }
}
