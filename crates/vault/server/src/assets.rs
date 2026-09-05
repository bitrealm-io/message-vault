//! Content-addressed asset storage under each account's `assets/` directory.
//!
//! Files are stored by SHA-256 fingerprint (`aa/aaaa…ext`) and every reuse
//! re-checks the bytes against the claimed fingerprint. The HTTP handlers for
//! `HEAD` / `GET` / `PUT /v1/assets/{sha256}` and the multipart upload routes
//! also live here; multipart staging itself is in `asset_uploads`.

use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use crate::extract::{Json, Path as AxumPath, Query};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::Response;

use crate::asset_uploads;
use crate::config::validate_source_id;
use crate::server::{
    ApiError, AppState, AuthIdentity, ExportAccess, ImportAccess, ImportOrExportAccess,
    discard_body, read_body_limited, resolve_import_account, stream_body_to_file,
    upload_content_type,
};

/// Read/write chunk for hashing and copying files: 1 MiB.
pub(crate) const COPY_BUFFER_BYTES: usize = 1024 * 1024;

/// Counts of files handled during one asset store pass.
#[derive(Debug, Default)]
pub struct AssetStats {
    /// Files written to the asset store.
    pub copied: u64,
    /// Files already present under the same fingerprint, skipped.
    pub deduped: u64,
    /// Source files not found on disk.
    pub missing: u64,
}

/// One stored attachment: fingerprint, relative path, and MIME type.
#[derive(Debug, Clone)]
pub struct StoredAsset {
    /// SHA-256 fingerprint of the stored bytes (64 lowercase hex digits).
    pub sha256: String,
    /// Path relative to the account's assets root.
    pub assets_path: String,
    /// MIME type of the file, when known.
    pub mime_type: Option<String>,
}

/// Encode bytes as lowercase hex.
pub fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 fingerprint of `data` as 64 lowercase hex digits.
///
/// SHA-256 is a short fingerprint of the file contents.
pub fn sha256_hex(data: &[u8]) -> String {
    hex_encode(&Sha256::digest(data))
}

/// Relative path under the assets root: first two hex digits as a folder, then
/// the full fingerprint, then `ext`. `.jpeg` is stored as `.jpg`.
pub fn shard_rel_path(sha256: &str, ext: &str) -> String {
    let ext = if ext == ".jpeg" { ".jpg" } else { ext };
    format!("{}/{}{}", &sha256[..2], sha256, ext)
}

/// Find a stored attachment by SHA-256 (a short fingerprint of the file
/// contents) and confirm the file on disk still matches that fingerprint.
///
/// Upload and import paths that skip sending bytes because "the file is already
/// here" must use this function. A truncated or replaced file is then never
/// treated as the real content.
pub fn lookup_by_sha256(assets_root: &Path, sha256: &str) -> Option<StoredAsset> {
    let stored = lookup_by_sha256_unverified(assets_root, sha256)?;
    let path = assets_root.join(&stored.assets_path);
    if hash_file(&path).ok()? != stored.sha256 {
        return None;
    }
    Some(stored)
}

/// Find the stored path and MIME type for a SHA-256 fingerprint without reading
/// the file.
///
/// Used only when streaming an authenticated download. The response body is the
/// file itself, the URL is the fingerprint, and the client can check what it
/// received. Hashing the whole file first would read every download twice.
pub fn lookup_by_sha256_unverified(assets_root: &Path, sha256: &str) -> Option<StoredAsset> {
    let sha = normalize_sha256(sha256)?;
    let existing = find_existing(assets_root, &sha)?;
    let assets_path = path_relative_to(assets_root, &existing).ok()?;
    let mime_type = mime_for_path_or_sidecar(assets_root, &existing, &sha);
    Some(StoredAsset {
        sha256: sha,
        assets_path,
        mime_type,
    })
}

/// Store `source` under `assets_root` using a caller-claimed SHA-256 fingerprint,
/// after checking that the file bytes match that claim.
///
/// When `consume_source` is true (HTTP upload temps), the source is removed
/// after the verified temporary copy is installed.
///
/// `skip_hash` is kept only so the function signature stays stable. Sources are
/// always hashed before reuse, so a wrong upload cannot be accepted just
/// because a matching file already exists.
///
/// Returns `(stored, already_present)`.
///
/// # Errors
///
/// Returns an error when the claimed fingerprint is invalid, the source is not
/// a regular file, the bytes do not match the claim, or the file cannot be
/// written under `assets_root`.
pub fn store_verified(
    source: &Path,
    claimed_sha256: &str,
    assets_root: &Path,
    export_mime: Option<&str>,
    consume_source: bool,
    _skip_hash: bool,
) -> Result<(StoredAsset, bool)> {
    store_verified_inner(
        source,
        claimed_sha256,
        assets_root,
        export_mime,
        consume_source,
        || {},
        || {},
    )
}

/// Same as [`store_verified`], with hooks so tests can observe copy vs reuse.
fn store_verified_inner(
    source: &Path,
    claimed_sha256: &str,
    assets_root: &Path,
    export_mime: Option<&str>,
    consume_source: bool,
    copy_ready: impl FnOnce(),
    selection_ready: impl FnOnce(),
) -> Result<(StoredAsset, bool)> {
    let claimed = require_sha256(claimed_sha256)?;
    ensure_regular_file(source)?;
    let source_mime = resolve_mime(export_mime, source);
    let (dest, already) = install_blob(
        source,
        assets_root,
        &claimed,
        consume_source,
        copy_ready,
        selection_ready,
    )?;
    let rel = path_relative_to(assets_root, &dest)?;
    let mime_type = if already {
        mime_for_existing_file(export_mime, &dest, source_mime)
    } else {
        source_mime
    };
    if let Some(mime) = mime_type.as_deref() {
        store_mime_metadata(assets_root, &claimed, mime)?;
    }
    Ok((
        StoredAsset {
            sha256: claimed,
            assets_path: rel,
            mime_type,
        },
        already,
    ))
}

/// Copy `source` into place through a temporary file in the same folder, then
/// rename. The second return value is `true` only when a concurrent or earlier
/// valid file already won.
///
/// Order of work matters for both safety and cost:
/// 1. Check an existing destination first. On a hit the source is hashed (a
///    wrong claimed fingerprint must never be accepted just because a valid
///    file exists) and the call returns without a copy, a disk flush, or a
///    rename. This is the common repeat-import and repeat-upload case.
/// 2. Otherwise copy into a temporary file in the destination folder, hashing
///    while writing, and refuse to keep the file unless the written bytes match
///    the claimed fingerprint. The bytes that land on disk are therefore always
///    bytes this call checked, even if the source changed underneath.
fn install_blob(
    source: &Path,
    assets_root: &Path,
    claimed_sha256: &str,
    consume_source: bool,
    copy_ready: impl FnOnce(),
    selection_ready: impl FnOnce(),
) -> Result<(PathBuf, bool)> {
    let shard = assets_root.join(&claimed_sha256[..2]);
    fs::create_dir_all(&shard).with_context(|| format!("failed to create {}", shard.display()))?;

    // New files use a single path with no extension, named only from the
    // fingerprint. `find_existing` still finds older files that kept an
    // extension, so those remain readable and reusable.
    let desired = assets_root.join(shard_rel_path(claimed_sha256, ""));
    let dest = if let Some(existing) = find_existing(assets_root, claimed_sha256) {
        if hash_file(&existing).is_ok_and(|actual| actual == claimed_sha256) {
            verify_source_digest(source, claimed_sha256)?;
            if consume_source {
                let _ = fs::remove_file(source);
            }
            return Ok((existing, true));
        }
        existing
    } else {
        desired
    };
    selection_ready();

    if let Ok(meta) = fs::symlink_metadata(&dest) {
        if meta.file_type().is_symlink() {
            bail!("refusing to install over symlink {}", dest.display());
        }
        if !meta.is_file() {
            bail!(
                "asset destination exists and is not a regular file: {}",
                dest.display()
            );
        }
        if hash_file(&dest).is_ok_and(|actual| actual == claimed_sha256) {
            verify_source_digest(source, claimed_sha256)?;
            if consume_source {
                let _ = fs::remove_file(source);
            }
            return Ok((dest, true));
        }
        let temporary = copy_to_verified_temp(source, &shard, claimed_sha256)?;
        copy_ready();
        temporary
            .persist(&dest)
            .map_err(|err| err.error)
            .with_context(|| format!("replace corrupt asset {}", dest.display()))?;
    } else {
        let temporary = copy_to_verified_temp(source, &shard, claimed_sha256)?;
        copy_ready();
        match temporary.persist_noclobber(&dest) {
            Ok(_) => {}
            Err(err) if err.error.kind() == std::io::ErrorKind::AlreadyExists => {
                // The copy above already checked that the source bytes match
                // the claimed fingerprint.
                if hash_file(&dest).is_ok_and(|actual| actual == claimed_sha256) {
                    if consume_source {
                        let _ = fs::remove_file(source);
                    }
                    return Ok((dest, true));
                }
                err.file
                    .persist(&dest)
                    .map_err(|persist_err| persist_err.error)
                    .with_context(|| format!("replace corrupt asset {}", dest.display()))?;
            }
            Err(err) => {
                return Err(err.error).with_context(|| format!("install {}", dest.display()));
            }
        }
    }
    if consume_source {
        let _ = fs::remove_file(source);
    }
    Ok((dest, false))
}

/// Reject a claimed SHA-256 fingerprint that the source bytes do not produce.
///
/// Used when skipping the copy because a matching file is already stored. Without
/// this check, a wrong claim would be accepted just because that file exists.
fn verify_source_digest(source: &Path, claimed_sha256: &str) -> Result<()> {
    let actual = hash_file(source).with_context(|| format!("read source {}", source.display()))?;
    if actual != claimed_sha256 {
        bail!("sha256 mismatch: claimed {claimed_sha256}, got {actual}");
    }
    Ok(())
}

/// Copy `source` into a flushed temporary file inside `shard`, hashing as it
/// writes, and fail unless the written bytes hash to `claimed_sha256`.
///
/// Hashing the bytes as they are written (rather than trusting an earlier hash
/// of the source path) keeps a source that changes mid-copy from being saved.
fn copy_to_verified_temp(
    source: &Path,
    shard: &Path,
    claimed_sha256: &str,
) -> Result<tempfile::NamedTempFile> {
    let mut temporary = tempfile::NamedTempFile::new_in(shard)
        .with_context(|| format!("create temporary asset in {}", shard.display()))?;
    let mut src =
        open_nofollow_read(source).with_context(|| format!("open source {}", source.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; COPY_BUFFER_BYTES];
    loop {
        let n = src
            .read(&mut buf)
            .with_context(|| format!("read source {}", source.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        temporary
            .write_all(&buf[..n])
            .with_context(|| format!("write temporary asset for {}", source.display()))?;
    }
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    let actual = hex_encode(&hasher.finalize());
    if actual != claimed_sha256 {
        bail!("sha256 mismatch: claimed {claimed_sha256}, got {actual}");
    }
    Ok(temporary)
}

/// Hash `source` and store it under `assets_root/<sha[0:2]>/<sha><ext>`.
/// If the file already exists, skip the copy and count it as reused.
///
/// # Errors
///
/// Returns an error when the source cannot be hashed or stored.
pub fn hash_and_store(
    source: &Path,
    assets_root: &Path,
    export_mime: Option<&str>,
    stats: &mut AssetStats,
) -> Result<Option<StoredAsset>> {
    if !is_regular_file(source) {
        stats.missing += 1;
        return Ok(None);
    }

    let sha = hash_file(source).with_context(|| format!("failed to hash {}", source.display()))?;
    let (stored, already) = store_verified(source, &sha, assets_root, export_mime, false, false)?;
    if already {
        stats.deduped += 1;
    } else {
        stats.copied += 1;
    }
    Ok(Some(stored))
}

/// Delete abandoned multipart upload folders under `{assets}/.incoming` older
/// than `max_age_secs`.
///
/// Finished uploads already remove their session folders. Abandoned ones can
/// sit forever without this sweep, which runs from upload start.
///
/// # Errors
///
/// Returns an error when `{assets}/.incoming` cannot be read.
pub fn gc_stale_incoming(assets_root: &Path, max_age_secs: u64) -> Result<u64> {
    let incoming = assets_root.join(".incoming");
    if !incoming.is_dir() {
        return Ok(0);
    }
    let now = std::time::SystemTime::now();
    let mut removed = 0u64;
    for sha_entry in
        fs::read_dir(&incoming).with_context(|| format!("read {}", incoming.display()))?
    {
        let sha_entry = sha_entry?;
        let sha_path = sha_entry.path();
        if !sha_path.is_dir() {
            continue;
        }
        for session_entry in fs::read_dir(&sha_path)? {
            let session_entry = session_entry?;
            let session_path = session_entry.path();
            if !session_path.is_dir() {
                continue;
            }
            let Ok(meta) = session_entry.metadata() else {
                continue;
            };
            let Ok(modified) = meta.modified() else {
                continue;
            };
            let Ok(age) = now.duration_since(modified) else {
                continue;
            };
            if age.as_secs() >= max_age_secs {
                let _ = fs::remove_dir_all(&session_path);
                removed += 1;
            }
        }
        // Remove empty fingerprint folders left after the last session is gone.
        if fs::read_dir(&sha_path)?.next().is_none() {
            let _ = fs::remove_dir(&sha_path);
        }
    }
    Ok(removed)
}

/// Accept a 64-character lowercase hex SHA-256 fingerprint, or return `None`.
pub(crate) fn normalize_sha256(sha: &str) -> Option<String> {
    let s = sha.trim().to_ascii_lowercase();
    if s.len() != 64 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(s)
}

/// Same as [`normalize_sha256`], but as an error when the value is invalid.
///
/// # Errors
///
/// Returns an error when `sha` is not 64 lowercase hex digits.
pub(crate) fn require_sha256(sha: &str) -> Result<String> {
    match normalize_sha256(sha) {
        Some(normalized) => Ok(normalized),
        None => Err(anyhow::anyhow!(
            "invalid sha256 (expected 64 lowercase hex digits)"
        )),
    }
}

/// SHA-256 fingerprint of the file at `path`, as 64 lowercase hex digits.
///
/// # Errors
///
/// Returns an error when the file cannot be opened or read.
pub(crate) fn hash_file(path: &Path) -> Result<String> {
    let file = open_nofollow_read(path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; COPY_BUFFER_BYTES];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_encode(&hasher.finalize()))
}

/// Find a stored file whose name (without extension) is `sha`.
///
/// When more than one match exists, the lexicographically first path is used so
/// the choice is stable across calls.
fn find_existing(assets_root: &Path, sha: &str) -> Option<PathBuf> {
    let shard = assets_root.join(&sha[..2]);
    if !shard.is_dir() {
        return None;
    }
    let entries = fs::read_dir(&shard).ok()?;
    let mut matches = Vec::new();
    for entry in entries {
        let path = entry.ok()?.path();
        if file_stem_equals(&path, sha) && is_regular_file(&path) {
            matches.push(path);
        }
    }
    matches.sort();
    matches.into_iter().next()
}

/// True when the file name without its extension is exactly `expected`.
fn file_stem_equals(path: &Path, expected: &str) -> bool {
    match path.file_stem().and_then(|s| s.to_str()) {
        Some(stem) => stem == expected,
        None => false,
    }
}

/// Path of the hidden `.<sha>.mime` sidecar that records a blob's MIME type when its name has no usable extension.
fn mime_metadata_path(assets_root: &Path, sha: &str) -> PathBuf {
    assets_root.join(&sha[..2]).join(format!(".{sha}.mime"))
}

/// MIME type from the stored file's extension, else from its sidecar.
fn mime_for_path_or_sidecar(assets_root: &Path, path: &Path, sha: &str) -> Option<String> {
    match resolve_mime(None, path) {
        Some(mime) => Some(mime),
        None => read_mime_metadata(assets_root, sha),
    }
}

/// MIME type to record for a blob that already exists: the export's claim wins, then what the
/// stored file's name says, then what the source file said.
fn mime_for_existing_file(
    export_mime: Option<&str>,
    dest: &Path,
    source_mime: Option<String>,
) -> Option<String> {
    if let Some(mime) = export_mime
        && !mime.is_empty()
    {
        return Some(mime.to_owned());
    }
    resolve_mime(None, dest).or(source_mime)
}

/// Read the MIME sidecar for `sha`, if present and non-empty.
fn read_mime_metadata(assets_root: &Path, sha: &str) -> Option<String> {
    let file = open_nofollow_read(&mime_metadata_path(assets_root, sha)).ok()?;
    let mut mime = String::new();
    file.take(1024).read_to_string(&mut mime).ok()?;
    let mime = mime.trim();
    if mime.is_empty() {
        None
    } else {
        Some(mime.to_owned())
    }
}

/// Write the MIME sidecar for `sha` unless one already exists. Empty types are not recorded.
fn store_mime_metadata(assets_root: &Path, sha: &str, mime: &str) -> Result<()> {
    let mime = mime.trim();
    if mime.is_empty() {
        return Ok(());
    }
    let path = mime_metadata_path(assets_root, sha);
    if read_mime_metadata(assets_root, sha).is_some() {
        return Ok(());
    }
    let Some(parent) = path.parent() else {
        return Err(anyhow::anyhow!("asset MIME metadata has no parent"));
    };
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("create MIME metadata in {}", parent.display()))?;
    temporary.write_all(mime.as_bytes())?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    match temporary.persist_noclobber(&path) {
        Ok(_) => Ok(()),
        Err(err) if err.error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(err) => Err(err.error).with_context(|| format!("install {}", path.display())),
    }
}

/// The path under `root` as a forward-slash string, the form `attachments.assets_path` stores.
fn path_relative_to(root: &Path, path: &Path) -> Result<String> {
    let relative = path.strip_prefix(root).with_context(|| {
        format!(
            "asset path {} is not under {}",
            path.display(),
            root.display()
        )
    })?;
    Ok(relative.to_string_lossy().replace('\\', "/"))
}

/// The export's MIME claim when it has one, else a guess from the file extension.
fn resolve_mime(export_mime: Option<&str>, source: &Path) -> Option<String> {
    if let Some(mime) = export_mime
        && !mime.is_empty()
    {
        return Some(mime.to_string());
    }
    let ext = source.extension().and_then(|e| e.to_str());
    guess_mime(ext)
}

/// Map a file extension to a MIME type via the shared table in [`media`].
///
/// Stored files are named only from their SHA-256 fingerprint, so they have no
/// extension. This mapping is the only chance to record what a file is: the
/// result is stored next to the file, returned to download callers, and written
/// to `attachments.mime_type`, which is what derived-media processing classifies
/// on. Extensions common in phone backups (voice notes, camera video, scans)
/// therefore need to be in the shared table.
fn guess_mime(ext: Option<&str>) -> Option<String> {
    media::mime_for_ext(ext?).map(str::to_string)
}

/// True for a plain file; symlinks are never followed into the asset store.
fn is_regular_file(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Ok(meta) => meta.is_file() && !meta.file_type().is_symlink(),
        Err(_) => false,
    }
}

/// Fail unless `path` is a regular file, not a symlink.
fn ensure_regular_file(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path).with_context(|| format!("stat {}", path.display()))?;
    if meta.file_type().is_symlink() {
        bail!("refusing to follow symlink: {}", path.display());
    }
    if !meta.is_file() {
        bail!("asset source is not a file: {}", path.display());
    }
    Ok(())
}

/// Open `path` for reading without following a symlink.
fn open_nofollow_read(path: &Path) -> Result<File> {
    ensure_regular_file(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .with_context(|| format!("open {}", path.display()))
    }
    #[cfg(not(unix))]
    {
        File::open(path).with_context(|| format!("open {}", path.display()))
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct AssetPutQuery {
    source: String,
    #[serde(default)]
    account: Option<String>,
}

/// Stored asset fingerprint and path.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct AssetPutResponse {
    sha256: String,
    assets_path: String,
    already_present: bool,
}

impl AssetPutResponse {
    /// Response body for a blob that is now in the store.
    fn stored(asset: StoredAsset, already_present: bool) -> Json<Self> {
        Json(Self {
            sha256: asset.sha256,
            assets_path: asset.assets_path,
            already_present,
        })
    }
}

enum AssetAccess {
    /// GET asset bytes — needs export (or full session).
    Read,
    /// PUT / multipart upload — needs import (or full session).
    Write,
    /// HEAD probe — import or export.
    Probe,
}

/// Resolve the account and source an asset route targets, check the caller may perform
/// `access` on it, and look the blob up by sha256.
async fn resolve_asset_lookup(
    state: &AppState,
    auth: &AuthIdentity,
    sha256: &str,
    query: &AssetPutQuery,
    access: AssetAccess,
) -> Result<(String, String, Option<StoredAsset>), ApiError> {
    // The handler's extractor already checked the capability for this access
    // mode; here the mode only picks the lookup strategy. A download streams
    // the file itself, so hashing it during lookup would read every byte
    // twice. Probe and write lookups decide whether a client may skip sending
    // bytes, so those keep verifying the stored blob.
    let verify_stored_bytes = match access {
        AssetAccess::Read => false,
        AssetAccess::Write | AssetAccess::Probe => true,
    };
    if query.source.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "query param source is required".into(),
        ));
    }
    validate_source_id(&query.source).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let account = resolve_import_account(auth, query.account.as_deref(), &state.db).await?;
    let source_id = query.source.clone();

    let cfg = Arc::clone(&state.cfg);
    let sha_lookup = sha256.to_string();
    let account_lookup = account.clone();
    let source_lookup = source_id.clone();
    let existing = tokio::task::spawn_blocking(move || {
        let assets_dir = cfg
            .paths
            .assets_dir_for_account(&account_lookup, &source_lookup);
        if verify_stored_bytes {
            lookup_by_sha256(&assets_dir, &sha_lookup)
        } else {
            lookup_by_sha256_unverified(&assets_dir, &sha_lookup)
        }
    })
    .await
    .map_err(|e| ApiError::Internal(format!("asset lookup task: {e}")))?;
    Ok((account, source_id, existing))
}

/// Probe whether a content-addressed asset is already stored (no body).
///
/// Clients may skip sending bytes when the asset exists.
#[utoipa::path(
    head,
    path = "/v1/assets/{sha256}",
    tag = "Assets",
    security(("bearer" = [])),
    params(
        ("sha256" = String, Path, description = "Content SHA-256 hex"),
        ("source" = String, Query),
        ("account" = Option<String>, Query)
    ),
    responses(
        (status = 200, body = AssetPutResponse),
        (status = 400, body = crate::server::ErrorBody),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody),
        (status = 404, body = crate::server::ErrorBody)
    )
)]
pub(crate) async fn asset_head_handler(
    State(state): State<AppState>,
    ImportOrExportAccess(auth): ImportOrExportAccess,
    AxumPath(sha256): AxumPath<String>,
    Query(query): Query<AssetPutQuery>,
) -> Result<Json<AssetPutResponse>, ApiError> {
    let (_account, _source_id, existing) =
        resolve_asset_lookup(&state, &auth, &sha256, &query, AssetAccess::Probe).await?;
    let Some(stored) = existing else {
        return Err(ApiError::NotFound("asset not found".into()));
    };
    Ok(AssetPutResponse::stored(stored, true))
}

/// Download a previously stored content-addressed asset (read-only).
///
/// The body streams the stored bytes; the URL is the SHA-256 fingerprint.
#[utoipa::path(
    get,
    path = "/v1/assets/{sha256}",
    tag = "Assets",
    security(("bearer" = [])),
    params(
        ("sha256" = String, Path, description = "Content SHA-256 hex"),
        ("source" = String, Query),
        ("account" = Option<String>, Query)
    ),
    responses(
        (status = 200, description = "Raw asset bytes", content_type = "application/octet-stream"),
        (status = 400, body = crate::server::ErrorBody),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody),
        (status = 404, body = crate::server::ErrorBody)
    )
)]
pub(crate) async fn asset_get_handler(
    State(state): State<AppState>,
    ExportAccess(auth): ExportAccess,
    AxumPath(sha256): AxumPath<String>,
    Query(query): Query<AssetPutQuery>,
) -> Result<Response, ApiError> {
    let (account, source_id, existing) =
        resolve_asset_lookup(&state, &auth, &sha256, &query, AssetAccess::Read).await?;
    let Some(stored) = existing else {
        return Err(ApiError::NotFound("asset not found".into()));
    };

    let assets_dir = state.cfg.paths.assets_dir_for_account(&account, &source_id);
    let path = assets_dir.join(&stored.assets_path);
    // Reject symlinks / missing files before streaming.
    let meta = tokio::fs::symlink_metadata(&path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ApiError::NotFound("asset file missing on disk".into())
        } else {
            ApiError::Internal(format!("stat {}: {e}", path.display()))
        }
    })?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(ApiError::NotFound("asset file missing on disk".into()));
    }

    let mime = stored
        .mime_type
        .clone()
        .unwrap_or_else(|| "application/octet-stream".into());
    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|e| ApiError::Internal(format!("open {}: {e}", path.display())))?;
    let stream = tokio_util::io::ReaderStream::new(file);
    let body = axum::body::Body::from_stream(stream);

    let mut response = Response::new(body);
    *response.status_mut() = StatusCode::OK;
    let headers_mut = response.headers_mut();
    if let Ok(value) = header::HeaderValue::from_str(&mime) {
        headers_mut.insert(header::CONTENT_TYPE, value);
    }
    headers_mut.insert(
        header::HeaderName::from_static("x-content-type-options"),
        header::HeaderValue::from_static("nosniff"),
    );
    // Force download-ish disposition with a fixed safe name (never echo client paths).
    headers_mut.insert(
        header::CONTENT_DISPOSITION,
        header::HeaderValue::from_static("attachment; filename=\"asset\""),
    );
    if meta.len() > 0 {
        headers_mut.insert(
            header::CONTENT_LENGTH,
            header::HeaderValue::from(meta.len()),
        );
    }
    Ok(response)
}

/// Store one asset body under its SHA-256 fingerprint.
#[utoipa::path(
    put,
    path = "/v1/assets/{sha256}",
    tag = "Assets",
    security(("bearer" = [])),
    params(
        ("sha256" = String, Path, description = "Content SHA-256 hex"),
        ("source" = String, Query),
        ("account" = Option<String>, Query)
    ),
    request_body(content_type = "application/octet-stream", description = "Raw asset bytes"),
    responses(
        (status = 200, body = AssetPutResponse),
        (status = 400, body = crate::server::ErrorBody),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody),
        (status = 413, body = crate::server::ErrorBody)
    )
)]
pub(crate) async fn asset_put_handler(
    State(state): State<AppState>,
    ImportAccess(auth): ImportAccess,
    headers: HeaderMap,
    AxumPath(sha256): AxumPath<String>,
    Query(query): Query<AssetPutQuery>,
    request: Request,
) -> Result<Json<AssetPutResponse>, ApiError> {
    let (account, source_id, existing) =
        resolve_asset_lookup(&state, &auth, &sha256, &query, AssetAccess::Write).await?;

    let mime = upload_content_type(&headers);

    if let Some(stored) = existing {
        discard_body(request.into_body(), state.max_body_bytes).await?;
        return Ok(AssetPutResponse::stored(stored, true));
    }

    // Write the upload into the account assets tree so verify can rename into place
    // instead of copying across filesystems (tempfile often lives on another mount).
    let assets_dir = state.cfg.paths.assets_dir_for_account(&account, &source_id);
    let incoming_dir = assets_dir.join(".incoming");
    tokio::fs::create_dir_all(&incoming_dir)
        .await
        .map_err(|e| ApiError::Internal(format!("mkdir {}: {e}", incoming_dir.display())))?;
    let tmp_path = incoming_dir.join(format!(
        "{sha256}-{}.part",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let n = match stream_body_to_file(request.into_body(), &tmp_path, state.max_body_bytes).await {
        Ok(n) => n,
        Err(err) => {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(err);
        }
    };
    if n == 0 {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(ApiError::BadRequest("request body is empty".into()));
    }

    let sha = sha256.clone();
    let tmp_for_store = tmp_path.clone();
    let assets_dir_store = assets_dir.clone();
    let (stored, already_present) = tokio::task::spawn_blocking(move || {
        std::fs::create_dir_all(&assets_dir_store)?;
        store_verified(
            &tmp_for_store,
            &sha,
            &assets_dir_store,
            mime.as_deref(),
            true,
            false,
        )
    })
    .await
    .map_err(|e| ApiError::Internal(format!("asset upload task: {e}")))?
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    // Rename consumes the temp file; remove leftovers after errors / already_present races.
    let _ = tokio::fs::remove_file(&tmp_path).await;
    Ok(AssetPutResponse::stored(stored, already_present))
}

/// Total bytes and optional MIME type for a chunked upload.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct AssetUploadStartBody {
    bytes: u64,
    #[serde(default)]
    mime: Option<String>,
}

/// Upload id and part size, or the already-stored asset.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct AssetUploadStartResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    upload_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    part_size: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    assets_path: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    already_present: bool,
}

/// Bytes written for one part.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct AssetUploadPartResponse {
    part: u32,
    bytes: u64,
}

/// Start a chunked (multipart) asset upload and get the part size.
#[utoipa::path(
    post,
    path = "/v1/assets/{sha256}/uploads",
    tag = "Assets",
    security(("bearer" = [])),
    params(
        ("sha256" = String, Path, description = "Content SHA-256 hex"),
        ("source" = String, Query),
        ("account" = Option<String>, Query)
    ),
    request_body = AssetUploadStartBody,
    responses(
        (status = 200, body = AssetUploadStartResponse),
        (status = 400, body = crate::server::ErrorBody),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody)
    )
)]
pub(crate) async fn asset_upload_start_handler(
    State(state): State<AppState>,
    ImportAccess(auth): ImportAccess,
    AxumPath(sha256): AxumPath<String>,
    Query(query): Query<AssetPutQuery>,
    Json(body): Json<AssetUploadStartBody>,
) -> Result<Json<AssetUploadStartResponse>, ApiError> {
    let (account, source_id, _existing) =
        resolve_asset_lookup(&state, &auth, &sha256, &query, AssetAccess::Write).await?;
    let assets_dir = state.cfg.paths.assets_dir_for_account(&account, &source_id);
    let mime = body.mime.clone();
    let bytes = body.bytes;
    let sha = sha256.clone();
    let limits = state.upload_limits;
    let result = tokio::task::spawn_blocking(move || {
        asset_uploads::start_upload(&assets_dir, &sha, bytes, mime.as_deref(), limits)
    })
    .await
    .map_err(|e| ApiError::Internal(format!("upload start task: {e}")))?
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    match result {
        (Some(stored), None) => Ok(Json(AssetUploadStartResponse {
            upload_id: None,
            part_size: None,
            sha256: Some(stored.sha256),
            assets_path: Some(stored.assets_path),
            already_present: true,
        })),
        (None, Some(start)) => Ok(Json(AssetUploadStartResponse {
            upload_id: Some(start.upload_id),
            part_size: Some(start.part_size),
            sha256: None,
            assets_path: None,
            already_present: false,
        })),
        _ => Err(ApiError::Internal(
            "upload start returned inconsistent state".into(),
        )),
    }
}

/// Write one part of a chunked asset upload.
#[utoipa::path(
    put,
    path = "/v1/assets/{sha256}/uploads/{upload_id}/parts/{part}",
    tag = "Assets",
    security(("bearer" = [])),
    params(
        ("sha256" = String, Path, description = "Content SHA-256 hex"),
        ("upload_id" = String, Path),
        ("part" = u32, Path),
        ("source" = String, Query),
        ("account" = Option<String>, Query)
    ),
    request_body(content_type = "application/octet-stream", description = "Raw part bytes"),
    responses(
        (status = 200, body = AssetUploadPartResponse),
        (status = 400, body = crate::server::ErrorBody),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody),
        (status = 413, body = crate::server::ErrorBody)
    )
)]
pub(crate) async fn asset_upload_part_handler(
    State(state): State<AppState>,
    ImportAccess(auth): ImportAccess,
    AxumPath((sha256, upload_id, part)): AxumPath<(String, String, u32)>,
    Query(query): Query<AssetPutQuery>,
    request: Request,
) -> Result<Json<AssetUploadPartResponse>, ApiError> {
    let (account, source_id, _existing) =
        resolve_asset_lookup(&state, &auth, &sha256, &query, AssetAccess::Write).await?;
    if part == 0 {
        return Err(ApiError::BadRequest("part number must be >= 1".into()));
    }
    let body = read_body_limited(request.into_body(), state.upload_limits.part_size).await?;
    let assets_dir = state.cfg.paths.assets_dir_for_account(&account, &source_id);
    let sha = sha256.clone();
    let uid = upload_id.clone();
    let written = tokio::task::spawn_blocking(move || {
        asset_uploads::put_part(&assets_dir, &sha, &uid, part, &body)
    })
    .await
    .map_err(|e| ApiError::Internal(format!("upload part task: {e}")))?
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(AssetUploadPartResponse {
        part,
        bytes: written,
    }))
}

/// Assemble the uploaded parts, verify the SHA-256 fingerprint, and install
/// the asset.
#[utoipa::path(
    post,
    path = "/v1/assets/{sha256}/uploads/{upload_id}/complete",
    tag = "Assets",
    security(("bearer" = [])),
    params(
        ("sha256" = String, Path, description = "Content SHA-256 hex"),
        ("upload_id" = String, Path),
        ("source" = String, Query),
        ("account" = Option<String>, Query)
    ),
    responses(
        (status = 200, body = AssetPutResponse),
        (status = 400, body = crate::server::ErrorBody),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody)
    )
)]
pub(crate) async fn asset_upload_complete_handler(
    State(state): State<AppState>,
    ImportAccess(auth): ImportAccess,
    AxumPath((sha256, upload_id)): AxumPath<(String, String)>,
    Query(query): Query<AssetPutQuery>,
) -> Result<Json<AssetPutResponse>, ApiError> {
    let (account, source_id, existing) =
        resolve_asset_lookup(&state, &auth, &sha256, &query, AssetAccess::Write).await?;
    if let Some(stored) = existing {
        // Drop staging if a concurrent single-PUT won the race.
        let assets_dir = state.cfg.paths.assets_dir_for_account(&account, &source_id);
        let sha = sha256.clone();
        let uid = upload_id.clone();
        let dropped = tokio::task::spawn_blocking(move || {
            asset_uploads::abort_upload(&assets_dir, &sha, &uid)
        })
        .await
        .map_err(anyhow::Error::from)
        .and_then(|result| result);
        if let Err(error) = dropped {
            eprintln!("warning: could not drop stale upload session {upload_id}: {error:#}");
        }
        return Ok(AssetPutResponse::stored(stored, true));
    }

    let lock_key = format!("{account}:{sha256}");
    let complete_lock = {
        let mut map = state.asset_complete_locks.lock().await;
        map.entry(lock_key)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    };
    let _guard = complete_lock.lock().await;

    let assets_dir = state.cfg.paths.assets_dir_for_account(&account, &source_id);
    let sha = sha256.clone();
    let uid = upload_id.clone();
    let (stored, already_present) = tokio::task::spawn_blocking(move || {
        asset_uploads::complete_upload(&assets_dir, &sha, &uid)
    })
    .await
    .map_err(|e| ApiError::Internal(format!("upload complete task: {e}")))?
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    Ok(AssetPutResponse::stored(stored, already_present))
}

/// Abort and delete a chunked asset upload's staging files.
#[utoipa::path(
    delete,
    path = "/v1/assets/{sha256}/uploads/{upload_id}",
    tag = "Assets",
    security(("bearer" = [])),
    params(
        ("sha256" = String, Path, description = "Content SHA-256 hex"),
        ("upload_id" = String, Path),
        ("source" = String, Query),
        ("account" = Option<String>, Query)
    ),
    responses(
        (status = 204, description = "Upload aborted"),
        (status = 400, body = crate::server::ErrorBody),
        (status = 401, body = crate::server::ErrorBody),
        (status = 403, body = crate::server::ErrorBody)
    )
)]
pub(crate) async fn asset_upload_abort_handler(
    State(state): State<AppState>,
    ImportAccess(auth): ImportAccess,
    AxumPath((sha256, upload_id)): AxumPath<(String, String)>,
    Query(query): Query<AssetPutQuery>,
) -> Result<axum::http::StatusCode, ApiError> {
    let (account, source_id, _existing) =
        resolve_asset_lookup(&state, &auth, &sha256, &query, AssetAccess::Write).await?;
    let assets_dir = state.cfg.paths.assets_dir_for_account(&account, &source_id);
    let sha = sha256.clone();
    let uid = upload_id.clone();
    tokio::task::spawn_blocking(move || asset_uploads::abort_upload(&assets_dir, &sha, &uid))
        .await
        .map_err(|e| ApiError::Internal(format!("upload abort task: {e}")))?
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests;
