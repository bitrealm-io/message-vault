//! Multipart (chunked) asset upload staging for Cloudflare-sized HTTP bodies.
//!
//! Staging layout: `{assets}/.incoming/{sha256}/{upload_id}/part-NNNN` + `manifest.json`.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::assets_api::{self, AssetError, Sha256, StoredAsset};

/// Default part size advertised to clients (under Cloudflare ~100 MiB).
pub const DEFAULT_PART_SIZE: usize = 64 * 1024 * 1024;
/// Drop abandoned `.incoming` sessions older than this (24h).
const STALE_INCOMING_SECS: u64 = 24 * 60 * 60;

/// Limits for one upload: the attachment size limit from the Server Settings
/// as it is when the upload starts, and the part size worked out from it and
/// the `[server]` config ([`UploadLimits::within`]).
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
            max_bytes: crate::db::server_settings::DEFAULT_ASSET_MAX_BYTES,
        }
    }
}

impl UploadLimits {
    /// The limits in force: the attachment size limit as the owner set it,
    /// and a part size that is the configured one or the limit, whichever is
    /// smaller. A part is never larger than the limit, so every limit the
    /// owner can set is one an upload can complete under, and no pairing of
    /// the setting and the config file is an error.
    pub fn within(configured_part_size: usize, max_bytes: u64) -> Self {
        let part_size = usize::try_from(max_bytes)
            .map_or(configured_part_size, |limit| {
                configured_part_size.min(limit)
            })
            .max(1);
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
    /// Part numbers received so far (1-based).
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
        .map_or(0, |d| d.as_nanos());
    format!("{nanos:x}")
}

/// Reject path separators and non-hex ids before joining into `.incoming/…`.
///
/// # Errors
///
/// Returns [`AssetError::Invalid`] when the id is empty, too long, or not hex.
pub fn require_upload_id(upload_id: &str) -> Result<String, AssetError> {
    let id = upload_id.trim();
    if id.is_empty() || id.len() > 64 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(AssetError::Invalid("invalid upload_id".into()));
    }
    Ok(id.to_ascii_lowercase())
}

/// Folder for one multipart upload: `{assets}/.incoming/{sha256}/{upload_id}`.
pub fn session_dir(assets_root: &Path, sha256: &Sha256, upload_id: &str) -> PathBuf {
    assets_root
        .join(".incoming")
        .join(sha256.as_str())
        .join(upload_id)
}

/// Path of the session's manifest file.
fn manifest_path(session: &Path) -> PathBuf {
    session.join("manifest.json")
}

/// Path of one uploaded part (1-based, zero-padded).
fn part_path(session: &Path, part: u32) -> PathBuf {
    session.join(format!("part-{part:04}"))
}

/// How many parts a file of `bytes` needs at `part_size`: none for the empty
/// file.
fn expected_part_count(bytes: u64, part_size: usize) -> u32 {
    bytes.div_ceil(part_size as u64) as u32
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

/// The manifest of an upload that a request names by `sha`, refusing an
/// upload that was started for another fingerprint.
fn read_manifest_for(session: &Path, sha: &Sha256) -> Result<UploadManifest, AssetError> {
    let manifest = read_manifest(session)?;
    if manifest.sha256 != sha.as_str() {
        return Err(AssetError::Invalid(
            "the upload was started for a different SHA-256".into(),
        ));
    }
    Ok(manifest)
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
///
/// The lock is not waited for. A request refused because another request
/// holds it is told so, because its bytes may well be right, and sending
/// them again once the other request finishes succeeds.
fn lock_session(session: &Path) -> Result<ManifestLock, AssetError> {
    let path = session.join("manifest.lock");
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    file.try_lock_exclusive().map_err(|_| AssetError::Locked)?;
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
    sha: &Sha256,
    bytes: u64,
    mime: Option<&str>,
    limits: UploadLimits,
) -> Result<(Option<StoredAsset>, Option<StartUpload>), AssetError> {
    if bytes > limits.max_bytes {
        return Err(AssetError::Invalid(format!(
            "object exceeds {} byte server limit ({} MiB)",
            limits.max_bytes,
            limits.max_bytes / message_ir::MIB
        )));
    }
    // Best-effort: drop abandoned multipart staging so disk does not grow forever.
    let _ = assets_api::gc_stale_incoming(assets_root, STALE_INCOMING_SECS);

    if let Some(existing) = assets_api::lookup_by_sha256(assets_root, sha) {
        return Ok((Some(existing), None));
    }

    let part_size = limits.part_size;
    let upload_id = new_upload_id();
    let session = session_dir(assets_root, sha, &upload_id);
    fs::create_dir_all(&session).with_context(|| format!("mkdir {}", session.display()))?;
    let manifest = UploadManifest {
        sha256: sha.to_string(),
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

/// The part size an upload started with, from its manifest. The part route
/// caps a body at this size, not at the part size the Server Settings give
/// now, so an upload in progress keeps the limits it started with when the
/// owner lowers the attachment size limit.
///
/// # Errors
///
/// Returns [`AssetError::Invalid`] when the upload id is invalid,
/// [`AssetError::UploadNotFound`] when no such upload is in progress, and
/// [`AssetError::Internal`] when its manifest is unreadable.
pub fn session_part_size(
    assets_root: &Path,
    sha: &Sha256,
    upload_id: &str,
) -> Result<usize, AssetError> {
    let session = existing_session(assets_root, sha, upload_id)?;
    let manifest = read_manifest(&session).map_err(|e| gone_if_removed(&session, e.into()))?;
    Ok(manifest.part_size)
}

/// An upload's folder can be removed while a request to it runs: by the
/// upload's completion, by an abort, or by the sweep of stale uploads. A
/// failure to read or write its files after that is an upload that is gone,
/// not a server that cannot store the file.
///
/// A removal deletes the folder's files one by one before the folder, so the
/// folder can outlive the upload. The manifest is in every live upload, so
/// its absence is what says the upload is gone.
fn gone_if_removed(session: &Path, err: AssetError) -> AssetError {
    match err {
        AssetError::Internal(_) if upload_is_gone(session) => AssetError::UploadNotFound,
        err => err,
    }
}

/// Whether the upload in `session` has been removed, or is being removed:
/// its manifest is gone, whether or not its folder is.
fn upload_is_gone(session: &Path) -> bool {
    !manifest_path(session).is_file()
}

/// [`gone_if_removed`] for work that reads and writes only the upload's own
/// files. A file it does not find is one a removal took, even while the
/// removal has not reached the manifest yet.
fn gone_if_session_file_removed(session: &Path, err: AssetError) -> AssetError {
    match err {
        AssetError::Internal(e) if is_not_found(&e) => AssetError::UploadNotFound,
        err => gone_if_removed(session, err),
    }
}

/// Whether an I/O error somewhere in `err` is a file or folder not found.
fn is_not_found(err: &anyhow::Error) -> bool {
    err.chain()
        .filter_map(|cause| cause.downcast_ref::<std::io::Error>())
        .any(|io| io.kind() == std::io::ErrorKind::NotFound)
}

/// The folder of an upload in progress.
fn existing_session(
    assets_root: &Path,
    sha: &Sha256,
    upload_id: &str,
) -> Result<PathBuf, AssetError> {
    let upload_id = require_upload_id(upload_id)?;
    let session = session_dir(assets_root, sha, &upload_id);
    if !session.is_dir() {
        return Err(AssetError::UploadNotFound);
    }
    Ok(session)
}

/// Write (or overwrite) one part. `body` is the full part payload.
pub fn put_part(
    assets_root: &Path,
    sha: &Sha256,
    upload_id: &str,
    part: u32,
    body: &[u8],
) -> Result<u64, AssetError> {
    if part == 0 {
        return Err(AssetError::Invalid("part number must be >= 1".into()));
    }
    let session = existing_session(assets_root, sha, upload_id)?;
    // The folder existed when the request arrived, and may be removed while
    // it runs.
    write_part(&session, sha, part, body).map_err(|e| gone_if_session_file_removed(&session, e))
}

/// [`put_part`] on the folder of its upload.
fn write_part(session: &Path, sha: &Sha256, part: u32, body: &[u8]) -> Result<u64, AssetError> {
    let _lock = lock_session(session)?;
    let mut manifest = read_manifest_for(session, sha)?;
    if body.len() > manifest.part_size {
        return Err(AssetError::Invalid(format!(
            "part body {} bytes exceeds session part_size {}",
            body.len(),
            manifest.part_size
        )));
    }
    let count = expected_part_count(manifest.bytes, manifest.part_size);
    if part > count {
        return Err(AssetError::Invalid(format!(
            "part {part} out of range (expected 1..={count})"
        )));
    }
    let expect = expected_part_len(manifest.bytes, manifest.part_size, part);
    if body.len() as u64 != expect {
        return Err(AssetError::Invalid(format!(
            "part {part} length {} does not match expected {expect}",
            body.len()
        )));
    }

    let path = part_path(session, part);
    fs::write(&path, body).with_context(|| format!("write {}", path.display()))?;
    manifest.received.insert(part);
    write_manifest(session, &manifest)?;
    Ok(body.len() as u64)
}

/// Concatenate parts, check the claimed SHA-256 fingerprint, and install into
/// the asset store.
///
/// Always hashes the assembled object and rejects fingerprint mismatches.
///
/// # Errors
///
/// Returns [`AssetError::UploadNotFound`] when the upload is missing or is
/// removed while it completes,
/// [`AssetError::Invalid`] when a part never arrived,
/// [`AssetError::Mismatch`] when the fingerprint does not match, and
/// [`AssetError::Internal`] when the file cannot be stored.
pub fn complete_upload(
    assets_root: &Path,
    sha: &Sha256,
    upload_id: &str,
) -> Result<(StoredAsset, bool), AssetError> {
    let session = existing_session(assets_root, sha, upload_id)?;
    complete_session(assets_root, &session, sha)
}

/// [`complete_upload`] on a session folder that existed when the request
/// arrived, and may be removed while it runs. Every failure is checked
/// against a removed folder before this completion removes it. Storing the
/// file also writes to the asset store, where a file not found is a fault of
/// the server, so only a missing manifest says the upload is gone there.
fn complete_session(
    assets_root: &Path,
    session: &Path,
    sha: &Sha256,
) -> Result<(StoredAsset, bool), AssetError> {
    let gone = |e| gone_if_session_file_removed(session, e);
    let _lock = lock_session(session).map_err(gone)?;
    let (manifest, assembled) = assemble(session, sha).map_err(gone)?;
    let result = assets_api::store_verified(
        &assembled,
        sha,
        assets_root,
        manifest.mime.as_deref(),
        true,
        false,
    )
    .map_err(|e| gone_if_removed(session, e));
    // Always drop the session directory after complete attempt.
    drop(_lock);
    let _ = fs::remove_dir_all(session);
    result
}

/// Join an upload's parts into one file in its session folder, checking
/// that every part arrived and that the total is the size the upload
/// declared. The caller holds the session's lock.
fn assemble(session: &Path, sha: &Sha256) -> Result<(UploadManifest, PathBuf), AssetError> {
    let manifest = read_manifest_for(session, sha)?;
    // The empty file has no parts: it completes with none and is checked
    // against its fingerprint like any other file.
    let count = expected_part_count(manifest.bytes, manifest.part_size);
    for n in 1..=count {
        if !manifest.received.contains(&n) || !part_path(session, n).is_file() {
            // A part file gone with the manifest is one a removal took. One
            // gone while the manifest stays is a part to send again.
            if upload_is_gone(session) {
                return Err(AssetError::UploadNotFound);
            }
            return Err(AssetError::Invalid(format!("missing part {n} of {count}")));
        }
    }

    let ext = ext_for_mime(manifest.mime.as_deref());
    let assembled = session.join(format!("assembled{ext}"));
    let mut total = 0u64;
    {
        let mut out =
            File::create(&assembled).with_context(|| format!("create {}", assembled.display()))?;
        let mut buf = vec![0u8; assets_api::COPY_BUFFER_BYTES];
        for n in 1..=count {
            let path = part_path(session, n);
            let mut file = File::open(&path).with_context(|| format!("open {}", path.display()))?;
            loop {
                let nread = file
                    .read(&mut buf)
                    .with_context(|| format!("read {}", path.display()))?;
                if nread == 0 {
                    break;
                }
                out.write_all(&buf[..nread])
                    .with_context(|| format!("write {}", assembled.display()))?;
                total += nread as u64;
            }
        }
        out.flush()
            .with_context(|| format!("write {}", assembled.display()))?;
        if total != manifest.bytes {
            let _ = fs::remove_file(&assembled);
            return Err(AssetError::Invalid(format!(
                "assembled size {total} does not match declared {}",
                manifest.bytes
            )));
        }
    }
    Ok((manifest, assembled))
}

/// Abort an upload and delete its folder. An upload that is already gone is
/// aborted too.
///
/// # Errors
///
/// Returns [`AssetError::Invalid`] when the upload id is invalid,
/// [`AssetError::Locked`] when a part or completion for the upload is still
/// running, and [`AssetError::Internal`] when the folder cannot be removed.
pub fn abort_upload(assets_root: &Path, sha: &Sha256, upload_id: &str) -> Result<(), AssetError> {
    let upload_id = require_upload_id(upload_id)?;
    let session = session_dir(assets_root, sha, &upload_id);
    if !session.exists() {
        return Ok(());
    }
    // A part or completion still running holds the lock, and emptying the
    // folder under it would fail it and leave files behind. The lock is
    // dropped before the removal, because Windows does not delete a file
    // that is open.
    match lock_session(&session) {
        Ok(lock) => drop(lock),
        Err(AssetError::Internal(_)) if !session.exists() => return Ok(()),
        Err(err) => return Err(err),
    }
    match fs::remove_dir_all(&session) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        // A request that took the lock after this abort let go of it is
        // writing into the folder: it made a file the removal did not see,
        // or, on Windows, holds one open that cannot be deleted.
        Err(e)
            if e.kind() == std::io::ErrorKind::DirectoryNotEmpty
                || (cfg!(windows) && e.kind() == std::io::ErrorKind::PermissionDenied) =>
        {
            Err(AssetError::Locked)
        }
        Err(e) => Err(anyhow::Error::new(e)
            .context(format!("remove {}", session.display()))
            .into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn hash_bytes(data: &[u8]) -> Sha256 {
        Sha256::of_bytes(data)
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
            sha256: sha.to_string(),
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
        assert_eq!(stored.sha256, sha.as_str());
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
            sha256: wrong_sha.to_string(),
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
    fn complete_rejects_a_made_up_fingerprint() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let data = b"large-enough-to-skip";
        // Claimed fingerprint deliberately wrong: completion must still hash and reject.
        let claimed_sha = Sha256::parse(&"a".repeat(64)).unwrap();
        let upload_id = "aabbccdd";
        let session = session_dir(root, &claimed_sha, upload_id);
        fs::create_dir_all(&session).unwrap();
        let mut manifest = UploadManifest {
            sha256: claimed_sha.to_string(),
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
        let sha = Sha256::parse(&"b".repeat(64)).unwrap();
        let parent = root.join(".incoming").join(sha.as_str());
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
            sha256: sha.to_string(),
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
        // Tiny parts, set on this test's own limits so no other test sees them.
        let limits = UploadLimits::within(10, 1024);
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
        assert_eq!(stored.sha256, sha.as_str());
    }

    #[test]
    fn start_returns_existing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let data = b"already-here";
        let sha = hash_bytes(data);
        let path = root.join(&sha.as_str()[..2]).join(sha.as_str());
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
        let sha = Sha256::parse(&"a".repeat(64)).unwrap();
        let limits = UploadLimits {
            part_size: 1024,
            max_bytes: 2048,
        };
        let err = start_upload(root, &sha, 4096, None, limits).unwrap_err();
        assert!(err.to_string().contains("server limit"));
    }

    /// The limit is the owner's and is never moved. A configured part size
    /// above it is brought down to it; one below it is used as configured.
    #[test]
    fn a_part_is_never_larger_than_the_limit() {
        let small_limit = UploadLimits::within(4096, 1024);
        assert_eq!((small_limit.part_size, small_limit.max_bytes), (1024, 1024));

        let small_part = UploadLimits::within(10, 1024);
        assert_eq!((small_part.part_size, small_part.max_bytes), (10, 1024));
    }

    /// An upload of the five bytes `hello` in one part, started in a new
    /// folder: the folder, the fingerprint, the upload id and its session.
    fn started_upload() -> (tempfile::TempDir, Sha256, String, PathBuf) {
        let dir = tempdir().unwrap();
        let sha = Sha256::of_bytes(b"hello");
        let limits = UploadLimits {
            part_size: 1024,
            max_bytes: 2048,
        };
        let (_, start) = start_upload(dir.path(), &sha, 5, None, limits).unwrap();
        let upload_id = start.unwrap().upload_id;
        let session = session_dir(dir.path(), &sha, &upload_id);
        (dir, sha, upload_id, session)
    }

    /// A completion that passed the check for its upload, whose folder an
    /// abort or the stale sweep then removed, finds the upload gone: the
    /// client's own state, not a server that cannot store the file.
    #[test]
    fn a_completion_for_an_upload_removed_under_it_finds_no_upload() {
        let (dir, sha, _, session) = started_upload();
        fs::remove_dir_all(&session).unwrap();

        // `complete_upload` checks the folder before it gets here, so the
        // removal between that check and the work is staged by calling the
        // work after the check directly.
        let err = complete_session(dir.path(), &session, &sha).unwrap_err();
        assert!(matches!(err, AssetError::UploadNotFound), "{err}");
    }

    /// A removal deletes an upload's files before its folder. A request that
    /// arrives while the manifest is gone and the folder is not finds the
    /// upload gone too.
    #[test]
    fn a_request_to_an_upload_part_way_through_its_removal_finds_no_upload() {
        let (dir, sha, upload_id, session) = started_upload();
        fs::remove_file(manifest_path(&session)).unwrap();

        let err = put_part(dir.path(), &sha, &upload_id, 1, b"hello").unwrap_err();
        assert!(matches!(err, AssetError::UploadNotFound), "{err}");
        let err = complete_upload(dir.path(), &sha, &upload_id).unwrap_err();
        assert!(matches!(err, AssetError::UploadNotFound), "{err}");
    }

    /// A part whose file is gone while its upload's manifest stays, as a
    /// removal that failed part-way leaves it, is a part to send again: the
    /// upload is still live.
    #[test]
    fn a_completion_whose_received_part_is_gone_asks_for_the_part_again() {
        let (dir, sha, upload_id, session) = started_upload();
        put_part(dir.path(), &sha, &upload_id, 1, b"hello").unwrap();
        fs::remove_file(part_path(&session, 1)).unwrap();

        let err = complete_upload(dir.path(), &sha, &upload_id).unwrap_err();
        assert!(matches!(err, AssetError::Invalid(_)), "{err}");
        put_part(dir.path(), &sha, &upload_id, 1, b"hello").unwrap();
        complete_upload(dir.path(), &sha, &upload_id).unwrap();
    }

    /// An abort while a part is still being written waits its turn, and
    /// leaves the upload in place.
    #[test]
    fn an_abort_while_a_part_is_written_is_refused_as_locked() {
        let (dir, sha, upload_id, session) = started_upload();

        let held = lock_session(&session).unwrap();
        let err = abort_upload(dir.path(), &sha, &upload_id).unwrap_err();
        assert!(matches!(err, AssetError::Locked), "{err}");
        assert!(manifest_path(&session).is_file());

        drop(held);
        abort_upload(dir.path(), &sha, &upload_id).unwrap();
        assert!(!session.exists());
        abort_upload(dir.path(), &sha, &upload_id).unwrap();
    }

    /// S2-8: a second request to an upload while the first holds the lock is
    /// refused without waiting. Its bytes may be right, so the refusal names
    /// the lock and says to send again, and does not name a server path.
    #[test]
    fn a_request_refused_for_the_lock_says_so() {
        let dir = tempdir().unwrap();
        let sha = Sha256::parse(&"c".repeat(64)).unwrap();
        let session = session_dir(dir.path(), &sha, "locktest01");
        fs::create_dir_all(&session).unwrap();

        let _held = lock_session(&session).unwrap();
        let err = lock_session(&session).unwrap_err();
        assert!(
            matches!(err, AssetError::Locked),
            "expected lock failure, got: {err}"
        );
        let message = err.to_string();
        assert!(
            message.contains("another request to this upload holds its lock"),
            "expected the lock to be named, got: {message}"
        );
        assert!(
            !message.contains(&dir.path().display().to_string()),
            "the refusal names a server path: {message}"
        );
    }
}
