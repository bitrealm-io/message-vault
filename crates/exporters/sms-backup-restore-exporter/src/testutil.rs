//! Test helpers for crates that write this crate's backup into a folder.

use std::fs;
use std::path::Path;

/// Leave in `dir` what a run that stopped before it finished leaves beside
/// the backup: every file the archive names that is not there yet.
///
/// # Panics
///
/// Panics when a file cannot be written.
pub fn leave_partial_backup(dir: &Path) {
    for name in sbr::backup_file_names() {
        let path = dir.join(name);
        if !path.exists() {
            fs::write(path, "partial").unwrap();
        }
    }
}

/// Assert that `dir` holds none of the files the archive names.
///
/// # Panics
///
/// Panics when one of them is left behind.
pub fn assert_no_backup_left(dir: &Path) {
    for name in sbr::backup_file_names() {
        assert!(!dir.join(&name).exists(), "{name} is left behind");
    }
}
