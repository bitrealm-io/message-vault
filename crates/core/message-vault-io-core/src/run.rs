//! The shared exporter run skeleton and tail.

use crate::config::ExporterConfig;
use crate::pipeline::{ExportReport, RunResult};
use crate::process::check_cancel;
use crate::transforms::ExportTransforms;

/// The shared exporter run skeleton: cancel check, transforms, conversion,
/// media-failure bail, and result assembly.
///
/// Exporters no longer resolve names from a contacts file. A backup that
/// carries its own contact data (Apple's address book, WhatsApp's contacts
/// database) is read by that exporter directly; everything else arrives at the
/// vault as raw identities and is reconciled there.
///
/// # Errors
///
/// Returns an error when the user cancels, conversion fails, or media
/// processing fails for every candidate file.
pub fn run_pipeline(
    config: &ExporterConfig,
    convert: impl FnOnce(ExportTransforms) -> anyhow::Result<ExportReport>,
) -> anyhow::Result<RunResult> {
    check_cancel(config.cancel.as_ref())?;
    let report = convert(ExportTransforms::from_config(config))?;
    finish_run(config, &report, config.media.mode.needs_tools())
}

/// The run tail shared by exporters whose middle diverges (WhatsApp, iMessage):
/// media-failure bail plus log-line and summary assembly.
///
/// # Errors
///
/// Returns an error when media processing fails for every candidate file.
pub fn finish_run(
    config: &ExporterConfig,
    report: &ExportReport,
    needs_tools: bool,
) -> anyhow::Result<RunResult> {
    report.check_media(needs_tools)?;
    let mut messages = report.media_lines();
    report.summary_lines(config.output_format, &config.output, &mut messages);
    Ok(RunResult { messages })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{FormatConfig, MediaConfig, ObfuscateConfig, OutputFormat, SourceConfig};
    use media::{CompressOptions, MediaMode};
    use std::path::PathBuf;

    fn config(mode: MediaMode, obfuscate: bool) -> ExporterConfig {
        ExporterConfig {
            inputs: Vec::new(),
            output: PathBuf::from("out"),
            timezone: None,
            obfuscate: ObfuscateConfig {
                enabled: obfuscate,
                seed: None,
            },
            media: MediaConfig {
                mode,
                compress: CompressOptions::default(),
            },
            cancel: None,
            log: None,
            progress: None,
            output_format: OutputFormat::Jsonl,
            resume: false,
            source: SourceConfig::Format(FormatConfig {}),
        }
    }

    #[test]
    fn run_pipeline_hands_the_config_transforms_to_convert_and_returns_the_summary() {
        let result = run_pipeline(&config(MediaMode::Convert, true), |t| {
            assert!(t.obfuscate);
            assert_eq!(t.media, MediaMode::Convert);
            Ok(ExportReport {
                conversations: 1,
                attachments_saved: 2,
                ..ExportReport::default()
            })
        })
        .unwrap();

        assert_eq!(
            result.messages,
            ["Wrote jsonl export under out", "  saved 2 attachments"]
        );
    }

    #[test]
    fn run_pipeline_fails_when_media_failed_on_every_file() {
        let err = run_pipeline(&config(MediaMode::Convert, false), |_| {
            let mut report = ExportReport::default();
            report.media.errors.push("a.heic: ffmpeg not found".into());
            Ok(report)
        })
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "media processing failed for all candidate files"
        );
    }

    #[test]
    fn run_pipeline_returns_the_convert_error() {
        let err = run_pipeline(&config(MediaMode::Clone, false), |_| {
            anyhow::bail!("unreadable backup")
        })
        .unwrap_err();
        assert_eq!(err.to_string(), "unreadable backup");
    }
}
