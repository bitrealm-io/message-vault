//! Empty Trash (`DELETE /v1/trash`), and the on-disk half of permanent
//! deletion that `DELETE /v1/conversations/{id}` shares with it.
//!
//! The database work lives in [`crate::db::trash`]. What is left for the
//! route layer is removing the attachment files the database reported as
//! unreferenced once its transaction has committed, which is filesystem work
//! and belongs here rather than in `db/`.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;

use crate::config::Config;
use crate::db::trash::{OrphanedFile, empty_trash};
use crate::server::{ApiError, AppState, FullDeleteAccess};

/// Remove the files `db::trash` reported as unreferenced: each original, its
/// MIME sidecar when one exists, and each derivative. A file already gone is
/// not an error — the database rows are the record, and a missing file only
/// means there is nothing left to do for it.
///
/// Runs on the blocking pool because it is plain filesystem work.
///
/// # Errors
///
/// `Internal` when a file exists and cannot be removed, or when a stored path
/// would escape the directory it belongs under. Either is a defect in the
/// vault's own data, not something the caller did, so the rows stay deleted
/// and the message is logged rather than shown.
pub(crate) async fn remove_orphaned_files(
    cfg: Arc<Config>,
    account_id: String,
    files: Vec<OrphanedFile>,
) -> Result<(), ApiError> {
    if files.is_empty() {
        return Ok(());
    }
    tokio::task::spawn_blocking(move || {
        for file in files {
            for path in paths_to_remove(&cfg, &account_id, &file)? {
                remove_if_present(&path)?;
            }
        }
        Ok(())
    })
    .await
    .map_err(|e| ApiError::Internal(anyhow::anyhow!("asset removal task: {e}")))?
}

/// The absolute paths one orphaned file occupies on disk.
fn paths_to_remove(
    cfg: &Config,
    account_id: &str,
    file: &OrphanedFile,
) -> Result<Vec<PathBuf>, ApiError> {
    match file {
        OrphanedFile::Original {
            source,
            sha256,
            assets_path,
        } => {
            let dir = cfg.paths.assets_dir_for_account(account_id, source);
            let original = join_under(&dir, assets_path)?;
            let sidecar = dir.join(&sha256[..2]).join(format!(".{sha256}.mime"));
            Ok(vec![original, sidecar])
        }
        OrphanedFile::Derived {
            source,
            assets_path,
        } => {
            let dir = cfg
                .paths
                .assets_converted_dir_for_account(account_id, source);
            Ok(vec![join_under(&dir, assets_path)?])
        }
    }
}

/// `dir/relative`, refusing a stored path that is absolute or climbs out of
/// `dir`. The vault wrote every `assets_path` itself, so this never fires on
/// its own data; it is the guard that keeps a corrupted row from naming a
/// file elsewhere on the machine.
fn join_under(dir: &Path, relative: &str) -> Result<PathBuf, ApiError> {
    let rel = Path::new(relative);
    let safe = rel.components().all(|c| matches!(c, Component::Normal(_)));
    if !safe || relative.is_empty() {
        return Err(ApiError::Internal(anyhow::anyhow!(
            "stored asset path {relative:?} is not a plain relative path"
        )));
    }
    Ok(dir.join(rel))
}

/// Remove `path` when it is a regular file; a missing file is fine.
fn remove_if_present(path: &Path) -> Result<(), ApiError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(ApiError::Internal(anyhow::anyhow!(
            "remove {}: {e}",
            path.display()
        ))),
    }
}

/// Empty the trash: every trashed conversation is deleted for good, with
/// its messages and any attachment file no other message uses, and every
/// trashed contact loses its name and details and becomes Unknown, its
/// conversations untouched. Trash is the only door to permanent deletion;
/// this is the door for everything in it at once.
#[utoipa::path(
    delete,
    path = "/v1/trash",
    tag = "Trash",
    security(("bearer" = [])),
    responses(
        (status = 204, description = "Trash emptied"),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem)
    )
)]
pub(crate) async fn empty_trash_handler(
    State(state): State<AppState>,
    FullDeleteAccess(auth): FullDeleteAccess,
) -> Result<StatusCode, ApiError> {
    let orphaned = {
        let mut conn = state.db.acquire().await?;
        empty_trash(&mut conn, &auth.account_id).await?
    };
    remove_orphaned_files(Arc::clone(&state.cfg), auth.account_id, orphaned).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests;
