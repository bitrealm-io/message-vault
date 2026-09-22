//! The one check that keeps an attachment's recorded path under its folder.
//!
//! Every attachment in a [`ConversationDocument`](crate::ConversationDocument)
//! carries a path relative to the export folder. That path is input: a
//! crafted CSV or JSON Lines file can name `/etc/passwd` or `../secrets`, and
//! whichever code joins it onto a folder — the format writers embedding bytes
//! into an EML, the transcode pass, the vault's import — would read outside
//! the folder. The check lives here, once, so every caller refuses the same
//! shapes with the same message.

use std::fmt;
use std::path::{Component, Path, PathBuf};

/// The message every refused path carries, so a test or a log line can
/// recognise the refusal without depending on the exact path text.
pub const UNSAFE_ATTACHMENT_PATH: &str = "unsafe attachment path";

/// An attachment path that could escape its folder, or names nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsafeAttachmentPath {
    /// The path as it was recorded, before trimming.
    pub path: String,
}

impl fmt::Display for UnsafeAttachmentPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{UNSAFE_ATTACHMENT_PATH}: {}", self.path)
    }
}

impl std::error::Error for UnsafeAttachmentPath {}

/// Resolve `rel`, an attachment's recorded relative path, under `base_dir`.
///
/// Leading and trailing whitespace is dropped and `.` components are removed,
/// so the returned path is `base_dir` followed by plain name components.
///
/// # Errors
///
/// Returns [`UnsafeAttachmentPath`] when `rel` is empty, absolute, or holds a
/// `..`, root, or drive-prefix component anywhere in it.
pub fn safe_attachment_path(base_dir: &Path, rel: &str) -> Result<PathBuf, UnsafeAttachmentPath> {
    let refuse = || UnsafeAttachmentPath {
        path: rel.to_string(),
    };
    let trimmed = rel.trim();
    if trimmed.is_empty() {
        return Err(refuse());
    }
    let mut out = PathBuf::new();
    for comp in Path::new(trimmed).components() {
        match comp {
            Component::Normal(name) => out.push(name),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(refuse());
            }
        }
    }
    if out.as_os_str().is_empty() {
        return Err(refuse());
    }
    Ok(base_dir.join(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The path check is a security boundary: a crafted `path` in a CSV or
    /// JSON export is the input, and reading `/etc/passwd` into an EML
    /// attachment is the outcome it prevents. Every rejected shape is named
    /// here, because a check that quietly starts accepting one of them
    /// breaks nothing else in the suite.
    #[test]
    fn refuses_every_escape() {
        let base = Path::new("/vault/staging");
        for rel in [
            "/etc/passwd",
            "../secrets.txt",
            "sub/../../secrets.txt",
            "..",
            "media/../../..//etc/passwd",
            "a/../../b",
            "",
            "   ",
            ".",
            "./",
        ] {
            let err = safe_attachment_path(base, rel).expect_err("must refuse {rel:?}");
            assert_eq!(err.path, rel);
            let text = err.to_string();
            assert!(
                text.starts_with(UNSAFE_ATTACHMENT_PATH),
                "{rel:?} was refused with an unexpected message: {text}"
            );
        }
    }

    /// The other half of the boundary: an ordinary relative path must still
    /// resolve, and resolve under the base directory rather than beside it.
    #[test]
    fn joins_an_ordinary_relative_path() {
        let base = Path::new("/vault/staging");
        assert_eq!(
            safe_attachment_path(base, "media/IMG_0001.jpg").unwrap(),
            Path::new("/vault/staging/media/IMG_0001.jpg")
        );
        assert_eq!(
            safe_attachment_path(base, " attachments/a.jpg ").unwrap(),
            Path::new("/vault/staging/attachments/a.jpg")
        );
        assert_eq!(
            safe_attachment_path(base, "./media/a.png").unwrap(),
            Path::new("/vault/staging/media/a.png")
        );
        // A leading `..` in the *file name* is not a parent-directory
        // component and must not be mistaken for one.
        assert_eq!(
            safe_attachment_path(base, "..hidden.jpg").unwrap(),
            Path::new("/vault/staging/..hidden.jpg")
        );
    }
}
