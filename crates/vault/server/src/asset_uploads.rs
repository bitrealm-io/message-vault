//! Multipart (chunked) asset upload staging for Cloudflare-sized HTTP bodies.
//!
//! Staging layout: `{assets}/.incoming/{sha256}/{upload_id}/part-NNNN` + `manifest.json`.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::assets::{self, StoredAsset};

/// Default part size advertised to clients (under Cloudflare ~100 MiB).
pub const DEFAULT_PART_SIZE: usize = 64 * 1024 * 1024;
/// Default max object size for one asset (single PUT or multipart).
pub const DEFAULT_MAX_BYTES: u64 = 512 * 1024 * 1024;
/// Drop abandoned `.incoming` sessions older than this (24h).
const STALE_INCOMING_SECS: u64 = 24 * 60 * 60;

/// Limits for multipart sessions (from `[server]` config; optional env override).
#[derive(Debug, Clone, Copy)]
pub struct UploadLimits {
    /// Multipart part size advertised to clients.
    pub part_size: usize,
    /// Max total size for one asset.
    pub max_bytes: u64,
}

impl Default for UploadLimits {
    fn default() -> Self {
        Self {
            part_size: DEFAULT_PART_SIZE,
            max_bytes: DEFAULT_MAX_BYTES,
        }
    }
}

impl UploadLimits {
    /// Build limits from config. `VAULT_ASSET_PART_SIZE` overrides part size when set
    /// to a value in `1..=part_size` (tests / smoke).
    pub fn resolve(part_size: usize, max_bytes: u64) -> Self {
        let part_size = std::env::var("VAULT_ASSET_PART_SIZE")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|&n| n >= 1 && n <= part_size.max(1))
            .unwrap_or(part_size.max(1));
        let max_bytes = max_bytes.max(part_size as u64);
        Self {
            part_size,
            max_bytes,
        }
    }
}

/// `manifest.json` of an in-progress multipart upload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadManifest {
    /// SHA-256 fingerprint of the final assembled file.
    pub sha256: String,
    /// Total byte size of the assembled file.
    pub bytes: u64,
    /// Part size this upload was started with.
    pub part_size: usize,
    /// MIME type of the file, when the client provided one.
    #[serde(default)]
    pub mime: Option<String>,
    /// Part numbers received so far (0-based).
    #[serde(default)]
    pub received: BTreeSet<u32>,
}

/// Response to a multipart upload start.
#[derive(Debug, Clone)]
pub struct StartUpload {
    /// Upload session id, echoed on every part and the complete request.
    pub upload_id: String,
    /// Part size to use for this upload.
    pub part_size: usize,
}

/// A fresh upload id: the current time in nanoseconds, hex-encoded.
fn new_upload_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}")
}

/// Reject path separators and non-hex ids before joining into `.incoming/…`.
///
/// # Errors
///
/// Returns an error when the id is empty, too long, or not hex.
pub fn require_upload_id(upload_id: &str) -> Result<String> {
    let id = upload_id.trim();
    if id.is_empty() || id.len() > 64 {
        bail!("invalid upload_id");
    }
    if !id.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("invalid upload_id");
    }
    Ok(id.to_ascii_lowercase())
}

/// Folder for one multipart upload: `{assets}/.incoming/{sha256}/{upload_id}`.
pub fn session_dir(assets_root: &Path, sha256: &str, upload_id: &str) -> PathBuf {
    assets_root.join(".incoming").join(sha256).join(upload_id)
}

/// Path of the session's manifest file.
fn manifest_path(session: &Path) -> PathBuf {
    session.join("manifest.json")
}

/// Path of one uploaded part (1-based, zero-padded).
fn part_path(session: &Path, part: u32) -> PathBuf {
    session.join(format!("part-{part:04}"))
}

/// How many parts a file of `bytes` needs at `part_size`.
fn expected_part_count(bytes: u64, part_size: usize) -> u32 {
    if bytes == 0 {
        return 0;
    }
    let ps = part_size as u64;
    bytes.div_ceil(ps) as u32
}

/// The size of part `part` (1-based); zero when the part number is out of range.
fn expected_part_len(bytes: u64, part_size: usize, part: u32) -> u64 {
    let count = expected_part_count(bytes, part_size);
    if part == 0 || part > count {
        return 0;
    }
    let start = (part as u64 - 1) * part_size as u64;
    (bytes - start).min(part_size as u64)
}

/// Parse the session manifest.
fn read_manifest(session: &Path) -> Result<UploadManifest> {
    let path = manifest_path(session);
    let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))
}

/// Write the manifest through a temp file and a rename, so a crash never leaves a half-written manifest.
fn write_manifest(session: &Path, manifest: &UploadManifest) -> Result<()> {
    let path = manifest_path(session);
    let tmp = session.join("manifest.json.tmp");
    let text = serde_json::to_string_pretty(manifest)?;
    fs::write(&tmp, text).with_context(|| format!("write {}", tmp.display()))?;
    fs::rename(&tmp, &path)
        .with_context(|| format!("rename {} → {}", tmp.display(), path.display()))?;
    Ok(())
}

/// Exclusive lock for manifest read-modify-write (concurrent part uploads).
#[derive(Debug)]
struct ManifestLock {
    _file: File,
}

/// Take the session's file lock so two part uploads cannot rewrite the manifest at once.
fn lock_session(session: &Path) -> Result<ManifestLock> {
    let path = session.join("manifest.lock");
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    file.try_lock_exclusive()
        .map_err(|_| anyhow::anyhow!("failed to lock {}", path.display()))?;
    Ok(ManifestLock { _file: file })
}

/// Canonical extension for an uploaded blob's MIME type, from the shared
/// table in [`media`]; empty for missing or unrecognized MIME types.
fn ext_for_mime(mime: Option<&str>) -> String {
    mime.and_then(media::ext_for_mime).unwrap_or("").to_string()
}

/// Start a chunked upload session. Returns `already_present` asset when the blob exists.
pub fn start_upload(
    assets_root: &Path,
    sha256: &str,
    bytes: u64,
    mime: Option<&str>,
    limits: UploadLimits,
) -> Result<(Option<StoredAsset>, Option<StartUpload>)> {
    let sha = assets::require_sha256(sha256)?;
    if bytes == 0 {
        bail!("bytes must be > 0");
    }
    if bytes > limits.max_bytes {
        bail!(
            "object exceeds {} byte server limit ({} MiB)",
            limits.max_bytes,
            limits.max_bytes / message_ir::MIB
        );
    }
    // Best-effort: drop abandoned multipart staging so disk does not grow forever.
    let _ = assets::gc_stale_incoming(assets_root, STALE_INCOMING_SECS);

    if let Some(existing) = assets::lookup_by_sha256(assets_root, &sha) {
        return Ok((Some(existing), None));
    }

    let part_size = limits.part_size;
    let upload_id = new_upload_id();
    let session = session_dir(assets_root, &sha, &upload_id);
    fs::create_dir_all(&session).with_context(|| format!("mkdir {}", session.display()))?;
    let manifest = UploadManifest {
        sha256: sha,
        bytes,
        part_size,
        mime: mime.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        received: BTreeSet::new(),
    };
    write_manifest(&session, &manifest)?;
    Ok((
        None,
        Some(StartUpload {
            upload_id,
            part_size,
        }),
    ))
}

/// Write (or overwrite) one part. `body` is the full part payload.
pub fn put_part(
    assets_root: &Path,
    sha256: &str,
    upload_id: &str,
    part: u32,
    body: &[u8],
) -> Result<u64> {
    let sha = assets::require_sha256(sha256)?;
    let upload_id = require_upload_id(upload_id)?;
    if part == 0 {
        bail!("part number must be >= 1");
    }
    let session = session_dir(assets_root, &sha, &upload_id);
    if !session.is_dir() {
        bail!("upload session not found");
    }
    let _lock = lock_session(&session)?;
    let mut manifest = read_manifest(&session)?;
    if manifest.sha256 != sha {
        bail!("upload session sha256 mismatch");
    }
    if body.len() > manifest.part_size {
        bail!(
            "part body {} bytes exceeds session part_size {}",
            body.len(),
            manifest.part_size
        );
    }
    let count = expected_part_count(manifest.bytes, manifest.part_size);
    if part > count {
        bail!("part {part} out of range (expected 1..={count})");
    }
    let expect = expected_part_len(manifest.bytes, manifest.part_size, part);
    if body.len() as u64 != expect {
        bail!(
            "part {part} length {} does not match expected {expect}",
            body.len()
        );
    }

    let path = part_path(&session, part);
    fs::write(&path, body).with_context(|| format!("write {}", path.display()))?;
    manifest.received.insert(part);
    write_manifest(&session, &manifest)?;
    Ok(body.len() as u64)
}

/// Concatenate parts, check the claimed SHA-256 fingerprint, and install into
/// the asset store.
///
/// Always hashes the assembled object and rejects fingerprint mismatches.
///
/// # Errors
///
/// Returns an error when the upload session is missing, a part is missing, the
/// fingerprint does not match, or the file cannot be stored.
pub fn complete_upload(
    assets_root: &Path,
    sha256: &str,
    upload_id: &str,
) -> Result<(StoredAsset, bool)> {
    let sha = assets::require_sha256(sha256)?;
    let upload_id = require_upload_id(upload_id)?;
    let session = session_dir(assets_root, &sha, &upload_id);
    if !session.is_dir() {
        bail!("upload session not found");
    }
    let _lock = lock_session(&session)?;
    let manifest = read_manifest(&session)?;
    if manifest.sha256 != sha {
        bail!("upload session sha256 mismatch");
    }
    let count = expected_part_count(manifest.bytes, manifest.part_size);
    if count == 0 {
        bail!("empty upload");
    }
    for n in 1..=count {
        if !manifest.received.contains(&n) {
            bail!("missing part {n} of {count}");
        }
        let path = part_path(&session, n);
        if !path.is_file() {
            bail!("missing part file {n}");
        }
    }

    let ext = ext_for_mime(manifest.mime.as_deref());
    let assembled = session.join(format!("assembled{ext}"));
    let mut total = 0u64;
    {
        let mut out =
            File::create(&assembled).with_context(|| format!("create {}", assembled.display()))?;
        let mut buf = vec![0u8; assets::COPY_BUFFER_BYTES];
        for n in 1..=count {
            let path = part_path(&session, n);
            let mut file = File::open(&path).with_context(|| format!("open {}", path.display()))?;
            loop {
                let nread = file.read(&mut buf)?;
                if nread == 0 {
                    break;
                }
                out.write_all(&buf[..nread])?;
                total += nread as u64;
            }
        }
        out.flush()?;
        if total != manifest.bytes {
            let _ = fs::remove_file(&assembled);
            bail!(
                "assembled size {total} does not match declared {}",
                manifest.bytes
            );
        }
    }
    let result = assets::store_verified(
        &assembled,
        &sha,
        assets_root,
        manifest.mime.as_deref(),
        true,
        false,
    );
    // Always drop the session directory after complete attempt.
    drop(_lock);
    let _ = fs::remove_dir_all(&session);
    result
}

/// Abort and delete staging for an upload session.
pub fn abort_upload(assets_root: &Path, sha256: &str, upload_id: &str) -> Result<()> {
    let sha = assets::require_sha256(sha256)?;
    let upload_id = require_upload_id(upload_id)?;
    let session = session_dir(assets_root, &sha, &upload_id);
    if session.exists() {
        fs::remove_dir_all(&session).with_context(|| format!("remove {}", session.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn hash_bytes(data: &[u8]) -> String {
        crate::assets::sha256_hex(data)
    }

    #[test]
    fn multipart_roundtrip() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let data = b"hello-multipart-asset-bytes!!";
        let sha = hash_bytes(data);

        let upload_id = "aabb0011";
        let session = session_dir(root, &sha, upload_id);
        fs::create_dir_all(&session).unwrap();
        let part_size = 10usize;
        let mut manifest = UploadManifest {
            sha256: sha.clone(),
            bytes: data.len() as u64,
            part_size,
            mime: Some("text/plain".into()),
            received: BTreeSet::new(),
        };
        write_manifest(&session, &manifest).unwrap();

        let count = expected_part_count(manifest.bytes, part_size);
        for n in 1..=count {
            let start = (n as usize - 1) * part_size;
            let end = (start + part_size).min(data.len());
            let chunk = &data[start..end];
            let path = part_path(&session, n);
            fs::write(&path, chunk).unwrap();
            manifest.received.insert(n);
        }
        write_manifest(&session, &manifest).unwrap();

        let (stored, already) = complete_upload(root, &sha, upload_id).unwrap();
        assert!(!already);
        assert_eq!(stored.sha256, sha);
        assert!(root.join(&stored.assets_path).is_file());
        assert!(!session.exists());
        assert_eq!(fs::read(root.join(&stored.assets_path)).unwrap(), data);
    }

    #[test]
    fn complete_rejects_hash_mismatch() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let data = b"abc123";
        let wrong_sha = hash_bytes(b"other");
        let upload_id = "badc0de1";
        let session = session_dir(root, &wrong_sha, upload_id);
        fs::create_dir_all(&session).unwrap();
        let mut manifest = UploadManifest {
            sha256: wrong_sha.clone(),
            bytes: data.len() as u64,
            part_size: 64,
            mime: None,
            received: BTreeSet::new(),
        };
        fs::write(part_path(&session, 1), data).unwrap();
        manifest.received.insert(1);
        write_manifest(&session, &manifest).unwrap();

        let err = complete_upload(root, &wrong_sha, upload_id).unwrap_err();
        assert!(err.to_string().contains("sha256 mismatch"));
        assert!(!session.exists());
    }

    #[test]
    fn complete_rejects_hash_mismatch_even_above_threshold() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let data = b"large-enough-to-skip";
        // Claimed fingerprint deliberately wrong: completion must still hash and reject.
        let claimed_sha = "a".repeat(64);
        let upload_id = "aabbccdd";
        let session = session_dir(root, &claimed_sha, upload_id);
        fs::create_dir_all(&session).unwrap();
        let mut manifest = UploadManifest {
            sha256: claimed_sha.clone(),
            bytes: data.len() as u64,
            part_size: 64,
            mime: None,
            received: BTreeSet::new(),
        };
        fs::write(part_path(&session, 1), data).unwrap();
        manifest.received.insert(1);
        write_manifest(&session, &manifest).unwrap();

        let err = complete_upload(root, &claimed_sha, upload_id).unwrap_err();
        assert!(
            err.to_string().contains("sha256 mismatch"),
            "expected mismatch, got: {err}"
        );
        assert!(!session.exists());
    }

    #[test]
    fn require_upload_id_rejects_path_components() {
        assert!(require_upload_id("..").is_err());
        assert!(require_upload_id("../x").is_err());
        assert!(require_upload_id("a/b").is_err());
        assert!(require_upload_id("").is_err());
        assert!(require_upload_id("deadbeef").is_ok());
        assert!(require_upload_id(&"a".repeat(64)).is_ok());
        assert!(require_upload_id(&"a".repeat(65)).is_err());
    }

    #[test]
    fn abort_rejects_parent_upload_id() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let sha = "b".repeat(64);
        let parent = root.join(".incoming").join(&sha);
        fs::create_dir_all(parent.join("keep")).unwrap();
        fs::write(parent.join("keep").join("marker"), b"x").unwrap();

        let err = abort_upload(root, &sha, "..").unwrap_err();
        assert!(err.to_string().contains("upload_id"));
        assert!(parent.join("keep").join("marker").is_file());
    }

    #[test]
    fn complete_rejects_missing_part() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let data = b"0123456789abcdef";
        let sha = hash_bytes(data);
        let upload_id = "11112222";
        let session = session_dir(root, &sha, upload_id);
        fs::create_dir_all(&session).unwrap();
        let part_size = 8usize;
        let mut manifest = UploadManifest {
            sha256: sha.clone(),
            bytes: data.len() as u64,
            part_size,
            mime: None,
            received: BTreeSet::new(),
        };
        // Only write part 1 of 2.
        fs::write(part_path(&session, 1), &data[..8]).unwrap();
        manifest.received.insert(1);
        write_manifest(&session, &manifest).unwrap();

        let err = complete_upload(root, &sha, upload_id).unwrap_err();
        assert!(err.to_string().contains("missing part"));
        // Incomplete sessions are kept so the client can resume missing parts.
        assert!(session.exists());
    }

    #[test]
    fn put_part_and_complete_via_api() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let data = b"abcdefghijklmnopqrstuvwxyz";
        let sha = hash_bytes(data);
        // Force tiny parts for this process.
        // SAFETY: single-threaded test process; no concurrent env readers.
        unsafe {
            std::env::set_var("VAULT_ASSET_PART_SIZE", "10");
        }
        let limits = UploadLimits::resolve(DEFAULT_PART_SIZE, DEFAULT_MAX_BYTES);
        let (existing, start) =
            start_upload(root, &sha, data.len() as u64, Some("text/plain"), limits).unwrap();
        assert!(existing.is_none());
        let start = start.expect("upload started");
        assert_eq!(start.part_size, 10);

        let mut offset = 0usize;
        let mut part = 1u32;
        while offset < data.len() {
            let end = (offset + start.part_size).min(data.len());
            put_part(root, &sha, &start.upload_id, part, &data[offset..end]).unwrap();
            offset = end;
            part += 1;
        }
        let (stored, already) = complete_upload(root, &sha, &start.upload_id).unwrap();
        assert!(!already);
        assert_eq!(stored.sha256, sha);
        unsafe {
            std::env::remove_var("VAULT_ASSET_PART_SIZE");
        }
    }

    #[test]
    fn start_returns_existing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let data = b"already-here";
        let sha = hash_bytes(data);
        let path = root.join(&sha[..2]).join(&sha);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, data).unwrap();

        let (existing, start) =
            start_upload(root, &sha, data.len() as u64, None, UploadLimits::default()).unwrap();
        assert!(existing.is_some());
        assert!(start.is_none());
    }

    #[test]
    fn start_rejects_over_max_bytes() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let sha = "a".repeat(64);
        let limits = UploadLimits {
            part_size: 1024,
            max_bytes: 2048,
        };
        let err = start_upload(root, &sha, 4096, None, limits).unwrap_err();
        assert!(err.to_string().contains("server limit"));
    }

    #[test]
    fn manifest_lock_is_exclusive() {
        let dir = tempdir().unwrap();
        let sha = "c".repeat(64);
        let session = session_dir(dir.path(), &sha, "locktest01");
        fs::create_dir_all(&session).unwrap();

        let _held = lock_session(&session).unwrap();
        let err = lock_session(&session).unwrap_err();
        assert!(
            err.to_string().contains("failed to lock"),
            "expected lock failure, got: {err}"
        );
    }
}
