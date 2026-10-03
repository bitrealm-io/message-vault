//! Remove leftover files from a previous export in the same directory.

use anyhow::{Context, Result, bail};
use mail::clean_previous_mail_output;
use std::fs;
use std::io::Write;
use std::path::Path;

/// Sentinel file written into export directories so `clean_previous_ir_output` can
/// distinguish a real export directory from a user directory that was pointed at
/// by mistake. It also lists, one per line, the files a merged archive wrote
/// into the folder ([`record_archive_files`]), which the next clean removes.
pub const EXPORT_SENTINEL: &str = ".message-crate-export";

/// Mark `output_dir` as an export target, creating the sentinel when it is
/// missing. An existing sentinel keeps the archive files it lists, so the
/// next clean still removes them.
/// Callers should run this after `create_dir_all` on a fresh export.
///
/// # Errors
///
/// Returns an error when the sentinel cannot be written.
pub fn write_export_sentinel(output_dir: &Path) -> Result<()> {
    let path = output_dir.join(EXPORT_SENTINEL);
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// Delete previous CSV, JSON, JSON Lines, meta, temps, staged attachments,
/// and mail archives. In a folder carrying the sentinel, also delete the
/// files a merged archive recorded in it ([`record_archive_files`]): the
/// crate that owns an archive names its files, and this crate knows none.
///
/// Only directories that contain the sentinel file `.message-crate-export`,
/// are empty, or already contain recognizable export files are cleaned. This
/// avoids deleting unrelated user files when the output path points at a
/// non-export directory by mistake.
///
/// # Errors
///
/// Returns an error when the directory cannot be read, a file cannot be
/// removed, or the directory looks like it is not an export folder.
pub fn clean_previous_ir_output(output_dir: &Path) -> Result<()> {
    if !output_dir.is_dir() {
        return Ok(());
    }
    let has_sentinel = output_dir.join(EXPORT_SENTINEL).is_file();
    if !has_sentinel {
        // Check if the directory looks like an export directory (contains files
        // matching known export patterns) or is empty. If neither, refuse to clean.
        let mut has_export_files = false;
        let mut has_other_files = false;
        for entry in
            fs::read_dir(output_dir).with_context(|| format!("read {}", output_dir.display()))?
        {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_str().unwrap_or("");
            if is_export_artifact(name) {
                has_export_files = true;
            } else if name != EXPORT_SENTINEL {
                has_other_files = true;
            }
        }
        if !has_export_files && has_other_files {
            bail!(
                "output directory {} exists but does not appear to contain export files. \
                 Refusing to clean unrecognized content. Use an empty directory or one \
                 previously used for exports.",
                output_dir.display()
            );
        }
    }
    if has_sentinel {
        for name in recorded_archive_files(output_dir)? {
            let path = output_dir.join(name);
            if path.is_file() {
                remove_previous(&path)?;
            }
        }
    }
    for entry in
        fs::read_dir(output_dir).with_context(|| format!("read {}", output_dir.display()))?
    {
        let path = entry?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !path.is_file() {
            continue;
        }
        if is_export_artifact(name) {
            remove_previous(&path)?;
        }
    }
    // Drop staged attachments from previous runs. Files named by a SHA-256
    // fingerprint of their bytes would otherwise pile up when a new run does
    // not reuse them. Media transforms also reprocess every file under
    // attachments/, so leftover files can fail a later run. Callers copy the
    // attachments they need after this function.
    let attachments = output_dir.join("attachments");
    if attachments.is_dir() {
        fs::remove_dir_all(&attachments)
            .with_context(|| format!("remove previous {}", attachments.display()))?;
    } else if attachments.is_file() {
        fs::remove_file(&attachments)
            .with_context(|| format!("remove previous {}", attachments.display()))?;
    }
    clean_previous_mail_output(output_dir)?;
    // Write the sentinel so future runs know this is a safe export directory.
    // The archive files it listed are gone, so it starts with no list.
    let sentinel = output_dir.join(EXPORT_SENTINEL);
    fs::write(&sentinel, "").with_context(|| format!("write {}", sentinel.display()))?;
    Ok(())
}

/// Remove one file a previous export left.
fn remove_previous(path: &Path) -> Result<()> {
    fs::remove_file(path).with_context(|| format!("remove previous {}", path.display()))
}

/// Record in the sentinel of `output_dir` the names of the files a merged
/// archive is about to write there, so the next fresh export removes them.
/// Recording before the write covers the partial files of a run that stops.
///
/// # Errors
///
/// Returns an error when the sentinel cannot be written.
pub(crate) fn record_archive_files(output_dir: &Path, names: &[String]) -> Result<()> {
    let path = output_dir.join(EXPORT_SENTINEL);
    let mut sentinel = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    for name in names {
        writeln!(sentinel, "{name}").with_context(|| format!("write {}", path.display()))?;
    }
    Ok(())
}

/// The file names recorded in the sentinel of `output_dir`. A line that is
/// not a plain file name in the folder is passed over, so a damaged sentinel
/// cannot remove anything outside it.
fn recorded_archive_files(output_dir: &Path) -> Result<Vec<String>> {
    let path = output_dir.join(EXPORT_SENTINEL);
    let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|name| {
            !name.is_empty()
                && !name.contains(['/', '\\'])
                && *name != "."
                && *name != ".."
                && *name != EXPORT_SENTINEL
        })
        .map(String::from)
        .collect())
}

/// Returns true when `name` matches a known export artifact pattern.
/// `.json` and `.json.tmp` cover the `.meta.json` sidecars as well.
fn is_export_artifact(name: &str) -> bool {
    name.ends_with(".csv")
        || name.ends_with(".csv.tmp")
        || name.ends_with(".json")
        || name.ends_with(".json.tmp")
        || name.ends_with(".jsonl")
        || name.ends_with(".jsonl.tmp")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(dir: &Path) -> Vec<String> {
        let mut out: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        out.sort();
        out
    }

    #[test]
    fn refuses_a_folder_of_the_users_own_files() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("notes.txt"), "mine").unwrap();

        let err = clean_previous_ir_output(tmp.path()).unwrap_err();

        assert!(err.to_string().contains("Refusing to clean"), "{err}");
        assert_eq!(names(tmp.path()), ["notes.txt"]);
    }

    #[test]
    fn removes_only_export_files_from_a_marked_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write_export_sentinel(dir).unwrap();
        for name in [
            "a.jsonl",
            "b.csv",
            "c.json",
            "c.meta.json",
            "d.jsonl.tmp",
            "notes.txt",
        ] {
            fs::write(dir.join(name), "x").unwrap();
        }
        fs::create_dir(dir.join("attachments")).unwrap();
        fs::write(dir.join("attachments").join("a.jpg"), "x").unwrap();
        // A folder named like an export file is not an export file.
        fs::create_dir(dir.join("kept.json")).unwrap();

        clean_previous_ir_output(dir).unwrap();

        assert_eq!(names(dir), [EXPORT_SENTINEL, "kept.json", "notes.txt"]);
    }

    #[test]
    fn cleans_a_marked_folder_that_holds_no_export_files() {
        let tmp = tempfile::tempdir().unwrap();
        write_export_sentinel(tmp.path()).unwrap();
        fs::write(tmp.path().join("notes.txt"), "mine").unwrap();

        clean_previous_ir_output(tmp.path()).unwrap();

        assert_eq!(names(tmp.path()), [EXPORT_SENTINEL, "notes.txt"]);
    }

    #[test]
    fn cleans_an_unmarked_folder_with_export_files_and_marks_it() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a.jsonl"), "x").unwrap();
        fs::write(tmp.path().join("notes.txt"), "mine").unwrap();

        clean_previous_ir_output(tmp.path()).unwrap();

        assert_eq!(names(tmp.path()), [EXPORT_SENTINEL, "notes.txt"]);
    }

    /// Every exporter cleans through here, so the files a merged archive
    /// recorded go whatever the next run writes, and an XML file nothing
    /// recorded, such as a person's own backup, stays.
    #[test]
    fn removes_the_files_an_archive_recorded_and_nothing_else() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("export");
        fs::create_dir(&dir).unwrap();
        let dir = dir.as_path();
        write_export_sentinel(dir).unwrap();
        let recorded = ["archive.xml", "archive.xml.tmp"].map(String::from);
        record_archive_files(dir, &recorded).unwrap();
        // A damaged list cannot reach outside the folder.
        record_archive_files(dir, &["../outside.xml".to_string()]).unwrap();
        let outside = tmp.path().join("outside.xml");
        fs::write(&outside, "mine").unwrap();
        for name in ["archive.xml", "archive.xml.tmp", "sms-20261001.xml"] {
            fs::write(dir.join(name), "x").unwrap();
        }

        clean_previous_ir_output(dir).unwrap();

        assert_eq!(names(dir), [EXPORT_SENTINEL, "sms-20261001.xml"]);
        assert!(outside.is_file(), "a file outside the folder is kept");
        assert_eq!(fs::read_to_string(dir.join(EXPORT_SENTINEL)).unwrap(), "");
    }

    /// Pull marks a folder without cleaning it, so marking it again keeps
    /// the archive files an earlier export listed for the next clean.
    #[test]
    fn marking_a_folder_again_keeps_the_archive_files_it_lists() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write_export_sentinel(dir).unwrap();
        record_archive_files(dir, &["archive.xml".to_string()]).unwrap();
        fs::write(dir.join("archive.xml"), "x").unwrap();

        write_export_sentinel(dir).unwrap();
        clean_previous_ir_output(dir).unwrap();

        assert_eq!(names(dir), [EXPORT_SENTINEL]);
    }

    #[test]
    fn marks_an_empty_folder() {
        let tmp = tempfile::tempdir().unwrap();

        clean_previous_ir_output(tmp.path()).unwrap();

        assert_eq!(names(tmp.path()), [EXPORT_SENTINEL]);
    }

    #[test]
    fn export_artifacts_are_recognised_by_name() {
        for name in [
            "a.csv",
            "a.csv.tmp",
            "a.meta.json",
            "a.meta.json.tmp",
            "a.json",
            "a.json.tmp",
            "a.jsonl",
            "a.jsonl.tmp",
        ] {
            assert!(is_export_artifact(name), "{name} is an export file");
        }
        for name in ["notes.txt", "photo.jpg", "other.xml", "a.csv.bak", ""] {
            assert!(!is_export_artifact(name), "{name} is not an export file");
        }
    }
}
