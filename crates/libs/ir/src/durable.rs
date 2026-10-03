//! Writing a file so that a power loss leaves either the old file or the
//! whole new one under its name, never an empty or cut-off one.
//!
//! A conversation file vouches for the attachment files it names: a resumed
//! Extract skips a conversation whose file is complete, and the Upload sends
//! the attachments that file points at. That holds only when every file is
//! on disk before anything that points at it. On a file system with delayed
//! allocation (ext4, for one) a rename can reach the disk ahead of the data
//! behind it, so each file is synced before it is renamed into place, and
//! the folder is synced after, so the rename itself is on disk before the
//! caller writes the next file that relies on it.

use anyhow::{Context, Result};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

/// Write `path` atomically through a `.tmp` sibling (`<file name>.tmp`).
///
/// Same as [`write_atomic_via`] with that sibling as the temporary file.
///
/// # Errors
///
/// Returns an error when `path` has no file name, or for any reason
/// [`write_atomic_via`] gives.
pub fn write_atomic(path: &Path, write: impl FnOnce(&mut dyn Write) -> Result<()>) -> Result<()> {
    let mut tmp_name = path
        .file_name()
        .map(|n| n.to_os_string())
        .with_context(|| format!("{} has no file name", path.display()))?;
    tmp_name.push(".tmp");
    write_atomic_via(&path.with_file_name(tmp_name), path, write)
}

/// Write `path` atomically: create its folder, hand `write` the temporary
/// file `tmp`, then [`rename_into_place`] it over `path`, so a reader never
/// sees a half-written file.
///
/// A caller that may write one `path` from two threads at once passes a
/// distinct `tmp` for each, because a shared one would let the first rename
/// pull the file from under the second writer. A failed write removes
/// `tmp`, since nothing points at it.
///
/// # Errors
///
/// Returns an error when the folder or `tmp` cannot be created, `write`
/// fails, or the sync or the rename fails.
pub fn write_atomic_via(
    tmp: &Path,
    path: &Path,
    write: impl FnOnce(&mut dyn Write) -> Result<()>,
) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let written = write_and_sync(tmp, write).and_then(|()| rename_synced(tmp, path));
    if written.is_err() {
        let _ = fs::remove_file(tmp);
    }
    written
}

/// Rename the finished file `from` over `to` so that a power loss leaves
/// either the old `to` or all of `from` under that name: sync `from`, rename
/// it, then sync the folder of `to`.
///
/// For a file another program wrote, such as an ffmpeg output, that this
/// process has not synced.
///
/// # Errors
///
/// Returns an error when `from` cannot be opened or synced, or the rename
/// or the folder sync fails.
pub fn rename_into_place(from: &Path, to: &Path) -> Result<()> {
    File::open(from)
        .and_then(|f| f.sync_all())
        .with_context(|| format!("sync {}", from.display()))?;
    rename_synced(from, to)
}

fn write_and_sync(tmp: &Path, write: impl FnOnce(&mut dyn Write) -> Result<()>) -> Result<()> {
    let file = File::create(tmp).with_context(|| format!("create {}", tmp.display()))?;
    let mut out = BufWriter::new(file);
    write(&mut out)?;
    out.flush()
        .with_context(|| format!("flush {}", tmp.display()))?;
    out.get_ref()
        .sync_all()
        .with_context(|| format!("sync {}", tmp.display()))
}

/// Rename `from` (already synced) over `to` and sync the folder of `to`.
fn rename_synced(from: &Path, to: &Path) -> Result<()> {
    fs::rename(from, to)
        .with_context(|| format!("rename {} → {}", from.display(), to.display()))?;
    match to.parent().filter(|p| !p.as_os_str().is_empty()) {
        Some(parent) => sync_dir(parent),
        None => sync_dir(Path::new(".")),
    }
}

/// Sync a folder's entries to disk, so a rename inside it survives a power
/// loss.
#[cfg(unix)]
fn sync_dir(dir: &Path) -> Result<()> {
    File::open(dir)
        .and_then(|d| d.sync_all())
        .with_context(|| format!("sync {}", dir.display()))
}

/// Windows cannot open a folder through `std::fs::File`, and NTFS journals
/// a rename with the metadata it changes, so there is nothing to sync.
#[cfg(not(unix))]
fn sync_dir(_dir: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_leaves_the_whole_file_and_no_temp_sibling() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("out.jsonl");
        write_atomic(&path, |out| {
            out.write_all(b"{}\n")?;
            Ok(())
        })
        .unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"{}\n");
        assert!(!path.with_file_name("out.jsonl.tmp").exists());
    }

    #[test]
    fn a_failed_write_keeps_the_old_file_and_removes_the_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.bin");
        let tmp = dir.path().join("out.bin.7.tmp");
        fs::write(&path, b"old").unwrap();
        let err = write_atomic_via(&tmp, &path, |out| {
            out.write_all(b"half")?;
            anyhow::bail!("stopped part way")
        });
        assert!(err.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"old");
        assert!(!tmp.exists());
    }

    #[test]
    fn rename_into_place_replaces_the_destination() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("a.mp4.in_progress");
        let to = dir.path().join("a-mv.mp4");
        fs::write(&from, b"new").unwrap();
        fs::write(&to, b"old").unwrap();
        rename_into_place(&from, &to).unwrap();
        assert_eq!(fs::read(&to).unwrap(), b"new");
        assert!(!from.exists());
    }
}
