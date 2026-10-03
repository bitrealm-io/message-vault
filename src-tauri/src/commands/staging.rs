//! `summarize_staging`, `transcode_staging`, `delete_staging`, and the
//! Import Run record commands.
//!
//! These back the two reviews a staged import stops at (Decision 16):
//! `summarize_staging` recomputes what a staged folder holds so the first
//! review can show it, `transcode_staging` runs the convert/compress pass the
//! exporter deferred (see `extract::exporter_attachment_media`), and
//! `delete_staging` removes the staging folder — when a review is closed
//! without approving, when a paused run is discarded, and when the server
//! records the run as finished, since nothing will read the folder again.
//! `read_import_run_record` and `save_import_run_record` keep the record of a
//! paused run's earlier parts in its folder ([`RUN_RECORD_NAME`]).
//!
//! `summarize_staging` and `transcode_staging` take only the folder. They
//! read the run's media settings from it, where `extract` recorded them
//! ([`message_staging::read_media_settings`]), so a summary, the pass it
//! forecasts, and the Staging before them all work to the one set of values
//! the Import Run was started with.
//!
//! ## The staging-child guard
//!
//! Every command here takes both a `staging_dir` to act on and a
//! `staging_root` naming the Staging Directory it must live under —
//! both strings come from the same caller, so containment alone only proves
//! the two are consistent with each other, not that `staging_dir` was ever
//! a folder this app wrote. [`resolve_staging_child`] is the one guard they
//! all route through: it resolves both paths the way `open_path` already
//! does ([`paths::resolve_openable_path`]/[`paths::resolve_staging_root`]),
//! requires the target to be a direct child of the root (never the root
//! itself, never a grandchild), and — for the commands that write to or
//! remove the folder — requires the `.message-crate-export` sentinel
//! `ir-format` writes into every folder it exports into. The sentinel check
//! is the decisive half: even a hostile or buggy `staging_root` value cannot
//! make a folder this app never exported into look deletable.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use message_ir_format::EXPORT_SENTINEL;
use message_staging::{StagingSummary, TranscodeOptions, TranscodeReport};

use super::events;
use super::events::ExtractProgressEvent;
use super::jobs::{spawn_job, start_job};
use super::paths::{resolve_openable_path, resolve_staging_root};
use crate::state::AppState;

/// The folder `summarize_staging`, `transcode_staging` and `delete_staging`
/// act on, and the Staging Directory it must live under.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StagingArgs {
    /// Staging folder written by an earlier `extract` run.
    pub staging_dir: String,
    /// Staging Directory root every staging folder must live under —
    /// the same root `open_path` guards.
    pub staging_root: String,
}

/// Resolve `staging_dir` and confirm it is safe to act on: a direct child of
/// `staging_root` — never the root itself, never a grandchild — and, when
/// `require_sentinel`, containing the `.message-crate-export` sentinel
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

/// Resolve the staged folder through [`resolve_staging_child`] and read the
/// media settings its Staging recorded there.
///
/// # Errors
///
/// Returns an error when the folder fails the guard, or holds no readable
/// media settings because its Staging never finished.
fn staged_folder(
    args: &StagingArgs,
    require_sentinel: bool,
) -> Result<(PathBuf, TranscodeOptions), String> {
    let staging_dir =
        resolve_staging_child(&args.staging_dir, &args.staging_root, require_sentinel)?;
    let options =
        message_staging::read_media_settings(&staging_dir).map_err(|error| format!("{error:#}"))?;
    Ok((staging_dir, options))
}

/// Recompute what a staged folder holds, for the first review.
///
/// Reports progress on `extract:progress` with `step: "check"`, so a long
/// summary of a huge folder shows movement on the step the user is already
/// looking at. The read itself (folder walk plus ffprobe calls) runs on a
/// blocking-pool thread via [`tauri::async_runtime::spawn_blocking`], so it
/// cannot stall the async runtime other commands share.
///
/// # Errors
///
/// Returns an error if `staging_dir` is not a direct child of
/// `staging_root`, holds no media settings, cannot be read, or the blocking
/// task panicked.
#[tauri::command]
pub async fn summarize_staging(
    app: tauri::AppHandle,
    args: StagingArgs,
) -> Result<StagingSummary, String> {
    // Read-only: no sentinel required, only containment.
    let (staging_dir, options) = staged_folder(&args, false)?;

    let progress_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        message_staging::summarize_staging(&staging_dir, &options, &mut |progress| {
            events::emit(
                &progress_app,
                events::PROGRESS,
                ExtractProgressEvent {
                    step: "check".into(),
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

/// Run the convert/compress pass over a staged folder, after the first review
/// approves it.
///
/// Follows `extract`'s job shape: the job starts through [`start_job`], with
/// a cancel flag of its own, the pass runs on a background thread, and
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
/// Returns an error if `staging_dir` is not a direct child of
/// `staging_root` or is missing the export sentinel or the media settings, another
/// job is running, or another thread panicked while holding the shared state lock. Failures
/// during the pass — including a cancellation and ffmpeg/ffprobe being
/// unavailable — are sent as `extract:error`, verbatim, not returned here.
#[tauri::command(async)]
pub fn transcode_staging(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    args: StagingArgs,
) -> Result<(), String> {
    // Writes to and deletes originals inside the folder: sentinel required.
    let (staging_dir, options) = staged_folder(&args, true)?;
    let job = start_job(&state, "a Media pass")?;
    let cancel = job.cancel_flag();
    let has_media_step = matches!(
        options.mode,
        media::MediaMode::Convert | media::MediaMode::Compress
    );

    let app_handle = app.clone();
    spawn_job(app, job, move || {
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
        let outcome = message_staging::transcode_staged(
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
        Ok(payload.to_string())
    });

    Ok(())
}

/// Delete a staging folder: the decline path's terminal action (Decision
/// 16), and the last step of a successful import, whose staged copy of the
/// messages, push log, journal and report the server has no further use for.
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
pub fn delete_staging(args: StagingArgs) -> Result<(), String> {
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

/// File in a staging folder holding the Import Run's record so far: the
/// Import Errors, timings and counts of the parts that ran before the run
/// paused or stopped at a Review.
///
/// A paused run posts no completion, so the server never sees what its
/// earlier parts recorded. The window writes the record here, beside the
/// push journal, and reads it back when the run resumes, so the completion
/// it finally posts covers the whole run. The leading dot keeps it out of
/// every listing of conversation files, and it is deleted with the folder.
/// It has no `.json` extension, for the reason the media settings file has
/// none (`message_staging::MEDIA_SETTINGS_FILE`): a fresh export into the
/// folder would delete it as an earlier run's output.
pub const RUN_RECORD_NAME: &str = ".message-crate-run";

/// Arguments for [`save_import_run_record`].
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveRunRecordArgs {
    /// Staging folder of the Import Run.
    pub staging_dir: String,
    /// Staging Directory root the folder must live under.
    pub staging_root: String,
    /// The record as the window builds it. Its shape belongs to the window.
    pub record: serde_json::Value,
}

/// Read the Import Run's record from its staging folder.
///
/// Returns `None` when the folder holds no record: a run that has not yet
/// paused or stopped at a Review has not written one.
///
/// # Errors
///
/// Returns an error when the folder fails the staging-child guard, or the
/// record cannot be read or is not JSON.
#[tauri::command(async)]
pub fn read_import_run_record(args: StagingArgs) -> Result<Option<serde_json::Value>, String> {
    read_run_record(&args.staging_root, &args.staging_dir)
}

/// Write the Import Run's record into its staging folder, replacing the one
/// there.
///
/// # Errors
///
/// Returns an error when the folder fails the staging-child guard (the
/// sentinel included, since this writes into the folder) or the file cannot
/// be written.
#[tauri::command(async)]
pub fn save_import_run_record(args: SaveRunRecordArgs) -> Result<(), String> {
    save_run_record(&args.staging_root, &args.staging_dir, &args.record)
}

/// The work of [`read_import_run_record`].
fn read_run_record(
    staging_root: &str,
    staging_dir: &str,
) -> Result<Option<serde_json::Value>, String> {
    let resolved = resolve_staging_child(staging_dir, staging_root, false)?;
    let path = resolved.join(RUN_RECORD_NAME);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Could not read {}: {error}", path.display())),
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| format!("{} is not readable: {error}", path.display()))
}

/// The work of [`save_import_run_record`]. It writes a temporary file and
/// renames it over the record, so a crash mid-write leaves the previous
/// record whole.
fn save_run_record(
    staging_root: &str,
    staging_dir: &str,
    record: &serde_json::Value,
) -> Result<(), String> {
    let resolved = resolve_staging_child(staging_dir, staging_root, true)?;
    let path = resolved.join(RUN_RECORD_NAME);
    let tmp = resolved.join(format!("{RUN_RECORD_NAME}.tmp"));
    let body = serde_json::to_vec(record).map_err(|error| error.to_string())?;
    std::fs::write(&tmp, body)
        .and_then(|()| std::fs::rename(&tmp, &path))
        .map_err(|error| format!("Could not write {}: {error}", path.display()))
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
    fn a_staged_folder_is_read_with_the_settings_its_staging_recorded() {
        // The summary and the Media stage are given only the folder, so they
        // cannot be handed a mode the run did not start with (#1153).
        let root = tempfile::tempdir().unwrap();
        let staged = stage_export(root.path(), "staging-run-1");
        let recorded = TranscodeOptions {
            mode: MediaMode::Compress,
            compress: media::CompressOptions {
                max_fps: 24.0,
                ..Default::default()
            },
            asset_max_bytes: 123_456_789,
        };
        message_staging::write_media_settings(&staged, &recorded).unwrap();

        for require_sentinel in [false, true] {
            let (dir, options) = staged_folder(
                &StagingArgs {
                    staging_dir: staged.to_str().unwrap().into(),
                    staging_root: root.path().to_str().unwrap().into(),
                },
                require_sentinel,
            )
            .unwrap();
            assert_eq!(dir, staged.canonicalize().unwrap());
            assert_eq!(options, recorded);
        }
    }

    #[test]
    fn a_folder_whose_staging_recorded_no_settings_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let staged = stage_export(root.path(), "staging-run-1");

        let err = staged_folder(
            &StagingArgs {
                staging_dir: staged.to_str().unwrap().into(),
                staging_root: root.path().to_str().unwrap().into(),
            },
            false,
        )
        .unwrap_err();

        assert!(err.contains("media settings"), "{err}");
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

    #[test]
    fn a_saved_run_record_reads_back_as_written() {
        let root = tempfile::tempdir().unwrap();
        let staged = stage_export(root.path(), "staging-run-1");
        let root_str = root.path().to_str().unwrap();
        let staged_str = staged.to_str().unwrap();
        let record = serde_json::json!({ "issues": [{ "kind": "skip" }], "uploadMs": 1200 });

        save_run_record(root_str, staged_str, &record).unwrap();

        assert_eq!(read_run_record(root_str, staged_str).unwrap(), Some(record));
        assert!(
            !staged.join(format!("{RUN_RECORD_NAME}.tmp")).exists(),
            "the temporary file is renamed over the record"
        );
    }

    #[test]
    fn a_folder_with_no_run_record_reads_as_none() {
        let root = tempfile::tempdir().unwrap();
        let staged = stage_export(root.path(), "staging-run-1");

        let read = read_run_record(root.path().to_str().unwrap(), staged.to_str().unwrap());

        assert_eq!(read, Ok(None));
    }

    #[test]
    fn saving_a_run_record_refuses_a_folder_without_the_sentinel() {
        // The record is written into the folder, so the guard a delete
        // passes applies: a folder this app never exported into is refused.
        let root = tempfile::tempdir().unwrap();
        let plain = root.path().join("not-an-export");
        fs::create_dir_all(&plain).unwrap();

        let err = save_run_record(
            root.path().to_str().unwrap(),
            plain.to_str().unwrap(),
            &serde_json::json!({}),
        )
        .unwrap_err();

        assert!(err.contains(EXPORT_SENTINEL), "{err}");
        assert!(!plain.join(RUN_RECORD_NAME).exists());
    }
}
