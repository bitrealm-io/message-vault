//! Full export pipeline; the desktop app calls it in process.

use crate::emit::{ConvertExportArgs, convert_export};
use anyhow::{Result, bail};
use message_crate_core::{ExporterConfig, RunResult, SourceConfig, prepare_outputs};

/// Convert, then apply media transforms and obfuscation.
///
/// # Errors
///
/// Returns an error when the source is not SMS Backup & Restore, the output
/// folder is or holds the input, conversion fails, media processing fails for
/// every candidate file, or the user cancels.
pub fn run(config: &ExporterConfig) -> Result<RunResult> {
    let SourceConfig::SmsBackupRestore(source) = &config.source else {
        bail!("sms-backup-restore-exporter requires SourceConfig::SmsBackupRestore");
    };
    message_crate_core::check_cancel(config.cancel.as_ref())?;
    let input = config.require_input().map_err(anyhow::Error::msg)?;
    let (inputs, output_dir) = prepare_outputs(&[input.to_path_buf()], &config.output)?;
    message_crate_core::run_pipeline(config, |transforms| {
        convert_export(ConvertExportArgs {
            input: &inputs[0],
            output_dir: &output_dir,
            owner_phones: &source.owner_phones,
            transforms,
            output_format: config.output_format,
            cancel: config.cancel.as_ref(),
            resume: config.resume,
        })
    })
}

#[cfg(test)]
mod tests {
    use message_crate_core::testutil::jsonl_run_config;
    use message_crate_core::{SmsBackupRestoreConfig, SourceConfig};
    use std::fs;
    use std::path::Path;

    /// The names in `dir`, sorted.
    fn entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// The writer cleans its output folder before it writes, so an output
    /// that is or holds the backup would delete the backup being read.
    #[test]
    fn run_refuses_an_output_that_is_or_contains_the_backup() {
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("backup");
        fs::create_dir_all(&input).unwrap();
        let xml = input.join("sms-1.xml");
        fs::write(
            &xml,
            r#"<smses count="1"><sms address="+15555550101" date="1400773261000" type="1" body="hello" /></smses>"#,
        )
        .unwrap();

        for output in [input.clone(), tmp.path().to_path_buf()] {
            let config = jsonl_run_config(
                &[&input],
                &output,
                SourceConfig::SmsBackupRestore(SmsBackupRestoreConfig {
                    owner_phones: vec!["+15555550100".into()],
                }),
            );
            let before = entries(&output);
            let err = crate::run(&config).unwrap_err().to_string();
            assert!(
                err.contains("must not be the same as, or contain, the input"),
                "{}: {err}",
                output.display()
            );
            assert!(xml.is_file(), "the backup is left as it was");
            assert_eq!(
                entries(&output),
                before,
                "nothing is written to {}",
                output.display()
            );
        }
    }
}
