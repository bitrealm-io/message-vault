//! The media settings an Import Run decides once, recorded in its staging
//! folder.
//!
//! Staging writes the run's [`TranscodeOptions`] (the attachment mode, the
//! compress options and the attachment size limit) beside the export
//! sentinel. The Staging Review's summary and the Media stage read them back
//! from the folder instead of being handed the form's fields again, so the
//! three can never disagree about what the run asked for.

use std::path::Path;

use anyhow::{Context, Result};

use crate::TranscodeOptions;

/// File name of the recorded settings, directly under the staging folder.
///
/// It has no `.json` extension on purpose: a fresh export into a folder
/// removes every `*.json` file it finds there as an earlier run's output.
pub const MEDIA_SETTINGS_FILE: &str = ".message-crate-media";

/// Record `options` in `staging_dir`, replacing any earlier record.
///
/// # Errors
///
/// Returns an error when the file cannot be written.
pub fn write_media_settings(staging_dir: &Path, options: &TranscodeOptions) -> Result<()> {
    let path = staging_dir.join(MEDIA_SETTINGS_FILE);
    let json = serde_json::to_vec_pretty(options).context("encode the media settings")?;
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))
}

/// Read the settings Staging recorded in `staging_dir`.
///
/// # Errors
///
/// Returns an error when the folder has no record, because its Staging never
/// finished, or the record cannot be read.
pub fn read_media_settings(staging_dir: &Path) -> Result<TranscodeOptions> {
    let path = staging_dir.join(MEDIA_SETTINGS_FILE);
    let bytes = std::fs::read(&path).with_context(|| {
        format!(
            "{} holds no media settings, so its Staging did not finish",
            staging_dir.display()
        )
    })?;
    serde_json::from_slice(&bytes).with_context(|| format!("read {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use media::{CompressOptions, MaxResolution, MediaMode};

    #[test]
    fn the_settings_read_back_as_they_were_written() {
        let dir = tempfile::tempdir().unwrap();
        let options = TranscodeOptions {
            mode: MediaMode::Compress,
            compress: CompressOptions {
                max_resolution: MaxResolution::P720,
                max_fps: 24.0,
                min_size_bytes: 5 * 1024 * 1024,
                skip_efficient: true,
            },
            asset_max_bytes: 123_456_789,
        };

        write_media_settings(dir.path(), &options).unwrap();

        assert_eq!(read_media_settings(dir.path()).unwrap(), options);
    }

    #[test]
    fn a_folder_with_no_settings_is_refused() {
        // Without the record a summary would fall back to some mode the run
        // never chose, which is the disagreement this file exists to end.
        let dir = tempfile::tempdir().unwrap();

        let err = read_media_settings(dir.path()).unwrap_err();

        assert!(format!("{err:#}").contains("no media settings"), "{err:#}");
    }
}
