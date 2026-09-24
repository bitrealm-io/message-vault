//! Full export pipeline (wtsexporter/JSON convert) for CLI and GUI.

use crate::emit::{ConvertRequest, convert_json};
use crate::owner::{owner_from_backup, owner_from_form};
use crate::wtsexporter::{Platform, WtsexporterArgs, resolve_wtsexporter, run_wtsexporter};
use anyhow::{Context, Result, bail};
use message_vault_io_core::{
    ExportTransforms, ExporterConfig, RunResult, SourceConfig, WhatsappPlatform as CorePlatform,
};
use std::env;
use std::fs;

/// Resolve JSON (via wtsexporter or `--json`), then convert.
///
/// # Errors
///
/// Returns an error when the source is not WhatsApp, wtsexporter cannot run,
/// conversion fails, media processing fails for every candidate file, or the
/// user cancels.
pub fn run(config: &ExporterConfig) -> Result<RunResult> {
    let SourceConfig::Whatsapp(source) = &config.source else {
        bail!("whatsapp-exporter requires SourceConfig::Whatsapp");
    };
    message_vault_io_core::check_cancel(config.cancel.as_ref())?;
    let mut messages = Vec::new();

    let platform = match source.platform {
        Some(CorePlatform::Android) => Some(Platform::Android),
        Some(CorePlatform::Ios) => Some(Platform::Ios),
        None => None,
    };
    let input = config.primary_input().map(|p| p.to_path_buf());

    // The number typed on the form, under the vault's handle key. Android's
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

        message_vault_io_core::check_cancel(config.cancel.as_ref())?;
        let bin = resolve_wtsexporter()?;
        fs::create_dir_all(&config.output)
            .with_context(|| format!("create {}", config.output.display()))?;
        // Scratch dir for wtsexporter cwd (iOS/Android extract) + result.json.
        // Kept until after convert so media copy can read extracted files.
        let work = tempfile::Builder::new()
            .prefix("wtsexporter-")
            .tempdir_in(&config.output)
            .context("create temp dir for wtsexporter")?;
        let json_out = work.path().join("result.json");

        // Cooperative only: cancel is checked before and after the external process.
        // Killing wtsexporter mid-run is not implemented.
        message_vault_io_core::check_cancel(config.cancel.as_ref())?;
        let log = run_wtsexporter(
            &bin,
            &WtsexporterArgs {
                platform,
                input: input.clone(),
                work_dir: work.path().to_path_buf(),
                key: source.key.clone(),
                backup: source.backup.clone(),
                wa: source.wa.clone(),
                media: source.media.clone(),
                db: source.db.clone(),
                business: source.business,
            },
            &json_out,
        )?;
        message_vault_io_core::check_cancel(config.cancel.as_ref())?;

        if !log.trim().is_empty() {
            let trimmed = log.trim_end_matches('\n');
            messages.push(trimmed.to_string());
        }

        let kept = config.output.join("wtsexporter_result.json");
        fs::copy(&json_out, &kept).with_context(|| format!("copy JSON to {}", kept.display()))?;

        // Work dir (wtsexporter extract) + backup input only — not CWD.
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

    message_vault_io_core::check_cancel(config.cancel.as_ref())?;
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

    let result = message_vault_io_core::finish_run(config, &report, needs_media_tools)?;
    messages.extend(result.messages);
    Ok(RunResult { messages })
}
