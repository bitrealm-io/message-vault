//! Full export pipeline; the desktop app calls it in process.

use crate::emit::{ConvertExportArgs, convert_export};
use anyhow::{Result, bail};
use message_crate_core::{ExportTransforms, ExporterConfig, RunResult, SourceConfig};

/// Check the required inputs, then convert.
///
/// The shared `run_pipeline` cannot apply `SmsBackupPlusConfig::include_summary`
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
    message_crate_core::check_cancel(config.cancel.as_ref())?;

    if source.owner_phones.is_empty() {
        bail!("SMS Backup+ needs the backup device's phone number");
    }
    if source.owner_emails.is_empty() {
        bail!("SMS Backup+ needs the backup device's email address");
    }

    let transforms = ExportTransforms::from_config(config);
    let report = convert_export(ConvertExportArgs {
        inputs: &config.inputs,
        output_dir: &config.output,
        owner_phones: &source.owner_phones,
        owner_emails: &source.owner_emails,
        verbose: source.verbose,
        transforms,
        output_format: config.output_format,
        cancel: config.cancel.as_ref(),
        log: config.log.as_ref(),
        resume: config.resume,
    })?;
    if source.include_summary {
        return message_crate_core::finish_run(config, &report, config.media.mode.needs_tools());
    }
    // No summary wanted: the shared tail appends the summary unconditionally, so
    // keep only the media lines.
    report.check_media(config.media.mode.needs_tools())?;
    Ok(RunResult::new(report.media_lines(), &report))
}

#[cfg(test)]
mod tests {
    use super::run;
    use crate::emit::{ConvertExportArgs, convert_export};
    use message_crate_core::testutil::jsonl_run_config;
    use message_crate_core::{ExportTransforms, OutputFormat, SmsBackupPlusConfig, SourceConfig};
    use std::path::Path;

    fn source(phones: &[&str], emails: &[&str]) -> SourceConfig {
        SourceConfig::SmsBackupPlus(SmsBackupPlusConfig {
            owner_phones: phones.iter().map(|p| (*p).to_string()).collect(),
            owner_emails: emails.iter().map(|e| (*e).to_string()).collect(),
            verbose: false,
            include_summary: false,
        })
    }

    /// The error the desktop app shows for a run the exporter refuses.
    fn refusal(inputs: &[&Path], source: SourceConfig) -> String {
        let tmp = tempfile::tempdir().unwrap();
        let config = jsonl_run_config(inputs, &tmp.path().join("out"), source);
        run(&config).unwrap_err().to_string()
    }

    /// The exporters have no command line, so a refusal says what is
    /// missing in the words of the desktop form and names no flag.
    #[test]
    fn a_refused_run_names_what_is_missing_and_no_flag() {
        let input = tempfile::tempdir().unwrap();
        assert_eq!(
            refusal(&[input.path()], source(&[], &["owner@example.com"])),
            "SMS Backup+ needs the backup device's phone number"
        );
        assert_eq!(
            refusal(&[input.path()], source(&["+15555550100"], &[])),
            "SMS Backup+ needs the backup device's email address"
        );
        assert_eq!(
            refusal(&[], source(&["+15555550100"], &["owner@example.com"])),
            "SMS Backup+ needs a backup directory"
        );

        let no_input = convert_export(ConvertExportArgs {
            inputs: &[] as &[&Path],
            output_dir: input.path(),
            owner_phones: &["+15555550100".into()],
            owner_emails: &["owner@example.com".into()],
            verbose: false,
            transforms: ExportTransforms::none(),
            output_format: OutputFormat::Jsonl,
            cancel: None,
            log: None,
            resume: false,
        })
        .unwrap_err()
        .to_string();
        assert_eq!(no_input, "SMS Backup+ needs a backup directory");
    }
}
