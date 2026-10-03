//! Path helpers the Settings and Import screens use.

use serde::Serialize;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

use crate::staging_folders::{StagingFolders, lock};

/// The folder under the app's cache folder that `imessage-reader` decrypts
/// an encrypted backup into while the Import form reads its identities.
const IMESSAGE_READER_SCRATCH: &str = "imessage-reader";

/// The logged-in user's home folder, plus which OS this process is running on.
#[derive(Debug, Clone, Serialize)]
pub struct HomeDirInfo {
    /// Home folder as an absolute path the UI can join onto.
    pub path: String,
    /// Operating system name as Rust reports it, for example `linux`, `macos`,
    /// or `windows`.
    pub os: String,
}

/// Whether a path exists on disk and what kind of entry it is.
///
/// `size_bytes` and `modified_unix_ms` are file-oriented: they come from a
/// single `std::fs::metadata` call on the path itself. For a directory
/// source -- an iOS backup folder, a WhatsApp folder -- that is the
/// directory entry, not its contents: the size is the entry's own (4096
/// bytes on most filesystems) and the mtime moves only when a child is
/// added or removed, never when one is written to. A fingerprint built
/// from these two values therefore cannot tell that a directory backup
/// grew between attempts. Anything reading them for that purpose needs a
/// directory strategy of its own -- child count plus newest descendant
/// mtime, say -- chosen alongside the code that consumes it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathStat {
    /// `true` when this path exists on disk.
    pub exists: bool,
    /// `true` when the path is a regular file.
    pub is_file: bool,
    /// `true` when the path is a directory.
    pub is_directory: bool,
    /// Size in bytes; `0` when the path does not exist. For a directory
    /// this is the directory entry's own size, not the total of its
    /// contents (see the type's docs).
    pub size_bytes: u64,
    /// Last modification time in milliseconds since the Unix epoch, or
    /// `None` when the platform does not report one. For a directory this
    /// does not move when a file inside it changes (see the type's docs).
    pub modified_unix_ms: Option<i64>,
}

/// Stat a path without canonicalizing it (the path may not exist yet). A
/// path that cannot be read is reported as absent rather than as an error.
pub(crate) fn path_stat_inner(path: &str) -> PathStat {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return PathStat {
            exists: false,
            is_file: false,
            is_directory: false,
            size_bytes: 0,
            modified_unix_ms: None,
        };
    }
    let path = Path::new(trimmed);
    let meta = std::fs::metadata(path).ok();
    let modified_unix_ms = meta
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_millis()).ok());
    PathStat {
        exists: path.exists(),
        is_file: path.is_file(),
        is_directory: path.is_dir(),
        size_bytes: meta.as_ref().map_or(0, std::fs::Metadata::len),
        modified_unix_ms,
    }
}

/// Return whether a path exists and whether it is a file or directory.
#[tauri::command]
pub fn path_stat(path: String) -> PathStat {
    path_stat_inner(&path)
}

/// Read `Manifest.plist` and return whether an iOS backup folder is
/// encrypted. `None` when the path is blank or is not an iOS backup.
#[tauri::command]
pub fn ios_backup_encrypted(path: String) -> Option<bool> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }
    imessage_ir_exporter::ios_backup_encrypted_flag(Path::new(trimmed))
}

/// Addresses an iMessage backup's device sent from, for the Import
/// identity check.
///
/// Runs on a blocking-pool thread: for an encrypted backup, answering this
/// decrypts `chat.db` into a folder under the app's cache folder, which the
/// next request cleans if this one is killed.
#[tauri::command]
pub async fn imessage_backup_identities(
    app: AppHandle,
    path: String,
    ios: bool,
    backup_password: Option<String>,
) -> Result<Vec<String>, String> {
    let scratch_root = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("Could not find the app's cache folder: {e}"))?
        .join(IMESSAGE_READER_SCRATCH);
    tauri::async_runtime::spawn_blocking(move || {
        let password = backup_password.as_deref().and_then(message_ir::trimmed);
        imessage_ir_exporter::backup_identities(
            Path::new(path.trim()),
            ios,
            password,
            &scratch_root,
        )
        .map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Ask this process for the current user's home directory.
///
/// The WebView cannot see the real home folder on its own. Settings uses the
/// result as a starting point for file paths.
///
/// # Errors
///
/// Returns an error if the operating system does not report a home directory.
#[tauri::command]
pub fn home_dir() -> Result<HomeDirInfo, String> {
    let path = dirs::home_dir()
        .ok_or_else(|| "Could not determine the user home directory".to_string())?;
    Ok(HomeDirInfo {
        path: path.display().to_string(),
        os: std::env::consts::OS.to_string(),
    })
}

/// Open a file or folder with the operating system's default handler.
///
/// Only a staging folder this app made, or a path inside one such as its
/// `message-crate-push.log`, is opened ([`StagingFolders::openable`]).
///
/// # Errors
///
/// Returns an error when the path is empty or relative, is in no staging
/// folder this app made, is missing on disk, or the OS cannot open it.
#[tauri::command]
pub fn open_path(
    folders: tauri::State<'_, Mutex<StagingFolders>>,
    path: String,
) -> Result<(), String> {
    let resolved = lock(&folders).openable(&path)?;
    missing_path_error(&resolved)?;
    open::that_detached(&resolved).map_err(|error| format!("Could not open path: {error}"))
}

/// Write `contents` to the file at `path`, replacing a file already there.
///
/// The window calls this with the path the person chose in the Save dialog,
/// for a file the server answered as text, such as the address book. A
/// desktop window has no downloads folder of its own, so the app writes the
/// file where the person asked.
///
/// # Errors
///
/// Returns an error when the path is empty or not absolute, or the file
/// cannot be written.
#[tauri::command]
pub fn save_text_file(path: String, contents: String) -> Result<(), String> {
    save_text_file_inner(&path, &contents)
}

/// The work of [`save_text_file`], apart from the command wrapper.
pub(crate) fn save_text_file_inner(path: &str, contents: &str) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("The path to save to is empty".to_string());
    }
    let path = Path::new(trimmed);
    if !path.is_absolute() {
        return Err(format!("The path to save to must be absolute: {trimmed}"));
    }
    std::fs::write(path, contents).map_err(|error| format!("Could not save {trimmed}: {error}"))
}

/// Error when a resolved staging path is not on disk yet.
///
/// The OS opener often reports success for a missing path (for example
/// `xdg-open` exiting 0), so the UI must fail here to show an inline alert.
pub(crate) fn missing_path_error(resolved: &Path) -> Result<(), String> {
    if resolved.exists() {
        return Ok(());
    }
    let name = resolved
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("path");
    Err(format!("Nothing exists at {name} yet"))
}

/// Collapse `.` and `..` without requiring the path to exist on disk.
fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            Component::Normal(part) => out.push(part),
            Component::RootDir | Component::Prefix(_) => out.push(component.as_os_str()),
        }
    }
    out
}

/// Trim `raw` and require it to be a non-empty absolute path.
fn resolve_absolute(
    raw: &str,
    empty_message: &str,
    relative_message: &str,
) -> Result<PathBuf, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(empty_message.to_string());
    }
    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err(relative_message.to_string());
    }
    Ok(path)
}

/// Resolve an absolute path to the form the filesystem gives it, whether or
/// not it exists yet.
///
/// The path is normalized lexically, then its nearest existing ancestor is
/// canonicalized and the missing part appended. A path that exists comes back
/// canonical; one that does not comes back in the same form as the folders
/// above it, so a root and a path under it compare alike even when the root
/// is reached through a symbolic link or, on Windows, canonicalizes to a
/// `\\?\` path.
fn resolve_on_disk(path: &Path) -> std::io::Result<PathBuf> {
    let normalized = normalize_lexically(path);
    let mut existing = normalized.as_path();
    let mut missing = Vec::new();
    while !existing.exists() {
        let Some(parent) = existing.parent() else {
            return Ok(normalized);
        };
        if let Some(name) = existing.file_name() {
            missing.push(name.to_os_string());
        }
        existing = parent;
    }
    let mut resolved = existing.canonicalize()?;
    resolved.extend(missing.iter().rev());
    Ok(resolved)
}

/// Resolve the Staging Directory: trimmed, absolute, resolved through
/// [`resolve_on_disk`] (so a root not made yet still resolves), and never the
/// filesystem root.
///
/// Shared by [`resolve_openable_path`] and `StagingFolders`, which checks
/// the Staging Directory from Settings with it, so a root and a path under
/// it are resolved the identical way.
///
/// # Errors
///
/// Returns an error when the root is empty, relative, cannot be
/// canonicalized, or is the filesystem root.
pub(crate) fn resolve_staging_root(staging_root: &str) -> Result<PathBuf, String> {
    let root = resolve_absolute(
        staging_root,
        "Staging directory is empty",
        "Staging directory must be absolute",
    )?;
    let root = resolve_on_disk(&root)
        .map_err(|error| format!("Could not resolve staging root: {error}"))?;
    reject_filesystem_root(&root)?;
    Ok(root)
}

/// Resolve `raw` to an absolute path that must stay under `staging_root`.
///
/// When the path already exists it is canonicalized, so a symbolic link cannot
/// escape the staging tree. When it does not exist yet (for example a staging
/// folder that extract is about to create), it is resolved through
/// [`resolve_on_disk`], the same way as the root, so the two compare in one
/// form.
pub(crate) fn resolve_openable_path(raw: &str, staging_root: &str) -> Result<PathBuf, String> {
    let candidate = resolve_absolute(raw, "Path is empty", "Path must be absolute")?;
    let root = resolve_staging_root(staging_root)?;

    if candidate.exists() {
        let canonical = candidate
            .canonicalize()
            .map_err(|error| format!("Could not resolve path: {error}"))?;
        if !canonical.starts_with(&root) {
            return Err("Path is outside the staging folder".to_string());
        }
        return Ok(canonical);
    }

    let resolved =
        resolve_on_disk(&candidate).map_err(|error| format!("Could not resolve path: {error}"))?;
    if !resolved.starts_with(&root) {
        return Err("Path is outside the staging folder".to_string());
    }
    Ok(resolved)
}

/// `/` (and a Windows drive root) would make `starts_with` true for every absolute path.
fn reject_filesystem_root(root: &Path) -> Result<(), String> {
    if root.parent().is_none() {
        return Err("Staging directory cannot be the filesystem root".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[cfg(unix)]
    #[test]
    fn a_missing_folder_under_a_symlinked_root_is_inside_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        fs::create_dir(&real).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let candidate = link.join("staging-not-made-yet");

        let resolved = resolve_openable_path(candidate.to_str().unwrap(), link.to_str().unwrap());

        assert!(resolved.is_ok(), "{resolved:?}");
    }

    /// A Staging Directory not made yet, under a folder reached through a
    /// symbolic link, resolves in the same form as a folder inside it.
    #[cfg(unix)]
    #[test]
    fn a_missing_folder_under_a_missing_root_behind_a_symlink_is_inside_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        fs::create_dir(&real).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let root = link.join("message-crate");
        let candidate = root.join("staging-not-made-yet");

        let resolved =
            resolve_openable_path(candidate.to_str().unwrap(), root.to_str().unwrap()).unwrap();

        assert_eq!(
            resolved,
            real.canonicalize()
                .unwrap()
                .join("message-crate")
                .join("staging-not-made-yet")
        );
    }

    /// A missing path that leaves the root through `..` is still refused
    /// when the root is reached through a symbolic link.
    #[cfg(unix)]
    #[test]
    fn a_missing_path_outside_a_symlinked_root_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        fs::create_dir(&real).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let candidate = link.join("..").join("elsewhere").join("not-made-yet");

        let err =
            resolve_openable_path(candidate.to_str().unwrap(), link.to_str().unwrap()).unwrap_err();

        assert_eq!(err, "Path is outside the staging folder");
    }

    #[test]
    fn save_text_file_writes_the_text_and_replaces_what_was_there() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("address-book.csv");
        fs::write(&path, "old").unwrap();

        save_text_file_inner(path.to_str().unwrap(), "contact_id,display_name\n").unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "contact_id,display_name\n"
        );
    }

    #[test]
    fn save_text_file_refuses_an_empty_or_relative_path_and_reports_a_failed_write() {
        assert!(
            save_text_file_inner("  ", "x")
                .unwrap_err()
                .contains("empty")
        );
        assert!(
            save_text_file_inner("address-book.csv", "x")
                .unwrap_err()
                .contains("absolute")
        );
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("no-such-folder").join("address-book.csv");
        let err = save_text_file_inner(missing.to_str().unwrap(), "x").unwrap_err();
        assert!(err.starts_with("Could not save "), "{err}");
    }

    #[test]
    fn rejects_empty_path() {
        let root = "/home/sam/message-crate";
        let err = resolve_openable_path("  ", root).unwrap_err();
        assert!(err.contains("empty"));
    }

    #[test]
    fn rejects_empty_staging_root() {
        let err = resolve_openable_path("/home/sam/message-crate/staging", "  ").unwrap_err();
        assert!(err.contains("Staging directory is empty"));
    }

    #[test]
    fn rejects_relative_path() {
        let root = "/home/sam/message-crate";
        let err = resolve_openable_path("message-crate/staging", root).unwrap_err();
        assert!(err.contains("absolute"));
    }

    #[test]
    fn rejects_relative_staging_root() {
        let err = resolve_openable_path("/tmp/staging", "message-crate").unwrap_err();
        assert!(err.contains("must be absolute"));
    }

    #[test]
    fn rejects_filesystem_root_staging_root() {
        let err = resolve_openable_path("/etc/passwd", "/").unwrap_err();
        assert!(err.contains("filesystem root"));
    }

    #[test]
    fn accepts_path_under_staging_when_missing() {
        let root = "/home/sam/message-crate";
        let path = "/home/sam/message-crate/staging-iphone-ios-260824-180509";
        let resolved = resolve_openable_path(path, root).unwrap();
        assert_eq!(resolved, PathBuf::from(path));
    }

    #[test]
    fn accepts_path_under_custom_staging_root() {
        let root = "/data/imports";
        let path = "/data/imports/staging-iphone-ios-260824-180509";
        let resolved = resolve_openable_path(path, root).unwrap();
        assert_eq!(resolved, PathBuf::from(path));
    }

    #[test]
    fn accepts_log_file_under_staging_when_missing() {
        let root = "/home/sam/message-crate";
        let path = "/home/sam/message-crate/staging-x/message-crate-push.log";
        let resolved = resolve_openable_path(path, root).unwrap();
        assert_eq!(resolved, PathBuf::from(path));
    }

    #[test]
    fn rejects_path_outside_staging() {
        let root = "/home/sam/message-crate";
        let err = resolve_openable_path("/home/sam/Documents/notes.txt", root).unwrap_err();
        assert!(err.contains("outside"));
    }

    #[test]
    fn rejects_parent_traversal_escape() {
        let root = "/home/sam/message-crate";
        let err =
            resolve_openable_path("/home/sam/message-crate/../.ssh/id_rsa", root).unwrap_err();
        assert!(err.contains("outside"));
    }

    #[test]
    fn accepts_existing_file_under_staging() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("message-crate");
        let staging = root.join("staging-test");
        fs::create_dir_all(&staging).unwrap();
        let log = staging.join("message-crate-push.log");
        fs::write(&log, "ok\n").unwrap();

        let resolved =
            resolve_openable_path(log.to_str().unwrap(), root.to_str().unwrap()).unwrap();
        assert_eq!(resolved, log.canonicalize().unwrap());
    }

    #[test]
    fn rejects_existing_file_outside_staging() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("message-crate");
        fs::create_dir_all(&root).unwrap();
        let outside = temp.path().join("secrets.txt");
        fs::write(&outside, "secret\n").unwrap();

        let err =
            resolve_openable_path(outside.to_str().unwrap(), root.to_str().unwrap()).unwrap_err();
        assert!(err.contains("outside"));
    }

    #[test]
    fn missing_log_is_reported_like_any_other_missing_path() {
        // The log is deleted with the staging directory once an import
        // succeeds, so a missing log has nothing special to explain.
        let log = PathBuf::from("/home/sam/message-crate/staging-x/message-crate-push.log");
        let err = missing_path_error(&log).unwrap_err();
        assert_eq!(err, "Nothing exists at message-crate-push.log yet");
    }

    #[test]
    fn missing_folder_uses_generic_message() {
        let staging = PathBuf::from("/home/sam/message-crate/staging-x");
        let err = missing_path_error(&staging).unwrap_err();
        assert!(err.contains("Nothing exists"));
        assert!(err.contains("staging-x"));
    }

    #[test]
    fn existing_path_passes_missing_check() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("message-crate-push.log");
        fs::write(&file, "ok\n").unwrap();
        missing_path_error(&file).unwrap();
    }

    #[test]
    fn path_stat_missing() {
        let stat = path_stat_inner("/no/such/message-crate-path-stat");
        assert!(!stat.exists);
        assert!(!stat.is_file);
        assert!(!stat.is_directory);
    }

    #[test]
    fn path_stat_file_and_directory() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("chat.db");
        fs::write(&file, b"sqlite").unwrap();
        let file_stat = path_stat_inner(file.to_str().unwrap());
        assert!(file_stat.exists && file_stat.is_file && !file_stat.is_directory);
        let dir_stat = path_stat_inner(dir.path().to_str().unwrap());
        assert!(dir_stat.exists && dir_stat.is_directory && !dir_stat.is_file);
    }

    #[test]
    fn blank_path_is_missing() {
        let stat = path_stat_inner("  ");
        assert!(!stat.exists);
    }

    /// The fingerprint a resumed import compares against is built from these
    /// two fields, so the byte count must be the file's own and the modified
    /// time must be the filesystem's, in milliseconds.
    #[test]
    fn path_stat_reports_size_and_modified_time() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("chat.db");
        fs::write(&file, b"sqlite").unwrap();

        let stat = path_stat_inner(file.to_str().unwrap());
        assert_eq!(stat.size_bytes, 6);

        let expected_ms = fs::metadata(&file)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        assert_eq!(
            stat.modified_unix_ms,
            Some(i64::try_from(expected_ms).unwrap())
        );
        // A seconds or nanoseconds value would be off by a factor of a
        // thousand either way; this pins the unit against the clock.
        let now_ms = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap();
        let modified = stat.modified_unix_ms.unwrap();
        assert!((now_ms - modified).abs() < 60_000, "{modified} vs {now_ms}");
    }

    /// A path that is not there has nothing to measure: zero bytes and no
    /// modified time, rather than an error the resume screen would have to
    /// tell apart from a real failure.
    #[test]
    fn path_stat_missing_has_no_size_or_modified_time() {
        let stat = path_stat_inner("/no/such/message-crate-path-stat");
        assert_eq!(stat.size_bytes, 0);
        assert_eq!(stat.modified_unix_ms, None);
        let blank = path_stat_inner("  ");
        assert_eq!(blank.size_bytes, 0);
        assert_eq!(blank.modified_unix_ms, None);
    }
}
