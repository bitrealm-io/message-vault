//! Regenerate the demo bundle, clear the demo account's data, re-import, and process media.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use message_ir::HandleType;
use serde::Deserialize;
use sqlx::Row;

use crate::config::Config;
use crate::db::account_profile;
use crate::db::dialect;
use crate::db::engine::{self, DbTarget};
use crate::db::schema;
use crate::dedupe;
use crate::import::{self, ImportExportArgs, ImportMode};
use crate::open_vault::OpenVault;
use crate::process_assets::{self, ProcessAssetsOptions};

/// Stable demo account id used when `reset-demo` runs without `--account`.
pub use crate::db::account_profile::DEMO_ACCOUNT_ID;

const IMESSAGE_SOURCE: &str = "imessage";
const SBR_SOURCE: &str = "sms-backup-restore";
const WHATSAPP_SOURCE: &str = "whatsapp";

/// Counts reported when a demo reset finishes.
#[derive(Debug)]
pub struct ResetDemoStats {
    /// Stats from regenerating the demo bundle.
    pub seed: demo_seed::GenStats,
    /// Stats from importing the regenerated bundle.
    pub import: import::ImportStats,
    /// Dedupe content keys filled during the reset (one per message; not a duplicate count).
    pub dedupe_keys_filled: u64,
    /// Stats from the post-import media processing pass.
    pub process_assets: process_assets::ProcessAssetsStats,
}

#[derive(Debug, Deserialize)]
struct DemoSeed {
    owner: DemoOwner,
    account: DemoAccount,
}

#[derive(Debug, Deserialize)]
struct DemoOwner {
    display_name: String,
    /// `(raw handle, handle type)` pairs linked into `account_handles`.
    #[serde(default)]
    handle_specs: Vec<(String, HandleType)>,
    #[serde(default)]
    emails: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct DemoAccount {
    username: String,
}

struct PreparedBundle {
    seed: DemoSeed,
    imessage_dir: PathBuf,
    sbr_dir: PathBuf,
    whatsapp_dir: PathBuf,
    contacts_vcf: PathBuf,
}

/// One per-source import in a demo reset. [`import_demo_sources`] loops over
/// [`DEMO_IMPORT_SOURCES`] for both transports, so the sources, their order,
/// and their Replace-then-Append modes stay in sync.
struct DemoImportSource {
    /// Label printed in the "Reset demo — preparing replacement" header.
    label: &'static str,
    /// Source id recorded on the imported conversations.
    source: &'static str,
    /// Staging directory inside the prepared bundle.
    staging_dir: fn(&PreparedBundle) -> &PathBuf,
    /// The first source replaces the demo account's data; the rest append.
    mode: ImportMode,
    /// Load the bundle's contacts VCF and overwrite existing contacts.
    with_contacts: bool,
}

const DEMO_IMPORT_SOURCES: [DemoImportSource; 3] = [
    DemoImportSource {
        label: "imessage",
        source: IMESSAGE_SOURCE,
        staging_dir: |bundle| &bundle.imessage_dir,
        mode: ImportMode::Replace,
        with_contacts: true,
    },
    DemoImportSource {
        label: "android",
        source: SBR_SOURCE,
        staging_dir: |bundle| &bundle.sbr_dir,
        mode: ImportMode::Append,
        with_contacts: false,
    },
    DemoImportSource {
        label: "whatsapp",
        source: WHATSAPP_SOURCE,
        staging_dir: |bundle| &bundle.whatsapp_dir,
        mode: ImportMode::Append,
        with_contacts: false,
    },
];

/// Print the "preparing replacement" header both reset transports share.
fn print_reset_header(account_id: i64, prepared: &PreparedBundle, db: &dyn std::fmt::Display) {
    println!("Reset demo — preparing replacement");
    println!("  account:      {account_id}");
    for source in &DEMO_IMPORT_SOURCES {
        println!(
            "  {:<14}{}",
            format!("{}:", source.label),
            (source.staging_dir)(prepared).display()
        );
    }
    println!("  db:           {db}");
}

/// Shared post-import tail: fill dedupe content keys, convert media, and warn
/// (but continue) when some attachments fail conversion.
async fn dedupe_and_process_assets(
    cfg: &Config,
    account_id: i64,
    target: DbTarget<'_>,
) -> Result<(dedupe::DedupeStats, process_assets::ProcessAssetsStats)> {
    let (db, db_url) = match target {
        DbTarget::Url(url) => (None, Some(url.to_string())),
        DbTarget::Path(path) => (Some(path.to_path_buf()), None),
    };
    let vault = OpenVault::open(cfg.clone().with_db_overrides(db, db_url)).await?;
    let dedupe_stats = {
        let mut conn = vault.conn().await?;
        dedupe::dedupe_cross_source(&mut conn, account_id, None, 2).await?
    };
    println!("Reset demo — processing prepared assets");
    let process_stats = process_assets::run(
        &vault,
        &ProcessAssetsOptions {
            force: false,
            dry_run: false,
            skip_image: false,
            skip_video: false,
            skip_audio: false,
            source: None,
        },
    )
    .await
    .context("process-assets after prepared demo import")?;
    vault.close().await;
    if process_stats.errors > 0 {
        eprintln!(
            "warning: {} demo attachment(s) failed conversion; originals stay in place and reset-demo continues",
            process_stats.errors
        );
    }
    Ok((dedupe_stats, process_stats))
}

struct ResetPreparedStats {
    import: import::ImportStats,
    dedupe_keys_filled: u64,
    process_assets: process_assets::ProcessAssetsStats,
}

/// Parent directory of `path`, or `.` when the path has no parent.
fn parent_dir_or_cwd(path: &Path) -> &Path {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

/// Work directory for the prepared account tree. Must live on the same mount
/// as `data_dir` so the later install can `rename` into `data_dir/<account>`.
fn reset_account_work_dir(data_dir: &Path) -> Result<tempfile::TempDir> {
    fs::create_dir_all(data_dir)
        .with_context(|| format!("create data directory {}", data_dir.display()))?;
    tempfile::Builder::new()
        .prefix(".reset-demo-data-")
        .tempdir_in(data_dir)
        .with_context(|| {
            format!(
                "create temporary demo account directory in {}",
                data_dir.display()
            )
        })
}

/// Rebuild the demo vault from the bundle at `bundle` and write the active
/// config to `config_dest`.
///
/// # Errors
///
/// Returns an error when the bundle is incomplete, the database cannot be
/// replaced, or import / media processing fails.
pub async fn run_reset_demo(
    bundle: &Path,
    config_dest: &Path,
    db_url: Option<&str>,
) -> Result<ResetDemoStats> {
    let bundle = if bundle.is_absolute() {
        bundle.to_path_buf()
    } else {
        std::env::current_dir()?.join(bundle)
    };

    println!("  bundle:       {}", bundle.display());
    let seed_stats = maybe_regenerate_bundle(&bundle, &demo_seed::SeedConfig::default_path())?;
    let reset_stats =
        prepare_config_and_reset(&bundle, config_dest, DEMO_ACCOUNT_ID, db_url).await?;

    Ok(ResetDemoStats {
        seed: seed_stats,
        import: reset_stats.import,
        dedupe_keys_filled: reset_stats.dedupe_keys_filled,
        process_assets: reset_stats.process_assets,
    })
}

/// Refuse a config that serves the database from a URL. The SQLite reset
/// replaces the file at `paths.db`, which cannot reach a URL-served database;
/// `--db-url` takes the other transport and never reaches this check.
fn refuse_url_config(cfg: &Config) -> Result<()> {
    if let Some(url) = cfg.database.url.as_deref() {
        bail!(
            "reset-demo replaces the on-disk vault at paths.db, but this config serves the database from {}; URL-served databases cannot be reset this way — run reset-demo on the host that owns the database file, or pass --db-url",
            engine::redact_db_url(url)
        );
    }
    Ok(())
}

/// Copy the bundle's config into place and reset the account: the connection-URL path when
/// `db_url` is set, else the SQLite snapshot-and-swap path.
async fn prepare_config_and_reset(
    bundle: &Path,
    config_dest: &Path,
    account_id: i64,
    db_url: Option<&str>,
) -> Result<ResetPreparedStats> {
    validate_prepared_bundle(bundle)?;
    let demo_config = bundle.join("config/config.toml");
    if !demo_config.is_file() {
        bail!(
            "incomplete demo bundle under {} (need config/config.toml)",
            bundle.display()
        );
    }
    if let Some(url) = db_url {
        let cfg = if config_dest.is_file() {
            Config::load(config_dest)?
        } else {
            Config::load(&demo_config)?
        };
        return reset_prepared_bundle_at_url(&cfg, bundle, account_id, url).await;
    }
    let config_parent = parent_dir_or_cwd(config_dest);
    fs::create_dir_all(config_parent)
        .with_context(|| format!("create config directory {}", config_parent.display()))?;
    let temporary_config = tempfile::Builder::new()
        .prefix(".reset-demo-config-")
        .tempfile_in(config_parent)
        .context("create temporary demo config")?;
    fs::copy(&demo_config, temporary_config.path()).with_context(|| {
        format!(
            "copy prepared config {} to {}",
            demo_config.display(),
            temporary_config.path().display()
        )
    })?;
    let cfg = Config::load(temporary_config.path())?;
    refuse_url_config(&cfg)?;
    let temporary_config = temporary_config.into_temp_path();
    reset_prepared_bundle(
        &cfg,
        bundle,
        account_id,
        config_dest,
        temporary_config.as_ref(),
    )
    .await
}

/// Connection-URL reset (Postgres, or SQLite by URL): rebuild the demo account
/// in the live database. There is no snapshot to swap, so this path relies on
/// the wipe being scoped to one account.
async fn reset_prepared_bundle_at_url(
    cfg: &Config,
    bundle: &Path,
    account_id: i64,
    db_url: &str,
) -> Result<ResetPreparedStats> {
    let prepared = validate_prepared_bundle(bundle)?;
    rebuild_demo_account(cfg, &prepared, account_id, DbTarget::Url(db_url)).await
}

/// SQLite reset: build the new state in a prepared database next to the active one, prove
/// nothing outside the demo account changed, then swap it in.
async fn reset_prepared_bundle(
    cfg: &Config,
    bundle: &Path,
    account_id: i64,
    config_dest: &Path,
    prepared_config: &Path,
) -> Result<ResetPreparedStats> {
    let prepared = validate_prepared_bundle(bundle)?;
    let _operation_lock = crate::operation_lock::acquire_for_reset(&cfg.paths.db)?;
    crate::operation_lock::clear_ready(&cfg.paths.db)?;
    let db_parent = parent_dir_or_cwd(&cfg.paths.db);
    fs::create_dir_all(db_parent)
        .with_context(|| format!("create database parent {}", db_parent.display()))?;
    let db_work = tempfile::Builder::new()
        .prefix(".reset-demo-db-")
        .tempdir_in(db_parent)
        .context("create temporary demo database directory")?;
    // Keep the prepared account tree on the same mount as data_dir so
    // install can rename into data_dir/<account>. A work directory on
    // another mount (tmp, a nested bind, a named volume) fails with EXDEV.
    let data_work = reset_account_work_dir(&cfg.paths.data_dir)?;
    let prepared_db = db_work.path().join("vault.db");
    checkpoint_and_clean_sidecars(&cfg.paths.db, "before creating the reset snapshot").await?;
    prepare_database_snapshot(&cfg.paths.db, &prepared_db).await?;

    let mut temporary_cfg = cfg.clone();
    temporary_cfg.paths.db = prepared_db.clone();
    temporary_cfg.paths.data_dir = data_work.path().to_path_buf();
    let stats = rebuild_demo_account(
        &temporary_cfg,
        &prepared,
        account_id,
        DbTarget::Path(&prepared_db),
    )
    .await?;

    verify_non_demo_state_preserved(&cfg.paths.db, &prepared_db, account_id).await?;
    let active_account = cfg.paths.data_dir.join(account_id.to_string());
    let prepared_account = temporary_cfg.paths.data_dir.join(account_id.to_string());
    let paths = ResetPaths {
        active_db: &cfg.paths.db,
        prepared_db: &prepared_db,
        active_account: &active_account,
        prepared_account: &prepared_account,
        active_config: config_dest,
        prepared_config,
    };
    install_reset_state_or_keep_work(&paths, db_work, data_work).await?;
    crate::operation_lock::mark_ready(&cfg.paths.db)?;
    Ok(stats)
}

/// Swap the prepared state in. When the swap fails and its rollback left any
/// of the previous state in the work directories, keep those directories on
/// disk and name them in the error so nothing is lost.
async fn install_reset_state_or_keep_work(
    paths: &ResetPaths<'_>,
    db_work: tempfile::TempDir,
    data_work: tempfile::TempDir,
) -> Result<()> {
    let Err(error) = install_reset_state(paths).await else {
        return Ok(());
    };
    let config_backup = sqlite_sidecar(paths.prepared_config, ".previous-active");
    let previous_state_still_in_work = db_work.path().join("previous-vault.db").exists()
        || data_work.path().join("previous-account").exists()
        || config_backup.exists();
    if previous_state_still_in_work {
        let db_work = db_work.keep();
        let data_work = data_work.keep();
        return Err(error.context(format!(
            "reset-demo rollback was incomplete; temporary database and account state were kept at {} and {}",
            db_work.display(),
            data_work.display()
        )));
    }
    Err(error)
}

/// Wipe, seed, import, dedupe, convert media, and vacuum the demo account on
/// `target`. Both transports run exactly this; what differs is what `target`
/// names and what the caller does around it (the SQLite path snapshots the
/// database first and swaps it in after).
async fn rebuild_demo_account(
    cfg: &Config,
    prepared: &PreparedBundle,
    account_id: i64,
    target: DbTarget<'_>,
) -> Result<ResetPreparedStats> {
    wipe_demo_account(cfg, account_id, target).await?;
    print_reset_header(account_id, prepared, &target);
    seed_demo_account(target, account_id, &prepared.seed).await?;
    let import = import_demo_sources(cfg, prepared, account_id, target).await?;
    let (dedupe_stats, process_stats) = dedupe_and_process_assets(cfg, account_id, target).await?;
    vacuum_after_demo(target).await;
    Ok(ResetPreparedStats {
        import,
        dedupe_keys_filled: dedupe_stats.keys_filled,
        process_assets: process_stats,
    })
}

/// Import the staged sources in [`DEMO_IMPORT_SOURCES`] order: the first
/// replaces the account's data and the rest append. Returns the summed counts.
async fn import_demo_sources(
    cfg: &Config,
    prepared: &PreparedBundle,
    account_id: i64,
    target: DbTarget<'_>,
) -> Result<import::ImportStats> {
    let mut totals = import::ImportStats::default();
    for source in &DEMO_IMPORT_SOURCES {
        let assets_dir = cfg.paths.assets_dir_for_account(account_id, source.source);
        let stats = import::import_export(&ImportExportArgs {
            export_dir: (source.staging_dir)(prepared),
            db: target,
            assets_dir: &assets_dir,
            contacts: source
                .with_contacts
                .then_some(prepared.contacts_vcf.as_path()),
            overwrite_contacts: source.with_contacts,
            mode: source.mode,
            source: source.source,
            account_id,
        })
        .await?;
        totals.add_run(&stats);
    }
    Ok(totals)
}
/// Check the bundle has its seed, the three staging folders, and the contacts file, and return their paths.
fn validate_prepared_bundle(bundle: &Path) -> Result<PreparedBundle> {
    let demo_seed = bundle.join("config/seed.toml");
    let imessage_dir = bundle.join("staging").join(IMESSAGE_SOURCE);
    let sbr_dir = bundle.join("staging").join(SBR_SOURCE);
    let whatsapp_dir = bundle.join("staging").join(WHATSAPP_SOURCE);
    let contacts_vcf = bundle.join("config/contacts.vcf");
    if !demo_seed.is_file()
        || !imessage_dir.is_dir()
        || !sbr_dir.is_dir()
        || !whatsapp_dir.is_dir()
        || !contacts_vcf.is_file()
    {
        bail!(
            "incomplete demo bundle under {} (need config/seed.toml, \
             staging/{IMESSAGE_SOURCE}/, staging/{SBR_SOURCE}/, staging/{WHATSAPP_SOURCE}/, config/contacts.vcf)",
            bundle.display()
        );
    }
    Ok(PreparedBundle {
        seed: load_demo_seed(&demo_seed)?,
        imessage_dir,
        sbr_dir,
        whatsapp_dir,
        contacts_vcf,
    })
}

/// Copy the active database to the prepared path (checkpointing the WAL first) so the reset
/// works on a snapshot and the live file is untouched until the swap.
async fn prepare_database_snapshot(active: &Path, prepared: &Path) -> Result<()> {
    if active.is_file() {
        let pool = engine::open_pool_for_path(active)
            .await
            .with_context(|| format!("open {} for reset snapshot", active.display()))?;
        let mut conn = pool
            .acquire()
            .await
            .with_context(|| format!("open {} for reset snapshot", active.display()))?;
        sqlx::query("VACUUM INTO $1")
            .bind(prepared.to_string_lossy().as_ref())
            .execute(&mut *conn)
            .await
            .with_context(|| {
                format!(
                    "copy database snapshot {} to {}",
                    active.display(),
                    prepared.display()
                )
            })?;
        conn.close().await?;
        pool.close().await;
    } else {
        let pool = engine::open_pool_for_path(prepared)
            .await
            .with_context(|| format!("create prepared database {}", prepared.display()))?;
        let mut conn = pool
            .acquire()
            .await
            .with_context(|| format!("create prepared database {}", prepared.display()))?;
        schema::ensure_vault_schema(&mut conn).await?;
        conn.close().await?;
        pool.close().await;
    }
    Ok(())
}

/// Copy pending SQLite writes into the main database file, then remove the
/// write-ahead log (`-wal`) and shared-memory (`-shm`) sidecar files so the
/// database can be renamed safely.
async fn checkpoint_and_clean_sidecars(db: &Path, operation: &str) -> Result<()> {
    if !db.is_file() {
        return Ok(());
    }
    let pool = engine::open_pool_for_path(db)
        .await
        .with_context(|| format!("open {} {operation}", db.display()))?;
    let mut conn = pool
        .acquire()
        .await
        .with_context(|| format!("open {} {operation}", db.display()))?;
    let row = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .fetch_one(&mut *conn)
        .await
        .with_context(|| {
            format!(
                "checkpoint SQLite write-ahead log for {} {operation}",
                db.display()
            )
        })?;
    let busy: i64 = row.try_get(0)?;
    let _log: i64 = row.try_get(1)?;
    let _checkpointed: i64 = row.try_get(2)?;
    // Close the connection deterministically: `pool.close()` only waits for
    // checked-out connections to be *returned*, and the sqlx worker thread
    // runs `sqlite3_close` later. A close that lands after the TRUNCATE
    // checkpoint can read the truncated `-shm` mapping and crash the process.
    conn.close().await?;
    // Close the pool so no connection stays attached to the database while
    // reset-demo replaces or renames it.
    pool.close().await;
    if busy != 0 {
        bail!(
            "cannot replace {} because its WAL could not be checkpointed; stop every process using the vault and run reset-demo offline",
            db.display()
        );
    }

    let wal = sqlite_sidecar(db, "-wal");
    if wal.exists() {
        let length = fs::metadata(&wal)
            .with_context(|| format!("inspect SQLite WAL {}", wal.display()))?
            .len();
        if length != 0 {
            bail!(
                "cannot replace {} because {} still contains {length} bytes after WAL checkpoint; stop every process using the vault and run reset-demo offline",
                db.display(),
                wal.display()
            );
        }
        fs::remove_file(&wal).with_context(|| {
            format!(
                "remove empty SQLite WAL {}; reset-demo requires offline database access",
                wal.display()
            )
        })?;
    }
    let shm = sqlite_sidecar(db, "-shm");
    if shm.exists() {
        fs::remove_file(&shm).with_context(|| {
            format!(
                "remove SQLite shared-memory sidecar {}; stop every process using the vault and run reset-demo offline",
                shm.display()
            )
        })?;
    }
    Ok(())
}

/// The `-wal` or `-shm` sidecar path next to a SQLite database file.
fn sqlite_sidecar(db: &Path, suffix: &str) -> PathBuf {
    let mut path: OsString = db.as_os_str().to_owned();
    path.push(suffix);
    PathBuf::from(path)
}

/// Refuse to install the prepared database if any non-demo account's row counts differ from
/// the active one: a reset must only ever touch the demo account.
async fn verify_non_demo_state_preserved(
    active: &Path,
    prepared: &Path,
    demo_id: i64,
) -> Result<()> {
    if !active.is_file() {
        return Ok(());
    }
    let active_state = non_demo_state(active, demo_id).await?;
    let prepared_state = non_demo_state(prepared, demo_id).await?;
    if active_state != prepared_state {
        bail!(
            "prepared reset database changed non-demo account state; active={active_state:?}, prepared={prepared_state:?}"
        );
    }
    Ok(())
}

/// Row counts per table for every account except the demo one, used to prove a reset changed nothing else.
async fn non_demo_state(db: &Path, demo_id: i64) -> Result<BTreeMap<i64, i64>> {
    let pool = engine::open_pool_for_path(db)
        .await
        .with_context(|| format!("open {} to verify non-demo accounts", db.display()))?;
    let mut conn = pool
        .acquire()
        .await
        .with_context(|| format!("open {} to verify non-demo accounts", db.display()))?;
    let has_accounts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'accounts'",
    )
    .fetch_one(&mut *conn)
    .await
    .with_context(|| format!("check accounts table in {}", db.display()))?;
    if has_accounts == 0 {
        conn.close().await?;
        pool.close().await;
        return Ok(BTreeMap::new());
    }
    let rows = sqlx::query(
        "SELECT a.id, COUNT(m.id)
         FROM accounts a
         LEFT JOIN messages m ON m.account_id = a.id
         WHERE a.id != $1
         GROUP BY a.id
         ORDER BY a.id",
    )
    .bind(demo_id)
    .fetch_all(&mut *conn)
    .await?;
    let mut state = BTreeMap::new();
    for row in rows {
        let account_id: i64 = row.try_get(0)?;
        let message_count: i64 = row.try_get(1)?;
        state.insert(account_id, message_count);
    }
    conn.close().await?;
    pool.close().await;
    Ok(state)
}

#[derive(Clone, Copy)]
struct ResetPaths<'a> {
    active_db: &'a Path,
    prepared_db: &'a Path,
    active_account: &'a Path,
    prepared_account: &'a Path,
    active_config: &'a Path,
    prepared_config: &'a Path,
}

/// Swap the prepared database, account folder, and config into their active paths.
async fn install_reset_state(paths: &ResetPaths<'_>) -> Result<()> {
    install_reset_state_with(paths, demo_seed::move_path).await
}

/// [`install_reset_state`] with the rename step injected, so tests can simulate a rename that fails midway.
async fn install_reset_state_with<F>(paths: &ResetPaths<'_>, rename: F) -> Result<()>
where
    F: FnMut(&Path, &Path) -> Result<()>,
{
    checkpoint_and_clean_sidecars(paths.prepared_db, "before installing the prepared database")
        .await?;
    checkpoint_and_clean_sidecars(
        paths.active_db,
        "immediately before replacing the active database",
    )
    .await?;
    replace_reset_state_with(paths, rename)
}

/// One of the three things a reset swaps: the database file, the account
/// folder, or the config file. Each has an active path, a prepared
/// replacement, and a backup path the active one is moved to first so the
/// swap can be undone.
struct Swap<'a> {
    /// What the paths hold, for messages: "database", "account directory", "config".
    what: &'static str,
    active: &'a Path,
    prepared: &'a Path,
    backup: PathBuf,
    /// Whether an active file existed before the swap, so there is a backup to restore.
    had_active: bool,
    /// Whether the prepared file has been moved into the active path.
    installed: bool,
}

impl<'a> Swap<'a> {
    fn new(what: &'static str, active: &'a Path, prepared: &'a Path, backup: PathBuf) -> Self {
        Self {
            what,
            active,
            prepared,
            backup,
            had_active: active.exists(),
            installed: false,
        }
    }

    /// Move the active file to its backup path, when there is one.
    fn back_up(&self, rename: &mut impl FnMut(&Path, &Path) -> Result<()>) -> Result<()> {
        if !self.had_active {
            return Ok(());
        }
        rename(self.active, &self.backup).with_context(|| {
            format!(
                "move existing {} {} into backup",
                self.what,
                self.active.display()
            )
        })
    }

    /// Move the prepared file into the active path.
    fn install(&mut self, rename: &mut impl FnMut(&Path, &Path) -> Result<()>) -> Result<()> {
        rename(self.prepared, self.active).with_context(|| {
            format!(
                "install prepared {} {} at {}",
                self.what,
                self.prepared.display(),
                self.active.display()
            )
        })?;
        self.installed = true;
        Ok(())
    }

    /// Undo whatever this swap did: remove an installed file, then put the
    /// backup back. Every step is attempted; the problems met are returned
    /// rather than stopping at the first, so as much as possible is restored.
    fn roll_back(&self, rename: &mut impl FnMut(&Path, &Path) -> Result<()>) -> Vec<String> {
        let mut problems = Vec::new();
        if self.installed
            && let Err(error) = remove_any_if_exists(self.active)
        {
            problems.push(format!(
                "remove installed {} {}: {error:#}",
                self.what,
                self.active.display()
            ));
        }
        if self.had_active
            && self.backup.exists()
            && let Err(error) = rename(&self.backup, self.active)
        {
            problems.push(format!(
                "restore previous {} {}: {error:#}",
                self.what,
                self.active.display()
            ));
        }
        problems
    }
}

impl<'a> ResetPaths<'a> {
    /// The three swaps in install order: database, account folder, config.
    fn swaps(&self) -> Result<[Swap<'a>; 3]> {
        let db_backup = self
            .prepared_db
            .parent()
            .context("prepared database has no parent")?
            .join("previous-vault.db");
        let account_backup = self
            .prepared_account
            .parent()
            .context("prepared account has no parent")?
            .join("previous-account");
        let config_backup = sqlite_sidecar(self.prepared_config, ".previous-active");
        Ok([
            Swap::new("database", self.active_db, self.prepared_db, db_backup),
            Swap::new(
                "account directory",
                self.active_account,
                self.prepared_account,
                account_backup,
            ),
            Swap::new(
                "config",
                self.active_config,
                self.prepared_config,
                config_backup,
            ),
        ])
    }
}

/// The swap itself: move the active state to backups, move the prepared state in, and roll
/// the backups back if any step fails.
fn replace_reset_state_with<F>(paths: &ResetPaths<'_>, mut rename: F) -> Result<()>
where
    F: FnMut(&Path, &Path) -> Result<()>,
{
    if !paths.prepared_db.is_file()
        || !paths.prepared_account.is_dir()
        || !paths.prepared_config.is_file()
    {
        bail!("prepared reset state is incomplete");
    }
    if let Some(parent) = paths.active_account.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create account data parent {}", parent.display()))?;
    }
    let mut swaps = paths.swaps()?;

    let outcome = (|| -> Result<()> {
        for swap in &swaps {
            swap.back_up(&mut rename)?;
        }
        for swap in &mut swaps {
            swap.install(&mut rename)?;
        }
        Ok(())
    })();
    let Err(error) = outcome else {
        cleanup_reset_backups(&swaps);
        return Ok(());
    };

    let problems: Vec<String> = swaps
        .iter()
        .rev()
        .flat_map(|swap| swap.roll_back(&mut rename))
        .collect();
    if problems.is_empty() {
        cleanup_reset_backups(&swaps);
        return Err(error.context("replace demo account state"));
    }
    let kept: Vec<String> = swaps
        .iter()
        .map(|swap| swap.backup.display().to_string())
        .collect();
    Err(anyhow::anyhow!(
        "replace demo account state: {error:#}; rollback incomplete; backups kept at {}: {}",
        kept.join(", "),
        problems.join("; ")
    ))
}

/// Remove the backups once the active state is installed or restored. Failure
/// is a warning: the state is right, only a copy is left behind.
fn cleanup_reset_backups(swaps: &[Swap<'_>]) {
    for swap in swaps {
        if let Err(error) = remove_any_if_exists(&swap.backup) {
            eprintln!(
                "warning: reset-demo installed or restored active state but could not remove backup {}: {error:#}",
                swap.backup.display()
            );
        }
    }
}

/// Remove a file or folder tree; a missing path is not an error.
fn remove_any_if_exists(path: &Path) -> Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path).with_context(|| format!("remove {}", path.display()))?;
    } else if path.exists() {
        fs::remove_file(path).with_context(|| format!("remove {}", path.display()))?;
    }
    Ok(())
}

/// Rebuild the demo bundle from `seed_toml` when that file is present (a
/// development checkout, where it is the crate's `demo_seed.toml`). Release
/// images copy a generated staging/config tree and omit the seed file, so a
/// complete bundle is used as it is and an incomplete one is an error.
///
/// Staging here is the temporary import area under the demo bundle
/// (`staging/imessage`, and so on).
fn maybe_regenerate_bundle(bundle: &Path, seed_toml: &Path) -> Result<demo_seed::GenStats> {
    if seed_toml.is_file() {
        println!(
            "Reset demo — regenerating bundle from {}",
            seed_toml.display()
        );
        return demo_seed::generate_to(seed_toml, bundle)
            .context("regenerate demo bundle (demo-seed)");
    }

    let complete = bundle.join("config/seed.toml").is_file()
        && bundle.join("staging").join(IMESSAGE_SOURCE).is_dir()
        && bundle.join("staging").join(SBR_SOURCE).is_dir()
        && bundle.join("staging").join(WHATSAPP_SOURCE).is_dir()
        && bundle.join("config/contacts.vcf").is_file();
    if complete {
        println!(
            "Reset demo — using image bundle (no {} in this image)",
            seed_toml.display()
        );
        return Ok(demo_seed::GenStats::default());
    }

    bail!(
        "cannot reset demo: {} is missing and {} is not a complete demo bundle \
         (need staging/{IMESSAGE_SOURCE}/, staging/{SBR_SOURCE}/, and staging/{WHATSAPP_SOURCE}/)",
        seed_toml.display(),
        bundle.display()
    );
}

/// Compact import tables after the sample inbox is fully loaded. Best effort:
/// failures are printed, not returned, because the demo rows are already committed.
async fn vacuum_after_demo(target: DbTarget<'_>) {
    let pool = match target.open().await {
        Ok(pool) => pool,
        Err(err) => {
            eprintln!("  sql:      warning: vacuum after demo failed to open the database: {err}");
            return;
        }
    };
    vacuum_after_demo_on_pool(pool).await;
}
/// Reclaim space after the demo import replaced most rows. Best effort: a failed vacuum only costs disk space.
async fn vacuum_after_demo_on_pool(pool: sqlx::AnyPool) {
    let mut conn = match pool.acquire().await {
        Ok(conn) => conn,
        Err(err) => {
            eprintln!("  sql:      warning: vacuum after demo failed to open a connection: {err}");
            pool.close().await;
            return;
        }
    };
    dialect::vacuum_import_tables(&mut conn).await;
    // Close deterministically before reset-demo renames the SQLite file.
    // `pool.close()` only waits for the connection to be returned; the
    // sqlx worker runs sqlite3_close later.
    if let Err(err) = conn.close().await {
        eprintln!("  sql:      warning: vacuum after demo failed to close the connection: {err}");
    }
    pool.close().await;
}

/// Parse `config/seed.toml` from the bundle.
fn load_demo_seed(path: &Path) -> Result<DemoSeed> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read demo seed {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("failed to parse demo seed {}", path.display()))
}

/// Open the target database and seed the demo account row and profile.
async fn seed_demo_account(target: DbTarget<'_>, account_id: i64, seed: &DemoSeed) -> Result<()> {
    let pool = target.open().await?;
    let mut conn = pool.acquire().await?;
    schema::ensure_vault_schema(&mut conn).await?;
    seed_demo_account_on_conn(&mut conn, account_id, seed).await?;
    seed_demo_owner_on_conn(&mut conn).await?;
    conn.close().await?;
    pool.close().await;
    Ok(())
}
/// Credentials the demo vault's owner signs in with. A demo vault is
/// throwaway, so these are the obvious pair rather than a secret; they exist
/// only here, because the claim route and `create-owner` both run the
/// password policy and `admin` is five characters.
pub const DEMO_OWNER_USERNAME: &str = "admin";
const DEMO_OWNER_PASSWORD: &str = "admin";

/// Claim the demo vault.
///
/// Without an owner, a seeded vault would be unclaimed and the entry screen
/// would offer Create Vault Owner and no login at all — so the documented
/// "sign in as `demo`" would reach a screen with nowhere to type it. The row
/// is written directly, the way the demo account's own row is, which is what
/// lets the password be shorter than the policy allows.
async fn seed_demo_owner_on_conn(conn: &mut sqlx::AnyConnection) -> Result<()> {
    let hash = crate::credentials::hash_password(DEMO_OWNER_PASSWORD)?;
    sqlx::query(
        r"
        INSERT INTO accounts (id, username, password_hash)
        VALUES ($1, $2, $3)
        ON CONFLICT(id) DO UPDATE SET
            username = excluded.username,
            password_hash = excluded.password_hash
        ",
    )
    .bind(account_profile::OWNER_ACCOUNT_ID)
    .bind(DEMO_OWNER_USERNAME)
    .bind(&hash)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Create the demo account row and the profile fields the seed names, so the demo signs in without setup.
async fn seed_demo_account_on_conn(
    conn: &mut sqlx::AnyConnection,
    account_id: i64,
    seed: &DemoSeed,
) -> Result<()> {
    account_profile::ensure_account_row(conn, account_id).await?;

    // The demo account exists so someone can try the whole vault without
    // making an account of their own, so it may import, export, and delete
    // like any other account. Reading a conversation goes through the export
    // route, so a demo account without export cannot open a single thread.
    sqlx::query(
        r"
        INSERT INTO accounts (
            id, username, password_hash, preferred_name, can_import, can_export, can_delete
        )
        VALUES ($1, $2, NULL, $3, 1, 1, 1)
        ON CONFLICT(id) DO UPDATE SET
            username = excluded.username,
            preferred_name = excluded.preferred_name,
            can_import = excluded.can_import,
            can_export = excluded.can_export,
            can_delete = excluded.can_delete
        ",
    )
    .bind(account_id)
    .bind(&seed.account.username)
    .bind(&seed.owner.display_name)
    .execute(&mut *conn)
    .await?;
    // Extra email addresses used only to recognize "you" in messages, not for login.
    sqlx::query("DELETE FROM account_emails WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut *conn)
        .await?;
    for email in &seed.owner.emails {
        sqlx::query(
            r"
            INSERT INTO account_emails (account_id, email, is_primary)
            VALUES ($1, $2, 0)
            ON CONFLICT DO NOTHING
            ",
        )
        .bind(account_id)
        .bind(email)
        .execute(&mut *conn)
        .await?;
    }
    // Demo has no Import API token until the user generates one in Settings.

    // Phone and email identities that mark messages as from "you" live in
    // `handles`, linked through `account_handles`.
    sqlx::query("DELETE FROM account_handles WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut *conn)
        .await?;
    for (raw, handle_type) in &seed.owner.handle_specs {
        account_profile::link_account_handle(conn, account_id, raw, *handle_type).await?;
    }
    Ok(())
}

/// Delete the demo account's vault rows (child rows follow via CASCADE) and
/// on-disk attachments. Leaves the database and other accounts intact.
async fn wipe_demo_account(cfg: &Config, account_id: i64, target: DbTarget<'_>) -> Result<()> {
    println!("Reset demo — clearing account data in {target}");
    let pool = target.open().await?;
    let mut conn = pool
        .acquire()
        .await
        .with_context(|| format!("open {target} for demo account wipe"))?;
    schema::ensure_vault_schema(&mut conn).await?;
    let deleted = sqlx::query("DELETE FROM accounts WHERE id = $1")
        .bind(account_id)
        .execute(&mut *conn)
        .await
        .with_context(|| format!("delete account {account_id}"))?
        .rows_affected();
    println!("  sql:      demo account rows removed (accounts matched={deleted})");
    conn.close().await?;
    pool.close().await;

    let account_root = cfg.paths.data_dir.join(account_id.to_string());
    remove_tree_if_exists(&account_root)?;
    Ok(())
}
/// Remove a folder tree; a missing folder is not an error.
fn remove_tree_if_exists(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path).with_context(|| format!("remove {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
