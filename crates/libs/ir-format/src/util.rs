//! Small shared helpers for reading and writing conversation documents.

use anyhow::{Context, Result};
use message_ir::{HandleType, IrAttachment};
use std::fs::{self, File};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Guess the handle type of a raw handle string when no type is known.
///
/// Rules (mirrored by the CSV `handle_type` cell and the EML/mbox reader):
/// - empty → [`HandleType::Other`]
/// - contains `@` → [`HandleType::Email`]
/// - digit-heavy string (digits, `+`, `-`, spaces, parentheses, dots, `#`,
///   `*`) → [`HandleType::Phone`]
/// - anything else → [`HandleType::Other`]
pub(crate) fn infer_handle_type(handle: &str) -> HandleType {
    let h = handle.trim();
    if h.is_empty() {
        return HandleType::Other;
    }
    if h.contains('@') {
        return HandleType::Email;
    }
    let has_digit = h.bytes().any(|b| b.is_ascii_digit());
    let all_phone_chars = h.bytes().all(|b| {
        b.is_ascii_digit() || matches!(b, b'+' | b'-' | b' ' | b'(' | b')' | b'.' | b'#' | b'*')
    });
    if has_digit && all_phone_chars {
        return HandleType::Phone;
    }
    HandleType::Other
}

/// Stem packaging suffix used for WhatsApp exports (`__whatsapp`).
pub(crate) fn packaging_suffix_from_stem(stem: &str) -> Option<String> {
    if stem.ends_with("__whatsapp") {
        Some("__whatsapp".into())
    } else {
        None
    }
}

/// Load attachment bytes from in-memory data or from `output_dir` + relative path.
///
/// Missing paths yield an empty buffer. IO failures return an error. The
/// loader every writer that embeds attachment bytes uses, including the
/// merged archives other crates implement.
pub fn load_attachment_bytes(att: &IrAttachment, output_dir: &Path) -> Result<Vec<u8>> {
    if let Some(b) = &att.bytes {
        return Ok(b.clone());
    }
    match read_attachment_file(att, output_dir) {
        Ok(Some(bytes)) => Ok(bytes),
        Ok(None) => Ok(Vec::new()),
        Err(err) => Err(err),
    }
}

/// Read attachment bytes from disk when the relative path exists.
///
/// Missing paths yield `Ok(None)`. IO failures return an error (strict) — callers
/// that want lenient behavior map with `.ok().flatten()`.
///
/// The relative path is validated via [`message_ir::safe_attachment_path`].
pub(crate) fn read_attachment_file(
    att: &IrAttachment,
    output_dir: &Path,
) -> Result<Option<Vec<u8>>> {
    let Some(rel) = att.path.as_deref() else {
        return Ok(None);
    };
    let path = message_ir::safe_attachment_path(output_dir, rel)?;
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    Ok(Some(bytes))
}

/// Whether `path` is a line-oriented file that was written to the end: it
/// exists, holds at least one byte, and its last byte is a newline.
///
/// Every writer here ends each record with `\n`, so a file that stops
/// anywhere else was cut off. A resumed run uses this instead of a bare
/// existence check: after a power loss the rename may have landed while the
/// bytes behind it did not, leaving an empty or truncated file under the
/// right name, and a file that merely exists would otherwise be skipped for
/// good.
#[must_use]
pub fn is_complete_file(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let Ok(meta) = file.metadata() else {
        return false;
    };
    if !meta.is_file() || meta.len() == 0 {
        return false;
    }
    let mut last = [0_u8; 1];
    file.seek(SeekFrom::End(-1)).is_ok() && file.read_exact(&mut last).is_ok() && last[0] == b'\n'
}

/// Write a file atomically: create the parent directory, write everything to
/// a `.tmp` sibling (`<file name>.tmp`), sync it to disk, and rename it over
/// `path`, so a reader never sees a half-written file.
///
/// The sync comes before the rename because a rename can reach the disk
/// ahead of the data it points at; without it a power loss can leave an
/// empty or cut-off file under the final name.
///
/// # Errors
///
/// Returns an error when the parent cannot be created, the temp file cannot
/// be created, written, or synced, or the rename fails.
pub(crate) fn write_atomic(
    path: &Path,
    write: impl FnOnce(&mut dyn Write) -> Result<()>,
) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let mut tmp_name = path
        .file_name()
        .map(|n| n.to_os_string())
        .with_context(|| format!("{} has no file name", path.display()))?;
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);
    {
        let file = File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
        let mut out = BufWriter::new(file);
        write(&mut out)?;
        out.flush()
            .with_context(|| format!("flush {}", tmp.display()))?;
        out.get_ref()
            .sync_all()
            .with_context(|| format!("sync {}", tmp.display()))?;
    }
    fs::rename(&tmp, path)
        .with_context(|| format!("rename {} → {}", tmp.display(), path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use message_ir::IrAttachment;
    use std::fs;
    use std::io::Write;

    fn att_with_path(rel: &str) -> IrAttachment {
        IrAttachment {
            path: Some(rel.into()),
            original_name: None,
            mime_type: None,
            digest_sha256: None,
            is_sticker: false,
            transcription: None,
            sticker_effect: None,
            size_bytes: None,
            missing_reason: None,
            bytes: None,
        }
    }

    #[test]
    fn a_complete_file_exists_and_ends_with_a_newline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.jsonl");
        fs::write(&path, b"{}\n{}\n").unwrap();
        assert!(is_complete_file(&path));
    }

    #[test]
    fn a_missing_empty_or_cut_off_file_is_not_complete() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_complete_file(&dir.path().join("missing.jsonl")));
        assert!(!is_complete_file(dir.path()), "a directory is not a file");

        let empty = dir.path().join("empty.jsonl");
        fs::write(&empty, b"").unwrap();
        assert!(!is_complete_file(&empty));

        let cut_off = dir.path().join("cut.jsonl");
        fs::write(&cut_off, b"{}\n{\"half").unwrap();
        assert!(!is_complete_file(&cut_off));
    }

    #[test]
    fn write_atomic_leaves_a_complete_file_and_no_temp_sibling() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.jsonl");
        write_atomic(&path, |out| {
            out.write_all(b"{}\n")?;
            Ok(())
        })
        .unwrap();
        assert!(is_complete_file(&path));
        assert!(!path.with_file_name("out.jsonl.tmp").exists());
    }

    #[test]
    fn strict_prefers_in_memory_bytes() {
        let mut att = att_with_path("attachments/missing.bin");
        att.bytes = Some(vec![1, 2, 3]);
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            load_attachment_bytes(&att, dir.path()).unwrap(),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn strict_reads_relative_path() {
        let dir = tempfile::tempdir().unwrap();
        let rel = "attachments/a.bin";
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(b"hello").unwrap();
        let att = att_with_path(rel);
        assert_eq!(load_attachment_bytes(&att, dir.path()).unwrap(), b"hello");
    }

    #[test]
    fn read_file_returns_none_for_missing() {
        let att = att_with_path("attachments/missing.bin");
        let dir = tempfile::tempdir().unwrap();
        assert!(read_attachment_file(&att, dir.path()).unwrap().is_none());
    }

    #[test]
    fn infer_handle_type_covers_email_phone_and_other() {
        use message_ir::HandleType;
        assert_eq!(infer_handle_type("alice@example.com"), HandleType::Email);
        assert_eq!(infer_handle_type("+15555550101"), HandleType::Phone);
        assert_eq!(infer_handle_type("1 (555) 555-0101"), HandleType::Phone);
        assert_eq!(infer_handle_type("alice"), HandleType::Other);
        assert_eq!(infer_handle_type(""), HandleType::Other);
    }
}
