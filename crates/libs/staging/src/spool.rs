//! Attachment payloads written to disk as an exporter parses them.
//!
//! A backup that carries its attachments inside the file (SMS Backup &
//! Restore's base64 parts, GO SMS Pro's PDUs, SMS Backup+'s EML parts) hands
//! the exporter each payload as bytes. Parse finishes before anything is
//! staged, so an exporter that kept those bytes would hold every attachment
//! of the backup in memory at once. The spool writes each payload to a file
//! named by its SHA-256 the moment it is parsed, and the write tail reads it
//! back one file at a time.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use message_ir::IrAttachment;
use sha2::{Digest, Sha256};

use crate::write_queue::AttachmentSource;

/// Folder name of the spool inside the Staging Directory.
const SPOOL_DIR: &str = ".attachment-spool";

/// Content-addressed store of attachment payloads for one run.
///
/// The folder is emptied when the spool opens, so a run interrupted before
/// it removed its spool leaves nothing for the next run to trip over, and it
/// is removed when the spool is dropped.
#[derive(Debug)]
pub struct AttachmentSpool {
    dir: PathBuf,
    /// Size of each spooled payload by its SHA-256, for the progress totals.
    sizes: Mutex<HashMap<String, u64>>,
}

impl AttachmentSpool {
    /// Open an empty spool inside `staging_dir`. Nothing is created on disk
    /// until the first payload arrives.
    ///
    /// # Errors
    ///
    /// Returns an error when a spool left by an earlier run cannot be removed.
    pub fn open(staging_dir: &Path) -> Result<Self> {
        let dir = staging_dir.join(SPOOL_DIR);
        if dir.exists() {
            fs::remove_dir_all(&dir).with_context(|| format!("remove {}", dir.display()))?;
        }
        Ok(Self {
            dir,
            sizes: Mutex::new(HashMap::new()),
        })
    }

    /// Write `bytes` to the spool and return their lowercase hex SHA-256.
    /// A payload already spooled is not written again.
    ///
    /// # Errors
    ///
    /// Returns an error when the spool folder or the payload file cannot be
    /// written.
    pub fn put(&self, bytes: &[u8]) -> Result<String> {
        let digest = hex::encode(Sha256::digest(bytes));
        let mut sizes = self.sizes.lock().unwrap_or_else(|e| e.into_inner());
        if sizes.contains_key(&digest) {
            return Ok(digest);
        }
        fs::create_dir_all(&self.dir).with_context(|| format!("create {}", self.dir.display()))?;
        let path = self.dir.join(&digest);
        // Written under a temporary name first, so a file under a digest is
        // always the whole payload.
        let tmp = self.dir.join(format!("{digest}.tmp"));
        fs::write(&tmp, bytes).with_context(|| format!("write {}", tmp.display()))?;
        fs::rename(&tmp, &path).with_context(|| format!("rename {}", path.display()))?;
        sizes.insert(digest.clone(), bytes.len() as u64);
        Ok(digest)
    }

    /// The spooled file holding the payload with SHA-256 `digest`, when one
    /// was spooled.
    pub fn path(&self, digest: &str) -> Option<PathBuf> {
        let sizes = self.sizes.lock().unwrap_or_else(|e| e.into_inner());
        sizes.contains_key(digest).then(|| self.dir.join(digest))
    }

    /// Size of the spooled payload with SHA-256 `digest`.
    pub fn size(&self, digest: &str) -> Option<u64> {
        let sizes = self.sizes.lock().unwrap_or_else(|e| e.into_inner());
        sizes.get(digest).copied()
    }

    /// Where `att`'s bytes come from when its digest was spooled: the
    /// spooled file, with the payload's size for the progress totals.
    /// `None` for an attachment the spool does not hold.
    pub fn source(&self, att: &IrAttachment) -> Option<(AttachmentSource, Option<u64>)> {
        let digest = att.digest_sha256.as_deref()?;
        let path = self.path(digest)?;
        let size = att.size_bytes.or_else(|| self.size(digest));
        Some((AttachmentSource::Path(path), size))
    }
}

impl Drop for AttachmentSpool {
    fn drop(&mut self) {
        if self.dir.exists() {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_payload_is_on_disk_under_its_digest_until_the_spool_is_dropped() {
        let tmp = tempfile::tempdir().unwrap();
        let spool = AttachmentSpool::open(tmp.path()).unwrap();
        let digest = spool.put(b"hello").unwrap();
        assert_eq!(
            digest,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(spool.put(b"hello").unwrap(), digest, "spooled once");
        let path = spool.path(&digest).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"hello");
        assert_eq!(spool.size(&digest), Some(5));
        assert!(spool.path("0000").is_none());
        drop(spool);
        assert!(!tmp.path().join(SPOOL_DIR).exists());
    }

    #[test]
    fn opening_a_spool_empties_one_an_earlier_run_left() {
        let tmp = tempfile::tempdir().unwrap();
        let left = tmp.path().join(SPOOL_DIR);
        fs::create_dir_all(&left).unwrap();
        fs::write(left.join("stale"), b"x").unwrap();
        let _spool = AttachmentSpool::open(tmp.path()).unwrap();
        assert!(!left.exists());
    }

    #[test]
    fn a_spooled_attachment_is_read_from_its_file() {
        let tmp = tempfile::tempdir().unwrap();
        let spool = AttachmentSpool::open(tmp.path()).unwrap();
        let digest = spool.put(b"photo").unwrap();
        let mut att = IrAttachment {
            path: None,
            original_name: Some("photo.jpg".into()),
            mime_type: Some("image/jpeg".into()),
            digest_sha256: Some(digest.clone()),
            is_sticker: false,
            transcription: None,
            sticker_effect: None,
            size_bytes: None,
            missing_reason: None,
            bytes: None,
        };
        let (source, size) = spool.source(&att).unwrap();
        assert!(matches!(source, AttachmentSource::Path(p) if p == spool.path(&digest).unwrap()));
        assert_eq!(size, Some(5));
        att.digest_sha256 = Some("0000".into());
        assert!(spool.source(&att).is_none(), "not spooled");
    }
}
