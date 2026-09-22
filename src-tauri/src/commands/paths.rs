//! Path helpers the Settings and Import screens use.

use serde::Serialize;
use std::path::{Component, Path, PathBuf};

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
/// decrypts `chat.db` to a temp file.
#[tauri::command]
pub async fn imessage_backup_identities(
    path: String,
    ios: bool,
    backup_password: Option<String>,
) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let password = backup_password.as_deref().and_then(message_ir::trimmed);
        imessage_ir_exporter::backup_identities(Path::new(path.trim()), ios, password)
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
/// Only paths under `staging_root` are allowed. That is the Staging
/// Directory from Settings (default `{home}/message-vault`), where staging
/// folders live, each with its `vault-push.log` while the run lasts.
///
/// # Errors
///
/// Returns an error when the path is empty, the staging root is empty, the path
/// is outside the allowed folder, missing on disk, or the OS cannot open it.
#[tauri::command]
pub fn open_path(path: String, staging_root: String) -> Result<(), String> {
    let resolved = resolve_openable_path(&path, &staging_root)?;
    missing_path_error(&resolved)?;
    open::that_detached(&resolved).map_err(|error| format!("Could not open path: {error}"))
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

/// Resolve the Staging Directory: trimmed, absolute, canonicalized
/// when it exists on disk (else lexically normalized so a not-yet-created
/// root still resolves), and never the filesystem root.
///
/// Shared by [`resolve_openable_path`] and `staging::resolve_staging_child`,
/// so every command that checks "is this path under the staging root"
/// resolves the root the identical way — the same fix applies everywhere at
/// once instead of drifting between callers.
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
    let root = if root.exists() {
        root.canonicalize()
            .map_err(|error| format!("Could not resolve staging root: {error}"))?
    } else {
        normalize_lexically(&root)
    };
    reject_filesystem_root(&root)?;
    Ok(root)
}

/// Resolve `raw` to an absolute path that must stay under `staging_root`.
///
/// When the path already exists, both sides are canonicalized so symlinks cannot
/// escape the staging tree. When it does not exist yet (for example a staging
/// folder that extract is about to create), lexical normalization is used.
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

    let normalized = normalize_lexically(&candidate);
    if !normalized.starts_with(&root) {
        return Err("Path is outside the staging folder".to_string());
    }
    Ok(normalized)
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

    #[test]
    fn rejects_empty_path() {
        let root = "/home/sam/message-vault";
        let err = resolve_openable_path("  ", root).unwrap_err();
        assert!(err.contains("empty"));
    }

    #[test]
    fn rejects_empty_staging_root() {
        let err = resolve_openable_path("/home/sam/message-vault/staging", "  ").unwrap_err();
        assert!(err.contains("Staging directory is empty"));
    }

    #[test]
    fn rejects_relative_path() {
        let root = "/home/sam/message-vault";
        let err = resolve_openable_path("message-vault/staging", root).unwrap_err();
        assert!(err.contains("absolute"));
    }

    #[test]
    fn rejects_relative_staging_root() {
        let err = resolve_openable_path("/tmp/staging", "message-vault").unwrap_err();
        assert!(err.contains("must be absolute"));
    }

    #[test]
    fn rejects_filesystem_root_staging_root() {
        let err = resolve_openable_path("/etc/passwd", "/").unwrap_err();
        assert!(err.contains("filesystem root"));
    }

    #[test]
    fn accepts_path_under_staging_when_missing() {
        let root = "/home/sam/message-vault";
        let path = "/home/sam/message-vault/staging-iphone-ios-260824-180509";
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
        let root = "/home/sam/message-vault";
        let path = "/home/sam/message-vault/staging-x/vault-push.log";
        let resolved = resolve_openable_path(path, root).unwrap();
        assert_eq!(resolved, PathBuf::from(path));
    }

    #[test]
    fn rejects_path_outside_staging() {
        let root = "/home/sam/message-vault";
        let err = resolve_openable_path("/home/sam/Documents/notes.txt", root).unwrap_err();
        assert!(err.contains("outside"));
    }

    #[test]
    fn rejects_parent_traversal_escape() {
        let root = "/home/sam/message-vault";
        let err =
            resolve_openable_path("/home/sam/message-vault/../.ssh/id_rsa", root).unwrap_err();
        assert!(err.contains("outside"));
    }

    #[test]
    fn accepts_existing_file_under_staging() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("message-vault");
        let staging = root.join("staging-test");
        fs::create_dir_all(&staging).unwrap();
        let log = staging.join("vault-push.log");
        fs::write(&log, "ok\n").unwrap();

        let resolved =
            resolve_openable_path(log.to_str().unwrap(), root.to_str().unwrap()).unwrap();
        assert_eq!(resolved, log.canonicalize().unwrap());
    }

    #[test]
    fn rejects_existing_file_outside_staging() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("message-vault");
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
        let log = PathBuf::from("/home/sam/message-vault/staging-x/vault-push.log");
        let err = missing_path_error(&log).unwrap_err();
        assert_eq!(err, "Nothing exists at vault-push.log yet");
    }

    #[test]
    fn missing_folder_uses_generic_message() {
        let staging = PathBuf::from("/home/sam/message-vault/staging-x");
        let err = missing_path_error(&staging).unwrap_err();
        assert!(err.contains("Nothing exists"));
        assert!(err.contains("staging-x"));
    }

    #[test]
    fn existing_path_passes_missing_check() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("vault-push.log");
        fs::write(&file, "ok\n").unwrap();
        missing_path_error(&file).unwrap();
    }

    #[test]
    fn path_stat_missing() {
        let stat = path_stat_inner("/no/such/message-vault-path-stat");
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
        let stat = path_stat_inner("/no/such/message-vault-path-stat");
        assert_eq!(stat.size_bytes, 0);
        assert_eq!(stat.modified_unix_ms, None);
        let blank = path_stat_inner("  ");
        assert_eq!(blank.size_bytes, 0);
        assert_eq!(blank.modified_unix_ms, None);
    }
}
