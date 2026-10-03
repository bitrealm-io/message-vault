//! Full export pipeline (wtsexporter, then JSON convert); the desktop app calls
//! it in process.

use crate::emit::{ConvertRequest, convert_json};
use crate::ios_backup::decrypt_if_encrypted;
use crate::owner::{owner_from_backup, owner_from_form};
use crate::wtsexporter::{Platform, WtsexporterArgs, resolve_wtsexporter, run_wtsexporter};
use anyhow::{Context, Result, bail};
use message_crate_core::{
    ExportTransforms, ExporterConfig, RunResult, SourceConfig, WhatsappPlatform as CorePlatform,
    prepare_outputs,
};
use std::env;
use std::fs;

/// Resolve JSON (via wtsexporter or `WhatsappConfig::json`), then convert.
///
/// # Errors
///
/// Returns an error when the source is not WhatsApp, the output folder is or
/// holds what the run reads, wtsexporter cannot run, conversion fails, media
/// processing fails for every candidate file, or the user cancels.
pub fn run(config: &ExporterConfig) -> Result<RunResult> {
    let SourceConfig::Whatsapp(source) = &config.source else {
        bail!("whatsapp-exporter requires SourceConfig::Whatsapp");
    };
    message_crate_core::check_cancel(config.cancel.as_ref())?;
    let mut messages = Vec::new();

    let platform = match source.platform {
        Some(CorePlatform::Android) => Some(Platform::Android),
        Some(CorePlatform::Ios) => Some(Platform::Ios),
        None => None,
    };
    let input = config.primary_input().map(|p| p.to_path_buf());

    // The output is cleaned before it is written, so one that is or holds
    // what this run reads is refused first: the backup (the working folder
    // when none is named and wtsexporter runs) and a ready-made JSON.
    let mut read_paths = config.inputs.clone();
    match &source.json {
        Some(json) => read_paths.push(json.clone()),
        None if input.is_none() => {
            read_paths.push(env::current_dir().context("resolve current working directory")?);
        }
        None => {}
    }
    prepare_outputs(&read_paths, &config.output)?;

    // The number typed on the form, under the server's handle key. Android's
    // only source; iPhone's fallback when the backup carries no owner key.
    let form_owner = source
        .owner_phone
        .as_deref()
        .map(owner_from_form)
        .transpose()?;

    let (json_path, media_roots, owner_handle, _work_keep_alive) = if let Some(json) = &source.json
    {
        // Allowed roots are only the backup input and the JSON parent — never
        // the process CWD, which would let crafted paths copy arbitrary files.
        let mut media_roots = Vec::new();
        if let Some(path) = &input {
            media_roots.push(path.clone());
        }
        if let Some(parent) = json.parent() {
            media_roots.push(parent.to_path_buf());
        }
        media_roots.sort();
        media_roots.dedup();
        // A ready-made result.json names no owner; the form's number is all
        // there is, and a conversion may leave it empty.
        (json.clone(), media_roots, form_owner, None)
    } else {
        let platform =
            platform.ok_or_else(|| anyhow::anyhow!("platform is required unless json is set"))?;
        let input = match input {
            Some(path) => path,
            None => env::current_dir().context("resolve current working directory")?,
        };

        message_crate_core::check_cancel(config.cancel.as_ref())?;
        let bin = resolve_wtsexporter()?;
        // Scratch dir for wtsexporter cwd (iOS/Android extract) + result.json.
        // Kept until after convert so media copy can read extracted files.
        let work = tempfile::Builder::new()
            .prefix("wtsexporter-")
            .tempdir_in(&config.output)
            .context("create temp dir for wtsexporter")?;
        let json_out = work.path().join("result.json");

        // Cooperative only: cancel is checked before and after the external process.
        // Killing wtsexporter mid-run is not implemented.
        message_crate_core::check_cancel(config.cancel.as_ref())?;
        let mut args = WtsexporterArgs {
            platform,
            input: input.clone(),
            work_dir: work.path().to_path_buf(),
            key: source.key.clone(),
            backup: source.backup.clone(),
            wa: source.wa.clone(),
            media: source.media.clone(),
            db: source.db.clone(),
            business: source.business,
        };
        // wtsexporter cannot be given an iPhone backup password, so an
        // encrypted backup's WhatsApp files are decrypted into the work dir
        // first and wtsexporter reads those instead of the backup.
        if platform == Platform::Ios
            && let Some(decrypted) = decrypt_if_encrypted(source, work.path(), config)?
        {
            args.read_decrypted(decrypted);
        }
        message_crate_core::check_cancel(config.cancel.as_ref())?;
        let log = run_wtsexporter(&bin, &args, &json_out)?;
        message_crate_core::check_cancel(config.cancel.as_ref())?;

        if !log.trim().is_empty() {
            let trimmed = log.trim_end_matches('\n');
            messages.push(trimmed.to_string());
        }

        let kept = config.output.join("wtsexporter_result.json");
        fs::copy(&json_out, &kept).with_context(|| format!("copy JSON to {}", kept.display()))?;

        // Work dir (wtsexporter extract) + backup input. The backup input is the
        // process cwd when the config names no input.
        let mut media_roots = vec![work.path().to_path_buf(), input];
        media_roots.sort();
        media_roots.dedup();

        let owner_handle = match platform {
            // wtsexporter copies the whole app-group domain into the work
            // dir, preferences plist included; a backup someone extracted by
            // hand has it under the input folder. The form's number covers a
            // backup that carries no owner key (the Business app, a moved key).
            Platform::Ios => match owner_from_backup(&media_roots, &mut messages).or(form_owner) {
                Some(owner) => owner,
                None => bail!(
                    "the backup does not contain your WhatsApp phone number; \
                     enter it on the import form"
                ),
            },
            // A crypt backup carries no owner; the form checks the field is
            // filled before the run starts, so this only guards a caller
            // that skipped the form.
            Platform::Android => {
                form_owner.ok_or_else(|| anyhow::anyhow!("Owner's WhatsApp number is required."))?
            }
        };

        (kept, media_roots, Some(owner_handle), Some(work))
    };

    if !json_path.is_file() {
        bail!("JSON not found: {}", json_path.display());
    }

    message_crate_core::check_cancel(config.cancel.as_ref())?;
    let transforms = ExportTransforms::from_config(config);
    let needs_media_tools = transforms.needs_media_tools();
    let report = convert_json(ConvertRequest {
        json_path: &json_path,
        output: &config.output,
        transforms,
        media_search_roots: &media_roots,
        owner_handle,
        output_format: config.output_format,
        cancel: config.cancel.as_ref(),
        resume: config.resume,
    })?;
    // Drop tempdir after convert (media files already copied).
    drop(_work_keep_alive);

    let mut result = message_crate_core::finish_run(config, &report, needs_media_tools)?;
    messages.append(&mut result.messages);
    result.messages = messages;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use message_crate_core::testutil::jsonl_run_config;
    use message_crate_core::{SourceConfig, WhatsappConfig};
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
    /// that is or holds the backup or the JSON would delete what is being
    /// read.
    #[test]
    fn run_refuses_an_output_that_is_or_contains_an_input() {
        let tmp = tempfile::tempdir().unwrap();
        let backup = tmp.path().join("backup");
        fs::create_dir_all(&backup).unwrap();
        let json = backup.join("result.json");
        fs::write(&json, "{}").unwrap();

        for (inputs, output) in [
            // The JSON's folder.
            (vec![], backup.clone()),
            // A folder above the backup named as the input.
            (vec![backup.as_path()], tmp.path().to_path_buf()),
        ] {
            let config = jsonl_run_config(
                &inputs,
                &output,
                SourceConfig::Whatsapp(WhatsappConfig {
                    json: Some(json.clone()),
                    ..WhatsappConfig::default()
                }),
            );
            let before = entries(&output);
            let err = crate::run(&config).unwrap_err().to_string();
            assert!(
                err.contains("must not be the same as, or contain, the input"),
                "{}: {err}",
                output.display()
            );
            assert!(json.is_file(), "the JSON is left as it was");
            assert_eq!(
                entries(&output),
                before,
                "nothing is written to {}",
                output.display()
            );
        }
    }
}
