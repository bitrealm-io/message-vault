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
