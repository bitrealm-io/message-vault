//! Convert or compress attachment media under a converter export directory.
//!
//! Modes:
//! - **Disabled** — do not copy or write attachment files (the export form's "Do not copy" choice)
//! - **Clone** — leave exported files as-is (a no-op after export)
//! - **Convert** — rewrite images to `.jpg`, videos to `.mp4`, audio to `.mp3`
//! - **Compress** — re-encode to shrink files, with optional video settings
//!
//! Convert and compress need `ffmpeg` / `ffprobe` beside the running binary,
//! in `MESSAGE_VAULT_IO_BIN`, or on `PATH`.

mod estimate;
mod mime;
mod probe;
mod process;
mod size;
#[cfg(any(test, feature = "testutil"))]
pub mod testutil;
mod tools;

pub use estimate::{SizeVerdict, classify_probed, estimate_bytes, needs_probe};
pub use mime::{Kind, ext_for_mime, kind_for_ext, kind_for_mime, mime_for_ext};
pub use probe::{MediaProbe, probe_media};
pub use process::{
    MediaReport, TranscodeOutcome, classify, collect_media_files, derivative_name,
    derivative_name_for_missing, format_bytes, kind_of, process_attachment_files, transcode_file,
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
    /// Canonical lowercase name (`disabled` / `clone` / `convert` / `compress`),
    /// the value the export form and the desktop commands pass as text.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Clone => "clone",
            Self::Convert => "convert",
            Self::Compress => "compress",
        }
    }

    /// Parse a mode name (case- and whitespace-insensitive; `none`, `skip` and
    /// `copy` are accepted aliases); `None` for unknown input.
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

/// Build [`CompressOptions`] from the export form's values: the resolution cap,
/// the frame-rate cap, the minimum size as the person typed it (`20M`, `2g`),
/// and whether already-efficient videos are skipped.
///
/// # Errors
///
/// Returns an error when `min_size` is not a size with an optional unit
/// (`20M`, `2g`, `512`).
pub fn compress_options_from_form(
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
        assert_eq!(parse_size("2g").unwrap(), 2 * 1024 * 1024 * 1024);
        assert_eq!(parse_size("2G").unwrap(), 2 * 1024 * 1024 * 1024);
    }

    #[test]
    fn every_mode_alias_maps_to_its_mode() {
        for (alias, mode) in [
            ("disabled", MediaMode::Disabled),
            ("none", MediaMode::Disabled),
            ("skip", MediaMode::Disabled),
            ("clone", MediaMode::Clone),
            ("copy", MediaMode::Clone),
            ("convert", MediaMode::Convert),
            (" COMPRESS ", MediaMode::Compress),
        ] {
            assert_eq!(MediaMode::parse(alias), Some(mode), "{alias}");
        }
        assert_eq!(MediaMode::parse("shrink"), None);
        assert!("shrink".parse::<MediaMode>().is_err());
    }

    #[test]
    fn every_mode_reads_back_what_it_writes() {
        // (mode, needs ffmpeg, copies attachments)
        for (mode, needs_tools, copies) in [
            (MediaMode::Disabled, false, false),
            (MediaMode::Clone, false, true),
            (MediaMode::Convert, true, true),
            (MediaMode::Compress, true, true),
        ] {
            assert_eq!(mode.to_string().parse::<MediaMode>(), Ok(mode));
            assert_eq!(mode.needs_tools(), needs_tools, "{mode}");
            assert_eq!(mode.copies_attachments(), copies, "{mode}");
        }
    }

    #[test]
    fn every_resolution_reads_back_what_it_writes() {
        for cap in [
            MaxResolution::P720,
            MaxResolution::P1080,
            MaxResolution::P4k,
        ] {
            assert_eq!(cap.to_string().parse::<MaxResolution>(), Ok(cap));
        }
        assert!("480p".parse::<MaxResolution>().is_err());
    }

    #[test]
    fn every_resolution_alias_maps_to_its_cap() {
        for (alias, cap) in [
            ("720p", MaxResolution::P720),
            ("720", MaxResolution::P720),
            ("1080p", MaxResolution::P1080),
            ("1080", MaxResolution::P1080),
            ("4K", MaxResolution::P4k),
            ("2160p", MaxResolution::P4k),
            ("2160", MaxResolution::P4k),
        ] {
            assert_eq!(MaxResolution::parse(alias), Some(cap), "{alias}");
        }
        assert_eq!(MaxResolution::parse("480p"), None);
    }

    #[test]
    fn compress_options_from_form_reads_every_field() {
        let options = compress_options_from_form(MaxResolution::P720, 24.0, "2g", false).unwrap();
        assert_eq!(
            options,
            CompressOptions {
                max_resolution: MaxResolution::P720,
                max_fps: 24.0,
                min_size_bytes: 2 * 1024 * 1024 * 1024,
                skip_efficient: false,
            }
        );
        assert!(compress_options_from_form(MaxResolution::P720, 24.0, "lots", false).is_err());
    }
}
