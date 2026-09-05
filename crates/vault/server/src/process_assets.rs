//! Generate browser-friendly derived media under `assets_converted/`.
//!
//! Keeps originals intact, writes content-addressed JPEG/MP4/MP3 blobs, and
//! updates `attachments.derived_*`. The conversions are the `media` crate's,
//! which finds ffmpeg and ffprobe beside the binary, in `MESSAGE_VAULT_IO_BIN`,
//! or on `PATH`.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use sqlx::AnyConnection;
use tempfile::TempDir;

use crate::config::Config;
use crate::db::{engine, schema};
use media::{CompressOptions, Kind, MediaMode, TranscodeOutcome};

/// Browser previews use the `media` crate's compress recipe, the same one the
/// desktop export applies, so a preview and an exported copy of the same
/// source are the same bytes.
const PREVIEW_MODE: MediaMode = MediaMode::Compress;

/// Options for one derived-media processing pass.
#[derive(Debug, Clone, Default)]
pub struct ProcessAssetsOptions {
    /// Re-convert even when a browser preview already exists.
    pub force: bool,
    /// Convert and log without writing files or updating the database.
    pub dry_run: bool,
    /// Skip image conversion.
    pub skip_image: bool,
    /// Skip video conversion.
    pub skip_video: bool,
    /// Skip audio conversion.
    pub skip_audio: bool,
    /// Override DB path from config.
    pub db: Option<PathBuf>,
    /// Only process this source id.
    pub source: Option<String>,
    /// Connection URL (`postgres://…` or `sqlite://…`); wins over `db` / `paths.db`.
    pub db_url: Option<String>,
}

/// Counts reported by one derived-media processing pass.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ProcessAssetsStats {
    /// Attachments examined.
    pub scanned: u64,
    /// Browser previews written (JPEG/MP4/MP3).
    pub derived: u64,
    /// Attachments left as-is (already converted, non-media, or small JPEG).
    pub skipped: u64,
    /// Conversions that failed.
    pub errors: u64,
}

#[derive(Debug, Clone)]
struct DerivedBlob {
    sha256: String,
    assets_path: String,
    mime_type: String,
}

#[derive(Debug)]
struct AssetRow {
    sha256: String,
    assets_path: String,
    mime_type: Option<String>,
    derived_assets_path: Option<String>,
    /// Attachment file name from the export (`attachments.original_name`).
    original_name: Option<String>,
    /// Attachment path inside the export (`attachments.path`).
    source_path: Option<String>,
}

impl AssetRow {
    /// Extension sources to fall back on when the stored blob has none.
    fn name_hints(&self) -> [Option<&str>; 2] {
        [self.original_name.as_deref(), self.source_path.as_deref()]
    }
}

/// Run derived-media conversion for every account/source in the vault.
///
/// # Errors
///
/// Returns an error when the database is missing, a conversion tool fails, or
/// a derived file cannot be written.
pub async fn run(cfg: &Config, opts: &ProcessAssetsOptions) -> Result<ProcessAssetsStats> {
    let db_path = opts.db.as_ref().unwrap_or(&cfg.paths.db);
    let target = engine::DbTarget::new(opts.db_url.as_deref(), db_path);
    if let engine::DbTarget::Path(path) = target
        && !path.is_file()
    {
        bail!("database not found: {}", path.display());
    }
    let pool = target.open().await?;
    let mut conn = pool.acquire().await?;
    schema::ensure_vault_schema(&mut conn).await?;

    let account_ids = list_account_ids(&mut conn, &cfg.paths.data_dir).await?;
    if account_ids.is_empty() {
        bail!("no accounts found — create an account or run reset-demo first");
    }

    let work = TempDir::new().context("create temp dir for derived media")?;
    let mut stats = ProcessAssetsStats::default();

    for account_id in &account_ids {
        let source_ids = sources_to_process(&mut conn, cfg, opts, account_id).await?;
        if source_ids.is_empty() {
            eprintln!("account {account_id}: no sources found — skip");
            continue;
        }
        for source_id in source_ids {
            let Some(pass) = SourcePass::open(cfg, opts, work.path(), account_id, &source_id)?
            else {
                continue;
            };
            let rows = list_attachments(&mut conn, account_id, &source_id).await?;
            for row in rows {
                stats.scanned += 1;
                match pass.process(&mut conn, &row).await {
                    Ok(Outcome::Derived) => stats.derived += 1,
                    Ok(Outcome::Skipped) => stats.skipped += 1,
                    Err(err) => {
                        stats.errors += 1;
                        eprintln!("failed {}: {err:#}", pass.label(&row));
                    }
                }
            }
        }
    }

    println!(
        "done: scanned={} converted_for_web={} left_as_is={} conversion_failures={}{}",
        stats.scanned,
        stats.derived,
        stats.skipped,
        stats.errors,
        if opts.dry_run { " (dry-run)" } else { "" }
    );
    Ok(stats)
}

enum Outcome {
    Derived,
    Skipped,
}

/// The account's source ids, narrowed to `--source` when one was given.
///
/// # Errors
///
/// Returns an error when the requested source is not one of the account's.
async fn sources_to_process(
    conn: &mut AnyConnection,
    cfg: &Config,
    opts: &ProcessAssetsOptions,
    account_id: &str,
) -> Result<Vec<String>> {
    let mut source_ids =
        discover_source_ids(conn, account_id, &cfg.paths.data_dir, &cfg.paths.assets_dir).await?;
    if let Some(filter) = opts.source.as_deref() {
        let filter = filter.trim();
        source_ids.retain(|id| id == filter);
        if source_ids.is_empty() {
            bail!("unknown source '{filter}' for account {account_id}");
        }
    }
    Ok(source_ids)
}

/// One account's source folder being processed: where its originals are,
/// where the derived files go, and the options every attachment shares.
struct SourcePass<'a> {
    opts: &'a ProcessAssetsOptions,
    work_dir: &'a Path,
    account_id: &'a str,
    source_id: &'a str,
    assets_dir: PathBuf,
    converted_dir: PathBuf,
}

/// What deriving one attachment produced.
enum Derived {
    /// The original is not converted: unsupported, already small, or declined.
    Skipped,
    /// A dry run: said what it would write and stored nothing.
    DryRun,
    Stored(DerivedBlob),
}

impl<'a> SourcePass<'a> {
    /// Find the folders, clean leftover upload temps, and make the converted
    /// folder. `None` when the source has no assets folder to process.
    ///
    /// # Errors
    ///
    /// Returns an error when the leftover cleanup or the folder creation fails.
    fn open(
        cfg: &Config,
        opts: &'a ProcessAssetsOptions,
        work_dir: &'a Path,
        account_id: &'a str,
        source_id: &'a str,
    ) -> Result<Option<Self>> {
        let assets_dir = cfg.paths.assets_dir_for_account(account_id, source_id);
        let converted_dir = cfg
            .paths
            .assets_converted_dir_for_account(account_id, source_id);
        println!(
            "account {account_id} source {source_id}: assets={}",
            assets_dir.display()
        );
        if !assets_dir.is_dir() {
            eprintln!("  skip — assets dir missing");
            return Ok(None);
        }
        let cleaned = cleanup_incoming_parts(&assets_dir, opts.dry_run)?;
        if cleaned > 0 {
            println!("  cleaned {cleaned} leftover .part upload temp(s) under .incoming/");
        }
        fs::create_dir_all(&converted_dir)
            .with_context(|| format!("create converted dir {}", converted_dir.display()))?;
        Ok(Some(Self {
            opts,
            work_dir,
            account_id,
            source_id,
            assets_dir,
            converted_dir,
        }))
    }

    /// `account/source/path`: how log lines name an attachment.
    fn label(&self, row: &AssetRow) -> String {
        format!("{}/{}/{}", self.account_id, self.source_id, row.assets_path)
    }

    /// Derive a browser preview for one stored blob and record it, or report why it was left as-is.
    ///
    /// # Errors
    ///
    /// Returns an error when the original is missing, a conversion fails, or
    /// the row cannot be updated.
    async fn process(&self, conn: &mut AnyConnection, row: &AssetRow) -> Result<Outcome> {
        // Incomplete transfers / aborted uploads — never hand these to ffmpeg.
        if is_part_path(&row.assets_path) {
            return self.remove_incomplete(row);
        }
        let Some(kind) = kind_of(
            &row.assets_path,
            row.mime_type.as_deref(),
            &row.name_hints(),
        ) else {
            return Ok(Outcome::Skipped);
        };
        let wanted = match kind {
            Kind::Image => !self.opts.skip_image,
            Kind::Video => !self.opts.skip_video,
            Kind::Audio => !self.opts.skip_audio,
        };
        if !wanted
            || should_skip_existing(
                self.opts.force,
                row.derived_assets_path.as_deref(),
                &self.converted_dir,
            )
        {
            return Ok(Outcome::Skipped);
        }
        let source_path = self.assets_dir.join(&row.assets_path);
        if !source_path.is_file() {
            bail!("missing original: {}", self.label(row));
        }
        let blob = match self.derive(kind, &source_path, row)? {
            Derived::Skipped => return Ok(Outcome::Skipped),
            Derived::DryRun => return Ok(Outcome::Derived),
            Derived::Stored(blob) => blob,
        };
        update_derived(conn, self.account_id, self.source_id, &row.sha256, &blob).await?;
        println!("{} -> {}", self.label(row), blob.assets_path);
        Ok(Outcome::Derived)
    }

    /// Delete a `.part` left by an interrupted upload, or say so in a dry
    /// run. Always counts as skipped.
    fn remove_incomplete(&self, row: &AssetRow) -> Result<Outcome> {
        let source_path = self.assets_dir.join(&row.assets_path);
        if source_path.is_file() {
            if self.opts.dry_run {
                println!("[dry-run] would remove incomplete {}", self.label(row));
            } else {
                fs::remove_file(&source_path)
                    .with_context(|| format!("remove incomplete {}", source_path.display()))?;
                println!("removed incomplete {}", self.label(row));
            }
        }
        Ok(Outcome::Skipped)
    }

    /// Convert one original by kind into the work folder, then store it.
    fn derive(&self, kind: Kind, source_path: &Path, row: &AssetRow) -> Result<Derived> {
        let (what, format) = match kind {
            Kind::Image => ("image", "jpg"),
            Kind::Video => ("video", "mp4"),
            Kind::Audio => ("audio", "mp3"),
        };
        let token = row.sha256.get(..12).unwrap_or(&row.sha256);
        let out = self.work_dir.join(format!("out-{token}.{format}"));
        let outcome = media::transcode_file_as(
            source_path,
            kind,
            &out,
            PREVIEW_MODE,
            &CompressOptions::default(),
        )
        .with_context(|| format!("{what} preview for {}", self.label(row)))?;
        let out = match outcome {
            TranscodeOutcome::Produced => Some(out),
            TranscodeOutcome::Skipped => None,
        };
        self.store_work_file(out, what, format, &format!(".{format}"), row)
    }

    /// Store a derivative the media pass wrote to the work folder, then remove
    /// the work file. `None` means the pass left the original alone.
    fn store_work_file(
        &self,
        out: Option<PathBuf>,
        what: &str,
        format: &str,
        ext: &str,
        row: &AssetRow,
    ) -> Result<Derived> {
        let Some(out) = out else {
            return Ok(Derived::Skipped);
        };
        if self.opts.dry_run {
            println!("[dry-run] {what} {} -> {format}", self.label(row));
            let _ = fs::remove_file(&out);
            return Ok(Derived::DryRun);
        }
        let blob = store_derived_file(&self.converted_dir, &out, ext);
        let _ = fs::remove_file(&out);
        Ok(Derived::Stored(blob?))
    }
}

/// Account ids from the database, falling back to the folder names under `data_dir` when the table does not exist yet.
async fn list_account_ids(conn: &mut AnyConnection, data_dir: &Path) -> Result<Vec<String>> {
    // Engine-branched: sqlite_master does not exist on Postgres.
    let mut ids = Vec::new();
    if schema::table_exists(conn, "accounts").await? {
        let rows = sqlx::query_scalar::<_, String>("SELECT id FROM accounts ORDER BY id")
            .fetch_all(&mut *conn)
            .await?;
        ids = rows;
    }
    if ids.is_empty() && data_dir.is_dir() {
        for entry in fs::read_dir(data_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                ids.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        ids.sort();
    }
    Ok(ids)
}

/// Source ids for one account: those with messages in the database plus any folder under the account's data dir.
async fn discover_source_ids(
    conn: &mut AnyConnection,
    account_id: &str,
    data_dir: &Path,
    assets_name: &str,
) -> Result<Vec<String>> {
    let mut ids = std::collections::BTreeSet::new();
    let rows = sqlx::query_scalar::<_, String>(
        r"
        SELECT DISTINCT m.source
        FROM messages m
        JOIN conversations c ON c.id = m.conversation_id
        WHERE c.account_id = $1
          AND m.source IS NOT NULL
          AND TRIM(m.source) != ''
        ORDER BY m.source
        ",
    )
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await?;
    for s in rows {
        let t = s.trim();
        if !t.is_empty() {
            ids.insert(t.to_string());
        }
    }

    let account_root = data_dir.join(account_id);
    if account_root.is_dir() {
        for entry in fs::read_dir(&account_root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if account_root.join(&name).join(assets_name).is_dir() {
                ids.insert(name);
            }
        }
    }
    Ok(ids.into_iter().collect())
}

/// One row per stored blob for this account and source, with the names that could hint at its media type.
async fn list_attachments(
    conn: &mut AnyConnection,
    account_id: &str,
    source_id: &str,
) -> Result<Vec<AssetRow>> {
    // One row per stored blob. Several messages can share a blob under different
    // names, and only one derived file per blob is ever produced, so collapse
    // those rows and keep any name that could identify the media type.
    let rows = sqlx::query_as::<
        _,
        (
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        ),
    >(
        r"
        SELECT
            a.sha256,
            a.assets_path,
            MAX(a.mime_type),
            MAX(a.derived_assets_path),
            MAX(a.original_name),
            MAX(a.path)
        FROM attachments a
        JOIN messages m ON m.id = a.message_id
        JOIN conversations c ON c.id = m.conversation_id
        WHERE m.source = $1
          AND c.account_id = $2
          AND a.sha256 IS NOT NULL AND a.sha256 != ''
          AND a.assets_path IS NOT NULL AND a.assets_path != ''
        GROUP BY a.sha256, a.assets_path
        ORDER BY a.sha256
        ",
    )
    .bind(source_id)
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await?;
    let out = rows
        .into_iter()
        .map(
            |(sha256, assets_path, mime_type, derived_assets_path, original_name, source_path)| {
                AssetRow {
                    sha256,
                    assets_path,
                    mime_type,
                    derived_assets_path,
                    original_name,
                    source_path,
                }
            },
        )
        .collect();
    Ok(out)
}

/// Point every attachment row for `original_sha` at its new derived blob.
async fn update_derived(
    conn: &mut AnyConnection,
    account_id: &str,
    source_id: &str,
    original_sha: &str,
    blob: &DerivedBlob,
) -> Result<()> {
    sqlx::query(
        r"
        UPDATE attachments
        SET derived_sha256 = $1, derived_assets_path = $2, derived_mime_type = $3
        WHERE sha256 = $4
          AND message_id IN (
            SELECT m.id FROM messages m
            JOIN conversations c ON c.id = m.conversation_id
            WHERE m.source = $5 AND c.account_id = $6
          )
        ",
    )
    .bind(&blob.sha256)
    .bind(&blob.assets_path)
    .bind(&blob.mime_type)
    .bind(original_sha)
    .bind(source_id)
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Incomplete iMessage/SMS transfers and aborted vault uploads use a `.part` suffix.
fn is_part_path(path: &str) -> bool {
    has_part_extension(Path::new(path))
}

/// True for a `.part` file left by an interrupted upload.
fn has_part_extension(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    ext.eq_ignore_ascii_case("part")
}

/// Max age for abandoned multipart upload sessions under `.incoming/{sha}/{upload_id}/`.
const STALE_UPLOAD_SESSION_SECS: u64 = 24 * 60 * 60;

/// Remove stale `{sha}-*.part` temps and abandoned multipart session dirs under `.incoming/`.
fn cleanup_incoming_parts(assets_dir: &Path, dry_run: bool) -> Result<u64> {
    let incoming = assets_dir.join(".incoming");
    if !incoming.is_dir() {
        return Ok(0);
    }
    let mut removed = 0u64;
    let now = std::time::SystemTime::now();
    for entry in fs::read_dir(&incoming).with_context(|| format!("read {}", incoming.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if !has_part_extension(&path) {
                continue;
            }
            if dry_run {
                println!("[dry-run] would remove {}", path.display());
                removed += 1;
                continue;
            }
            fs::remove_file(&path)
                .with_context(|| format!("remove leftover {}", path.display()))?;
            removed += 1;
            continue;
        }
        if !path.is_dir() {
            continue;
        }
        // Multipart staging: `.incoming/{sha256}/{upload_id}/`
        for session_ent in
            fs::read_dir(&path).with_context(|| format!("read {}", path.display()))?
        {
            let session_ent = session_ent?;
            let session = session_ent.path();
            if !session.is_dir() {
                continue;
            }
            if !upload_session_is_stale(&session, now)? {
                continue;
            }
            if dry_run {
                println!(
                    "[dry-run] would remove stale upload session {}",
                    session.display()
                );
                removed += 1;
                continue;
            }
            fs::remove_dir_all(&session)
                .with_context(|| format!("remove stale upload session {}", session.display()))?;
            removed += 1;
        }
        // Drop empty sha parent dirs.
        if path.is_dir() && fs::read_dir(&path)?.next().is_none() {
            if dry_run {
                println!("[dry-run] would remove empty {}", path.display());
            } else {
                let _ = fs::remove_dir(&path);
            }
        }
    }
    Ok(removed)
}

/// True when a multipart upload session's manifest (or, failing that, its folder) is older than the abandoned-session limit.
fn upload_session_is_stale(session: &Path, now: std::time::SystemTime) -> Result<bool> {
    let manifest = session.join("manifest.json");
    let meta = if manifest.is_file() {
        fs::metadata(&manifest)?
    } else {
        fs::metadata(session)?
    };
    let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
    let age = now.duration_since(modified).unwrap_or_default();
    Ok(age.as_secs() >= STALE_UPLOAD_SESSION_SECS)
}

/// Classify a stored file, falling back to the names the export supplied.
///
/// Stored files are named `<folder>/<sha256>` with no extension, so a row whose
/// `mime_type` is missing (older imports, or a source that declared nothing)
/// would otherwise have no kind and never be converted for the browser.
/// `name_hints` are the attachment's `original_name` and original export `path`,
/// used only when the stored path and the declared MIME say nothing. A declared
/// MIME is authoritative even when it names something that is not media.
///
/// GIFs are animations and never get a still-frame preview: a `.gif` name or
/// an `image/gif` MIME ends the search with `None`.
fn kind_of(assets_path: &str, mime: Option<&str>, name_hints: &[Option<&str>]) -> Option<Kind> {
    if is_part_path(assets_path) {
        return None;
    }
    let declared = mime.and_then(message_ir::trimmed);
    if declared == Some("image/gif") || has_gif_ext(assets_path) {
        return None;
    }
    if let Some(kind) = media::classify(Path::new(assets_path)) {
        return Some(kind);
    }
    if let Some(declared) = declared {
        return media::kind_for_mime(declared);
    }
    name_hints
        .iter()
        .flatten()
        .filter(|hint| !has_gif_ext(hint))
        .find_map(|hint| media::classify(Path::new(hint)))
}

/// True for a `.gif` name, in any case.
fn has_gif_ext(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("gif"))
}

/// Content-addressed relative path: `<aa>/<sha><ext>`.
pub fn derived_rel_path(sha256: &str, ext: &str) -> String {
    crate::assets::shard_rel_path(sha256, ext)
}

/// MIME type for a derived-media extension, from the shared table in
/// [`media`]; `application/octet-stream` for anything unrecognized.
fn mime_for_ext(ext: &str) -> &'static str {
    media::mime_for_ext(ext).unwrap_or("application/octet-stream")
}

/// Write derived bytes into the content-addressed store; the same bytes always land at the same path.
fn store_derived_bytes(derived_dir: &Path, buf: &[u8], ext: &str) -> Result<DerivedBlob> {
    let sha = crate::assets::sha256_hex(buf);
    let rel = derived_rel_path(&sha, ext);
    let dest = derived_dir.join(&rel);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    if !dest.exists() {
        let mut f = File::create(&dest).with_context(|| format!("create {}", dest.display()))?;
        f.write_all(buf)?;
    }
    Ok(DerivedBlob {
        sha256: sha,
        assets_path: rel,
        mime_type: mime_for_ext(ext).to_string(),
    })
}

/// Read a derived file from `work_dir` and store it like [`store_derived_bytes`].
fn store_derived_file(derived_dir: &Path, file_path: &Path, ext: &str) -> Result<DerivedBlob> {
    let buf = fs::read(file_path)?;
    store_derived_bytes(derived_dir, &buf, ext)
}

/// Whether an existing derived file should be skipped (idempotency).
fn should_skip_existing(
    force: bool,
    derived_assets_path: Option<&str>,
    converted_dir: &Path,
) -> bool {
    if force {
        return false;
    }
    match derived_assets_path {
        Some(rel) if !rel.is_empty() => converted_dir.join(rel).is_file(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::engine;

    #[test]
    fn derived_rel_path_layout() {
        let sha = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
        assert_eq!(derived_rel_path(sha, ".jpg"), format!("ab/{sha}.jpg"));
        assert_eq!(derived_rel_path(sha, ".jpeg"), format!("ab/{sha}.jpg"));
    }

    const SHA: &str = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";

    #[test]
    fn kind_classifies_and_skips_gif() {
        assert_eq!(kind_of("x.jpg", None, &[]), Some(Kind::Image));
        assert_eq!(kind_of("x.mp4", None, &[]), Some(Kind::Video));
        assert_eq!(kind_of("x.m4a", None, &[]), Some(Kind::Audio));
        assert_eq!(kind_of("x.gif", None, &[]), None);
        assert_eq!(kind_of("x.bin", Some("image/png"), &[]), Some(Kind::Image));
    }

    #[test]
    fn extensionless_blobs_classify_from_the_attachment_name() {
        let canonical = format!("ab/{SHA}");
        for (name, expected) in [
            ("voice-note.amr", Some(Kind::Audio)),
            ("memo.wav", Some(Kind::Audio)),
            ("podcast.ogg", Some(Kind::Audio)),
            ("clip.3gp", Some(Kind::Video)),
            ("clip.webm", Some(Kind::Video)),
            ("movie.mkv", Some(Kind::Video)),
            ("scan.tiff", Some(Kind::Image)),
            ("notes.txt", None),
        ] {
            assert_eq!(
                kind_of(&canonical, None, &[Some(name), None]),
                expected,
                "unexpected kind for {name}"
            );
            // The original export path is the second-choice hint.
            assert_eq!(
                kind_of(
                    &canonical,
                    Some("  "),
                    &[None, Some(&format!("media/{name}"))]
                ),
                expected,
                "unexpected kind for path hint media/{name}"
            );
        }
    }

    #[test]
    fn attachment_name_hints_never_override_declared_media_types() {
        let canonical = format!("ab/{SHA}");
        // A declared MIME is authoritative, including the deliberate GIF skip.
        assert_eq!(
            kind_of(&canonical, Some("image/gif"), &[Some("clip.mp4")]),
            None
        );
        assert_eq!(
            kind_of(&canonical, Some("application/pdf"), &[Some("clip.mp4")]),
            None
        );
        assert_eq!(kind_of("ab/photo.gif", None, &[Some("clip.mp4")]), None);
        // Incomplete transfers stay out of ffmpeg regardless of the hint.
        assert_eq!(kind_of("ab/upload.part", None, &[Some("clip.mp4")]), None);
    }

    #[test]
    fn skip_existing_derived_file() {
        let dir = tempfile::tempdir().unwrap();
        let rel = "ab/deadbeef.jpg";
        let dest = dir.path().join(rel);
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        fs::write(&dest, b"x").unwrap();
        assert!(should_skip_existing(false, Some(rel), dir.path()));
        assert!(!should_skip_existing(true, Some(rel), dir.path()));
        assert!(!should_skip_existing(
            false,
            Some("missing.jpg"),
            dir.path()
        ));
        assert!(!should_skip_existing(false, None, dir.path()));
    }

    #[tokio::test]
    async fn store_and_update_derived_db() {
        let (pool, dir) = engine::test_pool().await;
        schema::ensure_vault_schema(&mut pool.acquire().await.unwrap())
            .await
            .unwrap();
        let mut conn = pool.acquire().await.unwrap();
        sqlx::query("INSERT INTO accounts (id, username) VALUES ('acc', 'demo')")
            .execute(&mut *conn)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
             VALUES ('acc', '+1', '+1', 'phone', 'phone')",
        )
        .execute(&mut *conn)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO conversations (id, account_id, chat_handle_id, conversation_type, source_file)
             VALUES (1, 'acc', 1, 'individual', 't')",
        )
        .execute(&mut *conn)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO messages (id, conversation_id, account_id, source, timestamp, is_from_me, sort_order)
             VALUES (1, 1, 'acc', 'imessage', '2020-01-01T00:00:00Z', 0, 0)",
        )
        .execute(&mut *conn)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO attachments (id, message_id, sha256, assets_path, mime_type)
             VALUES (1, 1, 'aa11', 'aa/aa11.jpg', 'image/jpeg')",
        )
        .execute(&mut *conn)
        .await
        .unwrap();

        let converted = dir.path().join("converted");
        fs::create_dir_all(&converted).unwrap();
        let blob = store_derived_bytes(&converted, b"jpeg-bytes", ".jpg").unwrap();
        assert!(converted.join(&blob.assets_path).is_file());

        update_derived(&mut conn, "acc", "imessage", "aa11", &blob)
            .await
            .unwrap();

        let (d_sha, d_path, d_mime): (String, String, String) = sqlx::query_as(
            "SELECT derived_sha256, derived_assets_path, derived_mime_type FROM attachments WHERE id = 1",
        )
        .fetch_one(&mut *conn)
        .await
        .unwrap();
        assert_eq!(d_sha, blob.sha256);
        assert_eq!(d_path, blob.assets_path);
        assert_eq!(d_mime, "image/jpeg");
    }

    #[test]
    fn part_paths_are_not_media() {
        assert!(is_part_path("aa/aabbcc.part"));
        assert!(is_part_path("upload.PART"));
        assert!(!is_part_path("aa/aabbcc.mp4"));
        assert_eq!(kind_of("aa/x.part", Some("video/mp4"), &[]), None);
    }

    #[tokio::test]
    async fn listed_attachments_carry_name_hints_for_extensionless_blobs() {
        let (pool, _dir) = engine::test_pool().await;
        schema::ensure_vault_schema(&mut pool.acquire().await.unwrap())
            .await
            .unwrap();
        let mut conn = pool.acquire().await.unwrap();
        for statement in [
            "INSERT INTO accounts (id, username) VALUES ('acc', 'demo')".to_string(),
            "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
                VALUES ('acc', '+1', '+1', 'phone', 'phone')"
                .to_string(),
            "INSERT INTO conversations (id, account_id, chat_handle_id, conversation_type, source_file)
                VALUES (1, 'acc', 1, 'individual', 't')"
                .to_string(),
            "INSERT INTO messages (id, conversation_id, account_id, source, timestamp, is_from_me, sort_order)
                VALUES (1, 1, 'acc', 'imessage', '2020-01-01T00:00:00Z', 0, 0)"
                .to_string(),
            format!(
                "INSERT INTO attachments (id, message_id, sha256, assets_path, mime_type, original_name, path)
                VALUES (1, 1, '{SHA}', 'ab/{SHA}', NULL, 'voice-note.amr', 'attachments/voice-note.amr')"
            ),
        ] {
            sqlx::query(&statement).execute(&mut *conn).await.unwrap();
        }

        let rows = list_attachments(&mut conn, "acc", "imessage")
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(
            kind_of(
                &row.assets_path,
                row.mime_type.as_deref(),
                &row.name_hints()
            ),
            Some(Kind::Audio),
            "an extensionless blob with no declared MIME must classify from its attachment name"
        );
    }
}
