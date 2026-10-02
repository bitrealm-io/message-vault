//! Generate the demo bundle, clear the demo account's data, import, load the
//! demo's address book, and process media.
//!
//! Three callers: `reset-demo`; `serve` on a database that does not exist
//! yet ([`seed_new_database`]), which is how every new Message Crate starts
//! with the Demo Account; and `PUT /v1/server/demo-account`
//! ([`build_demo_account`]), the owner's rebuild on a running server.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use demo_seed::DemoSize;
use message_ir::HandleType;
use serde::Deserialize;
use sqlx::Row;

use crate::config::Config;
use crate::db::account_profile;
use crate::db::address_book::{self, LoadCounts, LoadMode};
use crate::db::dialect;
use crate::db::engine;
use crate::db::schema;
use crate::dedupe;
use crate::imports_api::{self, ImportExportArgs, ImportMode};
use crate::open_db::OpenDb;
use crate::process_assets::{self, ProcessAssetsOptions};

/// Stable id of the Demo Account, which every demo build writes.
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
    pub import: imports_api::ImportStats,
    /// What loading the bundle's address book changed, after the imports.
    pub address_book: LoadCounts,
    /// Dedupe content keys filled during the reset (one per message; not a duplicate count).
    pub dedupe_keys_filled: u64,
    /// Stats from the post-import media processing pass.
    pub process_assets: process_assets::ProcessAssetsStats,
}

/// `config/seed.toml` in a demo bundle. A key it does not use is an error,
/// never ignored (`deny_unknown_fields` here and on both sections): a misspelt
/// key would seed the Demo Account without what the line asked for.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DemoSeed {
    owner: DemoOwner,
    account: DemoAccount,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DemoOwner {
    display_name: String,
    /// `(raw handle, handle type)` pairs linked into `account_handles`.
    #[serde(default)]
    handle_specs: Vec<(String, HandleType)>,
    /// Email identities, written to `account_emails` and linked into
    /// `account_handles`.
    #[serde(default)]
    emails: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DemoAccount {
    username: String,
}

struct PreparedBundle {
    seed: DemoSeed,
    imessage_dir: PathBuf,
    sbr_dir: PathBuf,
    whatsapp_dir: PathBuf,
    contacts_csv: PathBuf,
}

/// One per-source import in a demo reset. [`import_demo_sources`] loops over
/// [`DEMO_IMPORT_SOURCES`], so the sources, their order, and their
/// Replace-then-Append modes are written down once.
struct DemoImportSource {
    /// Label printed in the "Reset demo — preparing replacement" header.
    label: &'static str,
    /// Source id recorded on the imported conversations.
    source: &'static str,
    /// Staging directory inside the prepared bundle.
    staging_dir: fn(&PreparedBundle) -> &PathBuf,
    /// The first source replaces the demo account's data; the rest append.
    mode: ImportMode,
}

const DEMO_IMPORT_SOURCES: [DemoImportSource; 3] = [
    DemoImportSource {
        label: "imessage",
        source: IMESSAGE_SOURCE,
        staging_dir: |bundle| &bundle.imessage_dir,
        mode: ImportMode::Replace,
    },
    DemoImportSource {
        label: "android",
        source: SBR_SOURCE,
        staging_dir: |bundle| &bundle.sbr_dir,
        mode: ImportMode::Append,
    },
    DemoImportSource {
        label: "whatsapp",
        source: WHATSAPP_SOURCE,
        staging_dir: |bundle| &bundle.whatsapp_dir,
        mode: ImportMode::Append,
    },
];

/// Print the "preparing replacement" header every demo build prints.
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
    target: &Path,
) -> Result<(dedupe::DedupeStats, process_assets::ProcessAssetsStats)> {
    let opened = OpenDb::open(cfg.clone().with_db_override(Some(target.to_path_buf()))).await?;
    let dedupe_stats = {
        let mut conn = opened.conn().await?;
        dedupe::dedupe_cross_source(&mut conn, account_id, None, 2).await?
    };
    // Demo Data holds only formats every browser shows as they are, so the
    // preview pass is an improvement and not a need. Without ffmpeg it would
    // fail once per attachment; say so once instead (#1018).
    if !media::ffmpeg_available() {
        opened.close().await;
        println!("Reset demo — ffmpeg not found; demo attachments stay as written");
        return Ok((dedupe_stats, process_assets::ProcessAssetsStats::default()));
    }
    println!("Reset demo — processing prepared assets");
    let process_stats = process_assets::run(
        &opened,
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
    opened.close().await;
    if let Some(warning) = conversion_warning(process_stats.errors) {
        eprintln!("warning: {warning}");
    }
    Ok((dedupe_stats, process_stats))
}

/// The warning printed when `errors` attachments failed conversion, or
/// `None` when every attachment converted.
fn conversion_warning(errors: u64) -> Option<String> {
    (errors > 0).then(|| {
        format!(
            "{errors} demo attachment(s) failed conversion; originals stay in place and reset-demo continues"
        )
    })
}

struct ResetPreparedStats {
    import: imports_api::ImportStats,
    address_book: LoadCounts,
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

/// Generate the Demo Data set of `size` and rebuild the demo account from it,
/// writing the active config to `config_dest`.
///
/// # Errors
///
/// Returns an error when generation fails, the database cannot be replaced,
/// or import / media processing fails.
pub async fn run_reset_demo(size: DemoSize, config_dest: &Path) -> Result<ResetDemoStats> {
    let work = tempfile::tempdir().context("create temporary demo bundle directory")?;
    let bundle = work.path().join("bundle");
    println!("Reset demo — generating the {size} data set");
    let seed_stats =
        demo_seed::generate_size_to(size, &bundle).context("generate demo bundle (demo-seed)")?;
    let reset_stats = prepare_config_and_reset(&bundle, config_dest, DEMO_ACCOUNT_ID).await?;

    Ok(ResetDemoStats {
        seed: seed_stats,
        import: reset_stats.import,
        address_book: reset_stats.address_book,
        dedupe_keys_filled: reset_stats.dedupe_keys_filled,
        process_assets: reset_stats.process_assets,
    })
}

/// Whether the database `cfg` names does not exist yet: a file that is not
/// there, or a database with no `accounts` table. `serve` seeds such a
/// database and no other, so one that was ever started, or made empty with
/// `create-database`, is left as it is.
///
/// # Errors
///
/// Returns an error when the database cannot be opened or read.
pub async fn database_is_new(cfg: &Config) -> Result<bool> {
    let path = cfg.paths.db.as_path();
    if !path.exists() {
        return Ok(true);
    }
    let pool = engine::open_pool_for_path(path).await?;
    let mut conn = pool.acquire().await?;
    let has_accounts = schema::table_exists(&mut conn, "accounts").await?;
    conn.close().await?;
    pool.close().await;
    Ok(!has_accounts)
}

/// Add the Demo Account, with the medium data set, to a database that does
/// not exist yet. `serve` calls this before it listens.
///
/// A failure is reported and not returned: the partly written Demo Account is
/// removed and the Message Crate starts without one, since a person can still
/// claim it and import, and the owner can add the Demo Account later.
pub async fn seed_new_database(cfg: &Config) {
    let size = DemoSize::Medium;
    eprintln!("New database: adding the Demo Account ({size} data set)…");
    let started = std::time::Instant::now();
    if let Some(messages) = seed_new_database_with(cfg, |bundle| {
        demo_seed::generate_size_to(size, bundle).map(|_| ())
    })
    .await
    {
        eprintln!(
            "Demo Account ready: {messages} messages in {:.1} s",
            started.elapsed().as_secs_f64()
        );
    }
}

/// [`seed_new_database`] with the step that writes the bundle injected, so a
/// test can seed from a few conversations, or from a bundle that fails
/// partway. Returns the number of messages imported, or `None` when seeding
/// failed and the Demo Account was removed again.
async fn seed_new_database_with<G>(cfg: &Config, generate: G) -> Option<u64>
where
    G: FnOnce(&Path) -> Result<()>,
{
    match build_demo_account_with(cfg, generate).await {
        Ok(messages) => Some(messages),
        Err(error) => {
            eprintln!("warning: could not add the Demo Account: {error:#}");
            eprintln!(
                "  this Message Crate starts without it; `message-crate-server reset-demo` adds it"
            );
            None
        }
    }
}

/// Writes a demo bundle of the given size into the given folder. The server
/// holds the real one ([`generate_bundle`]); a test holds one that writes a
/// few conversations.
pub type BundleGenerator = fn(DemoSize, &Path) -> Result<()>;

/// The generator a running server uses: the built-in data set of the size.
pub fn generate_bundle(size: DemoSize, bundle: &Path) -> Result<()> {
    demo_seed::generate_size_to(size, bundle).map(|_| ())
}

/// Build the Demo Account in the database `cfg` names while the server is
/// serving it: the Owner Home action. The account is removed and built again,
/// in the live database, and no other account is touched. Returns the number
/// of messages imported.
///
/// # Errors
///
/// Returns an error when generation, import or media processing fails; the
/// partly built Demo Account is removed first.
pub async fn build_demo_account(
    cfg: std::sync::Arc<Config>,
    size: DemoSize,
    generate: BundleGenerator,
) -> Result<u64> {
    let outcome = async {
        let work = tempfile::tempdir().context("create temporary demo bundle directory")?;
        let bundle = work.path().join("bundle");
        // Generating is CPU work with no await in it, so it runs off the
        // request-serving threads.
        let target = bundle.clone();
        tokio::task::spawn_blocking(move || generate(size, &target))
            .await
            .context("the demo bundle generator stopped")?
            .context("generate demo bundle (demo-seed)")?;
        seed_new_database_from_bundle(&cfg, &bundle).await
    }
    .await;
    whole_demo_account_or_none(&cfg, outcome).await
}

/// Generate a bundle with `generate` and build the Demo Account from it in
/// the database `cfg` names.
async fn build_demo_account_with<G>(cfg: &Config, generate: G) -> Result<u64>
where
    G: FnOnce(&Path) -> Result<()>,
{
    let outcome = async {
        let work = tempfile::tempdir().context("create temporary demo bundle directory")?;
        let bundle = work.path().join("bundle");
        generate(&bundle).context("generate demo bundle (demo-seed)")?;
        seed_new_database_from_bundle(cfg, &bundle).await
    }
    .await;
    whole_demo_account_or_none(cfg, outcome).await
}

/// The number of messages a build imported. When the build failed, the
/// partly built account is removed first, so the Message Crate holds a whole
/// Demo Account or none.
async fn whole_demo_account_or_none(
    cfg: &Config,
    outcome: Result<ResetPreparedStats>,
) -> Result<u64> {
    match outcome {
        Ok(stats) => Ok(stats.import.messages),
        Err(error) => {
            if let Err(error) = wipe_demo_account(cfg, DEMO_ACCOUNT_ID, &cfg.paths.db).await {
                eprintln!("warning: could not remove the partly added Demo Account: {error:#}");
            }
            Err(error)
        }
    }
}

/// Build the Demo Account in the database `cfg` names from the bundle at
/// `bundle`. There is nothing to snapshot or swap: this writes to the
/// database directly, touching the Demo Account alone, and leaves the config
/// file as it is.
async fn seed_new_database_from_bundle(cfg: &Config, bundle: &Path) -> Result<ResetPreparedStats> {
    let prepared = validate_prepared_bundle(bundle)?;
    let parent = parent_dir_or_cwd(&cfg.paths.db);
    fs::create_dir_all(parent)
        .with_context(|| format!("create database parent {}", parent.display()))?;
    rebuild_demo_account(cfg, &prepared, DEMO_ACCOUNT_ID, &cfg.paths.db).await
}

/// Copy the bundle's config into place and reset the account by the
/// snapshot-and-swap path.
async fn prepare_config_and_reset(
    bundle: &Path,
    config_dest: &Path,
    account_id: i64,
) -> Result<ResetPreparedStats> {
    validate_prepared_bundle(bundle)?;
    let demo_config = bundle.join("config/config.toml");
    if !demo_config.is_file() {
        bail!(
            "incomplete demo bundle under {} (need config/config.toml)",
            bundle.display()
        );
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

/// Build the new state in a prepared database next to the active one, prove
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
    let prepared_db = db_work.path().join("messagecrate.db");
    checkpoint_and_clean_sidecars(&cfg.paths.db, "before creating the reset snapshot").await?;
    prepare_database_snapshot(&cfg.paths.db, &prepared_db).await?;

    let mut temporary_cfg = cfg.clone();
    temporary_cfg.paths.db = prepared_db.clone();
    temporary_cfg.paths.data_dir = data_work.path().to_path_buf();
    let stats = rebuild_demo_account(&temporary_cfg, &prepared, account_id, &prepared_db).await?;

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
    let previous_state_still_in_work = db_work.path().join("previous-messagecrate.db").exists()
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

/// Wipe, seed, import, load the address book, dedupe, convert media, and
/// vacuum the demo account on the database file `target`. A new database and a reset both run exactly
/// this; what differs is what the caller does around it (a reset snapshots
/// the database first and swaps it in after).
async fn rebuild_demo_account(
    cfg: &Config,
    prepared: &PreparedBundle,
    account_id: i64,
    target: &Path,
) -> Result<ResetPreparedStats> {
    wipe_demo_account(cfg, account_id, target).await?;
    print_reset_header(account_id, prepared, &target.display());
    seed_demo_account(target, account_id, &prepared.seed).await?;
    let import = import_demo_sources(cfg, prepared, account_id, target).await?;
    let address_book = load_demo_address_book(prepared, account_id, target).await?;
    let (dedupe_stats, process_stats) = dedupe_and_process_assets(cfg, account_id, target).await?;
    vacuum_after_demo(target).await;
    Ok(ResetPreparedStats {
        import,
        address_book,
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
    target: &Path,
) -> Result<imports_api::ImportStats> {
    let mut totals = imports_api::ImportStats::default();
    for source in &DEMO_IMPORT_SOURCES {
        let assets_dir = cfg.paths.assets_dir_for_account(account_id, source.source);
        let stats = imports_api::import_export(&ImportExportArgs {
            export_dir: (source.staging_dir)(prepared),
            db: target,
            assets_dir: &assets_dir,
            mode: source.mode,
            source: source.source,
            account_id,
        })
        .await?;
        totals.add_run(&stats);
    }
    Ok(totals)
}

/// Load the bundle's address book into the demo account, in Edit mode, once
/// its messages are in.
///
/// The demo is built the way a person builds theirs: the imports bring the
/// people in as Unknowns, and the address book names them. It goes through
/// [`address_book::load`], the function `POST /v1/contacts` calls, so the
/// demo exercises the same rules a person's file does, the move from an
/// Unknown holder among them.
async fn load_demo_address_book(
    prepared: &PreparedBundle,
    account_id: i64,
    target: &Path,
) -> Result<LoadCounts> {
    let text = fs::read_to_string(&prepared.contacts_csv)
        .with_context(|| format!("read {}", prepared.contacts_csv.display()))?;
    let pool = engine::open_pool_for_path(target).await?;
    let mut conn = pool.acquire().await?;
    let loaded = address_book::load(&mut conn, account_id, &text, LoadMode::Edit).await;
    conn.close().await?;
    pool.close().await;
    let counts = loaded.map_err(|e| anyhow::anyhow!("load the demo address book: {e}"))?;
    println!(
        "  contacts: {} named from the address book ({} identities moved from Unknowns, {} added)",
        counts.contacts_created, counts.identities_moved, counts.identities_added
    );
    Ok(counts)
}

/// Check the bundle has its seed, the three staging folders, and the contacts file, and return their paths.
fn validate_prepared_bundle(bundle: &Path) -> Result<PreparedBundle> {
    let demo_seed = bundle.join("config/seed.toml");
    let imessage_dir = bundle.join("staging").join(IMESSAGE_SOURCE);
    let sbr_dir = bundle.join("staging").join(SBR_SOURCE);
    let whatsapp_dir = bundle.join("staging").join(WHATSAPP_SOURCE);
    let contacts_csv = bundle.join("config/contacts.csv");
    if !demo_seed.is_file()
        || !imessage_dir.is_dir()
        || !sbr_dir.is_dir()
        || !whatsapp_dir.is_dir()
        || !contacts_csv.is_file()
    {
        bail!(
            "incomplete demo bundle under {} (need config/seed.toml, \
             staging/{IMESSAGE_SOURCE}/, staging/{SBR_SOURCE}/, staging/{WHATSAPP_SOURCE}/, config/contacts.csv)",
            bundle.display()
        );
    }
    Ok(PreparedBundle {
        seed: load_demo_seed(&demo_seed)?,
        imessage_dir,
        sbr_dir,
        whatsapp_dir,
        contacts_csv,
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
        schema::ensure_schema(&mut conn).await?;
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
            "cannot replace {} because its WAL could not be checkpointed; stop every process using the database and run reset-demo offline",
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
                "cannot replace {} because {} still contains {length} bytes after WAL checkpoint; stop every process using the database and run reset-demo offline",
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
                "remove SQLite shared-memory sidecar {}; stop every process using the database and run reset-demo offline",
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

/// Message counts per account for every account except the demo one, used
/// to prove a reset changed nothing else. The owner is among them: a reset
/// writes no owner, so one that exists must still be there afterwards.
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
            .join("previous-messagecrate.db");
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

/// Compact import tables after the sample inbox is fully loaded. Best effort:
/// failures are printed, not returned, because the demo rows are already committed.
async fn vacuum_after_demo(target: &Path) {
    let pool = match engine::open_pool_for_path(target).await {
        Ok(pool) => pool,
        Err(err) => {
            eprintln!("  sql:      warning: vacuum after demo failed to open the database: {err}");
            return;
        }
    };
    vacuum_after_demo_on_pool(pool).await;
}
/// Reclaim space after the demo import replaced most rows. Best effort: a failed vacuum only costs disk space.
async fn vacuum_after_demo_on_pool(pool: sqlx::SqlitePool) {
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
async fn seed_demo_account(target: &Path, account_id: i64, seed: &DemoSeed) -> Result<()> {
    let pool = engine::open_pool_for_path(target).await?;
    let mut conn = pool.acquire().await?;
    schema::ensure_schema(&mut conn).await?;
    seed_demo_account_on_conn(&mut conn, account_id, seed).await?;
    conn.close().await?;
    pool.close().await;
    Ok(())
}
/// Create the demo account row and the profile fields the seed names, so the demo logs in without setup.
async fn seed_demo_account_on_conn(
    conn: &mut sqlx::SqliteConnection,
    account_id: i64,
    seed: &DemoSeed,
) -> Result<()> {
    account_profile::ensure_account_row(conn, account_id).await?;

    // Account creation reserves the name, but a database written before the
    // reservation may already give it to another account.
    let username = &seed.account.username;
    if let Some(holder) = account_profile::lookup_account_by_username(conn, username).await?
        && holder != account_id
    {
        let held_as = account_profile::username_for_account(conn, holder)
            .await?
            .unwrap_or_default();
        anyhow::bail!(
            "account {holder} ({held_as}) already has the username {username}, which belongs to the Demo Account; delete that account to add the Demo Account"
        );
    }

    // The Demo Account has no password, so anyone at the login card can enter
    // it. It may export, and trash and restore; it may not import, so a
    // person's own messages never land in Demo Data, and it may not delete
    // for good, so one visitor cannot empty it for the next
    // (`docs/adr/0016-the-demo-account-is-fixed-not-configured.md`).
    sqlx::query(
        r"
        INSERT INTO accounts (
            id, username, password_hash, preferred_name, can_import, can_export, can_delete
        )
        VALUES ($1, $2, NULL, $3, 0, 1, 0)
        ON CONFLICT(id) DO UPDATE SET
            username = excluded.username,
            preferred_name = excluded.preferred_name,
            can_import = excluded.can_import,
            can_export = excluded.can_export,
            can_delete = excluded.can_delete
        ",
    )
    .bind(account_id)
    .bind(username)
    .bind(&seed.owner.display_name)
    .execute(&mut *conn)
    .await?;
    // The profile reads the account's emails from `account_emails`. They are
    // identities, not a login.
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
    // The seed creates no API token. A token the Demo Account creates cannot
    // import either, because a token's grant is narrowed to the account's.

    // Phone and email identities that mark messages as from "you" live in
    // `handles`, linked through `account_handles`.
    sqlx::query("DELETE FROM account_handles WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut *conn)
        .await?;
    for (raw, handle_type) in &seed.owner.handle_specs {
        account_profile::link_account_handle(conn, account_id, raw, *handle_type).await?;
    }
    // An email is an identity like the phone, so it is linked here as well as
    // written to `account_emails` above, as adding one to any account does.
    // The profile and the identities list then name the same addresses (#955).
    for email in &seed.owner.emails {
        account_profile::link_account_handle(conn, account_id, email, HandleType::Email).await?;
    }
    Ok(())
}

/// Delete the demo account's rows (child rows follow via CASCADE) and
/// on-disk attachments. Leaves the database and other accounts intact.
async fn wipe_demo_account(cfg: &Config, account_id: i64, target: &Path) -> Result<()> {
    println!("Reset demo — clearing account data in {}", target.display());
    let pool = engine::open_pool_for_path(target).await?;
    let mut conn = pool
        .acquire()
        .await
        .with_context(|| format!("open {} for demo account wipe", target.display()))?;
    schema::ensure_schema(&mut conn).await?;
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
pub(crate) mod tests;
