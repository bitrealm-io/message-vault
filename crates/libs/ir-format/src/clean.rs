//! Remove leftover files from a previous export in the same directory.

use anyhow::{Context, Result, bail};
use mail::clean_previous_mail_output;
use std::fs;
use std::path::Path;

/// Sentinel file written into export directories so `clean_previous_ir_output` can
/// distinguish a real export directory from a person's own folder that was
/// pointed at by mistake.
pub const EXPORT_SENTINEL: &str = ".message-crate-export";

/// Write a sentinel file marking `output_dir` as an export target. Outside
/// tests, only [`mark_export_folder`] calls it, after checking the folder is
/// empty.
fn write_export_sentinel(output_dir: &Path) -> Result<()> {
    fs::write(output_dir.join(EXPORT_SENTINEL), "")?;
    Ok(())
}

/// Files an operating system leaves in a folder the person opened, such as
/// Finder's `.DS_Store`. A folder that holds only these looks empty to the
/// person, so it counts as empty.
const OPERATING_SYSTEM_FILES: [&str; 3] = [".DS_Store", "Thumbs.db", "desktop.ini"];

/// Mark `output_dir` as a folder an export wrote, unless it already is one.
///
/// A folder without the sentinel file `.message-crate-export` is marked only
/// when it is empty, ignoring the files an operating system leaves behind.
/// Any other folder belongs to the person who chose it, whatever its files
/// are named, so it is refused and nothing in it is touched.
///
/// # Errors
///
/// Returns an error when the directory cannot be read, the sentinel cannot be
/// written, or the directory has no sentinel and is not empty.
pub fn mark_export_folder(output_dir: &Path) -> Result<()> {
    if output_dir.join(EXPORT_SENTINEL).is_file() {
        return Ok(());
    }
    for entry in read_dir(output_dir)? {
        let name = entry?.file_name();
        if !OPERATING_SYSTEM_FILES.contains(&name.to_str().unwrap_or("")) {
            bail!(
                "{} is not empty and Message Crate did not write it. Refusing to write into it. \
                 Choose an empty folder or one an earlier export wrote.",
                output_dir.display()
            );
        }
    }
    write_export_sentinel(output_dir)
}

/// Clean a folder an earlier export marked, or mark an empty one.
///
/// A folder that holds the sentinel loses its previous CSV, JSON, JSON Lines,
/// meta, `smses.xml`, temps, staged attachments, and mail archives, and keeps
/// every other file. A folder without the sentinel goes through
/// [`mark_export_folder`], so nothing is removed from it.
///
/// # Errors
///
/// Returns an error when the directory cannot be read, a file cannot be
/// removed, or the directory has no sentinel and is not empty.
pub fn clean_previous_ir_output(output_dir: &Path) -> Result<()> {
    if !output_dir.is_dir() {
        return Ok(());
    }
    if !output_dir.join(EXPORT_SENTINEL).is_file() {
        return mark_export_folder(output_dir);
    }
    for entry in read_dir(output_dir)? {
        let path = entry?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !path.is_file() {
            continue;
        }
        if is_export_artifact(name) {
            fs::remove_file(&path)
                .with_context(|| format!("remove previous {}", path.display()))?;
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
    clean_previous_mail_output(output_dir)
}

fn read_dir(dir: &Path) -> Result<fs::ReadDir> {
    fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))
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
        || name == "smses.xml"
        || name.ends_with(".xml.tmp")
        || name.ends_with(".xml.sbrbody")
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
    fn refuses_a_folder_of_the_persons_own_files() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("notes.txt"), "mine").unwrap();

        let err = clean_previous_ir_output(tmp.path()).unwrap_err();

        assert!(
            err.to_string().contains("Refusing to write into it"),
            "{err}"
        );
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
            "smses.xml",
            "e.xml.tmp",
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
    fn refuses_an_unmarked_folder_that_holds_export_like_files() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("budget.csv"), "mine").unwrap();
        fs::write(tmp.path().join("settings.json"), "{}").unwrap();
        fs::write(tmp.path().join("notes.txt"), "mine").unwrap();
        fs::create_dir_all(tmp.path().join("attachments")).unwrap();
        fs::write(tmp.path().join("attachments/holiday.jpg"), "mine").unwrap();

        let err = clean_previous_ir_output(tmp.path()).unwrap_err();

        assert!(
            err.to_string().contains(&tmp.path().display().to_string()),
            "{err}"
        );
        assert_eq!(
            names(tmp.path()),
            ["attachments", "budget.csv", "notes.txt", "settings.json"]
        );
        assert!(tmp.path().join("attachments/holiday.jpg").exists());
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
            "smses.xml",
            "a.xml.tmp",
            "a.xml.sbrbody",
        ] {
            assert!(is_export_artifact(name), "{name} is an export file");
        }
        for name in ["notes.txt", "photo.jpg", "other.xml", "a.csv.bak", ""] {
            assert!(!is_export_artifact(name), "{name} is not an export file");
        }
    }

    #[test]
    fn marks_a_folder_that_holds_only_operating_system_files() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join(".DS_Store"), "finder").unwrap();

        clean_previous_ir_output(tmp.path()).unwrap();

        assert_eq!(names(tmp.path()), [".DS_Store", EXPORT_SENTINEL]);
    }
}
