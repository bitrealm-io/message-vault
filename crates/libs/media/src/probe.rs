//! Read one media file's shape with ffprobe, for the size forecast.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, anyhow};

use crate::tools::ffprobe_command;

/// What ffprobe reports about one media file's first video stream.
///
/// Stills have a stream too, with no frame rate.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaProbe {
    /// Codec as ffprobe spells it, lowercased: `hevc`, `h264`, `mjpeg`, `png`.
    pub codec: String,
    /// Pixel width. Zero only when ffprobe reported no dimensions.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
    /// Frames per second, `None` for stills.
    pub fps: Option<f32>,
    /// Bits per second, as ffprobe reports it. `0` when unreported (a still,
    /// or a stream ffprobe could not measure).
    pub bitrate: u64,
}

impl MediaProbe {
    /// Total pixels in one frame, as a float so ratios do not truncate.
    #[must_use]
    pub fn pixels(&self) -> f64 {
        f64::from(self.width) * f64::from(self.height)
    }
}

/// Ask ffprobe about `path`.
///
/// # Errors
///
/// Returns an error when ffprobe is missing, exits non-zero, or reports a
/// line this cannot read.
pub fn probe_media(path: &Path) -> Result<MediaProbe> {
    let mut cmd: Command = ffprobe_command()?;
    cmd.args([
        "-v",
        "error",
        "-select_streams",
        "v:0",
        "-show_entries",
        "stream=codec_name,width,height,avg_frame_rate,bit_rate",
        "-of",
        "csv=p=0",
    ]);
    cmd.arg(path);
    let out = cmd
        .output()
        .with_context(|| format!("run ffprobe on {}", path.display()))?;
    if !out.status.success() {
        return Err(anyhow!(
            "ffprobe failed on {}: {}",
            path.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text
        .lines()
        .find(|l| !l.trim().is_empty())
        .ok_or_else(|| anyhow!("ffprobe reported no stream for {}", path.display()))?;
    parse_probe_line(line.trim())
}

/// Read one `codec,width,height,avg_frame_rate,bit_rate` line.
///
/// `bit_rate` is trailing and optional-in-practice: absent, ffprobe writes
/// `N/A`, or a still simply has none. All of those default to `0` rather
/// than erroring, same as a malformed width/height — only a genuinely
/// missing codec/width/height column fails the read.
fn parse_probe_line(line: &str) -> Result<MediaProbe> {
    let mut parts = line.split(',');
    let codec = parts.next().unwrap_or_default().trim().to_ascii_lowercase();
    let width = parts
        .next()
        .ok_or_else(|| anyhow!("no width in {line:?}"))?;
    let height = parts
        .next()
        .ok_or_else(|| anyhow!("no height in {line:?}"))?;
    let rate = parts.next().unwrap_or_default();
    let bitrate = parts.next().unwrap_or_default();
    Ok(MediaProbe {
        codec,
        width: width.trim().parse().unwrap_or(0),
        height: height.trim().parse().unwrap_or(0),
        fps: parse_frame_rate(rate.trim()),
        bitrate: bitrate.trim().parse().unwrap_or(0),
    })
}

/// ffprobe writes frame rates as a rational: `30000/1001`, or `0/0` for a still.
fn parse_frame_rate(raw: &str) -> Option<f32> {
    let (num, den) = raw.split_once('/')?;
    let num: f32 = num.trim().parse().ok()?;
    let den: f32 = den.trim().parse().ok()?;
    if num <= 0.0 || den <= 0.0 {
        return None;
    }
    Some(num / den)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_rational_frame_rate() {
        assert_eq!(parse_frame_rate("30000/1001"), Some(29.97003));
        assert_eq!(parse_frame_rate("30/1"), Some(30.0));
    }

    #[test]
    fn a_still_image_has_no_frame_rate() {
        // ffprobe reports 0/0 for a still: there is no rate, and dividing by
        // the denominator would be a panic on some inputs and a lie on others.
        assert_eq!(parse_frame_rate("0/0"), None);
        assert_eq!(parse_frame_rate(""), None);
        assert_eq!(parse_frame_rate("N/A"), None);
    }

    #[test]
    fn reads_one_csv_line_into_a_probe() {
        let probe = parse_probe_line("hevc,3840,2160,30000/1001").unwrap();
        assert_eq!(probe.codec, "hevc");
        assert_eq!(probe.width, 3840);
        assert_eq!(probe.height, 2160);
        assert_eq!(probe.fps, Some(29.97003));
        // No bit_rate column on this line: defaults rather than errors, same
        // as a malformed width/height would.
        assert_eq!(probe.bitrate, 0);
    }

    #[test]
    fn a_short_csv_line_is_an_error_not_a_default() {
        // A defaulted probe reads as a 0x0 file, which the estimate would
        // divide by. Fail loudly instead.
        assert!(parse_probe_line("hevc,3840").is_err());
    }

    #[test]
    fn reads_the_bit_rate_column_when_present() {
        // The efficient-video skip (`is_efficient` in process.rs) gates on
        // bitrate, so this column has to come through, not just default.
        let probe = parse_probe_line("hevc,1920,1080,30/1,9000000").unwrap();
        assert_eq!(probe.bitrate, 9_000_000);
    }

    /// The parsing tests above never touch ffprobe itself: they hand-craft
    /// the CSV line. This exercises the real subprocess — the exact args,
    /// column order, and codec string ffprobe actually emits — against a
    /// generated fixture with a known codec, resolution, and frame rate.
    #[test]
    fn probes_a_real_video_file() {
        use crate::tools::run_ffmpeg;

        // Holds the tools lock: this test runs the real ffmpeg, and the tests
        // in `tools` point the process-wide override at mock and empty
        // directories while they run.
        let Some(_tools) = crate::tools::real_ffmpeg_test_guard() else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clip.mp4");
        let args: Vec<String> = [
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=64x48:rate=25",
            "-frames:v",
            "5",
            "-pix_fmt",
            "yuv420p",
            "-c:v",
            "libx264",
        ]
        .into_iter()
        .map(String::from)
        .chain(std::iter::once(path.to_string_lossy().into_owned()))
        .collect();
        run_ffmpeg(&args).expect("generate probe fixture");

        let probe = probe_media(&path).expect("probe the generated fixture");
        assert_eq!(probe.codec, "h264");
        assert_eq!(probe.width, 64);
        assert_eq!(probe.height, 48);
        let fps = probe.fps.expect("a real video reports a frame rate");
        assert!((fps - 25.0).abs() < 0.01, "fps was {fps}");
    }
}
