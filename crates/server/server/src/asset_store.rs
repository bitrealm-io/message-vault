//! The Assets and Previews on disk, and the code that removes them.
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
//! An Asset is unused when no attachment row, promoted or in staging, names
//! it. That test alone is not enough while the account has a running Import
//! Run: `HEAD /v1/assets/{sha256}` may have told the run a file exists, or
//! the run may have uploaded it, and the batch that names it has not arrived
//! yet. So an original is removed only while a connection holds the
//! database write lock and no run is running. Starting a run writes its row,
//! so no run can start between that check and the last removal. While a run
//! is running the original stays, and [`sweep_unreferenced`] removes it when
//! the run ends. A Preview is never kept for a run, because an import never
//! names one: the server makes Previews from originals after the fact.
//!
//! A removal that fails is logged and the rest go on. The database rows are
//! the record, so a request answers for what the database did, and a file
//! left behind is the sweep's to try again.
//!
//! Each remover has its own case:
//!
//! - [`remove_unreferenced`]: the Assets one delete left unnamed.
//! - [`remove_all_attachment_files`]: every Asset of an account whose
//!   messages were all deleted.
//! - [`remove_account_dir`]: everything of an account whose row is gone. No
//!   run can belong to a missing account, so it needs no run check.
//! - [`sweep_unreferenced`]: every unnamed Asset of an account, when a run
//!   ends.
//! - [`sweep_incoming`]: abandoned upload temps, by age.
//!
//! Upload and `process-assets` still remove their own temporary and
//! replaced files. Those are never an Asset a row names.

use std::collections::HashSet;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use anyhow::Context;
use sqlx::{SqliteConnection, SqlitePool};

use crate::assets_api::Sha256;
use crate::config::{Config, PathsConfig};
use crate::db::imports::has_running_import;
use crate::db::trash::OrphanedFile;
use crate::db::write_tx::begin_write;

/// The folder that holds everything on disk for `account_id`.
pub(crate) fn account_dir(paths: &PathsConfig, account_id: i64) -> PathBuf {
    paths.data_dir.join(account_id.to_string())
}

/// The MIME sidecar of the original `sha256` under `originals_dir`.
pub(crate) fn sidecar_path(originals_dir: &Path, sha256: &Sha256) -> PathBuf {
    originals_dir
        .join(sha256.shard())
        .join(format!(".{sha256}.mime"))
}

/// The MIME sidecar of an original whose fingerprint was read from an
/// attachment row, or `None` when the stored value is not a fingerprint.
/// The server only stores 64-hex fingerprints, so `None` means a damaged
/// row, which has no sidecar to remove.
pub(crate) fn stored_sidecar_path(originals_dir: &Path, sha256: &str) -> Option<PathBuf> {
    Sha256::parse(sha256)
        .ok()
        .map(|sha| sidecar_path(originals_dir, &sha))
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
    gone_is_ok(std::fs::remove_file(path))
}

/// Remove the folder tree at `path`; a missing folder is not an error.
fn remove_tree(path: &Path) -> io::Result<()> {
    gone_is_ok(std::fs::remove_dir_all(path))
}

/// `result`, with a path that was not there counted as removed.
fn gone_is_ok(result: io::Result<()>) -> io::Result<()> {
    match result {
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
                "a file could not be removed"
            );
            false
        }
    }
}

/// Remove the files a delete reported as unreferenced: each Preview, and
/// each original with its MIME sidecar unless the account has a running
/// Import Run (see the module notes). Call it after the delete committed.
/// Never fails: a file that cannot be removed, or a lock that cannot be
/// taken, is logged and the originals stay for [`sweep_unreferenced`].
pub(crate) async fn remove_unreferenced(
    pool: &SqlitePool,
    cfg: Arc<Config>,
    account_id: i64,
    files: Vec<OrphanedFile>,
) {
    let (originals, previews): (Vec<_>, Vec<_>) = files
        .into_iter()
        .partition(|file| matches!(file, OrphanedFile::Original { .. }));
    if !previews.is_empty() {
        let paths = paths_of_all(&cfg.paths, account_id, &previews);
        run_blocking_logged(account_id, move || remove_each(account_id, &paths)).await;
    }
    if !originals.is_empty() {
        let paths = paths_of_all(&cfg.paths, account_id, &originals);
        unless_import_running(pool, account_id, move || remove_each(account_id, &paths)).await;
    }
}

/// Every path [`paths_of`] gives for `files`.
fn paths_of_all(paths: &PathsConfig, account_id: i64, files: &[OrphanedFile]) -> Vec<PathBuf> {
    files
        .iter()
        .flat_map(|file| paths_of(paths, account_id, file))
        .collect()
}

/// Remove each of `paths`, logging any that cannot be removed.
fn remove_each(account_id: i64, paths: &[PathBuf]) {
    for path in paths {
        remove_logged(account_id, path);
    }
}

/// The paths `file` occupies on disk: the file and, for an original, its
/// MIME sidecar. A stored path that would leave its folder is logged and
/// passed over.
fn paths_of(paths: &PathsConfig, account_id: i64, file: &OrphanedFile) -> Vec<PathBuf> {
    let (dir, assets_path, sidecar) = match file {
        OrphanedFile::Original {
            source,
            sha256,
            assets_path,
        } => {
            let dir = paths.assets_dir_for_account(account_id, source);
            let sidecar = stored_sidecar_path(&dir, sha256);
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

/// Remove every Asset and Preview of `account_id` after its messages were
/// deleted: every Preview folder, and every originals folder unless the
/// account has a running Import Run (see the module notes). A folder that
/// cannot be removed is logged and the others still go.
pub(crate) async fn remove_all_attachment_files(
    pool: &SqlitePool,
    cfg: Arc<Config>,
    account_id: i64,
) {
    let remove_kind_dirs = |kind: String| {
        let cfg = Arc::clone(&cfg);
        move || {
            let paths = &cfg.paths;
            for source in source_dirs(account_id, &account_dir(paths, account_id)) {
                let dir = source.join(&kind);
                if let Err(error) = remove_tree(&dir) {
                    tracing::warn!(
                        account_id,
                        path = %dir.display(),
                        %error,
                        "a folder could not be removed"
                    );
                }
            }
        }
    };
    run_blocking_logged(
        account_id,
        remove_kind_dirs(cfg.paths.assets_converted_dir.clone()),
    )
    .await;
    unless_import_running(
        pool,
        account_id,
        remove_kind_dirs(cfg.paths.assets_dir.clone()),
    )
    .await;
}

/// Run `remove` on the blocking pool while a connection from `pool` holds
/// the database write lock, unless `account_id` has a running Import Run.
/// Starting a run writes its row, so no run can start until `remove` has
/// finished. Every other writer waits for `remove` too, which is the price
/// of that guarantee.
///
/// The lock, the check and `remove` run in a task of their own that owns
/// its connection. A request dropped part way, such as a closed tab, then
/// cannot let go of the lock while files are still being removed. The
/// caller must hold no connection from `pool` while it waits, or a full
/// pool would leave the task waiting for one.
///
/// A run that is running, a lock that cannot be taken, or a stopped task
/// leaves the originals for [`sweep_unreferenced`], and the last two are
/// logged.
async fn unless_import_running<F>(pool: &SqlitePool, account_id: i64, remove: F)
where
    F: FnOnce() + Send + 'static,
{
    let pool = pool.clone();
    let task = tokio::spawn(async move {
        let mut conn = pool.acquire().await?;
        let mut tx = begin_write(&mut conn).await?;
        if has_running_import(&mut tx, account_id).await? {
            return Ok(());
        }
        tokio::task::spawn_blocking(remove)
            .await
            .context("removing files stopped")?;
        tx.commit().await?;
        anyhow::Ok(())
    });
    let result = task
        .await
        .context("removing files stopped")
        .and_then(|done| done);
    if let Err(error) = result {
        tracing::warn!(
            account_id,
            error = format!("{error:#}"),
            "files were not removed; the sweep at the next Import Run's end will try again"
        );
    }
}

/// Run `work` on the blocking pool, and log it if the task stops.
async fn run_blocking_logged<F>(account_id: i64, work: F)
where
    F: FnOnce() + Send + 'static,
{
    if let Err(error) = tokio::task::spawn_blocking(work).await {
        tracing::warn!(account_id, %error, "removing files stopped");
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
/// writes nothing, but it holds the database write lock for the whole walk
/// of the account's store, so every writer on the server waits for the
/// walk. The walk reads folders and removes files and nothing else, so it
/// is short next to an import. Like [`unless_import_running`], it runs in a
/// task that owns its connection, and the caller must hold no connection
/// from `pool` while it waits.
///
/// A Preview written in the last [`PREVIEW_GRACE_SECS`] is left alone:
/// `process-assets` writes a Preview before the row that names it, and a
/// sweep between the two would remove it.
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
    pool: &SqlitePool,
    paths: &PathsConfig,
    account_id: i64,
) -> anyhow::Result<u64> {
    let pool = pool.clone();
    let paths = paths.clone();
    tokio::spawn(async move {
        let mut conn = pool.acquire().await?;
        sweep_holding_lock(&mut conn, paths, account_id).await
    })
    .await
    .context("sweep of unreferenced files stopped")?
}

/// The body of [`sweep_unreferenced`], on a connection of its own.
async fn sweep_holding_lock(
    conn: &mut SqliteConnection,
    paths: PathsConfig,
    account_id: i64,
) -> anyhow::Result<u64> {
    let mut tx = begin_write(conn).await?;
    if has_running_import(&mut tx, account_id).await? {
        return Ok(0);
    }
    let named = named_fingerprints(&mut tx, account_id).await?;
    let removed = tokio::task::spawn_blocking(move || {
        let mut removed = 0u64;
        for source in source_dirs(account_id, &account_dir(&paths, account_id)) {
            removed += sweep_store_dir(account_id, &source.join(&paths.assets_dir), &named, 0);
            removed += sweep_store_dir(
                account_id,
                &source.join(&paths.assets_converted_dir),
                &named,
                PREVIEW_GRACE_SECS,
            );
        }
        removed
    })
    .await
    .context("sweep of unreferenced files")?;
    tx.commit().await?;
    Ok(removed)
}

/// [`sweep_unreferenced`] once an Import Run of `account_id` has ended, so
/// the files it was told about or uploaded and never named go. A failure is
/// logged: the run's end is what the caller answers for.
pub(crate) async fn sweep_after_run(pool: &SqlitePool, paths: &PathsConfig, account_id: i64) {
    if let Err(error) = sweep_unreferenced(pool, paths, account_id).await {
        tracing::warn!(
            account_id,
            error = format!("{error:#}"),
            "unreferenced files could not be swept after an Import Run"
        );
    }
}

/// Age under which the sweep leaves a Preview alone, because
/// `process-assets` may not have written the row that names it yet.
pub(crate) const PREVIEW_GRACE_SECS: u64 = 60 * 60;

/// What one attachment row says about the files it names.
#[derive(sqlx::FromRow)]
struct NamingRow {
    sha256: Option<String>,
    assets_path: Option<String>,
    derived_sha256: Option<String>,
    derived_assets_path: Option<String>,
}

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
    for row in rows {
        for sha in [row.sha256, row.derived_sha256].into_iter().flatten() {
            named.insert(sha.to_ascii_lowercase());
        }
        for path in [row.assets_path, row.derived_assets_path]
            .into_iter()
            .flatten()
        {
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
/// `named` lacks and that is at least `grace_secs` old, and return how many
/// went. Folders starting with a dot, `.incoming/` among them, are not
/// shards and are left alone.
fn sweep_store_dir(
    account_id: i64,
    store_dir: &Path,
    named: &HashSet<String>,
    grace_secs: u64,
) -> u64 {
    let now = SystemTime::now();
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
            if named.contains(&fingerprint) {
                continue;
            }
            let path = file.path();
            if grace_secs > 0 && !modified_at_least(&path, now, grace_secs).unwrap_or(false) {
                continue;
            }
            if remove_logged(account_id, &path) {
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
        match modified_at_least(part, now, STALE_UPLOAD_SECS) {
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
        match remove_file(part) {
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
        match remove_tree(&session) {
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
        modified_at_least(&manifest, now, STALE_UPLOAD_SECS)
    } else {
        modified_at_least(session, now, STALE_UPLOAD_SECS)
    }
}

/// True when `path` was last modified `secs` or more before `now`.
fn modified_at_least(path: &Path, now: SystemTime, secs: u64) -> io::Result<bool> {
    let modified = std::fs::metadata(path)?
        .modified()
        .unwrap_or(std::time::UNIX_EPOCH);
    let age = now.duration_since(modified).unwrap_or_default();
    Ok(age.as_secs() >= secs)
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
