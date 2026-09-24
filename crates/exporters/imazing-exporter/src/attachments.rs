//! Locate and copy iMazing attachment files next to CSV exports.

use message_csv::AttachmentCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Maximum directory depth for attachment discovery. iMazing export trees are
/// only a few levels deep; this bounds any pathological nesting.
const MAX_WALK_DEPTH: usize = 64;

/// One-time index of every file under the input tree.
///
/// Built once per export so attachment lookup does not re-walk the tree for
/// every attachment row.
pub(crate) struct AttachmentIndex {
    /// Lowercase file name -> paths sorted by path (exact-name lookup).
    by_name: HashMap<String, Vec<PathBuf>>,
    /// (lowercase file name, path) pairs sorted by path (suffix-match fallback).
    all: Vec<(String, PathBuf)>,
}

impl AttachmentIndex {
    /// Walk `root` once and index every regular file. When `root` is a file
    /// (single-CSV input), index its parent directory instead so sibling
    /// media is still discoverable.
    pub(crate) fn build(root: &Path) -> Self {
        let mut files = Vec::new();
        let base = if root.is_dir() {
            root
        } else {
            root.parent().unwrap_or(root)
        };
        collect_files(base, 0, &mut files);
        files.sort_by(|a, b| a.1.cmp(&b.1));
        let mut by_name: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for (name, path) in &files {
            by_name.entry(name.clone()).or_default().push(path.clone());
        }
        AttachmentIndex {
            by_name,
            all: files,
        }
    }

    /// Find a file whose name matches `csv_name` (exact, `_`/`-` prefixed, or
    /// suffix match), preferring the lexicographically first path.
    fn lookup(&self, csv_name: &str) -> Option<PathBuf> {
        if let Some(paths) = self.by_name.get(&csv_name.to_ascii_lowercase())
            && let Some(p) = paths.first()
        {
            return Some(p.clone());
        }
        for (name, path) in &self.all {
            if attachment_name_matches(name, csv_name) {
                return Some(path.clone());
            }
        }
        None
    }
}

/// Collect every regular file under `dir` (symlinks skipped, depth-bounded).
fn collect_files(dir: &Path, depth: usize, out: &mut Vec<(String, PathBuf)>) {
    if depth > MAX_WALK_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        // Skip symlinks: following them can loop (stack overflow) and can reach
        // files outside the input tree.
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            collect_files(&path, depth + 1, out);
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            out.push((name.to_ascii_lowercase(), path));
        }
    }
}

/// Inputs for [`resolve_attachment_cell`].
pub(crate) struct ResolveAttachmentArgs<'a> {
    pub csv_name: &'a str,
    pub attachment_type: &'a str,
    pub csv_parent: &'a Path,
    pub index: Option<&'a AttachmentIndex>,
    pub copy_attachments: bool,
}

/// Resolve a CSV attachment name into an [`AttachmentCell`] and optional source path.
///
/// Lookup order (unchanged):
/// 1. Files in the CSV's parent directory
/// 2. Indexed walk under the input tree
///
/// Does not copy files. When `copy_attachments` is false, keep the CSV name only.
/// On a missing file, fall back to the CSV name so the row still projects.
pub(crate) fn resolve_attachment_cell(
    args: ResolveAttachmentArgs<'_>,
) -> (AttachmentCell, Option<PathBuf>) {
    let ResolveAttachmentArgs {
        csv_name,
        attachment_type,
        csv_parent,
        index,
        copy_attachments,
    } = args;
    let mime = mime_hint(attachment_type, csv_name);
    let is_sticker = attachment_type.eq_ignore_ascii_case("sticker");
    let cell_from_csv = || AttachmentCell {
        meta: message_ir::AttachmentMeta {
            path: None,
            original_name: Some(csv_name.to_string()),
            mime_type: mime.clone(),
            digest_sha256: None,
        },
        is_sticker,
        transcription: None,
        sticker_effect: None,
    };
    if !copy_attachments {
        return (cell_from_csv(), None);
    }
    match find_attachment_source(csv_name, csv_parent, index) {
        Some(src) => (cell_from_csv(), Some(src)),
        None => (cell_from_csv(), None),
    }
}

fn attachment_name_matches(disk_name: &str, csv_name: &str) -> bool {
    let disk = disk_name.to_ascii_lowercase();
    let csv = csv_name.to_ascii_lowercase();
    if disk == csv {
        return true;
    }
    // Require a separator boundary before a suffix match so short CSV names
    // like `1.jpg` do not match unrelated files such as `photo11.jpg`.
    if disk.ends_with(&format!("_{csv}")) || disk.ends_with(&format!("-{csv}")) {
        return true;
    }
    if disk.len() > csv.len() {
        let prefix = &disk[..disk.len() - csv.len()];
        if let Some(c) = prefix.chars().last()
            && !c.is_ascii_alphanumeric()
        {
            return true;
        }
    }
    false
}

fn find_attachment_on_disk(
    csv_name: &str,
    csv_parent: &Path,
    index: &AttachmentIndex,
) -> Option<PathBuf> {
    if let Ok(entries) = fs::read_dir(csv_parent) {
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            // Match the index walker: do not follow symlinks out of the tree.
            if file_type.is_symlink() || !file_type.is_file() {
                continue;
            }
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str())
                && attachment_name_matches(name, csv_name)
            {
                return Some(path);
            }
        }
    }
    index.lookup(csv_name)
}

fn find_attachment_source(
    csv_name: &str,
    csv_parent: &Path,
    index: Option<&AttachmentIndex>,
) -> Option<PathBuf> {
    index.and_then(|i| find_attachment_on_disk(csv_name, csv_parent, i))
}

fn mime_hint(attachment_type: &str, filename: &str) -> Option<String> {
    let t = attachment_type.trim().to_ascii_lowercase();
    if !t.is_empty() {
        return Some(match t.as_str() {
            "image" => "image/jpeg".into(),
            "video" => "video/mp4".into(),
            "audio" => "audio/mpeg".into(),
            "gif" => "image/gif".into(),
            "sticker" => "image/webp".into(),
            other => other.to_string(),
        });
    }
    let lower = filename.to_ascii_lowercase();
    if lower.ends_with(".png") {
        Some("image/png".into())
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        Some("image/jpeg".into())
    } else if lower.ends_with(".gif") {
        Some("image/gif".into())
    } else if lower.ends_with(".heic") {
        Some("image/heic".into())
    } else if lower.ends_with(".mp4") || lower.ends_with(".mov") {
        Some("video/mp4".into())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use message_ir::IrAttachment;
    use message_vault_io_core::{AttachmentJob, MediaConfig, run_attachment_jobs};

    /// The attachment-type column names the type when iMazing filled it in;
    /// when it is empty, the file name's extension does, and an extension
    /// nobody knows gives no type rather than a wrong one.
    #[test]
    fn the_mime_type_comes_from_the_column_or_else_the_file_name() {
        for (column, mime) in [
            ("Image", "image/jpeg"),
            ("video", "video/mp4"),
            ("Audio", "audio/mpeg"),
            ("GIF", "image/gif"),
            ("sticker", "image/webp"),
            ("image/png", "image/png"),
        ] {
            assert_eq!(
                mime_hint(column, "IMG_0001.heic").as_deref(),
                Some(mime),
                "{column}"
            );
        }
        for (name, mime) in [
            ("IMG_0001.PNG", Some("image/png")),
            ("IMG_0001.jpg", Some("image/jpeg")),
            ("IMG_0001.jpeg", Some("image/jpeg")),
            ("IMG_0001.gif", Some("image/gif")),
            ("IMG_0001.heic", Some("image/heic")),
            ("IMG_0001.mp4", Some("video/mp4")),
            ("IMG_0001.MOV", Some("video/mp4")),
            ("notes.xyz", None),
        ] {
            assert_eq!(mime_hint(" ", name).as_deref(), mime, "{name}");
        }
    }

    #[test]
    fn attachment_name_matches_suffix_and_separators() {
        assert!(attachment_name_matches("IMG_1234.jpg", "1234.jpg"));
        assert!(attachment_name_matches("photo_abc.jpg", "abc.jpg"));
        assert!(attachment_name_matches("photo-abc.jpg", "abc.jpg"));
        assert!(attachment_name_matches("prefix.abc.jpg", "abc.jpg"));
        assert!(!attachment_name_matches("other.jpg", "abc.jpg"));
        // Bare ends_with would wrongly match these short CSV names.
        assert!(!attachment_name_matches("photo11.jpg", "1.jpg"));
        assert!(!attachment_name_matches("image10.jpg", "0.jpg"));
        assert!(!attachment_name_matches("photo11.jpg", "11.jpg"));
    }

    #[test]
    fn copied_attachment_includes_digest() {
        let dir = tempfile::tempdir().unwrap();
        let chat = dir.path().join("chat");
        let attachments = dir.path().join("attachments");
        fs::create_dir_all(&chat).unwrap();
        fs::create_dir_all(&attachments).unwrap();
        fs::write(chat.join("photo.jpg"), b"jpeg-bytes").unwrap();
        let index = AttachmentIndex::build(dir.path());
        let (cell, source) = resolve_attachment_cell(ResolveAttachmentArgs {
            csv_name: "photo.jpg",
            attachment_type: "image",
            csv_parent: &chat,
            index: Some(&index),
            copy_attachments: true,
        });
        assert!(
            cell.meta.digest_sha256.is_none(),
            "resolve must not hash or write"
        );
        assert!(cell.meta.path.is_none());
        assert!(
            fs::read_dir(&attachments).unwrap().next().is_none(),
            "resolve must not write files"
        );
        let source = source.expect("source path found");
        let mut att = IrAttachment {
            path: None,
            original_name: cell.meta.original_name,
            mime_type: cell.meta.mime_type,
            digest_sha256: None,
            is_sticker: cell.is_sticker,
            transcription: None,
            sticker_effect: None,
            size_bytes: None,
            missing_reason: None,
            bytes: None,
        };
        let mut jobs = [AttachmentJob {
            attachment: &mut att,
            timestamp_unix_ms: 1_600_000_000_000,
            size_hint: None,
        }];
        run_attachment_jobs(
            &mut jobs,
            &attachments,
            &MediaConfig::default(),
            |_| fs::read(&source).map(Some).or(Ok(None)),
            |_| {},
            None,
            None,
        )
        .unwrap();
        let digest = att.digest_sha256.expect("digest set after runner");
        assert_eq!(digest.len(), 64);
        assert!(att.path.as_deref().unwrap().starts_with("attachments/"));
        assert_eq!(fs::read_dir(&attachments).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn parent_dir_skips_symlinks() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let chat = dir.path().join("chat");
        let outside = dir.path().join("outside");
        fs::create_dir_all(&chat).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.jpg"), b"secret").unwrap();
        symlink(outside.join("secret.jpg"), chat.join("photo.jpg")).unwrap();
        let index = AttachmentIndex::build(&chat);
        assert_eq!(
            find_attachment_on_disk("photo.jpg", &chat, &index),
            None,
            "symlink in CSV parent must not be followed"
        );
    }

    #[test]
    fn index_finds_exact_and_suffix_matches() {
        let dir = tempfile::tempdir().unwrap();
        let chat = dir.path().join("chat");
        fs::create_dir_all(&chat).unwrap();
        fs::write(chat.join("ABC123_image000000.jpg"), b"jpeg").unwrap();
        fs::write(chat.join("notes.txt"), b"x").unwrap();
        let index = AttachmentIndex::build(dir.path());
        assert_eq!(
            index.lookup("image000000.jpg"),
            Some(chat.join("ABC123_image000000.jpg"))
        );
        assert_eq!(index.lookup("notes.txt"), Some(chat.join("notes.txt")));
        assert_eq!(index.lookup("missing.jpg"), None);
    }

    #[cfg(unix)]
    #[test]
    fn index_skips_symlink_loops() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        fs::create_dir_all(&a).unwrap();
        // Self-referential symlink: must be skipped, not followed forever.
        symlink(&a, a.join("loop")).unwrap();
        fs::write(a.join("photo.jpg"), b"jpeg").unwrap();
        let index = AttachmentIndex::build(dir.path());
        assert_eq!(index.lookup("photo.jpg"), Some(a.join("photo.jpg")));
    }
}
