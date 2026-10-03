//! The Staging Directory and the staging folders this app made under it.
//!
//! The desktop process owns both. The window never names a root:
//! `set_staging_root` stores the Staging Directory here, `create_staging_dir`
//! makes a run's folder under it, and every other command that touches a
//! staging folder takes only the folder.
//!
//! A folder is acted on when this app made it, which this record says, and
//! when it still holds the `.message-crate-export` sentinel. Where the
//! Staging Directory points now does not matter. A run keeps its folder
//! when the setting changes, and the new setting applies to the folders
//! made after it (issue #1154).
//!
//! The record lives in one JSON file in the app's data folder
//! ([`RECORD_FILE`]), so it survives a restart, as a paused Import Run does.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use message_ir_format::{EXPORT_SENTINEL, mark_export_folder};

use crate::commands::paths::{resolve_openable_path, resolve_staging_root};

/// File in the app's data folder that holds the Staging Directory setting
/// and the staging folders this app made.
pub const RECORD_FILE: &str = "staging.json";

/// Folder under the home folder that is the Staging Directory when Settings
/// name none.
const DEFAULT_ROOT_NAME: &str = "message-crate";

/// What [`RECORD_FILE`] holds.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct Record {
    /// The Staging Directory from Settings. `None` means the default,
    /// `{home}/message-crate`.
    root: Option<PathBuf>,
    /// Every staging folder this app made and has not deleted, in the
    /// canonical form it was given when it was made.
    folders: Vec<PathBuf>,
}

/// The Staging Directory as Settings show it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StagingRoot {
    /// The folder new staging folders are made in.
    pub root: String,
    /// `{home}/message-crate`: the folder used when Settings name none.
    pub default_root: String,
}

/// The Staging Directory and the staging folders made under it, kept in
/// [`RECORD_FILE`].
#[derive(Debug)]
pub struct StagingFolders {
    /// Where the record is saved.
    file: PathBuf,
    /// The home folder, for the default Staging Directory. `None` when the
    /// operating system reports none.
    home: Option<PathBuf>,
    record: Record,
}

impl StagingFolders {
    /// Read the record from `file`. A file that is missing or cannot be read
    /// starts an empty record: no folders, and the default Staging
    /// Directory.
    pub fn load(file: PathBuf, home: Option<PathBuf>) -> Self {
        let record = std::fs::read(&file)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self { file, home, record }
    }

    /// `{home}/message-crate`.
    fn default_root(&self) -> Result<PathBuf, String> {
        self.home
            .as_ref()
            .map(|home| home.join(DEFAULT_ROOT_NAME))
            .ok_or_else(|| "Could not determine the user home directory".to_string())
    }

    /// The Staging Directory new staging folders are made in.
    ///
    /// # Errors
    ///
    /// Returns an error when Settings name no folder and the operating
    /// system reports no home folder.
    pub fn root(&self) -> Result<PathBuf, String> {
        match &self.record.root {
            Some(root) => Ok(root.clone()),
            None => self.default_root(),
        }
    }

    /// The Staging Directory and its default, for Settings.
    ///
    /// # Errors
    ///
    /// Returns an error when the operating system reports no home folder.
    pub fn describe(&self) -> Result<StagingRoot, String> {
        Ok(StagingRoot {
            root: self.root()?.display().to_string(),
            default_root: self.default_root()?.display().to_string(),
        })
    }

    /// Store the Staging Directory from Settings. An empty value, or the
    /// default itself, goes back to the default. Folders made under the
    /// earlier setting keep working.
    ///
    /// # Errors
    ///
    /// Returns an error when the folder is relative or the filesystem root,
    /// or the record cannot be saved.
    pub fn set_root(&mut self, root: &str) -> Result<(), String> {
        let trimmed = root.trim();
        let root = if trimmed.is_empty() {
            None
        } else {
            resolve_staging_root(trimmed)?;
            let path = PathBuf::from(trimmed);
            if self.default_root().ok().as_ref() == Some(&path) {
                None
            } else {
                Some(path)
            }
        };
        self.record.root = root;
        self.save()
    }

    /// Make a new staging folder, `staging-<label>-<timestamp>`, under the
    /// Staging Directory, write the export sentinel into it, and record it.
    /// Returns the folder in its canonical form.
    ///
    /// `label` names what the folder is for: an Import source such as
    /// `imessage-ios`, or `export`.
    ///
    /// # Errors
    ///
    /// Returns an error when `label` is not lowercase letters, digits and
    /// dashes, the Staging Directory is unusable, or the folder, its
    /// sentinel or the record cannot be written.
    pub fn create(&mut self, label: &str, timestamp: &str) -> Result<PathBuf, String> {
        let slug = folder_slug(label)?;
        let root = resolve_staging_root(&self.root()?.display().to_string())?;
        std::fs::create_dir_all(&root)
            .map_err(|error| format!("Could not make {}: {error}", root.display()))?;
        let base = format!("staging-{slug}-{timestamp}");
        let mut folder = root.join(&base);
        let mut n = 1;
        loop {
            match std::fs::create_dir(&folder) {
                Ok(()) => break,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    n += 1;
                    folder = root.join(format!("{base}-{n}"));
                }
                Err(error) => {
                    return Err(format!("Could not make {}: {error}", folder.display()));
                }
            }
        }
        mark_export_folder(&folder)
            .map_err(|error| format!("Could not mark {}: {error:#}", folder.display()))?;
        let folder = folder
            .canonicalize()
            .map_err(|error| format!("Could not resolve {}: {error}", folder.display()))?;
        self.record.folders.push(folder.clone());
        self.save()?;
        Ok(folder)
    }

    /// The staging folder `dir`, when this app made it and it still holds the
    /// export sentinel. Where the Staging Directory points now plays no part.
    ///
    /// # Errors
    ///
    /// Returns an error when `dir` is empty or relative, is not on disk, was
    /// not made by [`StagingFolders::create`], or has no sentinel.
    pub fn folder(&self, dir: &str) -> Result<PathBuf, String> {
        let path = absolute(dir)?;
        let canonical = path
            .canonicalize()
            .map_err(|error| format!("Could not find the staging folder {dir}: {error}"))?;
        if !self.record.folders.contains(&canonical) {
            return Err(format!(
                "{} is not a staging folder Message Crate made",
                canonical.display()
            ));
        }
        if !canonical.join(EXPORT_SENTINEL).is_file() {
            return Err(format!(
                "{} does not look like an export folder (missing {EXPORT_SENTINEL})",
                canonical.display()
            ));
        }
        Ok(canonical)
    }

    /// Delete the staging folder `dir` and forget it. A folder that is no
    /// longer on disk counts as deleted.
    ///
    /// The record is locked only to check the folder and to forget it, not
    /// while a folder of several gigabytes is removed.
    ///
    /// # Errors
    ///
    /// Returns an error when the folder fails [`StagingFolders::folder`]'s
    /// checks, cannot be removed, or the record cannot be saved.
    pub fn delete(shared: &Mutex<Self>, dir: &str) -> Result<(), String> {
        let path = absolute(dir)?;
        if !path.exists() {
            return lock(shared).forget(&path);
        }
        let folder = lock(shared).folder(dir)?;
        std::fs::remove_dir_all(&folder)
            .map_err(|error| format!("Could not delete {}: {error}", folder.display()))?;
        lock(shared).forget(&folder)
    }

    /// Drop `folder` from the record.
    fn forget(&mut self, folder: &Path) -> Result<(), String> {
        let before = self.record.folders.len();
        self.record.folders.retain(|recorded| recorded != folder);
        if self.record.folders.len() == before {
            Ok(())
        } else {
            self.save()
        }
    }

    /// Resolve `path` for opening: a staging folder this app made, or a file
    /// or folder inside one, such as its push log.
    ///
    /// # Errors
    ///
    /// Returns an error when the path is empty or relative, or is in no
    /// staging folder this app made.
    pub fn openable(&self, path: &str) -> Result<PathBuf, String> {
        absolute(path)?;
        self.record
            .folders
            .iter()
            .find_map(|folder| resolve_openable_path(path, &folder.display().to_string()).ok())
            .ok_or_else(|| "Path is not in a staging folder Message Crate made".to_string())
    }

    /// Write the record to [`RECORD_FILE`] through a temporary file, so a
    /// crash mid-write leaves the previous record whole.
    fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.file.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("Could not make {}: {error}", parent.display()))?;
        }
        let body = serde_json::to_vec_pretty(&self.record).map_err(|error| error.to_string())?;
        let tmp = self.file.with_extension("json.tmp");
        std::fs::write(&tmp, body)
            .and_then(|()| std::fs::rename(&tmp, &self.file))
            .map_err(|error| format!("Could not save {}: {error}", self.file.display()))
    }
}

/// The record, locked. A thread that panicked while holding it left it
/// whole, since every change is saved before the lock is released, so a
/// poisoned lock is used as it is.
pub fn lock(shared: &Mutex<StagingFolders>) -> MutexGuard<'_, StagingFolders> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `raw`, trimmed, when it is a non-empty absolute path.
fn absolute(raw: &str) -> Result<PathBuf, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Path is empty".to_string());
    }
    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err("Path must be absolute".to_string());
    }
    Ok(path)
}

/// The short name a staging folder carries for `label`: an Import source id
/// shortened the way the folder names have always read, or `label` itself.
fn folder_slug(label: &str) -> Result<&str, String> {
    let valid = !label.is_empty()
        && label.len() <= 40
        && label
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !valid {
        return Err(format!("{label:?} cannot name a staging folder"));
    }
    Ok(match label {
        "imessage-ios" => "iphone-ios",
        "imessage-macos" => "macos",
        "imessage-jailbreak" => "iphone-jailbreak",
        other => other,
    })
}

/// The local date and time as `YYMMDD-HHMMSS`, for a staging folder's name.
pub fn timestamp_now() -> String {
    chrono::Local::now().format("%y%m%d-%H%M%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const NOW: &str = "261002-101500";

    /// A record in `dir`, with `home` as the home folder.
    fn folders_in(dir: &Path, home: &Path) -> StagingFolders {
        StagingFolders::load(dir.join(RECORD_FILE), Some(home.to_path_buf()))
    }

    #[test]
    fn a_run_keeps_its_folder_when_the_staging_directory_changes() {
        // Issue #1154: a run's folder was made under the Staging Directory
        // set when it started. Changing the setting must not strand it.
        let app_data = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let mut folders = folders_in(app_data.path(), home.path());
        folders.set_root(first.path().to_str().unwrap()).unwrap();
        let run = folders.create("imessage-ios", NOW).unwrap();
        let run_str = run.to_str().unwrap();

        folders.set_root(second.path().to_str().unwrap()).unwrap();
        // The app restarts with the new setting.
        let folders = Mutex::new(folders_in(app_data.path(), home.path()));

        assert_eq!(lock(&folders).folder(run_str).unwrap(), run, "resume");
        assert!(lock(&folders).openable(run_str).is_ok(), "open the folder");
        StagingFolders::delete(&folders, run_str).unwrap();
        assert!(!run.exists(), "discard and clean up");
        let next = lock(&folders).create("imessage-ios", NOW).unwrap();
        assert!(next.starts_with(second.path().canonicalize().unwrap()));
    }

    #[test]
    fn a_new_folder_is_named_for_its_source_and_holds_the_sentinel() {
        let app_data = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let mut folders = folders_in(app_data.path(), home.path());

        let run = folders.create("imessage-ios", NOW).unwrap();
        let again = folders.create("imessage-ios", NOW).unwrap();

        let root = home.path().canonicalize().unwrap().join("message-crate");
        assert_eq!(run, root.join("staging-iphone-ios-261002-101500"));
        assert_eq!(again, root.join("staging-iphone-ios-261002-101500-2"));
        assert!(run.join(EXPORT_SENTINEL).is_file());
    }

    #[test]
    fn a_label_that_could_leave_the_staging_directory_is_refused() {
        let app_data = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let mut folders = folders_in(app_data.path(), home.path());

        for label in ["", "../x", "a/b", "Export"] {
            assert!(folders.create(label, NOW).is_err(), "{label:?}");
        }
    }

    #[test]
    fn a_folder_this_app_did_not_make_is_refused_even_with_the_sentinel() {
        // A folder the person exported into holds the sentinel too. It is
        // still not a staging folder, and must never be deleted as one.
        let app_data = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let folders = folders_in(app_data.path(), home.path());
        let export = home.path().join("message-crate").join("my-export");
        fs::create_dir_all(&export).unwrap();
        fs::write(export.join(EXPORT_SENTINEL), "").unwrap();
        let export_str = export.to_str().unwrap();

        let err = folders.folder(export_str).unwrap_err();
        assert!(err.contains("not a staging folder"), "{err}");
        assert!(
            StagingFolders::delete(
                &Mutex::new(folders_in(app_data.path(), home.path())),
                export_str
            )
            .is_err()
        );
        assert!(folders.openable(export_str).is_err());
        assert!(export.exists());
    }

    #[test]
    fn a_made_folder_whose_sentinel_is_gone_is_refused() {
        let app_data = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let mut folders = folders_in(app_data.path(), home.path());
        let run = folders.create("export", NOW).unwrap();
        fs::remove_file(run.join(EXPORT_SENTINEL)).unwrap();

        let err = StagingFolders::delete(&Mutex::new(folders), run.to_str().unwrap()).unwrap_err();

        assert!(err.contains(EXPORT_SENTINEL), "{err}");
        assert!(run.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_that_cannot_be_removed_is_an_error_and_stays_recorded() {
        use std::os::unix::fs::PermissionsExt;

        let app_data = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let mut folders = folders_in(app_data.path(), home.path());
        let run = folders.create("export", NOW).unwrap();
        let locked = run.join("locked");
        fs::create_dir_all(locked.join("inner")).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let folders = Mutex::new(folders);

        let result = StagingFolders::delete(&folders, run.to_str().unwrap());

        // Restore permissions so the tempdir can clean itself up.
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(result.is_err(), "a failed removal must not be a quiet Ok");
        assert!(lock(&folders).folder(run.to_str().unwrap()).is_ok());
    }

    #[test]
    fn deleting_a_folder_already_gone_succeeds_and_forgets_it() {
        let app_data = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let mut folders = folders_in(app_data.path(), home.path());
        let run = folders.create("export", NOW).unwrap();
        fs::remove_dir_all(&run).unwrap();

        StagingFolders::delete(&Mutex::new(folders), run.to_str().unwrap()).unwrap();

        let reloaded = folders_in(app_data.path(), home.path());
        assert!(reloaded.record.folders.is_empty());
    }

    #[test]
    fn a_file_inside_a_made_folder_is_openable_and_one_beside_it_is_not() {
        let app_data = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let mut folders = folders_in(app_data.path(), home.path());
        let run = folders.create("export", NOW).unwrap();
        let log = run.join("message-crate-push.log");
        let beside = run.parent().unwrap().join("notes.txt");
        fs::write(&beside, "").unwrap();

        assert_eq!(folders.openable(log.to_str().unwrap()).unwrap(), log);
        assert!(folders.openable(beside.to_str().unwrap()).is_err());
        let escape = run.join("..").join("notes.txt");
        assert!(folders.openable(escape.to_str().unwrap()).is_err());
    }

    #[test]
    fn the_staging_directory_is_stored_and_the_default_is_not() {
        let app_data = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let mut folders = folders_in(app_data.path(), home.path());
        let default_root = home.path().join("message-crate");

        folders.set_root("/data/imports").unwrap();
        let reloaded = folders_in(app_data.path(), home.path());
        assert_eq!(reloaded.root().unwrap(), PathBuf::from("/data/imports"));
        assert_eq!(
            reloaded.describe().unwrap().default_root,
            default_root.display().to_string()
        );

        folders.set_root(default_root.to_str().unwrap()).unwrap();
        assert_eq!(folders.record.root, None);
        folders.set_root("/data/imports").unwrap();
        folders.set_root("  ").unwrap();
        assert_eq!(folders.root().unwrap(), default_root);
    }

    #[test]
    fn a_relative_or_filesystem_root_staging_directory_is_refused() {
        let app_data = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let mut folders = folders_in(app_data.path(), home.path());

        assert!(folders.set_root("message-crate").is_err());
        assert!(folders.set_root("/").is_err());
        assert_eq!(folders.record.root, None);
    }
}
