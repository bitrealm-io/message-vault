//! Full export pipeline for the in-process desktop app.

use crate::emit::{ConvertExportArgs, convert_export};
use anyhow::{Result, bail};
use message_vault_io_core::{ExporterConfig, RunResult, SourceConfig};

/// Convert, then apply media transforms and obfuscation.
///
/// # Errors
///
/// Returns an error when the source is not iMazing, conversion fails, media
/// processing fails for every candidate file, or the user cancels.
pub fn run(config: &ExporterConfig) -> Result<RunResult> {
    let SourceConfig::Imazing(_) = &config.source else {
        bail!("imazing-exporter requires SourceConfig::Imazing");
    };
    message_vault_io_core::check_cancel(config.cancel.as_ref())?;
    let input = config.require_input().map_err(anyhow::Error::msg)?;
    message_ir_format::run_pipeline(config, |transforms| {
        convert_export(ConvertExportArgs {
            input,
            output: &config.output,
            time_zone: config.time_zone,
            transforms,
            output_format: config.output_format,
            cancel: config.cancel.as_ref(),
            resume: config.resume,
        })
    })
}
