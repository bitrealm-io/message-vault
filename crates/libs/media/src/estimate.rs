//! Forecast what the media step will do to a staged file's size.
//!
//! Every number here is an estimate and the screen says so. The point is not
//! precision: it is telling the difference between a file that will comfortably
//! fit, one that will not, and one that is fine now and will not be afterwards.

use std::path::Path;

use crate::process::{Kind, classify, is_efficient};
use crate::{CompressOptions, MediaMode, MediaProbe};

/// Files smaller than this fraction of the limit are not probed.
///
/// The largest growth factor in [`format_factor`] is well under 2.5x, so a file
/// this far below the limit cannot cross it, and probing every thumbnail in a
/// backup costs more than the answer is worth (decision 13).
const PROBE_BAND_FLOOR: f64 = 0.4;

/// An over-limit file whose estimate lands above this fraction of the limit
/// reads as probably still too big rather than likely to fit.
///
/// The margin is what stops a near miss from reading as a promise (decision 13).
const PROBABLY_FITS_MARGIN: f64 = 0.8;

/// How a staged attachment is expected to land against the size limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SizeVerdict {
    /// Under the limit now, and expected to stay under.
    FitsAsIs,
    /// Over the limit now, expected to come under after the media step.
    LikelyFits,
    /// Under the limit now, expected to cross it during the media step.
    MayGrow,
    /// Over the limit now, and expected to stay over.
    ProbablyTooBig,
    /// The media step does not handle this kind of file, so its size is fixed.
    CannotProcess,
}

/// Is this file worth an ffprobe call?
#[must_use]
pub fn needs_probe(size_bytes: u64, limit_bytes: u64) -> bool {
    size_bytes as f64 >= limit_bytes as f64 * PROBE_BAND_FLOOR
}

/// Estimated size after the media step, in bytes. Never capped at the original.
///
/// `ext` is matched case-insensitively (normalized internally), so callers
/// may pass it exactly as read from a file name.
#[must_use]
pub fn estimate_bytes(
    size_bytes: u64,
    probe: Option<&MediaProbe>,
    ext: &str,
    mode: MediaMode,
    compress: &CompressOptions,
) -> u64 {
    let ext = ext.to_ascii_lowercase();
    let ext = ext.as_str();
    if untouched_by(ext, mode) || skipped_as_efficient(ext, probe, mode, compress) {
        return size_bytes;
    }
    let factor = format_factor(ext, probe, mode);
    let scale = match (probe, mode) {
        // Only compress scales video. convert_video re-encodes at the source
        // resolution, so its size change is entirely the format's doing.
        (Some(p), MediaMode::Compress) if p.fps.is_some() => {
            pixel_ratio(p, compress) * fps_ratio(p, compress)
        }
        _ => 1.0,
    };
    (size_bytes as f64 * scale * factor).round() as u64
}

/// Classify one file, probing it first when it is close enough to matter.
///
/// `ext` is matched case-insensitively (normalized internally), so callers
/// may pass it exactly as read from a file name.
#[must_use]
pub fn classify_probed(
    size_bytes: u64,
    probe: Option<&MediaProbe>,
    ext: &str,
    mode: MediaMode,
    compress: &CompressOptions,
    limit_bytes: u64,
) -> SizeVerdict {
    let ext = ext.to_ascii_lowercase();
    let ext = ext.as_str();
    if !is_processable(ext) {
        return size_only(size_bytes, limit_bytes, SizeVerdict::CannotProcess);
    }
    if untouched_by(ext, mode) || skipped_as_efficient(ext, probe, mode, compress) {
        return size_only(size_bytes, limit_bytes, SizeVerdict::ProbablyTooBig);
    }
    if !needs_probe(size_bytes, limit_bytes) {
        return SizeVerdict::FitsAsIs;
    }
    let estimate = estimate_bytes(size_bytes, probe, ext, mode, compress);
    if size_bytes <= limit_bytes {
        return if estimate > limit_bytes {
            SizeVerdict::MayGrow
        } else {
            SizeVerdict::FitsAsIs
        };
    }
    if (estimate as f64) <= limit_bytes as f64 * PROBABLY_FITS_MARGIN {
        SizeVerdict::LikelyFits
    } else {
        SizeVerdict::ProbablyTooBig
    }
}

/// A file whose size the media step will not change is judged on that size.
fn size_only(size_bytes: u64, limit_bytes: u64, over: SizeVerdict) -> SizeVerdict {
    if size_bytes <= limit_bytes {
        SizeVerdict::FitsAsIs
    } else {
        over
    }
}

/// Does the media pass recognize this extension at all?
///
/// Mirrors [`crate::process::classify`] exactly — same three extension lists,
/// `false` for anything else — by calling it on a synthetic path, so a new
/// extension added to the media pass cannot be missed by the forecast.
fn is_processable(ext: &str) -> bool {
    classify(&Path::new("f").with_extension(ext)).is_some()
}

/// Does the media step leave a file with this extension alone in this mode,
/// independent of size?
///
/// Mirrors the early returns in `process_one`/`run_one` that do not depend on
/// a size gate: nothing is touched in `Clone`/`Disabled` (`run_one`'s last
/// arm skips every file), GIF is untouched in either of the remaining modes,
/// JPEG is already in `Convert`'s target form, MP3 already in `Convert`'s.
/// Deliberately hand-written rather than delegated to
/// [`crate::derivative_name`]: that function decides the JPEG/MP3 `Compress`
/// case by statting the file on disk, and this classifier is handed a size it
/// already knows for a file that may not exist on disk at all (a forecast
/// runs before any conversion). Reusing it here would mean synthesizing a
/// path to stat — and a missing path reads as size zero, which would wrongly
/// mark every JPEG/MP3 in `Compress` mode as untouched regardless of its real
/// size. The 500 KB / 100 KB floors stay out of this function on purpose: a
/// file under either floor is small enough that it never reaches this check
/// (`classify_probed` already answered `FitsAsIs` before probing), so leaving
/// them out cannot make this function disagree with the pass.
fn untouched_by(ext: &str, mode: MediaMode) -> bool {
    if matches!(mode, MediaMode::Clone | MediaMode::Disabled) {
        return true;
    }
    matches!(
        (ext, mode),
        ("gif", _) | ("jpg" | "jpeg" | "mp3", MediaMode::Convert)
    )
}

/// Would `compress_video` skip re-encoding this file and only remux it,
/// because it is already an efficient HEVC stream?
///
/// Calls the pass's own [`is_efficient`] predicate rather than restating its
/// codec/resolution/bitrate thresholds, so the forecast cannot drift from
/// what `compress_video` (process.rs) actually decides. Only applies to
/// video in `Compress` mode with `skip_efficient` on and an actual probe in
/// hand — an un-probed file (outside the probe band, or an audio/image file
/// this crate never calls ffprobe for) cannot be judged efficient, so this
/// answers `false` rather than guessing.
fn skipped_as_efficient(
    ext: &str,
    probe: Option<&MediaProbe>,
    mode: MediaMode,
    compress: &CompressOptions,
) -> bool {
    if !matches!(mode, MediaMode::Compress) || !compress.skip_efficient {
        return false;
    }
    if !matches!(
        classify(&Path::new("f").with_extension(ext)),
        Some(Kind::Video)
    ) {
        return false;
    }
    let Some(probe) = probe else {
        return false;
    };
    is_efficient(
        &probe.codec,
        probe.width,
        probe.height,
        probe.bitrate,
        compress,
    )
}

/// Output pixels over source pixels after the resolution cap, for the size estimate.
fn pixel_ratio(probe: &MediaProbe, compress: &CompressOptions) -> f64 {
    let source_long = f64::from(probe.width.max(probe.height));
    if source_long <= 0.0 {
        return 1.0;
    }
    let target_long = f64::from(compress.max_resolution.max_long_edge());
    let ratio = (target_long / source_long).min(1.0);
    ratio * ratio
}

/// Output frame rate over source frame rate after the fps cap, for the size estimate.
fn fps_ratio(probe: &MediaProbe, compress: &CompressOptions) -> f64 {
    let Some(source) = probe.fps.filter(|f| *f > 0.0) else {
        return 1.0;
    };
    let target = if compress.max_fps > 0.0 {
        compress.max_fps
    } else {
        30.0
    };
    f64::from(target / source).min(1.0)
}

/// Size change from the format alone, holding pixels and frame rate fixed.
///
/// Above 1.0 means the target format is bulkier than the source — the case
/// decision 12 exists to catch, and the common one on an iPhone backup.
fn format_factor(ext: &str, probe: Option<&MediaProbe>, mode: MediaMode) -> f64 {
    let compressing = matches!(mode, MediaMode::Compress);
    match ext {
        // Apple stills. HEIC is roughly half an equivalent JPEG, so it grows.
        "heic" | "heif" => 1.8,
        // Lossless and near-lossless stills re-encoded to JPEG.
        "png" | "tif" | "tiff" | "bmp" => 1.3,
        "webp" => 1.2,
        // Already JPEG: only compress touches it, at -q:v 5.
        "jpg" | "jpeg" => 0.7,
        // Already MP3: only compress touches it, at 96k mono.
        "mp3" => 0.6,
        // Anything else to MP3.
        "m4a" | "aac" | "caf" | "amr" | "wav" | "ogg" | "opus" => 0.8,
        // Video: the codec decides, not the container.
        //
        // `convert_video` always lands on H.264 (its remux path preserves the
        // source codec and so is not size-changing at all; only its re-encode
        // fallback is format_factor's concern), so a source already on a more
        // efficient codec grows — decision 12's headline case.
        //
        // `compress_video` re-encodes to HEVC (libx265) at a fixed CRF, so an
        // already-efficient HEVC source never reaches this arm at all — it is
        // caught upstream by `skipped_as_efficient` and judged on its
        // unchanged size instead. What *does* land here in `Compress` mode is
        // a codec (HEVC included) that failed the efficiency check — too big,
        // too high-bitrate, or the wrong resolution — so the flat 0.7 general
        // compress factor applies uniformly; there is no case left where the
        // convert-mode growth factor also belongs to a compressing file.
        _ => match probe.map(|p| p.codec.as_str()) {
            Some("hevc" | "vp9" | "av1") if !compressing => 1.4,
            Some(_) if compressing => 0.7,
            _ => 1.0,
        },
    }
}

#[cfg(test)]
mod tests;
