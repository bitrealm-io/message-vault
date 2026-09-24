//! What a run does to its documents on the way out: media mode, obfuscation,
//! and the sinks the write tail reports through.

use crate::config::ExporterConfig;
use crate::process::LogSink;
use crate::progress::ProgressSink;
use media::{CompressOptions, MediaMode};

/// The transforms a run applies at write time, handed to the format sink.
#[derive(Debug, Clone)]
pub struct ExportTransforms {
    /// Media mode applied at finish (clone/convert/compress/disabled).
    pub media: MediaMode,
    /// Video/audio compression options used with Compress mode.
    pub compress: CompressOptions,
    /// Whether to replace PII and media with obfuscated placeholders.
    pub obfuscate: bool,
    /// Seed for deterministic obfuscation; `None` generates one.
    pub obfuscate_seed: Option<String>,
    /// Mid-run notes (e.g. generated obfuscate seed). `None` → stderr.
    pub log: Option<LogSink>,
    /// Typed progress events for the write tail. `None` reports nothing.
    pub progress: Option<ProgressSink>,
}

impl Default for ExportTransforms {
    fn default() -> Self {
        Self {
            media: MediaMode::Clone,
            compress: CompressOptions::default(),
            obfuscate: false,
            obfuscate_seed: None,
            log: None,
            progress: None,
        }
    }
}

impl ExportTransforms {
    /// The transforms an exporter run wants: media and obfuscation from the
    /// config (obfuscation is on when either its flag or a seed is set), plus
    /// the run's log and progress sinks so the write tail reports through
    /// the same hooks the exporter does.
    pub fn from_config(config: &ExporterConfig) -> Self {
        Self {
            media: config.media.mode,
            compress: config.media.compress.clone(),
            obfuscate: config.obfuscate_active(),
            obfuscate_seed: config.obfuscate.seed.clone(),
            log: config.log.clone(),
            progress: config.progress.clone(),
        }
    }

    /// All-defaults transform set (clone, no obfuscation, no log).
    pub fn none() -> Self {
        Self::default()
    }

    /// True when ffmpeg/ffprobe will be required (false when obfuscating,
    /// which replaces media with placeholders).
    pub fn needs_media_tools(&self) -> bool {
        // Obfuscate replaces all media with placeholders — no ffmpeg work.
        !self.obfuscate && self.media.needs_tools()
    }

    /// True when attachment bytes should be staged under `attachments/`
    /// (false when obfuscating).
    pub fn copies_attachments(&self) -> bool {
        // Obfuscate discards real bytes; skip staging them in the first place.
        !self.obfuscate && self.media.copies_attachments()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{FormatConfig, MediaConfig, ObfuscateConfig, OutputFormat, SourceConfig};
    use media::MaxResolution;
    use std::path::PathBuf;

    fn config(obfuscate: ObfuscateConfig, media: MediaConfig) -> ExporterConfig {
        ExporterConfig {
            inputs: Vec::new(),
            output: PathBuf::from("out"),
            timezone: None,
            obfuscate,
            media,
            cancel: None,
            log: None,
            progress: None,
            output_format: OutputFormat::Json,
            resume: false,
            source: SourceConfig::Format(FormatConfig {}),
        }
    }

    #[test]
    fn from_config_obfuscates_when_the_flag_is_set_without_a_seed() {
        let t = ExportTransforms::from_config(&config(
            ObfuscateConfig {
                enabled: true,
                seed: None,
            },
            MediaConfig::default(),
        ));
        assert!(t.obfuscate);
        assert_eq!(t.obfuscate_seed, None);
    }

    #[test]
    fn from_config_obfuscates_when_only_a_seed_is_given() {
        let t = ExportTransforms::from_config(&config(
            ObfuscateConfig {
                enabled: false,
                seed: Some("abc123".to_string()),
            },
            MediaConfig::default(),
        ));
        assert!(t.obfuscate);
        assert_eq!(t.obfuscate_seed.as_deref(), Some("abc123"));
    }

    #[test]
    fn from_config_leaves_obfuscation_off_without_flag_or_seed() {
        let t = ExportTransforms::from_config(&config(
            ObfuscateConfig::default(),
            MediaConfig::default(),
        ));
        assert!(!t.obfuscate);
    }

    #[test]
    fn from_config_carries_the_media_mode_compress_options_and_sinks() {
        let compress = CompressOptions {
            max_resolution: MaxResolution::P720,
            max_fps: 24.0,
            min_size_bytes: 1234,
            skip_efficient: false,
        };
        let mut cfg = config(
            ObfuscateConfig::default(),
            MediaConfig {
                mode: MediaMode::Compress,
                compress: compress.clone(),
            },
        );
        cfg.log = Some(LogSink::new(|_| {}));
        cfg.progress = Some(ProgressSink::new(|_| {}));

        let t = ExportTransforms::from_config(&cfg);

        assert_eq!(t.media, MediaMode::Compress);
        assert_eq!(t.compress, compress);
        assert!(t.log.is_some());
        assert!(t.progress.is_some());
        assert!(t.needs_media_tools());
    }

    #[test]
    fn obfuscate_disables_copy_and_media_tools() {
        let t = ExportTransforms {
            media: MediaMode::Convert,
            obfuscate: true,
            ..ExportTransforms::none()
        };
        assert!(!t.copies_attachments());
        assert!(!t.needs_media_tools());

        let t = ExportTransforms {
            media: MediaMode::Clone,
            obfuscate: false,
            ..ExportTransforms::none()
        };
        assert!(t.copies_attachments());
        assert!(!t.needs_media_tools());
    }
}
