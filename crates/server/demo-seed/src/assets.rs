//! Writes the photo and other attachment files used by demo conversations.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use image::{ImageBuffer, Rgb};
use sha2::{Digest, Sha256};

/// Demo JPEG photos (path relative to export dir, display name, base color).
pub struct JpgPhoto {
    pub path: &'static str,
    pub original_name: &'static str,
    /// Base RGB the generated gradient starts from.
    pub color: [u8; 3],
}

pub const JPG_PHOTOS: &[JpgPhoto] = &[
    JpgPhoto {
        path: "attachments/sunset.jpg",
        original_name: "IMG_2847.jpg",
        color: [255, 140, 60],
    },
    JpgPhoto {
        path: "attachments/park.jpg",
        original_name: "IMG_3102.jpg",
        color: [72, 160, 95],
    },
    JpgPhoto {
        path: "attachments/dinner.jpg",
        original_name: "IMG_4521.jpg",
        color: [180, 85, 70],
    },
    JpgPhoto {
        path: "attachments/puppy.jpg",
        original_name: "IMG_5098.jpg",
        color: [210, 175, 130],
    },
    JpgPhoto {
        path: "attachments/receipt.jpg",
        original_name: "Scan_2024-03-15.jpg",
        color: [245, 245, 240],
    },
    JpgPhoto {
        path: "attachments/selfie.jpg",
        original_name: "IMG_6110.jpg",
        color: [90, 130, 200],
    },
    JpgPhoto {
        path: "attachments/beach.jpg",
        original_name: "IMG_7203.jpg",
        color: [60, 175, 220],
    },
    JpgPhoto {
        path: "attachments/flowers.jpg",
        original_name: "IMG_8011.jpg",
        color: [220, 100, 150],
    },
];

/// Non-JPEG attachments so the demo includes more than photos.
pub const OTHER_ATTACHMENTS: &[(&str, &str, bool)] = &[
    ("attachments/landscape.png", "image/png", false),
    ("attachments/sticker.gif", "image/gif", true),
    ("attachments/voice.caf", "audio/x-caf", false),
    ("attachments/notes.pdf", "application/pdf", false),
    ("attachments/missing-file.heic", "image/heic", false),
];

/// Write colorful JPEGs large enough to show inline in the web UI, plus a few
/// small stand-in files for PNG, GIF, audio, and PDF.
///
/// Returns a map from attachment path to a short fingerprint of the file
/// contents and the file size in bytes. Conversation files store those
/// values next to each attachment. One path, `attachments/missing-file.heic`,
/// is left out on purpose so import can show a missing-file warning.
///
/// # Errors
///
/// Returns an error if a JPEG cannot be written or a file cannot be read back.
pub fn write_attachment_blobs(dir: &Path) -> Result<HashMap<String, (String, u64)>> {
    let mut digests = HashMap::new();
    for photo in JPG_PHOTOS {
        let name = photo
            .path
            .strip_prefix("attachments/")
            .unwrap_or(photo.path);
        let path = dir.join(name);
        write_color_jpeg(&path, photo.color, 320, 240)?;
        let bytes = std::fs::read(&path)?;
        record_blob(&mut digests, photo.path, &bytes);
    }

    fs::write(dir.join("landscape.png"), MINI_PNG)?;
    record_blob(&mut digests, "attachments/landscape.png", MINI_PNG);

    fs::write(dir.join("sticker.gif"), MINI_GIF)?;
    record_blob(&mut digests, "attachments/sticker.gif", MINI_GIF);

    let voice = mini_caf();
    fs::write(dir.join("voice.caf"), &voice)?;
    record_blob(&mut digests, "attachments/voice.caf", &voice);

    fs::write(dir.join("notes.pdf"), MINI_PDF)?;
    record_blob(&mut digests, "attachments/notes.pdf", MINI_PDF);

    // attachments/missing-file.heic is left out of this map on purpose.
    // Conversation JSONL still points at it, but the file is not on disk, so
    // import can show its missing-file warning.

    Ok(digests)
}

/// Store the content fingerprint and byte length for `relative_path`.
fn record_blob(digests: &mut HashMap<String, (String, u64)>, relative_path: &str, bytes: &[u8]) {
    let sha = hex::encode(Sha256::digest(bytes));
    digests.insert(relative_path.into(), (sha, bytes.len() as u64));
}

/// Write a solid-color JPEG with a light gradient so thumbnails look different.
///
/// # Errors
///
/// Returns an error if the image cannot be saved.
fn write_color_jpeg(path: &Path, rgb: [u8; 3], width: u32, height: u32) -> Result<()> {
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(width, height, |x, y| {
        // Subtle gradient so thumbnails are visibly distinct.
        let r = rgb[0].saturating_add(((x * 40) / width) as u8);
        let g = rgb[1].saturating_add(((y * 30) / height) as u8);
        let b = rgb[2];
        Rgb([r, g, b])
    });
    img.save(path)
        .with_context(|| format!("write jpeg {}", path.display()))?;
    Ok(())
}

// Tiny valid 1x1 red PNG.
const MINI_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D, 0xB0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E,
    0x44, 0xAE, 0x42, 0x60, 0x82,
];

const MINI_GIF: &[u8] = &[
    0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0xFF, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x21, 0xF9, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2C, 0x00, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x02, 0x44, 0x01, 0x00, 0x3B,
];

const MINI_PDF: &[u8] = b"%PDF-1.1\n1 0 obj<<>>endobj\ntrailer<<>>\n%%EOF\n";

/// Silent 0.1s mono 16-bit CAF (Core Audio Format) clip.
///
/// The old 20-byte header had no audio frames. ffmpeg refused it during
/// `reset-demo` (`Invalid data found when processing input`). This file is a
/// real CAF so the web converter can turn it into MP3.
pub fn mini_caf() -> Vec<u8> {
    const SAMPLE_RATE: f64 = 8_000.0;
    const CHANNELS: u32 = 1;
    const BITS: u32 = 16;
    const FRAMES: u32 = 800;
    let data_bytes = FRAMES * CHANNELS * (BITS / 8);

    let mut out = Vec::with_capacity(64 + data_bytes as usize);
    out.extend(b"caff");
    out.extend(1u16.to_be_bytes());
    out.extend(0u16.to_be_bytes());

    out.extend(b"desc");
    out.extend(32i64.to_be_bytes());
    out.extend(SAMPLE_RATE.to_be_bytes());
    out.extend(b"lpcm");
    out.extend(2u32.to_be_bytes());
    out.extend((BITS / 8 * CHANNELS).to_be_bytes());
    out.extend(1u32.to_be_bytes());
    out.extend(CHANNELS.to_be_bytes());
    out.extend(BITS.to_be_bytes());

    out.extend(b"data");
    out.extend(i64::from(4 + data_bytes).to_be_bytes());
    out.extend(0u32.to_be_bytes());
    out.extend(vec![0u8; data_bytes as usize]);
    out
}

#[cfg(test)]
mod tests {
    use super::mini_caf;
    use std::process::Command;

    #[test]
    fn mini_caf_is_a_real_caf_file() {
        let bytes = mini_caf();
        assert!(bytes.starts_with(b"caff"), "CAF magic");
        assert!(bytes.windows(4).any(|w| w == b"desc"), "desc chunk");
        assert!(bytes.windows(4).any(|w| w == b"data"), "data chunk");
        assert!(
            bytes.len() > 64,
            "must include PCM samples, not only a header"
        );
    }

    #[test]
    fn mini_caf_is_readable_by_ffprobe() {
        let Some(_tools) = media::testutil::real_ffmpeg_test_guard() else {
            return;
        };
        let ffprobe = media::probe_ffmpeg_tools(None)
            .ffprobe_path
            .expect("the guard found ffprobe");

        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("voice.caf");
        std::fs::write(&path, mini_caf()).expect("write caf");
        let probed = Command::new(ffprobe)
            .args([
                "-v",
                "error",
                "-show_entries",
                "format=duration",
                "-of",
                "csv=p=0",
            ])
            .arg(&path)
            .status()
            .expect("run ffprobe");
        assert!(
            probed.success(),
            "ffprobe must accept the demo voice note so reset-demo can convert it"
        );
    }
}
