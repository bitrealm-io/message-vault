//! `summarize_staging`, `transcode_staging`, and `delete_staging` commands.
//!
//! These back the two approval gates a staged import stops at (Decision 16):
//! `summarize_staging` recomputes what a staged folder holds so the first
//! gate can show it, `transcode_staging` runs the convert/compress pass the
//! exporter deferred (see `extract::exporter_media_mode`), and
//! `delete_staging` is the decline path — closing a gate without approving
//! deletes the staging folder outright.
//!
//! `summarize_staging` and `transcode_staging` both build a
//! [`message_ir_format::TranscodeOptions`] from the same form fields
//! `extract` parses, reusing its parsing helpers rather than re-deriving
//! them, so a summary and the pass it forecasts always agree on what
//! `Convert`/`Compress` mean.
//!
//! ## The staging-child guard
//!
//! All three commands take both a `staging_dir` to act on and a
//! `staging_root` naming the Staging Directory it must live under —
//! both strings come from the same caller, so containment alone only proves
//! the two are consistent with each other, not that `staging_dir` was ever
//! a folder this app wrote. [`resolve_staging_child`] is the one guard all
//! three route through: it resolves both paths the way `open_path` already
//! does ([`paths::resolve_openable_path`]/[`paths::resolve_staging_root`]),
//! requires the target to be a direct child of the root (never the root
//! itself, never a grandchild), and — for the two commands that write to or
//! remove the folder — requires the `.message-vault-export` sentinel
//! `ir-format` writes into every folder it exports into. The sentinel check
//! is the decisive half: even a hostile or buggy `staging_root` value cannot
//! make a folder this app never exported into look deletable.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use message_ir_format::{EXPORT_SENTINEL, StagingSummary, TranscodeOptions, TranscodeReport};

use super::events;
use super::events::ExtractProgressEvent;
use super::extract::{parse_attachment_media, parse_compress_options, parse_max_resolution};
use super::jobs::{reset_and_clone_cancel, spawn_job};
use super::paths::{resolve_openable_path, resolve_staging_root};
use super::push::ASSET_MAX_BYTES;
use crate::state::AppState;

/// Form fields shared by `summarize_staging` and `transcode_staging` — the
/// same media fields the Extract form parses, addressed at an already-staged
/// folder instead of a fresh backup.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StagingArgs {
    /// Staging folder written by an earlier `extract` run.
    pub staging_dir: String,
    /// Staging Directory root every staging folder must live under —
    /// the same root `open_path` guards.
    pub staging_root: String,
    /// Attachment handling choice: `copy`, `convert`, `compress`, or `skip`.
    pub attachment_media: Option<String>,
    /// Video/image size cap for convert and compress: `720p`, `1080p`, or `4k`.
    pub media_max_resolution: Option<String>,
    /// Frame-rate cap for compressed video, for example `30`.
    pub media_max_fps: Option<String>,
    /// Smallest media file size that still counts as an attachment, for example `20M`.
    pub media_min_size: Option<String>,
}

/// Resolve `staging_dir` and confirm it is safe to act on: a direct child of
/// `staging_root` — never the root itself, never a grandchild — and, when
/// `require_sentinel`, containing the `.message-vault-export` sentinel
/// `ir-format` writes into every folder it exports into.
///
/// Both paths are resolved through [`resolve_openable_path`]/
/// [`resolve_staging_root`] — the same mechanism `open_path` already uses —
/// so the same empty/absolute/traversal/symlink checks guard this too.
/// Containment alone only proves `staging_dir` and `staging_root` are
/// consistent with each other, since both come from the same caller; the
/// sentinel check is the decisive one, catching a hostile or buggy
/// `staging_root` value that containment alone cannot.
///
/// `summarize_staging` is read-only and passes `require_sentinel: false`;
/// `transcode_staging` and `delete_staging` write to or remove the folder
/// and require it.
///
/// # Errors
///
/// Returns an error, naming which check failed: empty or relative path,
/// `staging_dir` resolves outside `staging_root`, is the root itself, is not
/// a direct child, or (when `require_sentinel`) is missing the sentinel file.
fn resolve_staging_child(
    staging_dir: &str,
    staging_root: &str,
    require_sentinel: bool,
) -> Result<PathBuf, String> {
    let resolved = resolve_openable_path(staging_dir, staging_root)?;
    let root = resolve_staging_root(staging_root)?;

    if resolved == root {
        return Err("Staging path must not be the Staging Directory itself".to_string());
    }
    if resolved.parent() != Some(root.as_path()) {
        return Err("Staging path must be a direct child of the Staging Directory".to_string());
    }
    if require_sentinel && !resolved.join(EXPORT_SENTINEL).is_file() {
        return Err(format!(
            "{} does not look like an export folder (missing {EXPORT_SENTINEL})",
            resolved.display()
        ));
    }
    Ok(resolved)
}

/// Build the [`TranscodeOptions`] a summary or media pass runs with, from the
/// same fields the Extract form parses.
///
/// # Errors
///
/// Returns an error if any field fails to parse (see
/// [`parse_attachment_media`], [`parse_max_resolution`], and
/// [`parse_compress_options`]).
fn build_transcode_options(args: &StagingArgs) -> Result<TranscodeOptions, String> {
    let chosen = parse_attachment_media(args.attachment_media.as_deref())?;
    let max_resolution = parse_max_resolution(args.media_max_resolution.as_deref())?;
    let max_fps = args.media_max_fps.as_deref().unwrap_or("30");
    let min_size = args.media_min_size.as_deref().unwrap_or("20M");
    let compress = parse_compress_options(chosen, max_resolution, max_fps, min_size)?;
    Ok(TranscodeOptions {
        mode: chosen.media_mode(),
        compress,
        asset_max_bytes: ASSET_MAX_BYTES,
    })
}

/// Recompute what a staged folder holds, for the first approval gate.
///
/// Reports progress on `extract:progress` with `step: "prepare"`, so a long
/// summary of a huge folder shows movement on the step the user is already
/// looking at. The read itself (folder walk plus ffprobe calls) runs on a
/// blocking-pool thread via [`tauri::async_runtime::spawn_blocking`], so it
/// cannot stall the async runtime other commands share.
///
/// # Errors
///
/// Returns an error if a form field is invalid, `staging_dir` is not a
/// direct child of `staging_root`, the folder cannot be read, or the
/// blocking task panicked.
#[tauri::command]
pub async fn summarize_staging(
    app: tauri::AppHandle,
    args: StagingArgs,
) -> Result<StagingSummary, String> {
    let options = build_transcode_options(&args)?;
    // Read-only: no sentinel required, only containment.
    let staging_dir = resolve_staging_child(&args.staging_dir, &args.staging_root, false)?;

    let progress_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        message_ir_format::summarize_staging(&staging_dir, &options, &mut |progress| {
            events::emit(
                &progress_app,
                events::PROGRESS,
                ExtractProgressEvent {
                    step: "prepare".into(),
                    done: progress.done,
                    total: progress.total,
                    bytes_done: None,
                    bytes_total: None,
                    status: None,
                },
            );
        })
        .map_err(|error| format!("{error:#}"))
    })
    .await
    .map_err(|join_error| format!("summarize_staging did not complete: {join_error}"))?
}

/// `count == 1` ? "" : "s" — the only pluralization these summaries need.
fn plural_s(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

/// One human-readable sentence describing a transcode pass's outcome.
///
/// Used both as the `extract:finished` payload's `summary` field (so a
/// client that falls back to raw JSON still has readable text) and, when
/// either count is nonzero, as an `extract:log` line so the same wording is
/// visible while the pass runs, not only after it finishes.
///
/// `too_large` and `failed` get separate clauses on purpose: a `too_large`
/// file WAS converted — it just came out over the limit and will not be
/// uploaded — which is a different fact from `failed`, a file the pass could
/// not convert at all. The report has no per-file reasons (those are written
/// into the conversation files' `missing_reason` instead), so this can only
/// speak in counts.
fn transcode_summary(report: &TranscodeReport) -> String {
    let mut clauses = vec![format!(
        "Converted {n} file{s}",
        n = report.converted,
        s = plural_s(report.converted)
    )];
    if report.too_large > 0 {
        clauses.push(format!(
            "{n} will not be uploaded (still too large after conversion)",
            n = report.too_large
        ));
    }
    if report.failed > 0 {
        clauses.push(format!(
            "{n} could not be converted; details are recorded in the staged files",
            n = report.failed
        ));
    }
    format!("{}.", clauses.join("; "))
}

/// Run the convert/compress pass over a staged folder, after the first gate
/// approves it.
///
/// Follows `extract`'s job shape: the cancel flag is reset through
/// [`reset_and_clone_cancel`], the pass runs on a background thread, and
/// progress/log/finished go back as `extract:*` events so the UI reuses one
/// progress view. A cancelled pass is reported through `extract:error` the
/// same way any other failure is — exactly how a cancelled `extract` run
/// already behaves (`extract` never special-cases its own cancellation
/// either; `spawn_job`'s generic `Err` handling covers both). An earlier
/// version of this command ended a cancelled pass quietly instead (an
/// `extract:log` line, `Ok(())`, no `extract:error`); that left
/// `awaitTauriJob`'s promise on the web side permanently unsettled — no
/// `extract:finished`, no `extract:error` — wedging the screen with `running`
/// stuck true and no way back except restarting the app. Do not restore the
/// quiet path.
///
/// The report only carries counts, not per-file reasons, so a nonzero
/// `failed`/`too_large` count is surfaced as one summarizing `extract:log`
/// line (see [`transcode_summary`]) rather than invented per-file
/// `extract:issue` events.
///
/// # Errors
///
/// Returns an error if a form field is invalid, `staging_dir` is not a
/// direct child of `staging_root` or is missing the export sentinel, or
/// another thread panicked while holding the shared state lock. Failures
/// during the pass — including a cancellation and ffmpeg/ffprobe being
/// unavailable — are sent as `extract:error`, verbatim, not returned here.
#[tauri::command]
pub async fn transcode_staging(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    args: StagingArgs,
) -> Result<(), String> {
    let options = build_transcode_options(&args)?;
    // Writes to and deletes originals inside the folder: sentinel required.
    let staging_dir = resolve_staging_child(&args.staging_dir, &args.staging_root, true)?;
    let cancel = reset_and_clone_cancel(&state)?;
    let has_media_step = matches!(
        options.mode,
        media::MediaMode::Convert | media::MediaMode::Compress
    );

    let app_handle = app.clone();
    spawn_job(app, move || {
        if has_media_step {
            events::emit(
                &app_handle,
                events::LOG,
                "Converting and compressing attachments…".to_string(),
            );
        }

        // Not `move`: the closure only needs `&app_handle` (`emit` takes
        // `&self`), so it borrows the one clone above rather than needing a
        // second — the same handle is still available by reference below,
        // once this borrow ends at the end of the `transcode_staged` call.
        let outcome = message_ir_format::transcode_staged(
            &staging_dir,
            &options,
            Some(&cancel),
            &mut |progress| {
                events::emit(
                    &app_handle,
                    events::PROGRESS,
                    ExtractProgressEvent {
                        step: "media".into(),
                        done: progress.done,
                        total: progress.total,
                        bytes_done: None,
                        bytes_total: None,
                        status: None,
                    },
                );
            },
        );

        // A cancellation is just another `Err` here — `spawn_job` reports it
        // as `extract:error` with the error chain as `detail`, the same
        // generic path a cancelled `extract` run already goes through. See
        // this function's doc comment for why the earlier quiet-cancel
        // special case was removed.
        let report = outcome?;

        let summary = transcode_summary(&report);
        if report.failed > 0 || report.too_large > 0 {
            events::emit(&app_handle, events::LOG, summary.clone());
        }

        let payload = serde_json::json!({
            "summary": summary,
            "converted": report.converted,
            "skipped": report.skipped,
            "too_large": report.too_large,
            "failed": report.failed,
            "missing": report.missing,
            "repointed": report.repointed,
            "bytes_before": report.bytes_before,
            "bytes_after": report.bytes_after,
        });
        events::emit(&app_handle, events::FINISHED, payload.to_string());
        Ok(())
    });

    Ok(())
}

/// Arguments for [`delete_staging`].
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteStagingArgs {
    /// Staging folder to remove.
    pub staging_dir: String,
    /// Staging Directory root every staging folder must live under —
    /// the same root `open_path` guards.
    pub staging_root: String,
}

/// Delete a staging folder — the decline path's terminal action (Decision
/// 16): closing an approval gate without approving deletes the folder
/// outright.
///
/// Runs on the async task pool (`#[tauri::command(async)]`) rather than the
/// main thread: `remove_dir_all` over a large staging folder would otherwise
/// freeze the window.
///
/// # Errors
///
/// Returns an error when `staging_dir` is not a direct child of
/// `staging_root`, is missing the export sentinel, or the folder cannot be
/// removed. Refuses rather than silently doing nothing, so a path bug here
/// cannot turn into a delete somewhere else on disk.
#[tauri::command(async)]
pub fn delete_staging(args: DeleteStagingArgs) -> Result<(), String> {
    delete_staging_dir(&args.staging_root, &args.staging_dir)
}

/// Delete `staging_dir`, refusing anything that is not a direct child of
/// `staging_root` carrying the export sentinel.
///
/// A `staging_dir` that no longer exists is treated as already deleted — the
/// decline path may run after a crash that already removed it. This check
/// runs against the raw path before any guard, so a target that plainly
/// isn't there never depends on the guard's outcome to stay a no-op.
///
/// # Errors
///
/// Returns an error when `staging_dir` fails [`resolve_staging_child`]'s
/// guard or the folder cannot be removed.
fn delete_staging_dir(staging_root: &str, staging_dir: &str) -> Result<(), String> {
    if !Path::new(staging_dir).exists() {
        return Ok(());
    }
    let resolved = resolve_staging_child(staging_dir, staging_root, true)?;
    std::fs::remove_dir_all(&resolved)
        .map_err(|error| format!("Could not delete {}: {error}", resolved.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use media::MediaMode;
    use std::fs;

    /// Stage a folder directly under `root` with the export sentinel, as
    /// `extract`/`ir-format` would leave it.
    fn stage_export(root: &Path, name: &str) -> PathBuf {
        let staged = root.join(name);
        fs::create_dir_all(&staged).unwrap();
        fs::write(staged.join(EXPORT_SENTINEL), "").unwrap();
        staged
    }

    #[test]
    fn delete_staging_refuses_a_path_outside_the_staging_root() {
        // This command deletes a directory tree. The only thing standing
        // between a path bug and someone's home folder is this check.
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let victim = stage_export(outside.path(), "keep-me");

        let err = delete_staging_dir(root.path().to_str().unwrap(), victim.to_str().unwrap())
            .unwrap_err();

        assert!(err.contains("staging"), "the refusal should say why: {err}");
        assert!(victim.exists());
    }

    #[test]
    fn delete_staging_removes_a_folder_inside_the_root() {
        let root = tempfile::tempdir().unwrap();
        let staged = stage_export(root.path(), "staging-run-1");
        fs::write(staged.join("a.jsonl"), b"{}").unwrap();

        delete_staging_dir(root.path().to_str().unwrap(), staged.to_str().unwrap()).unwrap();

        assert!(!staged.exists());
    }

    #[test]
    fn delete_staging_is_quiet_about_a_folder_that_is_already_gone() {
        // The decline path may run after a crash that already removed it.
        let root = tempfile::tempdir().unwrap();
        let never_existed = root.path().join("never-existed");
        assert!(
            delete_staging_dir(
                root.path().to_str().unwrap(),
                never_existed.to_str().unwrap()
            )
            .is_ok()
        );
    }

    #[test]
    fn delete_staging_refuses_the_root_itself() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join(EXPORT_SENTINEL), "").unwrap();

        let err = delete_staging_dir(root.path().to_str().unwrap(), root.path().to_str().unwrap())
            .unwrap_err();

        assert!(err.contains("itself"), "{err}");
        assert!(root.path().exists());
    }

    #[test]
    fn delete_staging_refuses_a_grandchild() {
        let root = tempfile::tempdir().unwrap();
        let staged = stage_export(root.path(), "staging-run-1");
        let grandchild = staged.join("attachments");
        fs::create_dir_all(&grandchild).unwrap();
        fs::write(grandchild.join(EXPORT_SENTINEL), "").unwrap();

        let err = delete_staging_dir(root.path().to_str().unwrap(), grandchild.to_str().unwrap())
            .unwrap_err();

        assert!(err.contains("direct child"), "{err}");
        assert!(grandchild.exists());
    }

    #[test]
    fn delete_staging_refuses_a_direct_child_without_the_sentinel() {
        // Containment alone only proves the two argument strings agree with
        // each other. The sentinel is what proves this folder was ever an
        // export target.
        let root = tempfile::tempdir().unwrap();
        let staged = root.path().join("not-an-export");
        fs::create_dir_all(&staged).unwrap();

        let err = delete_staging_dir(root.path().to_str().unwrap(), staged.to_str().unwrap())
            .unwrap_err();

        assert!(err.contains(EXPORT_SENTINEL), "{err}");
        assert!(staged.exists());
    }

    #[test]
    fn delete_staging_refuses_parent_traversal() {
        let root = tempfile::tempdir().unwrap();
        let staging_root = root.path().join("staging-root");
        fs::create_dir_all(&staging_root).unwrap();
        let victim = stage_export(root.path(), "victim");

        let traversal = staging_root.join("..").join("victim");
        let err = delete_staging_dir(staging_root.to_str().unwrap(), traversal.to_str().unwrap())
            .unwrap_err();

        assert!(err.contains("outside"), "{err}");
        assert!(victim.exists());
    }

    #[test]
    fn delete_staging_refuses_a_sibling_whose_name_merely_prefix_matches() {
        // `/x/staging-root-evil` is not under `/x/staging-root` even though
        // the raw string starts with it — path containment compares
        // components, not string prefixes, and this pins that.
        let base = tempfile::tempdir().unwrap();
        let staging_root = base.path().join("staging-root");
        fs::create_dir_all(&staging_root).unwrap();
        let evil_root = base.path().join("staging-root-evil");
        let victim = stage_export(&evil_root, "target");

        let err = delete_staging_dir(staging_root.to_str().unwrap(), victim.to_str().unwrap())
            .unwrap_err();

        assert!(err.contains("outside"), "{err}");
        assert!(victim.exists());
    }

    #[cfg(unix)]
    #[test]
    fn delete_staging_refuses_a_symlink_inside_the_root_pointing_outside() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let victim = stage_export(outside.path(), "victim");
        let link = root.path().join("staging-link");
        symlink(&victim, &link).unwrap();

        let err =
            delete_staging_dir(root.path().to_str().unwrap(), link.to_str().unwrap()).unwrap_err();

        assert!(err.contains("outside"), "{err}");
        assert!(victim.exists());
    }

    #[test]
    fn delete_staging_refuses_a_relative_root() {
        let root = tempfile::tempdir().unwrap();
        let staged = stage_export(root.path(), "staging-run-1");

        let err = delete_staging_dir(".", staged.to_str().unwrap()).unwrap_err();

        assert!(err.contains("absolute"), "{err}");
        assert!(staged.exists());
    }

    #[cfg(unix)]
    #[test]
    fn delete_staging_fails_loudly_when_removal_itself_fails() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let staged = stage_export(root.path(), "staging-run-1");
        let locked = staged.join("locked");
        fs::create_dir_all(&locked).unwrap();
        let mut perms = fs::metadata(&locked).unwrap().permissions();
        perms.set_mode(0o000);
        fs::set_permissions(&locked, perms).unwrap();

        let result = delete_staging_dir(root.path().to_str().unwrap(), staged.to_str().unwrap());

        // Restore permissions so the tempdir can clean itself up regardless
        // of the assertion outcome below.
        let mut perms = fs::metadata(&locked).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&locked, perms).unwrap();

        assert!(
            result.is_err(),
            "a folder that fails to remove must yield Err, not a quiet Ok"
        );
    }

    #[test]
    fn transcode_options_use_the_shared_asset_max_bytes() {
        let args = StagingArgs {
            staging_dir: "/tmp/staging-root/staging-run".into(),
            staging_root: "/tmp/staging-root".into(),
            attachment_media: Some("compress".into()),
            media_max_resolution: Some("720p".into()),
            media_max_fps: Some("24".into()),
            media_min_size: Some("5M".into()),
        };
        let options = build_transcode_options(&args).unwrap();
        // Pins the literal, not just the wiring — a change to the constant
        // elsewhere must not silently move this too.
        assert_eq!(options.asset_max_bytes, 50 * 1024 * 1024);
        assert_eq!(options.mode, MediaMode::Compress);
        assert_eq!(options.compress.max_fps, 24.0);
    }

    #[test]
    fn transcode_options_default_the_media_fields_like_extract_does() {
        let args = StagingArgs {
            staging_dir: "/tmp/staging-root/staging-run".into(),
            staging_root: "/tmp/staging-root".into(),
            attachment_media: Some("convert".into()),
            media_max_resolution: None,
            media_max_fps: None,
            media_min_size: None,
        };
        let options = build_transcode_options(&args).unwrap();
        assert_eq!(options.mode, MediaMode::Convert);
        // Convert does not use CompressOptions, but defaulting must still
        // succeed rather than error on missing fields.
        assert_eq!(options.compress, media::CompressOptions::default());
    }

    #[test]
    fn transcode_summary_gives_too_large_and_failed_separate_clauses() {
        // A too_large file WAS converted (it just won't be uploaded); a
        // failed file was not converted at all. Lumping them into one
        // "could not be converted" count would misstate the too_large ones.
        let report = TranscodeReport {
            converted: 12,
            too_large: 2,
            failed: 1,
            ..Default::default()
        };
        let summary = transcode_summary(&report);
        assert!(summary.contains("Converted 12 files"), "{summary}");
        assert!(summary.contains("2 will not be uploaded"), "{summary}");
        assert!(!summary.contains("2 could not be converted"), "{summary}");
        assert!(summary.contains("1 could not be converted"), "{summary}");
    }

    #[test]
    fn transcode_summary_with_no_issues_is_just_the_converted_count() {
        let report = TranscodeReport {
            converted: 5,
            ..Default::default()
        };
        assert_eq!(transcode_summary(&report), "Converted 5 files.");
    }
}
