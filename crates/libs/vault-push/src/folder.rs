//! The export folder on disk: which files in it are conversations, and how
//! attachment paths inside those files map back to real files.
//!
//! Nothing here talks to the network. It is the read-only view of the folder
//! that both the desktop app (to label an import) and the push run share.

use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use message_ir::ConversationHeader;

use crate::journal;
use crate::project;

/// The folder a push works from. A conversation file path is accepted too and
/// resolved to its parent, because the desktop app hands over whichever the
/// person picked.
///
/// # Errors
///
/// Returns an error when the resolved folder does not exist.
pub(crate) fn input_folder(input: &Path) -> Result<PathBuf> {
    let folder = if input.is_file() {
        input
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
    } else {
        input.to_path_buf()
    };
    if !folder.is_dir() {
        bail!("input directory does not exist: {}", folder.display());
    }
    Ok(folder)
}

/// True for files vault-push itself writes (journal/report/log), not conversations.
fn is_push_artifact(name: &str) -> bool {
    name.eq_ignore_ascii_case(journal::JOURNAL_NAME)
        || name.eq_ignore_ascii_case(journal::REPORT_NAME)
        || name.eq_ignore_ascii_case(journal::LOG_NAME)
        || name.ends_with(".jsonl.tmp")
        || name.starts_with('.')
}

/// True when `path` is a conversation JSON Lines file, not a push log or report.
fn is_conversation_jsonl(path: &Path, exclude: &[&Path]) -> bool {
    if exclude.contains(&path) {
        return false;
    }
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    if is_push_artifact(name) {
        return false;
    }
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("jsonl"))
}

/// List conversation JSON Lines (`.jsonl`) files in `dir`, sorted, skipping journal/report/log.
///
/// `exclude` names extra files to leave out, such as a custom report path
/// that happens to end in `.jsonl`.
///
/// # Errors
///
/// Returns an error when `dir` cannot be read.
pub(crate) fn list_jsonl_files(dir: &Path, exclude: &[&Path]) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        if is_conversation_jsonl(&path, exclude) {
            paths.push(path);
        }
    }
    // Stable order so progress "3/681" is repeatable across runs.
    paths.sort();
    Ok(paths)
}

/// The file name of a conversation path, or `?` when it has none.
pub(crate) fn file_label(path: &Path) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("?")
        .to_string()
}

/// Read the first conversation file's header and return its `export.source` string.
///
/// The GUI uses this to label the import session (for example `imessage`).
///
/// # Errors
///
/// Returns an error when the folder cannot be listed, the file cannot be read,
/// or the header is invalid.
pub fn detect_source(input: &Path) -> Result<Option<String>> {
    let dir = if input.is_file() {
        input.parent().unwrap_or(input)
    } else {
        input
    };
    let files = list_jsonl_files(dir, &[])?;
    let Some(path) = files.first() else {
        return Ok(None);
    };
    let file = File::open(path)?;
    let mut lines = BufReader::new(file).lines();
    let header_line = lines
        .next()
        .ok_or_else(|| anyhow::anyhow!("empty JSONL"))??;
    let header: ConversationHeader = serde_json::from_str(&header_line)?;
    Ok(Some(project::validate_header(&header)?))
}

/// Turn an attachment path from a JSON Lines file into a real file path under
/// the export folder. `None` means the path is safe but no file is there.
///
/// # Errors
///
/// Returns an error when the path could escape the export folder.
pub(crate) fn resolve_attachment(export_root: &Path, rel: &str) -> Result<Option<PathBuf>> {
    let under = message_ir::safe_attachment_path(export_root, rel)?;
    Ok(under.is_file().then_some(under))
}

/// Name an attachment that has no path, for an Import Errors row.
///
/// Falls back to the position in the message so two pathless attachments in one
/// conversation stay distinguishable.
pub(crate) fn attachment_label(att: &message_ir::IrAttachment, index: usize) -> String {
    att.original_name
        .as_deref()
        .and_then(message_ir::trimmed)
        .map_or_else(|| format!("attachment {index}"), str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_jsonl_files_skips_push_artifacts_and_sorts() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "b.jsonl",
            "a.jsonl",
            journal::JOURNAL_NAME,
            "x.jsonl.tmp",
            "notes.txt",
        ] {
            fs::write(dir.path().join(name), "").unwrap();
        }
        let files = list_jsonl_files(dir.path(), &[]).unwrap();
        let names: Vec<String> = files.iter().map(|p| file_label(p)).collect();
        assert_eq!(names, ["a.jsonl", "b.jsonl"]);
    }
}
