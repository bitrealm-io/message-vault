//! Full export pipeline for CLI and in-process GUI.

use crate::emit::{ConvertExportArgs, convert_export};
use anyhow::{Result, bail};
use message_ir_format::ExportTransforms;
use message_vault_io_core::{ExporterConfig, RunResult, SourceConfig};

/// Check the required inputs, then convert.
///
/// The shared `run_pipeline` cannot host this exporter's `--no-summary` flag
/// (it appends the summary lines unconditionally), so the SMS Backup+ specifics
/// stay here and only the shared `finish_run` tail is reused.
///
/// # Errors
///
/// Returns an error when the source is not SMS Backup+, an input, owner phone,
/// or owner email is missing, conversion fails, media processing fails for
/// every candidate file, or the user cancels.
pub fn run(config: &ExporterConfig) -> Result<RunResult> {
    let SourceConfig::SmsBackupPlus(source) = &config.source else {
        bail!("sms-backup-plus-exporter requires SourceConfig::SmsBackupPlus");
    };
    message_vault_io_core::check_cancel(config.cancel.as_ref())?;

    if source.owner_phones.is_empty() {
        bail!("owner phone required: pass --owner-phone");
    }
    if source.owner_emails.is_empty() {
        bail!("owner email required: pass --owner-email");
    }
    if config.inputs.is_empty() {
        bail!("no input given: pass --input PATH");
    }
    // An archive transcript is a wall clock with no offset, so there is no
    // instant to recover without a zone -- and nothing sane to fall back on.
    // The host's zone is the bug this refuses to reintroduce.
    let Some(time_zone) = config.time_zone else {
        bail!("time zone required: archive times are wall-clock and need the account's zone");
    };

    let transforms = ExportTransforms::from_config(config);
    let (report, sink) = convert_export(ConvertExportArgs {
        inputs: &config.inputs,
        output_dir: &config.output,
        owner_phones: &source.owner_phones,
        owner_emails: &source.owner_emails,
        time_zone,
        verbose: source.verbose,
        transforms,
        output_format: config.output_format,
        cancel: config.cancel.as_ref(),
        log: config.log.as_ref(),
        resume: config.resume,
    })?;
    if source.include_summary {
        return message_ir_format::finish_run(
            config,
            &report,
            &sink,
            config.media.mode.needs_tools(),
        );
    }
    // --no-summary: the shared tail appends the summary unconditionally, so
    // repeat its media-failure bail and keep only the sink log lines.
    if !sink.media.errors.is_empty() && sink.media.processed == 0 && config.media.mode.needs_tools()
    {
        anyhow::bail!("media processing failed for all candidate files");
    }
    Ok(RunResult {
        messages: sink.log_lines(),
    })
}
