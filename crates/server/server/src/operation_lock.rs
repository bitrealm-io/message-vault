//! Cross-process exclusion between the HTTP server and database replacement.
//!
//! Serve and reset-demo must not run at the same time against the same
//! database. This lock file sits next to the database and is taken exclusively
//! for the life of either operation.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use fs2::FileExt;

/// Holds an exclusive lock on `{database}.operation.lock` until dropped.
#[derive(Debug)]
pub(crate) struct OperationLock {
    _file: File,
}

/// Take the lock for the HTTP server. Fails if reset-demo or another server
/// already holds it.
///
/// # Errors
///
/// Returns an error when the lock file cannot be created or is already held.
pub(crate) fn acquire_for_serve(db: &Path) -> Result<OperationLock> {
    acquire(db).with_context(|| {
        format!(
            "cannot start serve for {} while reset-demo or another server is active",
            db.display()
        )
    })
}

/// Take the lock for reset-demo. Fails if the HTTP server already holds it.
///
/// # Errors
///
/// Returns an error when the lock file cannot be created or is already held.
pub(crate) fn acquire_for_reset(db: &Path) -> Result<OperationLock> {
    acquire(db).with_context(|| {
        format!(
            "cannot reset demo while serve is active for {}; stop the server and run reset-demo offline",
            db.display()
        )
    })
}

/// Take the exclusive lock file next to the database, creating its folder if needed.
fn acquire(db: &Path) -> Result<OperationLock> {
    let lock_path = lock_path(db);
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create lock directory {}", parent.display()))?;
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("open operation lock {}", lock_path.display()))?;
    file.try_lock_exclusive()
        .with_context(|| format!("acquire operation lock {}", lock_path.display()))?;
    Ok(OperationLock { _file: file })
}

/// `<db>.operation.lock` next to the database file.
fn lock_path(db: &Path) -> PathBuf {
    let mut name: OsString = db.as_os_str().to_owned();
    name.push(".operation.lock");
    PathBuf::from(name)
}

/// `server.ready` next to `messagecrate.db`. sqlite-web waits for this file.
pub(crate) fn ready_path(db: &Path) -> PathBuf {
    db.with_file_name("server.ready")
}

/// Remove `server.ready` so waiters know the database is being rebuilt.
pub(crate) fn clear_ready(db: &Path) -> Result<()> {
    let path = ready_path(db);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("remove {}", path.display())),
    }
}

/// Create `server.ready` after the database has a usable schema.
pub(crate) fn mark_ready(db: &Path) -> Result<()> {
    let path = ready_path(db);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create ready directory {}", parent.display()))?;
    }
    fs::write(&path, []).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{clear_ready, mark_ready, ready_path};

    #[test]
    fn ready_sentinel_is_cleared_and_written_beside_the_database() {
        let temp = tempfile::tempdir().expect("create test directory");
        let db = temp.path().join("messagecrate.db");
        let ready = ready_path(&db);
        assert_eq!(ready, temp.path().join("server.ready"));

        clear_ready(&db).expect("clear missing ready file");
        mark_ready(&db).expect("write ready file");
        assert!(ready.is_file());
        clear_ready(&db).expect("remove ready file");
        assert!(!ready.exists());
    }
}
