//! The attachment files on disk, and the only code that removes one.
//!
//! An account's files sit under `data_dir/<account>/<source>/`:
//!
//! - `<assets_dir>/<aa>/<sha256><ext>` is an original, named by its
//!   SHA-256 and sharded by the fingerprint's first two characters.
//! - `<assets_dir>/<aa>/.<sha256>.mime` is the MIME sidecar beside an
//!   original whose name carries no type.
//! - `<assets_dir>/.incoming/` holds uploads in progress: `{sha256}-*.part`
//!   files and multipart folders `{sha256}/{upload_id}/`.
//! - `<assets_converted_dir>/<aa>/<sha256><ext>` is a Preview.
//!
//! A file is unused when no attachment row, promoted or in staging, names
//! it. That test alone is not enough while the account has a running Import
//! Run: `HEAD /v1/assets/{sha256}` may have told the run a file exists, or
//! the run may have uploaded it, and the batch that names it has not arrived
//! yet. So while a run is running, an original stays on disk, and
//! [`sweep_unreferenced`] removes it when the run ends. A Preview is never
//! kept for a run, because an import never names one: the server makes
//! Previews from originals after the fact.
//!
//! A removal that fails is logged and the rest go on. The database rows are
//! the record, so a request answers for what the database did, and a file
//! left behind is the sweep's to try again.

use std::collections::HashSet;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use anyhow::Context;
use sqlx::{Connection, SqliteConnection};

use crate::config::{Config, PathsConfig};
use crate::db::engine::BEGIN_IMMEDIATE_SQL;
use crate::db::trash::{OrphanedFile, UnreferencedFiles};

/// The folder that holds everything on disk for `account_id`.
pub(crate) fn account_dir(paths: &PathsConfig, account_id: i64) -> PathBuf {
    paths.data_dir.join(account_id.to_string())
}

/// The MIME sidecar of the original `sha256` under `originals_dir`, or
/// `None` when the fingerprint is too short or too odd to name a shard
/// folder. The server only stores 64-hex fingerprints, so `None` means a
/// damaged row, which has no sidecar to remove.
pub(crate) fn sidecar_path(originals_dir: &Path, sha256: &str) -> Option<PathBuf> {
    let shard = sha256.get(..2)?;
    if !shard.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(originals_dir.join(shard).join(format!(".{sha256}.mime")))
}

/// `dir/relative`, or `None` for a stored path that is empty, absolute, or
/// climbs out of `dir`. The server wrote every `assets_path` itself, so this
/// never fires on its own data. It keeps a damaged row from naming a file
/// elsewhere on the machine.
pub(crate) fn join_under(dir: &Path, relative: &str) -> Option<PathBuf> {
    let rel = Path::new(relative);
    let safe = rel.components().all(|c| matches!(c, Component::Normal(_)));
    (safe && !relative.is_empty()).then(|| dir.join(rel))
}

/// Remove one file. A file already gone is not an error, because removing it
/// was the goal.
///
/// # Errors
///
/// Returns the error of a file that exists and cannot be removed.
pub(crate) fn remove_file(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// Remove `path` and log a failure with the account and the path. True when
/// the file is gone.
fn remove_logged(account_id: i64, path: &Path) -> bool {
    match remove_file(path) {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(
                account_id,
                path = %path.display(),
                %error,
                "an attachment file could not be removed"
            );
            false
        }
    }
}

/// Remove the files a delete reported as unreferenced: each original with
/// its MIME sidecar, and each Preview. While the account had a running
/// Import Run when the delete committed, the originals stay for
/// [`sweep_unreferenced`]. Runs on the blocking pool, and never fails: a
/// file that cannot be removed is logged and the others still go.
pub(crate) async fn remove_unreferenced(
    cfg: Arc<Config>,
    account_id: i64,
    unreferenced: UnreferencedFiles,
) {
    if unreferenced.files.is_empty() {
        return;
    }
    let removed = tokio::task::spawn_blocking(move || {
        for file in &unreferenced.files {
            for path in paths_of(&cfg.paths, account_id, file, unreferenced.import_running) {
                remove_logged(account_id, &path);
            }
        }
    })
    .await;
    if let Err(error) = removed {
        tracing::warn!(account_id, %error, "removing attachment files stopped");
    }
}

/// The paths `file` occupies on disk that may go now. An original is kept
/// while `import_running`. A stored path that would leave its folder is
/// logged and passed over.
fn paths_of(
    paths: &PathsConfig,
    account_id: i64,
    file: &OrphanedFile,
    import_running: bool,
) -> Vec<PathBuf> {
    let (dir, assets_path, sidecar) = match file {
        OrphanedFile::Original { .. } if import_running => return Vec::new(),
        OrphanedFile::Original {
            source,
            sha256,
            assets_path,
        } => {
            let dir = paths.assets_dir_for_account(account_id, source);
            let sidecar = sidecar_path(&dir, sha256);
            (dir, assets_path, sidecar)
        }
        OrphanedFile::Derived {
            source,
            assets_path,
        } => (
            paths.assets_converted_dir_for_account(account_id, source),
            assets_path,
            None,
        ),
    };
    let Some(path) = join_under(&dir, assets_path) else {
        tracing::warn!(
            account_id,
            assets_path,
            "a stored attachment path is not a plain relative path; its file is left alone"
        );
        return Vec::new();
    };
    std::iter::once(path).chain(sidecar).collect()
}

/// Remove the files of every attachment of `account_id` after its messages
/// were deleted: every Preview, and every original unless the account had a
/// running Import Run when the delete committed. A folder that cannot be
/// removed is logged and the others still go.
pub(crate) async fn remove_all_attachment_files(
    cfg: Arc<Config>,
    account_id: i64,
    import_running: bool,
) {
    let removed = tokio::task::spawn_blocking(move || {
        let paths = &cfg.paths;
        let mut kinds = vec![paths.assets_converted_dir.as_str()];
        if !import_running {
            kinds.push(paths.assets_dir.as_str());
        }
        for source in source_dirs(account_id, &account_dir(paths, account_id)) {
            for kind in &kinds {
                let dir = source.join(kind);
                if let Err(error) = remove_tree(&dir) {
                    tracing::warn!(
                        account_id,
                        path = %dir.display(),
                        %error,
                        "an attachment folder could not be removed"
                    );
                }
            }
        }
    })
    .await;
    if let Err(error) = removed {
        tracing::warn!(account_id, %error, "removing attachment files stopped");
    }
}

/// Remove everything on disk for `account_id`, once its account row is gone.
/// No account can take that id again, so nothing can need these files.
///
/// # Errors
///
/// Returns the error of a folder that exists and cannot be removed.
pub(crate) fn remove_account_dir(paths: &PathsConfig, account_id: i64) -> io::Result<()> {
    remove_tree(&account_dir(paths, account_id))
}

/// Remove the folder tree at `path`; a missing folder is not an error.
fn remove_tree(path: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// The source folders under `account_root`. A folder that cannot be read is
/// logged and yields none.
fn source_dirs(account_id: i64, account_root: &Path) -> Vec<PathBuf> {
    let entries = match std::fs::read_dir(account_root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            tracing::warn!(
                account_id,
                path = %account_root.display(),
                %error,
                "an account folder could not be read"
            );
            return Vec::new();
        }
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
        .map(|entry| entry.path())
        .collect()
}

/// Remove every original, sidecar and Preview of `account_id` that no
/// attachment row names, and return how many files went. Does nothing while
/// the account has a running Import Run, because that run may hold files it
/// has not named yet.
///
/// The check and the removal happen inside one write transaction, so no run
/// can start and no batch can name a file between them. The transaction
/// writes nothing.
///
/// A file is named when its fingerprint, the part of its name before the
/// first dot, is the `sha256` or `derived_sha256` of an attachment of the
/// account, or the name of a file an `assets_path` or `derived_assets_path`
/// points at. A name that is not a 64-hex fingerprint is not the server's,
/// and is left alone, as is `.incoming/`.
///
/// # Errors
///
/// Returns a database error, or an error when the file walk stops. A file
/// that cannot be removed is logged and the walk goes on.
pub(crate) async fn sweep_unreferenced(
    conn: &mut SqliteConnection,
    paths: &PathsConfig,
    account_id: i64,
) -> anyhow::Result<u64> {
    let mut tx = conn.begin_with(BEGIN_IMMEDIATE_SQL).await?;
    if crate::db::imports::has_running_import(&mut tx, account_id).await? {
        return Ok(0);
    }
    let named = named_fingerprints(&mut tx, account_id).await?;
    let paths = paths.clone();
    let removed = tokio::task::spawn_blocking(move || {
        let mut removed = 0u64;
        for source in source_dirs(account_id, &account_dir(&paths, account_id)) {
            for kind in [&paths.assets_dir, &paths.assets_converted_dir] {
                removed += sweep_store_dir(account_id, &source.join(kind), &named);
            }
        }
        removed
    })
    .await
    .context("sweep of unreferenced attachment files")?;
    tx.commit().await?;
    Ok(removed)
}

/// [`sweep_unreferenced`] once an Import Run of `account_id` has ended, so
/// the files it was told about or uploaded and never named go. A failure is
/// logged: the run's end is what the caller answers for.
pub(crate) async fn sweep_after_run(
    conn: &mut SqliteConnection,
    paths: &PathsConfig,
    account_id: i64,
) {
    if let Err(error) = sweep_unreferenced(conn, paths, account_id).await {
        tracing::warn!(
            account_id,
            error = format!("{error:#}"),
            "unreferenced attachment files could not be swept after an Import Run"
        );
    }
}

/// What one attachment row says about the files it names: `sha256`,
/// `assets_path`, `derived_sha256`, `derived_assets_path`.
type NamingRow = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Every fingerprint an attachment of `account_id` names, promoted or in
/// staging, lowercased.
async fn named_fingerprints(
    conn: &mut SqliteConnection,
    account_id: i64,
) -> Result<HashSet<String>, sqlx::Error> {
    let rows: Vec<NamingRow> = sqlx::query_as(
        "SELECT a.sha256, a.assets_path, a.derived_sha256, a.derived_assets_path
             FROM attachments a
             JOIN messages m ON m.id = a.message_id
             WHERE m.account_id = $1
             UNION ALL
             SELECT sa.sha256, sa.assets_path, sa.derived_sha256, sa.derived_assets_path
             FROM staging_attachments sa
             JOIN staging_messages sm ON sm.id = sa.message_id
             WHERE sm.account_id = $1",
    )
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await?;
    let mut named = HashSet::new();
    for (sha, path, derived_sha, derived_path) in rows {
        named.extend(sha.into_iter().map(|s| s.to_ascii_lowercase()));
        named.extend(derived_sha.into_iter().map(|s| s.to_ascii_lowercase()));
        for path in [path, derived_path].into_iter().flatten() {
            if let Some(fingerprint) = Path::new(&path)
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(fingerprint_of)
            {
                named.insert(fingerprint);
            }
        }
    }
    Ok(named)
}

/// The 64-hex fingerprint a stored file's name starts with, lowercased:
/// `<sha256><ext>` for a file, `.<sha256>.mime` for a sidecar. `None` for
/// any other name, such as a temporary file.
fn fingerprint_of(name: &str) -> Option<String> {
    let stem = match name.strip_prefix('.') {
        Some(rest) => rest.strip_suffix(".mime")?,
        None => name.split('.').next()?,
    };
    (stem.len() == 64 && stem.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| stem.to_ascii_lowercase())
}

/// Remove each file in the shard folders of `store_dir` whose fingerprint
/// `named` lacks, and return how many went. Folders starting with a dot,
/// `.incoming/` among them, are not shards and are left alone.
fn sweep_store_dir(account_id: i64, store_dir: &Path, named: &HashSet<String>) -> u64 {
    let Ok(shards) = std::fs::read_dir(store_dir) else {
        return 0;
    };
    let mut removed = 0u64;
    for shard in shards.filter_map(Result::ok) {
        let is_shard = shard.file_type().is_ok_and(|t| t.is_dir())
            && !shard.file_name().to_string_lossy().starts_with('.');
        if !is_shard {
            continue;
        }
        let Ok(files) = std::fs::read_dir(shard.path()) else {
            continue;
        };
        for file in files.filter_map(Result::ok) {
            if !file.file_type().is_ok_and(|t| t.is_file()) {
                continue;
            }
            let name = file.file_name();
            let Some(fingerprint) = name.to_str().and_then(fingerprint_of) else {
                continue;
            };
            if !named.contains(&fingerprint) && remove_logged(account_id, &file.path()) {
                removed += 1;
            }
        }
    }
    removed
}

/// Age after which an upload temp under `.incoming/` counts as abandoned: a
/// `{sha}-*.part` file or a multipart session folder `{sha}/{upload_id}/`.
/// A live upload keeps writing its temp while a sweep runs, so only a temp
/// left untouched this long is removed.
pub(crate) const STALE_UPLOAD_SECS: u64 = 24 * 60 * 60;

/// Remove abandoned `{sha}-*.part` temps and multipart session folders under
/// `originals_dir/.incoming/`, and return how many it removed (or would
/// remove, in a dry run).
///
/// The server writes and removes these files while the sweep runs, so a
/// file that is gone by the time the sweep reaches it is passed over, and
/// any other failure is logged and the sweep goes on.
pub(crate) fn sweep_incoming(originals_dir: &Path, dry_run: bool) -> u64 {
    let incoming = originals_dir.join(".incoming");
    let entries = match std::fs::read_dir(&incoming) {
        Ok(entries) => entries,
        Err(err) => {
            log_sweep_error("read", &incoming, &err);
            return 0;
        }
    };
    let mut parts = Vec::new();
    let mut sha_dirs = Vec::new();
    for entry in entries {
        let path = match entry {
            Ok(entry) => entry.path(),
            Err(err) => {
                log_sweep_error("read", &incoming, &err);
                continue;
            }
        };
        if path.is_file() && has_part_extension(&path) {
            parts.push(path);
        } else if path.is_dir() {
            sha_dirs.push(path);
        }
    }
    let now = SystemTime::now();
    let mut removed = remove_stale_parts(&parts, now, dry_run);
    for sha_dir in &sha_dirs {
        removed += remove_stale_sessions(sha_dir, now, dry_run);
    }
    removed
}

/// True for a `.part` file left by an interrupted upload.
pub(crate) fn has_part_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("part"))
}

/// Remove each listed `.part` file older than [`STALE_UPLOAD_SECS`] at
/// `now`, and return how many it removed (or would remove, in a dry run).
fn remove_stale_parts(parts: &[PathBuf], now: SystemTime, dry_run: bool) -> u64 {
    let mut removed = 0u64;
    for part in parts {
        match modified_before_limit(part, now) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(err) => {
                log_sweep_error("read", part, &err);
                continue;
            }
        }
        if dry_run {
            println!("[dry-run] would remove {}", part.display());
            removed += 1;
            continue;
        }
        match std::fs::remove_file(part) {
            Ok(()) => removed += 1,
            Err(err) => log_sweep_error("remove leftover", part, &err),
        }
    }
    removed
}

/// Remove each multipart session folder under `sha_dir`
/// (`.incoming/{sha256}/{upload_id}/`) that is stale at `now`, then
/// `sha_dir` itself once it is empty. Returns how many sessions it removed
/// (or would remove, in a dry run).
fn remove_stale_sessions(sha_dir: &Path, now: SystemTime, dry_run: bool) -> u64 {
    let entries = match std::fs::read_dir(sha_dir) {
        Ok(entries) => entries,
        Err(err) => {
            log_sweep_error("read", sha_dir, &err);
            return 0;
        }
    };
    let mut removed = 0u64;
    for entry in entries {
        let session = match entry {
            Ok(entry) => entry.path(),
            Err(err) => {
                log_sweep_error("read", sha_dir, &err);
                continue;
            }
        };
        if !session.is_dir() {
            continue;
        }
        match upload_session_is_stale(&session, now) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(err) => {
                log_sweep_error("read", &session, &err);
                continue;
            }
        }
        if dry_run {
            println!(
                "[dry-run] would remove stale upload session {}",
                session.display()
            );
            removed += 1;
            continue;
        }
        match std::fs::remove_dir_all(&session) {
            Ok(()) => removed += 1,
            Err(err) => log_sweep_error("remove stale upload session", &session, &err),
        }
    }
    let is_empty = std::fs::read_dir(sha_dir).is_ok_and(|mut rest| rest.next().is_none());
    if is_empty {
        if dry_run {
            println!("[dry-run] would remove empty {}", sha_dir.display());
        } else {
            // A new upload for this fingerprint may have made a session in
            // the meantime, and then the folder stays.
            let _ = std::fs::remove_dir(sha_dir);
        }
    }
    removed
}

/// True when a multipart upload session's manifest (or, failing that, its
/// folder) is older than the abandoned-upload limit.
fn upload_session_is_stale(session: &Path, now: SystemTime) -> io::Result<bool> {
    let manifest = session.join("manifest.json");
    if manifest.is_file() {
        modified_before_limit(&manifest, now)
    } else {
        modified_before_limit(session, now)
    }
}

/// True when `path` was last modified [`STALE_UPLOAD_SECS`] or more before `now`.
fn modified_before_limit(path: &Path, now: SystemTime) -> io::Result<bool> {
    let modified = std::fs::metadata(path)?
        .modified()
        .unwrap_or(std::time::UNIX_EPOCH);
    let age = now.duration_since(modified).unwrap_or_default();
    Ok(age.as_secs() >= STALE_UPLOAD_SECS)
}

/// Log a failed step of the `.incoming/` sweep. A path that no longer
/// exists is not logged, because the server removes its own temps when an
/// upload finishes, and that is the outcome the sweep wanted.
fn log_sweep_error(action: &str, path: &Path, err: &io::Error) {
    if err.kind() != io::ErrorKind::NotFound {
        tracing::warn!(path = %path.display(), error = %err, "could not {action} an upload temp");
    }
}

#[cfg(test)]
pub(crate) mod tests;
