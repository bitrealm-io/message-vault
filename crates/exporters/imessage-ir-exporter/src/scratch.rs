//! The folder one `imessage-reader` request decrypts into.
//!
//! An encrypted iPhone backup's message and contacts databases are decrypted
//! to plain files before the reader can open them. The reader deletes them
//! when it ends normally, but a killed reader, or an app killed while the
//! reader runs, deletes nothing. So each request gets its own folder under a
//! root the app owns, the app deletes that folder when the request ends, and
//! every new request first deletes what a killed request left in the root.
//!
//! Two requests can run at once (the Import form can ask for a second
//! backup's identities before the first answer arrives), so the clean-up
//! must tell a dead request's folder from a live one. A live request holds
//! an exclusive lock on the `.lock` file in its folder for as long as it
//! runs. The operating system drops that lock when the process ends, however
//! it ends, so a folder whose lock can be taken belongs to nobody. Cleaning
//! up and making a new folder happen under a lock on the root's own `.lock`
//! file, so no request is between making its folder and locking it while
//! another cleans up.

use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

/// The lock file, in the root and in each request's folder.
const LOCK: &str = ".lock";

/// One request's scratch folder. Dropping it deletes the folder and
/// everything in it.
#[derive(Debug)]
pub(crate) struct ScratchDir {
    path: PathBuf,
    /// Held for the life of the request. `None` once [`Drop`] has let go
    /// of it, because Windows cannot delete a file that is still open.
    lock: Option<File>,
}

impl ScratchDir {
    /// Delete what earlier requests left under `root`, then make and lock a
    /// new folder there for this request.
    ///
    /// # Errors
    ///
    /// Returns an error when `root` or the new folder cannot be made, or a
    /// lock file cannot be made or locked.
    pub(crate) fn create(root: &Path) -> Result<Self> {
        fs::create_dir_all(root)
            .with_context(|| format!("make the scratch folder {}", root.display()))?;
        restrict_to_owner(root)?;
        let root_lock = File::create(root.join(LOCK))
            .with_context(|| format!("make the lock file in {}", root.display()))?;
        root_lock
            .lock()
            .with_context(|| format!("lock {}", root.display()))?;

        remove_leftovers(root);

        let path = tempfile::Builder::new()
            .prefix("request-")
            .tempdir_in(root)
            .with_context(|| format!("make a request folder in {}", root.display()))?
            .keep();
        // From here the folder is deleted on drop, and on an error below.
        let mut scratch = Self { path, lock: None };
        restrict_to_owner(&scratch.path)?;
        let lock = File::create(scratch.path.join(LOCK))
            .and_then(|lock| lock.lock().map(|()| lock))
            .with_context(|| format!("lock {}", scratch.path.display()))?;
        scratch.lock = Some(lock);
        Ok(scratch)
    }

    /// The folder the reader may write decrypted files into.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        drop(self.lock.take());
        // A folder that will not go now is removed by the next request.
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Delete every entry of `root` except its lock file and the folders of
/// requests still running. The caller holds the root's lock.
fn remove_leftovers(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_name() == LOCK {
            continue;
        }
        if !path.is_dir() {
            // Nothing writes loose files here; whatever this is, it is not
            // a running request's.
            let _ = fs::remove_file(&path);
            continue;
        }
        if belongs_to_a_running_request(&path) {
            continue;
        }
        let _ = fs::remove_dir_all(&path);
    }
}

/// Whether another request holds the lock in `folder`. A folder without a
/// lock file belongs to a request killed before it took one: requests make
/// the folder and its lock under the root's lock, which the caller holds.
fn belongs_to_a_running_request(folder: &Path) -> bool {
    let Ok(lock) = File::open(folder.join(LOCK)) else {
        return false;
    };
    lock.try_lock().is_err()
}

/// Make `folder` readable by its owner only, because it holds decrypted
/// message data.
#[cfg(unix)]
fn restrict_to_owner(folder: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(folder, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("restrict {} to its owner", folder.display()))
}

/// Off Unix the folder keeps the permissions of the app's own folder it
/// sits in.
#[cfg(not(unix))]
fn restrict_to_owner(_folder: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{LOCK, ScratchDir};

    /// The names under `root`, sorted, without the root's lock file.
    fn entries(root: &std::path::Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name != LOCK)
            .collect();
        names.sort();
        names
    }

    /// A request's folder sits under the root and is gone, with what the
    /// reader decrypted into it, once the request ends.
    #[test]
    fn a_request_folder_is_deleted_when_the_request_ends() {
        let root = tempfile::tempdir().unwrap();
        let scratch = ScratchDir::create(root.path()).unwrap();
        assert_eq!(scratch.path().parent(), Some(root.path()));
        fs::write(scratch.path().join("crabapple-sms-x.db"), b"decrypted").unwrap();

        drop(scratch);
        assert!(
            entries(root.path()).is_empty(),
            "{:?}",
            entries(root.path())
        );
    }

    /// What a killed request left is deleted when the next request starts:
    /// its folder (whose lock nobody holds), a folder it made before it
    /// took a lock, and a loose decrypted database.
    #[test]
    fn a_killed_request_s_leftovers_are_deleted_at_the_next_request() {
        let root = tempfile::tempdir().unwrap();
        let dead = root.path().join("request-dead");
        fs::create_dir(&dead).unwrap();
        fs::write(dead.join(LOCK), b"").unwrap();
        fs::write(dead.join("crabapple-sms-x.db"), b"decrypted").unwrap();
        let unlocked = root.path().join("request-unlocked");
        fs::create_dir(&unlocked).unwrap();
        fs::write(unlocked.join("crabapple-contacts-x.db"), b"decrypted").unwrap();
        fs::write(root.path().join("crabapple-sms-y.db"), b"decrypted").unwrap();

        let scratch = ScratchDir::create(root.path()).unwrap();
        let own = scratch.path().file_name().unwrap().to_string_lossy();
        assert_eq!(entries(root.path()), vec![own.into_owned()]);
    }

    /// A request still running keeps its folder while another starts.
    #[test]
    fn a_running_request_s_folder_is_kept() {
        let root = tempfile::tempdir().unwrap();
        let first = ScratchDir::create(root.path()).unwrap();
        fs::write(first.path().join("crabapple-sms-x.db"), b"decrypted").unwrap();

        let second = ScratchDir::create(root.path()).unwrap();
        assert!(first.path().join("crabapple-sms-x.db").exists());
        assert_ne!(first.path(), second.path());
    }

    /// The root holds decrypted message data, so on Unix only its owner
    /// may open it, whatever it was made with.
    #[cfg(unix)]
    #[test]
    fn the_root_is_restricted_to_its_owner() {
        use std::os::unix::fs::PermissionsExt;

        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("imessage-reader");
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();

        let scratch = ScratchDir::create(&root).unwrap();
        for folder in [root.as_path(), scratch.path()] {
            let mode = fs::metadata(folder).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "{}: {mode:o}", folder.display());
        }
    }
}
