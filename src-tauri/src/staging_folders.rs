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
//! That file is the only copy: every read takes a shared lock on
//! [`LOCK_FILE`], and every change takes it alone and reads the file again
//! before it writes. Two app processes at once (two launches, or a dev build
//! beside an installed one) then never write over each other's folders.

use std::fs::File;
use std::path::{Path, PathBuf};

use message_ir_format::{EXPORT_SENTINEL, mark_export_folder};

use crate::commands::paths::{resolve_openable_path, resolve_staging_root};

/// File in the app's data folder that holds the Staging Directory setting
/// and the staging folders this app made.
pub const RECORD_FILE: &str = "staging.json";

/// File beside [`RECORD_FILE`] that every process locks to read or change it.
const LOCK_FILE: &str = "staging.json.lock";

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
}

impl StagingFolders {
    /// The record kept in `file`, read from it on every use.
    pub fn at(file: PathBuf, home: Option<PathBuf>) -> Self {
        Self { file, home }
    }

    /// The record as the file holds it now.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be locked or read.
    fn read(&self) -> Result<Record, String> {
        let _lock = self.lock_file(false)?;
        self.read_locked()
    }

    /// Change the record with `change`, holding the lock from the read to the
    /// write, so no other process's change is written over.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be locked, read or written, or
    /// `change` refuses.
    fn change<T>(
        &self,
        change: impl FnOnce(&mut Record) -> Result<T, String>,
    ) -> Result<T, String> {
        let _lock = self.lock_file(true)?;
        let mut record = self.read_locked()?;
        let answer = change(&mut record)?;
        self.write_locked(&record)?;
        Ok(answer)
    }

    /// [`LOCK_FILE`], locked alone when `exclusive`, shared otherwise. The
    /// lock is released when the file is dropped.
    fn lock_file(&self, exclusive: bool) -> Result<File, String> {
        let dir = self.file.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir)
            .map_err(|error| format!("Could not make {}: {error}", dir.display()))?;
        let path = dir.join(LOCK_FILE);
        let file = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|error| format!("Could not open {}: {error}", path.display()))?;
        if exclusive {
            file.lock()
        } else {
            file.lock_shared()
        }
        .map_err(|error| format!("Could not lock {}: {error}", path.display()))?;
        Ok(file)
    }

    /// The record in [`RECORD_FILE`], or an empty one when there is no file
    /// yet. A file that cannot be read or parsed is an error, never an empty
    /// record, so the next change cannot write over the folders it lists.
    fn read_locked(&self) -> Result<Record, String> {
        match std::fs::read(&self.file) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
                format!(
                    "{} is damaged ({error}). Move it aside to start a new record of staging folders",
                    self.file.display()
                )
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Record::default()),
            Err(error) => Err(format!("Could not read {}: {error}", self.file.display())),
        }
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
        match self.read()?.root {
            Some(root) => Ok(root),
            None => self.default_root(),
        }
    }

    /// The Staging Directory and its default, for Settings.
    ///
    /// # Errors
    ///
    /// Returns an error when the operating system reports no home folder, or
    /// the record cannot be read.
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
    pub fn set_root(&self, root: &str) -> Result<(), String> {
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
        self.change(|record| {
            record.root = root;
            Ok(())
        })
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
    pub fn create(&self, label: &str, timestamp: &str) -> Result<PathBuf, String> {
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
        self.change(|record| {
            record.folders.push(folder.clone());
            Ok(())
        })?;
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
        if !self.read()?.folders.contains(&canonical) {
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
    /// longer on disk counts as deleted, but only when the folder it was in is
    /// still there: an unplugged drive, or a folder the app may not read, is
    /// an error, and the folder stays recorded.
    ///
    /// The record's lock is held only to check the folder and to forget it,
    /// not while a folder of several gigabytes is removed.
    ///
    /// # Errors
    ///
    /// Returns an error when the folder fails [`StagingFolders::folder`]'s
    /// checks, cannot be removed, or the record cannot be saved.
    pub fn delete(&self, dir: &str) -> Result<(), String> {
        let path = absolute(dir)?;
        if is_gone(&path) {
            return self.forget(&path);
        }
        let folder = self.folder(dir)?;
        remove_sentinel_last(&folder)
            .map_err(|error| format!("Could not delete {}: {error}", folder.display()))?;
        self.forget(&folder)
    }

    /// Drop `folder` from the record.
    fn forget(&self, folder: &Path) -> Result<(), String> {
        self.change(|record| {
            record.folders.retain(|recorded| recorded != folder);
            Ok(())
        })
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
        self.read()?
            .folders
            .iter()
            .find_map(|folder| resolve_openable_path(path, &folder.display().to_string()).ok())
            .ok_or_else(|| "Path is not in a staging folder Message Crate made".to_string())
    }

    /// Write `record` to [`RECORD_FILE`] through a temporary file of this
    /// process's own, so a crash mid-write leaves the previous record whole.
    fn write_locked(&self, record: &Record) -> Result<(), String> {
        let body = serde_json::to_vec_pretty(record).map_err(|error| error.to_string())?;
        let tmp = self
            .file
            .with_extension(format!("json.{}.tmp", std::process::id()));
        std::fs::write(&tmp, body)
            .and_then(|()| std::fs::rename(&tmp, &self.file))
            .map_err(|error| format!("Could not save {}: {error}", self.file.display()))
    }
}

/// Whether `path` is known to be gone: not found, in a folder that is still
/// there. A path that cannot be read for any other reason is not gone.
fn is_gone(path: &Path) -> bool {
    matches!(
        std::fs::symlink_metadata(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound
    ) && path.parent().is_some_and(Path::is_dir)
}

/// Remove `folder`, its export sentinel last. A removal that fails part-way
/// leaves the sentinel, so the folder is still one the app may delete, and a
/// later delete can finish it.
fn remove_sentinel_last(folder: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(folder)? {
        let entry = entry?;
        if entry.file_name() == EXPORT_SENTINEL {
            continue;
        }
        if entry.file_type()?.is_dir() {
            std::fs::remove_dir_all(entry.path())?;
        } else {
            std::fs::remove_file(entry.path())?;
        }
    }
    std::fs::remove_file(folder.join(EXPORT_SENTINEL))?;
    std::fs::remove_dir(folder)
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

/// A record of staging folders in its own temporary app-data and home
/// folders, for tests here and in the staging commands.
#[cfg(test)]
pub(crate) struct Scratch {
    pub folders: StagingFolders,
    pub app_data: tempfile::TempDir,
    pub home: tempfile::TempDir,
}

#[cfg(test)]
impl Scratch {
    pub fn new() -> Self {
        let app_data = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let folders = Self::open(&app_data, &home);
        Self {
            folders,
            app_data,
            home,
        }
    }

    /// The record kept in `app_data`, with `home` as the home folder.
    fn open(app_data: &tempfile::TempDir, home: &tempfile::TempDir) -> StagingFolders {
        StagingFolders::at(
            app_data.path().join(RECORD_FILE),
            Some(home.path().to_path_buf()),
        )
    }

    /// The same record, as another process or a restarted app opens it.
    pub fn reopen(&self) -> StagingFolders {
        Self::open(&self.app_data, &self.home)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const NOW: &str = "261002-101500";

    #[test]
    fn a_run_keeps_its_folder_when_the_staging_directory_changes() {
        // Issue #1154: a run's folder was made under the Staging Directory
        // set when it started. Changing the setting must not strand it.
        let scratch = Scratch::new();
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        scratch
            .folders
            .set_root(first.path().to_str().unwrap())
            .unwrap();
        let run = scratch.folders.create("imessage-ios", NOW).unwrap();
        let run_str = run.to_str().unwrap();

        scratch
            .folders
            .set_root(second.path().to_str().unwrap())
            .unwrap();
        // The app restarts with the new setting.
        let folders = scratch.reopen();

        assert_eq!(folders.folder(run_str).unwrap(), run, "resume");
        assert!(folders.openable(run_str).is_ok(), "open the folder");
        // Discard and the clean-up after a finished run both reach this one
        // delete, which no longer looks at the Staging Directory.
        folders.delete(run_str).unwrap();
        assert!(!run.exists(), "discard and clean up");
        let next = folders.create("imessage-ios", NOW).unwrap();
        assert!(next.starts_with(second.path().canonicalize().unwrap()));
    }

    #[test]
    fn a_new_folder_is_named_for_its_source_and_holds_the_sentinel() {
        let scratch = Scratch::new();

        let run = scratch.folders.create("imessage-ios", NOW).unwrap();
        let again = scratch.folders.create("imessage-ios", NOW).unwrap();

        let root = scratch
            .home
            .path()
            .canonicalize()
            .unwrap()
            .join("message-crate");
        assert_eq!(run, root.join("staging-iphone-ios-261002-101500"));
        assert_eq!(again, root.join("staging-iphone-ios-261002-101500-2"));
        assert!(run.join(EXPORT_SENTINEL).is_file());
    }

    #[test]
    fn a_label_that_could_leave_the_staging_directory_is_refused() {
        let scratch = Scratch::new();

        for label in ["", "../x", "a/b", "Export"] {
            assert!(scratch.folders.create(label, NOW).is_err(), "{label:?}");
        }
    }

    #[test]
    fn a_folder_this_app_did_not_make_is_refused_even_with_the_sentinel() {
        // A folder the person exported into holds the sentinel too. It is
        // still not a staging folder, and must never be deleted as one.
        let scratch = Scratch::new();
        let export = scratch.home.path().join("message-crate").join("my-export");
        fs::create_dir_all(&export).unwrap();
        fs::write(export.join(EXPORT_SENTINEL), "").unwrap();
        let export_str = export.to_str().unwrap();

        let err = scratch.folders.folder(export_str).unwrap_err();
        assert!(err.contains("not a staging folder"), "{err}");
        assert!(scratch.reopen().delete(export_str).is_err());
        assert!(scratch.folders.openable(export_str).is_err());
        assert!(export.exists());
    }

    #[test]
    fn a_made_folder_whose_sentinel_is_gone_is_refused() {
        let scratch = Scratch::new();
        let run = scratch.folders.create("export", NOW).unwrap();
        fs::remove_file(run.join(EXPORT_SENTINEL)).unwrap();

        let err = scratch.reopen().delete(run.to_str().unwrap()).unwrap_err();

        assert!(err.contains(EXPORT_SENTINEL), "{err}");
        assert!(run.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_that_cannot_be_removed_keeps_its_sentinel_and_stays_recorded() {
        use std::os::unix::fs::PermissionsExt;

        let scratch = Scratch::new();
        let run = scratch.folders.create("export", NOW).unwrap();
        let locked = run.join("locked");
        fs::create_dir_all(locked.join("inner")).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let folders = scratch.reopen();

        let result = folders.delete(run.to_str().unwrap());

        // Restore permissions so the tempdir can clean itself up.
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(result.is_err(), "a failed removal must not be a quiet Ok");
        // The sentinel goes last, whatever order the disk lists the folder
        // in, so a later delete can finish the job.
        assert!(folders.folder(run.to_str().unwrap()).is_ok());
        folders.delete(run.to_str().unwrap()).unwrap();
        assert!(!run.exists());
    }

    #[test]
    fn deleting_a_folder_already_gone_succeeds_and_forgets_it() {
        let scratch = Scratch::new();
        let run = scratch.folders.create("export", NOW).unwrap();
        fs::remove_dir_all(&run).unwrap();

        scratch.reopen().delete(run.to_str().unwrap()).unwrap();

        assert!(scratch.reopen().read().unwrap().folders.is_empty());
    }

    #[test]
    fn a_folder_on_a_drive_that_is_gone_is_not_counted_as_deleted() {
        // The folder it was in is gone too, as when the drive holding the
        // Staging Directory is unplugged: nothing says the folder was deleted.
        let scratch = Scratch::new();
        let drive = tempfile::tempdir().unwrap();
        scratch
            .folders
            .set_root(drive.path().to_str().unwrap())
            .unwrap();
        let run = scratch.folders.create("export", NOW).unwrap();
        fs::remove_dir_all(drive.path()).unwrap();

        let err = scratch.reopen().delete(run.to_str().unwrap()).unwrap_err();

        assert!(err.contains("Could not find"), "{err}");
        assert_eq!(scratch.reopen().read().unwrap().folders, vec![run]);
    }

    #[test]
    fn two_processes_keep_each_others_folders() {
        // Two app processes share the file, as two launches or a dev build
        // beside an installed one do. Neither writes over the other.
        let scratch = Scratch::new();
        let other = scratch.reopen();

        let mine = scratch.folders.create("export", NOW).unwrap();
        let theirs = other.create("imessage-ios", NOW).unwrap();

        assert!(scratch.folders.folder(theirs.to_str().unwrap()).is_ok());
        assert!(other.folder(mine.to_str().unwrap()).is_ok());
    }

    #[test]
    fn a_damaged_record_is_an_error_and_is_never_written_over() {
        let scratch = Scratch::new();
        let run = scratch.folders.create("export", NOW).unwrap();
        let file = scratch.app_data.path().join(RECORD_FILE);
        fs::write(&file, "{ not json").unwrap();

        let err = scratch.folders.create("export", NOW).unwrap_err();

        assert!(err.contains("damaged"), "{err}");
        assert!(scratch.folders.folder(run.to_str().unwrap()).is_err());
        assert_eq!(fs::read_to_string(&file).unwrap(), "{ not json");
    }

    #[test]
    fn a_file_inside_a_made_folder_is_openable_and_one_beside_it_is_not() {
        let scratch = Scratch::new();
        let run = scratch.folders.create("export", NOW).unwrap();
        let log = run.join("message-crate-push.log");
        let beside = run.parent().unwrap().join("notes.txt");
        fs::write(&beside, "").unwrap();

        assert_eq!(
            scratch.folders.openable(log.to_str().unwrap()).unwrap(),
            log
        );
        assert!(scratch.folders.openable(beside.to_str().unwrap()).is_err());
        let escape = run.join("..").join("notes.txt");
        assert!(scratch.folders.openable(escape.to_str().unwrap()).is_err());
    }

    #[test]
    fn the_staging_directory_is_stored_and_the_default_is_not() {
        let scratch = Scratch::new();
        let folders = &scratch.folders;
        let default_root = scratch.home.path().join("message-crate");

        folders.set_root("/data/imports").unwrap();
        let reloaded = scratch.reopen();
        assert_eq!(reloaded.root().unwrap(), PathBuf::from("/data/imports"));
        assert_eq!(
            reloaded.describe().unwrap().default_root,
            default_root.display().to_string()
        );

        folders.set_root(default_root.to_str().unwrap()).unwrap();
        assert_eq!(folders.read().unwrap().root, None);
        folders.set_root("/data/imports").unwrap();
        folders.set_root("  ").unwrap();
        assert_eq!(folders.root().unwrap(), default_root);
    }

    #[test]
    fn a_relative_or_filesystem_root_staging_directory_is_refused() {
        let scratch = Scratch::new();

        assert!(scratch.folders.set_root("message-crate").is_err());
        assert!(scratch.folders.set_root("/").is_err());
        assert_eq!(scratch.folders.read().unwrap().root, None);
    }
}
