//! Generate browser-friendly derived media under `assets_converted/`.
//!
//! Keeps originals intact, writes content-addressed JPEG/MP4/MP3 blobs, and
//! updates `attachments.derived_*`. The conversions are the `media` crate's,
//! which finds ffmpeg and ffprobe beside the binary, in `MESSAGE_CRATE_BIN`,
//! or on `PATH`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use sqlx::SqliteConnection;
use tempfile::TempDir;

use crate::config::Config;
use crate::db::schema;
use crate::open_db::OpenDb;
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
    /// Only process this source id.
    pub source: Option<String>,
    /// Only process this account. `None` processes every account, which is
    /// what the `process-assets` command does. A Demo Account build names
    /// the Demo Account, so it converts nothing of any other account and
    /// never sweeps another account's upload temps.
    pub account: Option<i64>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
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

/// Run derived-media conversion for every source of the account `opts`
/// names, or of every account in the database when it names none.
///
/// # Errors
///
/// Returns an error when no account is named and the database has none, a query fails, or a
/// source's asset folders cannot be prepared. A conversion that fails for one
/// attachment is counted in `errors` and printed, and the run goes on.
pub async fn run(opened: &OpenDb, opts: &ProcessAssetsOptions) -> Result<ProcessAssetsStats> {
    let cfg = &opened.cfg;
    let mut conn = opened.conn().await?;

    let account_ids = match opts.account {
        Some(account_id) => vec![account_id],
        None => list_account_ids(&mut conn, &cfg.paths.data_dir).await?,
    };
    if account_ids.is_empty() {
        bail!("no accounts found — create an account or run reset-demo first");
    }

    let work = TempDir::new().context("create temp dir for derived media")?;
    let mut stats = ProcessAssetsStats::default();

    for &account_id in &account_ids {
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
    conn: &mut SqliteConnection,
    cfg: &Config,
    opts: &ProcessAssetsOptions,
    account_id: i64,
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
    account_id: i64,
    source_id: &'a str,
    assets_dir: PathBuf,
    converted_dir: PathBuf,
}

/// What deriving one attachment produced.
#[derive(Debug, PartialEq, Eq)]
enum Derived {
    /// The original is not converted: unsupported, already small, or declined.
    Skipped,
    /// A dry run: said what it would write and stored nothing.
    DryRun,
    Stored(DerivedBlob),
}

impl<'a> SourcePass<'a> {
    /// Find the folders, remove abandoned upload temps, and make the
    /// converted folder. `None` when the source has no assets folder to
    /// process.
    ///
    /// # Errors
    ///
    /// Returns an error when the converted folder cannot be made. A failed
    /// removal of an upload temp is logged and does not stop the pass.
    fn open(
        cfg: &Config,
        opts: &'a ProcessAssetsOptions,
        work_dir: &'a Path,
        account_id: i64,
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
        let cleaned = crate::asset_store::sweep_incoming(&assets_dir, opts.dry_run);
        if cleaned > 0 {
            println!("  cleaned {cleaned} abandoned upload temp(s) under .incoming/");
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
    /// Reads the two facts only the disk can supply, lets [`plan`] decide,
    /// then does what the plan says.
    ///
    /// # Errors
    ///
    /// Returns an error when the original is missing, a conversion fails, or
    /// the row cannot be updated.
    async fn process(&self, conn: &mut SqliteConnection, row: &AssetRow) -> Result<Outcome> {
        let source_path = self.assets_dir.join(&row.assets_path);
        let on_disk = OnDisk {
            original_exists: source_path.is_file(),
            derived_exists: derived_file_exists(
                row.derived_assets_path.as_deref(),
                &self.converted_dir,
            ),
        };
        let kind = match plan(row, self.opts, on_disk)? {
            Plan::RemoveIncomplete => return self.remove_incomplete(row, &source_path),
            Plan::Skip(_) => return Ok(Outcome::Skipped),
            Plan::Derive(kind) => kind,
        };
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
    fn remove_incomplete(&self, row: &AssetRow, source_path: &Path) -> Result<Outcome> {
        if source_path.is_file() {
            if self.opts.dry_run {
                println!("[dry-run] would remove incomplete {}", self.label(row));
            } else {
                crate::asset_store::remove_file(source_path)
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
async fn list_account_ids(conn: &mut SqliteConnection, data_dir: &Path) -> Result<Vec<i64>> {
    let mut ids = Vec::new();
    if schema::table_exists(conn, "accounts").await? {
        let rows = sqlx::query_scalar::<_, i64>("SELECT id FROM accounts ORDER BY id")
            .fetch_all(&mut *conn)
            .await?;
        ids = rows;
    }
    if ids.is_empty() && data_dir.is_dir() {
        // Account folders are named by id; anything else under `data/` is
        // not an account.
        for entry in fs::read_dir(data_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir()
                && let Ok(id) = entry.file_name().to_string_lossy().parse::<i64>()
            {
                ids.push(id);
            }
        }
        ids.sort_unstable();
    }
    Ok(ids)
}

/// Source ids for one account: those with messages in the database plus any folder under the account's data dir.
async fn discover_source_ids(
    conn: &mut SqliteConnection,
    account_id: i64,
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

    let account_root = data_dir.join(account_id.to_string());
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
    conn: &mut SqliteConnection,
    account_id: i64,
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
    conn: &mut SqliteConnection,
    account_id: i64,
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

/// Incomplete iMessage/SMS transfers and aborted uploads use a `.part` suffix.
fn is_part_path(path: &str) -> bool {
    crate::asset_store::has_part_extension(Path::new(path))
}

/// What one stored blob needs, decided before any file is touched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Plan {
    /// A `.part` left by an interrupted transfer: delete it, never hand it to ffmpeg.
    RemoveIncomplete,
    /// Leave the original as it is.
    Skip(SkipReason),
    /// Convert the original for the browser as this kind of media.
    Derive(Kind),
}

/// Why a blob is left as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SkipReason {
    /// Not an image, video or audio file, or a GIF, which is never converted.
    NotMedia,
    /// The options turn this kind off (`--skip-image` and friends).
    KindDisabled,
    /// A browser preview already exists and `--force` was not given.
    AlreadyDerived,
}

/// The two facts about one blob that only the filesystem can supply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OnDisk {
    /// The stored original is present under the assets folder.
    original_exists: bool,
    /// The preview the row points at is present under the converted folder.
    derived_exists: bool,
}

/// Decide what one blob needs from the row, the options and what is on disk.
/// No IO happens here: `on_disk` carries the facts the caller read.
///
/// # Errors
///
/// Returns an error when a conversion is wanted and the original is missing.
fn plan(row: &AssetRow, opts: &ProcessAssetsOptions, on_disk: OnDisk) -> Result<Plan> {
    if is_part_path(&row.assets_path) {
        return Ok(Plan::RemoveIncomplete);
    }
    let Some(kind) = media::kind_of(
        Path::new(&row.assets_path),
        row.mime_type.as_deref(),
        &row.name_hints(),
    ) else {
        return Ok(Plan::Skip(SkipReason::NotMedia));
    };
    let wanted = match kind {
        Kind::Image => !opts.skip_image,
        Kind::Video => !opts.skip_video,
        Kind::Audio => !opts.skip_audio,
    };
    if !wanted {
        return Ok(Plan::Skip(SkipReason::KindDisabled));
    }
    if on_disk.derived_exists && !opts.force {
        return Ok(Plan::Skip(SkipReason::AlreadyDerived));
    }
    if !on_disk.original_exists {
        bail!("missing original");
    }
    Ok(Plan::Derive(kind))
}

/// True when the preview `derived_assets_path` names is present under `converted_dir`.
fn derived_file_exists(derived_assets_path: Option<&str>, converted_dir: &Path) -> bool {
    match derived_assets_path {
        Some(rel) if !rel.is_empty() => converted_dir.join(rel).is_file(),
        _ => false,
    }
}

/// Content-addressed relative path: `<aa>/<sha><ext>`.
pub fn derived_rel_path(sha256: &crate::assets_api::Sha256, ext: &str) -> String {
    crate::assets_api::shard_rel_path(sha256, ext)
}

/// MIME type for a derived-media extension, from the shared table in
/// [`media`]; `application/octet-stream` for anything unrecognized.
fn mime_for_ext(ext: &str) -> &'static str {
    media::mime_for_ext(ext).unwrap_or("application/octet-stream")
}

/// Write derived bytes into the content-addressed store; the same bytes always land at the same path.
///
/// A file already at that path is kept only when its bytes hash to the
/// fingerprint in its name. Anything else, such as a preview cut short by an
/// interrupted run, is replaced. The bytes go to a synced temporary file in the
/// same folder first and are renamed over the path, so a run killed partway
/// never leaves a partial file under a content-addressed name.
fn store_derived_bytes(derived_dir: &Path, buf: &[u8], ext: &str) -> Result<DerivedBlob> {
    let sha = crate::assets_api::Sha256::of_bytes(buf);
    let rel = derived_rel_path(&sha, ext);
    let dest = derived_dir.join(&rel);
    let parent = dest
        .parent()
        .with_context(|| format!("derived path {} has no folder", dest.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    if !crate::assets_api::hash_file(&dest).is_ok_and(|actual| actual == sha.as_str()) {
        let mut temporary = tempfile::NamedTempFile::new_in(parent)
            .with_context(|| format!("create temporary preview in {}", parent.display()))?;
        temporary
            .write_all(buf)
            .with_context(|| format!("write temporary preview in {}", parent.display()))?;
        temporary
            .as_file()
            .sync_all()
            .with_context(|| format!("sync temporary preview in {}", parent.display()))?;
        temporary
            .persist(&dest)
            .map_err(|err| err.error)
            .with_context(|| format!("install {}", dest.display()))?;
    }
    Ok(DerivedBlob {
        sha256: sha.to_string(),
        assets_path: rel,
        mime_type: mime_for_ext(ext).to_string(),
    })
}

/// Read a derived file from `work_dir` and store it like [`store_derived_bytes`].
fn store_derived_file(derived_dir: &Path, file_path: &Path, ext: &str) -> Result<DerivedBlob> {
    let buf = fs::read(file_path)?;
    store_derived_bytes(derived_dir, &buf, ext)
}

#[cfg(test)]
pub(crate) mod tests;
