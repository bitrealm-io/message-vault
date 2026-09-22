//! Full export pipeline for CLI and in-process GUI.

use crate::emit::{ConvertExportArgs, convert_export};
use anyhow::{Result, bail};
use message_vault_io_core::{ExporterConfig, RunResult, SourceConfig};

/// Resolve contacts, convert, then apply media transforms and obfuscation.
///
/// # Errors
///
/// Returns an error when the source is not GO SMS Pro, conversion fails, media
/// processing fails for every candidate file, or the user cancels.
pub fn run(config: &ExporterConfig) -> Result<RunResult> {
    let SourceConfig::GoSmsPro(source) = &config.source else {
        bail!("go-sms-pro-exporter requires SourceConfig::GoSmsPro");
    };
    message_vault_io_core::check_cancel(config.cancel.as_ref())?;
    let input = config.require_input().map_err(anyhow::Error::msg)?;
    message_vault_io_core::run_pipeline(config, |transforms| {
        convert_export(ConvertExportArgs {
            input_dir: input,
            output_dir: &config.output,
            owner_phones: &source.owner_phones,
            transforms,
            output_format: config.output_format,
            cancel: config.cancel.as_ref(),
            resume: config.resume,
        })
    })
}
