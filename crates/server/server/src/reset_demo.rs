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
use sqlx::{Row, SqlitePool};

use crate::config::Config;
use crate::db::account_profile;
use crate::db::address_book::{self, CreateContactsResponse, LoadMode};
use crate::db::audit_trail::AuditActor;
use crate::db::demo_account_build;
use crate::db::engine;
use crate::db::maintenance;
use crate::db::schema;
use crate::dedupe;
use crate::imports_api::{self, FixedImportArgs, ImportMode, ImportOptions, ImportSchemaMode};
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
    pub address_book: CreateContactsResponse,
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

/// Shared post-import tail: fill dedupe content keys, convert the account's
/// media, and warn (but continue) when some attachments fail conversion.
/// Only `account_id` is converted: on a running server every other account
/// is someone's, with Uploads of its own in progress.
async fn dedupe_and_process_assets(
    cfg: &Config,
    db: &SqlitePool,
    account_id: i64,
) -> Result<(dedupe::DedupeStats, process_assets::ProcessAssetsStats)> {
    let dedupe_stats = {
        let mut conn = db.acquire().await?;
        dedupe::dedupe_cross_source(&mut conn, account_id, None, 2).await?
    };
    // Demo Data holds only formats every browser shows as they are, so the
    // preview pass is an improvement and not a need. Without ffmpeg it would
    // fail once per attachment; say so once instead (#1018).
    if !media::ffmpeg_available() {
        println!("Reset demo — ffmpeg not found; demo attachments stay as written");
        return Ok((dedupe_stats, process_assets::ProcessAssetsStats::default()));
    }
    println!("Reset demo — processing prepared assets");
    let opened = OpenDb {
        cfg: cfg.clone(),
        db: db.clone(),
    };
    let process_stats = process_assets::run(
        &opened,
        &ProcessAssetsOptions {
            force: false,
            dry_run: false,
            skip_image: false,
            skip_video: false,
            skip_audio: false,
            source: None,
            account: Some(account_id),
        },
    )
    .await
    .context("process-assets after prepared demo import")?;
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
    address_book: CreateContactsResponse,
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

/// Generate the Demo Data set of `size` and rebuild the demo account from it
/// in the database `cfg` names. The config file is the operator's: the caller
/// reads it, and nothing here writes it (#1216).
///
/// # Errors
///
/// Returns an error when generation fails, the database cannot be replaced,
/// or import / media processing fails.
pub async fn run_reset_demo(size: DemoSize, cfg: &Config) -> Result<ResetDemoStats> {
    let work = tempfile::tempdir().context("create temporary demo bundle directory")?;
    let bundle = work.path().join("bundle");
    println!("Reset demo — generating the {size} data set");
    let seed_stats =
        demo_seed::generate_size_to(size, &bundle).context("generate demo bundle (demo-seed)")?;
    let reset_stats = reset_prepared_bundle(cfg, &bundle, DEMO_ACCOUNT_ID).await?;

    Ok(ResetDemoStats {
        seed: seed_stats,
        import: reset_stats.import,
        address_book: reset_stats.address_book,
        dedupe_keys_filled: reset_stats.dedupe_keys_filled,
        process_assets: reset_stats.process_assets,
    })
}

/// Whether the database `cfg` names does not exist yet: a file that is not
/// there, or an empty one ([`schema::DatabaseKind::Empty`]). `serve` seeds
/// such a database and no other, so one that was ever started, or made
/// empty with `create-database`, is left as it is. Another program's file is
/// not new either: seeding would move the new database over it, so `serve`
/// goes on to refuse it by name.
///
/// # Errors
///
/// Returns an error when the file cannot be read as SQLite.
pub async fn database_is_new(cfg: &Config) -> Result<bool> {
    Ok(matches!(
        crate::open_db::inspect(&cfg.paths.db).await?,
        None | Some(schema::DatabaseKind::Empty)
    ))
}

/// Add the Demo Account, with the medium data set, to a database that does
/// not exist yet. `serve` calls this before it listens. The database is
/// written beside the configured path and moved into place once it is whole
/// ([`seed_into_place`]).
///
/// A failure is reported and not returned: the unfinished database is
/// removed and `serve` creates an empty one, so the Message Crate starts
/// without the Demo Account, since a person can still claim it and import,
/// and the owner can add the Demo Account later.
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
    let built = seed_into_place(cfg, async |seeding: &Config, db: &SqlitePool| {
        build_demo_account_with(seeding, db, generate).await
    })
    .await;
    match built {
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

/// Where `serve` writes a new database until seeding is complete:
/// `<database>.seeding`, beside the configured file, so the rename into
/// place stays on one file system.
fn seeding_path(db: &Path) -> PathBuf {
    sqlite_sidecar(db, ".seeding")
}

/// Remove the `-wal` and `-shm` sidecars of the database file `db`; one that
/// is not there is not an error.
fn remove_sidecars(db: &Path) -> Result<()> {
    remove_any_if_exists(&sqlite_sidecar(db, "-wal"))?;
    remove_any_if_exists(&sqlite_sidecar(db, "-shm"))
}

/// Create a new database with `build` at [`seeding_path`] and rename it to
/// the path `cfg` names once `build` has finished, so a database file at the
/// configured path is always fully seeded. A start that is stopped while
/// seeding, the way the desktop app kills a server that is still starting,
/// leaves only the seeding file, and the next start removes it and seeds
/// again (#1215).
///
/// When `build` fails the seeding file is removed and nothing is renamed,
/// so `serve` goes on to create an empty database at the configured path.
async fn seed_into_place<B>(cfg: &Config, build: B) -> Result<u64>
where
    B: AsyncFnOnce(&Config, &SqlitePool) -> Result<u64>,
{
    let seeding = seeding_path(&cfg.paths.db);
    remove_any_if_exists(&seeding)?;
    remove_sidecars(&seeding)?;
    let mut seeding_cfg = cfg.clone();
    seeding_cfg.paths.db = seeding.clone();
    let seeded = async {
        let opened = OpenDb::create_or_open(seeding_cfg.clone()).await?;
        let built = build(&seeding_cfg, &opened.db).await;
        // Closed before the file is checkpointed and renamed.
        opened.close().await;
        let messages = built?;
        checkpoint_and_clean_sidecars(&seeding, "before moving the new database into place")
            .await?;
        // A configured file can be there only when it is empty
        // ([`database_is_new`]); its sidecars must not attach to the new one.
        remove_sidecars(&cfg.paths.db)?;
        fs::rename(&seeding, &cfg.paths.db).with_context(|| {
            format!(
                "move the new database {} to {}",
                seeding.display(),
                cfg.paths.db.display()
            )
        })?;
        Ok(messages)
    }
    .await;
    if seeded.is_err()
        && let Err(error) = remove_any_if_exists(&seeding).and_then(|()| remove_sidecars(&seeding))
    {
        eprintln!("warning: could not remove the unfinished new database: {error:#}");
    }
    seeded
}

/// Record in `db` that a Demo Account build has started, before the build
/// writes anything else. The record stays until the build has finished or
/// what it wrote has been removed.
async fn begin_demo_build(db: &SqlitePool) -> Result<()> {
    let mut conn = db.acquire().await?;
    demo_account_build::begin(&mut conn)
        .await
        .context("record that the Demo Account build has started")
}

/// Remove what a failed build wrote: the Demo Account, then the build's
/// record. The record stays when the account cannot be removed, so the next
/// start tries again.
///
/// # Errors
///
/// Returns an error when the account or the record cannot be removed.
pub async fn remove_failed_demo_build(cfg: &Config, db: &SqlitePool) -> Result<()> {
    wipe_demo_account(cfg, db, DEMO_ACCOUNT_ID, AuditActor::Server).await?;
    let mut conn = db.acquire().await?;
    demo_account_build::end(&mut conn).await
}

/// Remove the Demo Account a build left when the server stops during it, and
/// keep the build's record, so the next start reports the build as failed.
/// A build that has already finished has no record, and its Demo Account
/// is kept. Returns whether there was a build to stop.
///
/// # Errors
///
/// Returns an error when the record cannot be read or the account cannot be
/// removed.
pub async fn remove_part_built_demo_account(cfg: &Config, db: &SqlitePool) -> Result<bool> {
    let unfinished = {
        let mut conn = db.acquire().await?;
        demo_account_build::is_unfinished(&mut conn).await?
    };
    if unfinished {
        wipe_demo_account(cfg, db, DEMO_ACCOUNT_ID, AuditActor::Server).await?;
    }
    Ok(unfinished)
}

/// On start: when the database holds the record of a Demo Account build that
/// did not finish, because the server stopped during it, remove the Demo
/// Account the build left and the record. Returns whether there was one.
///
/// # Errors
///
/// Returns an error when the record cannot be read or the account or the
/// record cannot be removed.
pub async fn remove_stopped_demo_build(cfg: &Config, db: &SqlitePool) -> Result<bool> {
    let unfinished = {
        let mut conn = db.acquire().await?;
        demo_account_build::is_unfinished(&mut conn).await?
    };
    if unfinished {
        remove_failed_demo_build(cfg, db).await?;
    }
    Ok(unfinished)
}

/// Writes a demo bundle of the given size into the given folder. The server
/// holds the real one ([`generate_bundle`]); a test holds one that writes a
/// few conversations.
pub type BundleGenerator = fn(DemoSize, &Path) -> Result<()>;

/// The generator a running server uses: the built-in data set of the size.
pub fn generate_bundle(size: DemoSize, bundle: &Path) -> Result<()> {
    demo_seed::generate_size_to(size, bundle).map(|_| ())
}

/// Build the Demo Account in the server's database `db` while the server is
/// serving it: the Owner Home action. The account is removed and built again,
/// in the live database, and no other account is touched. Each source is
/// imported in batches, each its own transaction, so another account's write
/// waits for one batch at most. Returns the number of messages imported.
///
/// # Errors
///
/// Returns an error when generation, import or media processing fails; the
/// partly built Demo Account is removed first.
pub async fn build_demo_account(
    db: SqlitePool,
    cfg: std::sync::Arc<Config>,
    size: DemoSize,
    generate: BundleGenerator,
) -> Result<u64> {
    let outcome = async {
        // Recorded before generating, so a server stopped at any point of
        // the build removes the Demo Account it was replacing.
        begin_demo_build(&db).await?;
        let work = tempfile::tempdir().context("create temporary demo bundle directory")?;
        let bundle = work.path().join("bundle");
        // Generating is CPU work with no await in it, so it runs off the
        // request-serving threads.
        let target = bundle.clone();
        tokio::task::spawn_blocking(move || generate(size, &target))
            .await
            .context("the demo bundle generator stopped")?
            .context("generate demo bundle (demo-seed)")?;
        build_from_bundle(&cfg, &db, &bundle, AuditActor::Owner).await
    }
    .await;
    whole_demo_account_or_none(&cfg, &db, outcome).await
}

/// Generate a bundle with `generate` and build the Demo Account from it in
/// the database `db`, the one `cfg` names.
async fn build_demo_account_with<G>(cfg: &Config, db: &SqlitePool, generate: G) -> Result<u64>
where
    G: FnOnce(&Path) -> Result<()>,
{
    let outcome = async {
        let work = tempfile::tempdir().context("create temporary demo bundle directory")?;
        let bundle = work.path().join("bundle");
        generate(&bundle).context("generate demo bundle (demo-seed)")?;
        build_from_bundle(cfg, db, &bundle, AuditActor::Server).await
    }
    .await;
    whole_demo_account_or_none(cfg, db, outcome).await
}

/// The number of messages a build imported. When the build failed, the
/// partly built account is removed first, so the Message Crate holds a whole
/// Demo Account or none.
async fn whole_demo_account_or_none(
    cfg: &Config,
    db: &SqlitePool,
    outcome: Result<ResetPreparedStats>,
) -> Result<u64> {
    match outcome {
        Ok(stats) => Ok(stats.import.messages),
        Err(error) => {
            if let Err(error) = remove_failed_demo_build(cfg, db).await {
                eprintln!("warning: could not remove the partly added Demo Account: {error:#}");
            }
            Err(error)
        }
    }
}

/// Build the Demo Account in the database `db`, the one `cfg` names, from
/// the bundle at `bundle`. There is nothing to snapshot or swap: this writes
/// to the database directly, touching the Demo Account alone.
async fn build_from_bundle(
    cfg: &Config,
    db: &SqlitePool,
    bundle: &Path,
    actor: AuditActor,
) -> Result<ResetPreparedStats> {
    let prepared = validate_prepared_bundle(bundle)?;
    rebuild_demo_account(cfg, db, &prepared, DEMO_ACCOUNT_ID, actor).await
}

/// Build the new state in a prepared database next to the active one, prove
/// nothing outside the demo account changed, then swap it in.
async fn reset_prepared_bundle(
    cfg: &Config,
    bundle: &Path,
    account_id: i64,
) -> Result<ResetPreparedStats> {
    reset_prepared_bundle_with(cfg, bundle, account_id, async |_| Ok(())).await
}

/// [`reset_prepared_bundle`] with `after_rebuild` run on the prepared
/// database once the rebuild has finished, so a test can change a row the
/// rebuild must not change and see the reset refused.
async fn reset_prepared_bundle_with(
    cfg: &Config,
    bundle: &Path,
    account_id: i64,
    after_rebuild: impl AsyncFnOnce(&SqlitePool) -> Result<()>,
) -> Result<ResetPreparedStats> {
    let prepared = validate_prepared_bundle(bundle)?;
    // Refused here, by its own name, before the lock file, the snapshot or
    // the swap touch anything beside it.
    if crate::open_db::inspect(&cfg.paths.db).await? == Some(schema::DatabaseKind::Foreign) {
        return Err(schema::not_a_message_crate_database(&cfg.paths.db));
    }
    let _operation_lock = crate::operation_lock::acquire_for_reset(&cfg.paths.db)?;
    let mut ready = crate::operation_lock::ReadyWhileRebuilding::clear(&cfg.paths.db)?;
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
    let opened = OpenDb::create_or_open(temporary_cfg.clone()).await?;
    let stats = match rebuild_demo_account(
        &temporary_cfg,
        &opened.db,
        &prepared,
        account_id,
        AuditActor::CommandLine,
    )
    .await
    {
        Ok(stats) => after_rebuild(&opened.db).await.map(|()| stats),
        Err(error) => Err(error),
    };
    // Closed before anything checks, checkpoints or renames the file.
    opened.close().await;
    let stats = stats?;

    verify_non_demo_state_preserved(&cfg.paths.db, &prepared_db, account_id).await?;
    let active_account = cfg.paths.data_dir.join(account_id.to_string());
    let prepared_account = temporary_cfg.paths.data_dir.join(account_id.to_string());
    let paths = ResetPaths {
        active_db: &cfg.paths.db,
        prepared_db: &prepared_db,
        active_account: &active_account,
        prepared_account: &prepared_account,
    };
    install_reset_state_or_keep_work(&paths, db_work, data_work, &mut ready).await?;
    ready.mark_ready()?;
    Ok(stats)
}

/// Swap the prepared state in. When the swap fails and its rollback left any
/// of the previous state in the work directories, keep those directories on
/// disk, name them in the error so nothing is lost, and leave `server.ready`
/// removed, since the active state is not whole. Every other failure here
/// leaves the previous state installed, and `ready` writes the marker back.
async fn install_reset_state_or_keep_work(
    paths: &ResetPaths<'_>,
    db_work: tempfile::TempDir,
    data_work: tempfile::TempDir,
    ready: &mut crate::operation_lock::ReadyWhileRebuilding,
) -> Result<()> {
    let Err(error) = install_reset_state(paths).await else {
        return Ok(());
    };
    let previous_state_still_in_work = db_work.path().join("previous-messagecrate.db").exists()
        || data_work.path().join("previous-account").exists();
    if previous_state_still_in_work {
        ready.keep_cleared();
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
/// vacuum the demo account in the database `db`, the one `cfg` names. A new
/// database, a reset and a build on a running server all run exactly this;
/// what differs is what the caller does around it (a reset snapshots the
/// database first and swaps it in after). Every step uses `db`, so on a
/// running server the build shares the server's pool. The build's record in
/// `demo_account_build` is written first and removed last, so a database
/// that still holds it after a stop has a part-built Demo Account (#1215).
async fn rebuild_demo_account(
    cfg: &Config,
    db: &SqlitePool,
    prepared: &PreparedBundle,
    account_id: i64,
    actor: AuditActor,
) -> Result<ResetPreparedStats> {
    begin_demo_build(db).await?;
    wipe_demo_account(cfg, db, account_id, actor).await?;
    print_reset_header(account_id, prepared, &cfg.paths.db.display());
    seed_demo_account(db, account_id, &prepared.seed).await?;
    let import = import_demo_sources(cfg, db, prepared, account_id).await?;
    let address_book = load_demo_address_book(db, prepared, account_id).await?;
    let (dedupe_stats, process_stats) = dedupe_and_process_assets(cfg, db, account_id).await?;
    vacuum_after_demo(db).await;
    let mut conn = db.acquire().await?;
    demo_account_build::end(&mut conn)
        .await
        .context("record that the Demo Account build has finished")?;
    drop(conn);
    Ok(ResetPreparedStats {
        import,
        address_book,
        dedupe_keys_filled: dedupe_stats.keys_filled,
        process_assets: process_stats,
    })
}

/// The most JSONL a build imports in one transaction: what an Upload sends
/// in one request, `message_crate_push::MAX_IMPORT_BODY_BYTES` (64 MiB).
/// Every other write on the server waits for the transaction, so it waits
/// for one batch at most.
const IMPORT_BATCH_BYTES: u64 = 64 * 1024 * 1024;

/// Group `paths`, in order, into batches whose sizes add up to `limit` bytes
/// or less. A file larger than `limit` is a batch of its own, because a
/// conversation file is never split.
///
/// # Errors
///
/// Returns an error when a file's size cannot be read.
fn import_batches(paths: Vec<PathBuf>, limit: u64) -> Result<Vec<Vec<PathBuf>>> {
    let mut batches: Vec<Vec<PathBuf>> = Vec::new();
    let mut batch_bytes = 0u64;
    for path in paths {
        let bytes = fs::metadata(&path)
            .with_context(|| format!("read the size of {}", path.display()))?
            .len();
        match batches.last_mut() {
            Some(batch) if batch_bytes.saturating_add(bytes) <= limit => {
                batch.push(path);
                batch_bytes += bytes;
            }
            _ => {
                batches.push(vec![path]);
                batch_bytes = bytes;
            }
        }
    }
    Ok(batches)
}

/// Import the staged sources in [`DEMO_IMPORT_SOURCES`] order: the first
/// replaces the account's data and the rest append. Returns the summed counts.
async fn import_demo_sources(
    cfg: &Config,
    db: &SqlitePool,
    prepared: &PreparedBundle,
    account_id: i64,
) -> Result<imports_api::ImportStats> {
    import_demo_sources_with(
        cfg,
        db,
        prepared,
        account_id,
        IMPORT_BATCH_BYTES,
        async || Ok(()),
    )
    .await
}

/// [`import_demo_sources`] in batches of at most `batch_bytes`, calling
/// `after_batch` once each batch is committed, so a test can write for
/// another account between two batches.
///
/// Each source is one Import Run, sent as batches the way an Upload sends
/// them: each batch is its own transaction, and a source that replaces does
/// so on its first batch alone, so a later batch never removes an earlier
/// one's messages.
async fn import_demo_sources_with(
    cfg: &Config,
    db: &SqlitePool,
    prepared: &PreparedBundle,
    account_id: i64,
    batch_bytes: u64,
    mut after_batch: impl AsyncFnMut() -> Result<()>,
) -> Result<imports_api::ImportStats> {
    let mut totals = imports_api::ImportStats::default();
    for source in &DEMO_IMPORT_SOURCES {
        let export_dir = (source.staging_dir)(prepared);
        let paths = crate::import_cli::list_jsonl_files(export_dir)?;
        let assets_dir = cfg.paths.assets_dir_for_account(account_id, source.source);
        let mut conn = db.acquire().await?;
        let session = imports_api::OwnedSession::start(
            &mut conn,
            account_id,
            source.source,
            source.mode,
            "message-crate-server",
        )
        .await?;
        let mut run = imports_api::ImportStats {
            mode: source.mode,
            ..Default::default()
        };
        let mut result = Ok(());
        for (index, batch) in import_batches(paths, batch_bytes)?.iter().enumerate() {
            let mode = if index == 0 {
                source.mode
            } else {
                ImportMode::Append
            };
            let imported = imports_api::import_on_conn(
                &mut conn,
                batch,
                &ImportOptions::fixed(FixedImportArgs {
                    assets_dir: &assets_dir,
                    asset_root: export_dir,
                    mode,
                    source: source.source,
                    account_id,
                    fill_content_keys: true,
                    import_id: Some(session.id),
                }),
                ImportSchemaMode::AssumeReady,
            )
            .await;
            match imported {
                Ok(stats) => run.add_run(&stats),
                Err(error) => {
                    result = Err(error);
                    break;
                }
            }
            if let Err(error) = after_batch().await {
                result = Err(error);
                break;
            }
        }
        let result = result.map(|()| run);
        session.finish(&mut conn, &result).await;
        totals.add_run(&result?);
    }
    Ok(totals)
}

/// Load the bundle's address book into the demo account, in Append mode,
/// once its messages are in.
///
/// The demo is built the way a person builds theirs: the imports bring the
/// people in as Unknowns, and the person exports the address book, types the
/// names onto the Unknowns' rows, and loads it back.
/// [`address_book::rewrite_ids_to_nameless`] does the typing: it
/// gives each contact of the bundle's file the id of the Unknown the imports
/// made for it. The load goes through
/// [`address_book::load`], the function `POST /v1/contacts` calls, so the
/// demo exercises the same rules a person's file does.
///
/// Naming the Unknown in place keeps it in the Contact Group of each Import
/// Run that touched it (#1511). A new contact taking the Unknown's
/// identities instead would leave the Unknown empty, so the load would
/// delete it and the run's group would lose it. The mode is Append because
/// Edit takes a contact out of every Contact Group its rows do not list, and
/// the bundle's file cannot list the groups the Import Runs make.
async fn load_demo_address_book(
    db: &SqlitePool,
    prepared: &PreparedBundle,
    account_id: i64,
) -> Result<CreateContactsResponse> {
    let text = fs::read_to_string(&prepared.contacts_csv)
        .with_context(|| format!("read {}", prepared.contacts_csv.display()))?;
    let mut conn = db.acquire().await?;
    let text = address_book::rewrite_ids_to_nameless(&mut conn, account_id, &text)
        .await
        .context("match the demo address book to the Unknowns the imports made")?;
    let loaded = address_book::load(&mut conn, account_id, &text, LoadMode::Append).await;
    let counts = loaded.map_err(|e| anyhow::anyhow!("load the demo address book: {e}"))?;
    println!(
        "  contacts: {} named from the address book ({} Unknowns named in place, {} new; {} identities moved from Unknowns, {} added)",
        counts.contacts_changed(),
        counts.contacts_updated,
        counts.contacts_created,
        counts.identities_moved,
        counts.identities_added
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

/// Refuse to install the prepared database when it changed anything outside
/// the Demo Account: a row of any other account in any table, or a row that
/// belongs to no account, such as the Server Settings. A reset must only ever
/// touch the Demo Account (#1225).
async fn verify_non_demo_state_preserved(
    active: &Path,
    prepared: &Path,
    demo_id: i64,
) -> Result<()> {
    if !active.is_file() || !has_accounts_table(active).await? {
        // A database with no accounts holds nothing a reset could lose.
        return Ok(());
    }
    let active_state = non_demo_state(active, demo_id).await?;
    let prepared_state = non_demo_state(prepared, demo_id).await?;
    let changed: Vec<String> = active_state
        .keys()
        .chain(prepared_state.keys())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter(|table| active_state.get(*table) != prepared_state.get(*table))
        .map(|table| {
            let rows = |state: &BTreeMap<String, TableDigest>| {
                state
                    .get(table)
                    .map_or_else(|| "no table".to_owned(), |digest| digest.rows.to_string())
            };
            format!(
                "{table} (active rows={}, prepared rows={})",
                rows(&active_state),
                rows(&prepared_state)
            )
        })
        .collect();
    if !changed.is_empty() {
        bail!(
            "prepared reset database changed rows outside the Demo Account in: {}",
            changed.join(", ")
        );
    }
    Ok(())
}

/// Whether the database at `db` has an `accounts` table.
async fn has_accounts_table(db: &Path) -> Result<bool> {
    let pool = engine::open_pool_for_path(db)
        .await
        .with_context(|| format!("open {} to verify non-demo accounts", db.display()))?;
    let mut conn = pool
        .acquire()
        .await
        .with_context(|| format!("open {} to verify non-demo accounts", db.display()))?;
    let exists = schema::table_exists(&mut conn, "accounts")
        .await
        .with_context(|| format!("check accounts table in {}", db.display()))?;
    conn.close().await?;
    pool.close().await;
    Ok(exists)
}

/// The rows of one table that a reset must leave as they were: how many, and
/// a SHA-256 over all of them in a fixed order.
#[derive(Debug, PartialEq, Eq)]
struct TableDigest {
    rows: u64,
    sha256: String,
}

/// Tables a reset compares no rows of. `demo_account_build` is the build's
/// own record; it is written and removed by the build itself.
const UNCOMPARED_TABLES: [&str; 1] = ["demo_account_build"];

/// A digest per table of every row that does not belong to the Demo Account:
/// the rows of every other account, the owner's among them, and the rows that
/// belong to no account, such as `server_settings`. A row belongs to the
/// account in its `account_id` column, or, for a table without one, to the
/// account of the row a `NOT NULL` foreign key points at.
///
/// Full-text index tables are left out: the index is contentless, so it has
/// no rows to read back, and it is written from `messages`, which is compared.
async fn non_demo_state(db: &Path, demo_id: i64) -> Result<BTreeMap<String, TableDigest>> {
    let pool = engine::open_pool_for_path(db)
        .await
        .with_context(|| format!("open {} to verify non-demo accounts", db.display()))?;
    let mut conn = pool
        .acquire()
        .await
        .with_context(|| format!("open {} to verify non-demo accounts", db.display()))?;
    let state = non_demo_state_on_conn(&mut conn, demo_id)
        .await
        .with_context(|| format!("read the non-demo rows of {}", db.display()));
    conn.close().await?;
    pool.close().await;
    state
}

async fn non_demo_state_on_conn(
    conn: &mut sqlx::SqliteConnection,
    demo_id: i64,
) -> Result<BTreeMap<String, TableDigest>> {
    use futures_util::TryStreamExt;
    use sha2::{Digest, Sha256};

    let tables: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, COALESCE(sql, '') FROM sqlite_master
         WHERE type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\'
         ORDER BY name",
    )
    .fetch_all(&mut *conn)
    .await?;
    let virtual_tables: Vec<&str> = tables
        .iter()
        .filter(|(_, sql)| {
            sql.trim_start()
                .to_ascii_uppercase()
                .starts_with("CREATE VIRTUAL TABLE")
        })
        .map(|(name, _)| name.as_str())
        .collect();
    let mut state = BTreeMap::new();
    for (table, _) in &tables {
        let is_index = virtual_tables
            .iter()
            .any(|vt| table == vt || table.starts_with(&format!("{vt}_")));
        if is_index || UNCOMPARED_TABLES.contains(&table.as_str()) {
            continue;
        }
        let columns = table_columns(conn, table).await?;
        let row_text = columns
            .iter()
            .map(|column| format!("quote(t.{})", quote_ident(column)))
            .collect::<Vec<_>>()
            .join(" || ',' || ");
        let owner = row_owner(conn, table, &columns).await?;
        let selection = match &owner {
            Some(owner) => format!(
                "FROM {} t {} WHERE {} != $1",
                quote_ident(table),
                owner.join,
                owner.account
            ),
            None => format!("FROM {} t", quote_ident(table)),
        };
        let sql = format!("SELECT {row_text} {selection} ORDER BY 1");
        let query = sqlx::query_scalar::<_, String>(&sql);
        // Only a table whose rows belong to accounts leaves the Demo
        // Account's rows out.
        let query = match owner {
            Some(_) => query.bind(demo_id),
            None => query,
        };
        let mut rows = query.fetch(&mut *conn);
        let mut hasher = Sha256::new();
        let mut count = 0u64;
        while let Some(row) = rows
            .try_next()
            .await
            .with_context(|| format!("read {table}"))?
        {
            hasher.update(row.as_bytes());
            hasher.update(b"\n");
            count += 1;
        }
        drop(rows);
        state.insert(
            table.clone(),
            TableDigest {
                rows: count,
                sha256: crate::assets_api::hex_encode(&hasher.finalize()),
            },
        );
    }
    Ok(state)
}

/// How to find the account a row of a table belongs to: a join to add after
/// the table, aliased `t`, and the expression that names the account.
struct RowOwner {
    join: String,
    account: String,
}

/// The account a row of `table` belongs to, or `None` for a table whose rows
/// belong to no account (it has no `account_id` and no foreign key).
///
/// # Errors
///
/// Returns an error for a table with a foreign key but no `NOT NULL` one to a
/// table that has `account_id`: its rows cannot be told apart, and a new
/// table like that needs a rule here.
async fn row_owner(
    conn: &mut sqlx::SqliteConnection,
    table: &str,
    columns: &[String],
) -> Result<Option<RowOwner>> {
    if table == "accounts" {
        return Ok(Some(RowOwner {
            join: String::new(),
            account: "t.id".into(),
        }));
    }
    if columns.iter().any(|column| column == "account_id") {
        return Ok(Some(RowOwner {
            join: String::new(),
            account: "t.account_id".into(),
        }));
    }
    let keys: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT fk.\"table\", fk.\"from\", fk.\"to\"
         FROM pragma_foreign_key_list($1) fk
         JOIN pragma_table_info($1) col ON col.name = fk.\"from\"
         WHERE col.\"notnull\" = 1
         ORDER BY fk.id",
    )
    .bind(table)
    .fetch_all(&mut *conn)
    .await?;
    for (parent, from, to) in &keys {
        if table_columns(conn, parent)
            .await?
            .iter()
            .any(|c| c == "account_id")
        {
            return Ok(Some(RowOwner {
                join: format!(
                    "JOIN {} p ON p.{} = t.{}",
                    quote_ident(parent),
                    quote_ident(to),
                    quote_ident(from)
                ),
                account: "p.account_id".into(),
            }));
        }
    }
    let any_key: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pragma_foreign_key_list($1)")
        .bind(table)
        .fetch_one(&mut *conn)
        .await?;
    if any_key > 0 {
        bail!("cannot tell which account a row of {table} belongs to");
    }
    Ok(None)
}

/// The column names of `table`, in declaration order.
async fn table_columns(conn: &mut sqlx::SqliteConnection, table: &str) -> Result<Vec<String>> {
    Ok(
        sqlx::query_scalar("SELECT name FROM pragma_table_info($1) ORDER BY cid")
            .bind(table)
            .fetch_all(&mut *conn)
            .await?,
    )
}

/// `name` as a quoted SQLite identifier.
fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

#[derive(Clone, Copy)]
struct ResetPaths<'a> {
    active_db: &'a Path,
    prepared_db: &'a Path,
    active_account: &'a Path,
    prepared_account: &'a Path,
}

/// Swap the prepared database and account folder into their active paths.
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

/// One of the two things a reset swaps: the database file or the account
/// folder. Each has an active path, a prepared replacement, and a backup path
/// the active one is moved to first so the swap can be undone.
struct Swap<'a> {
    /// What the paths hold, for messages: "database" or "account directory".
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
    /// The two swaps in install order: database, then account folder.
    fn swaps(&self) -> Result<[Swap<'a>; 2]> {
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
        Ok([
            Swap::new("database", self.active_db, self.prepared_db, db_backup),
            Swap::new(
                "account directory",
                self.active_account,
                self.prepared_account,
                account_backup,
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
    if !paths.prepared_db.is_file() || !paths.prepared_account.is_dir() {
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

/// Reclaim space after the demo import replaced most rows. Best effort:
/// failures are printed, not returned, because the demo rows are already
/// committed and a failed vacuum only costs disk space.
async fn vacuum_after_demo(db: &SqlitePool) {
    let mut conn = match db.acquire().await {
        Ok(conn) => conn,
        Err(err) => {
            eprintln!("  sql:      warning: vacuum after demo failed to open a connection: {err}");
            return;
        }
    };
    maintenance::vacuum_import_tables(&mut conn).await;
}

/// Parse `config/seed.toml` from the bundle.
fn load_demo_seed(path: &Path) -> Result<DemoSeed> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read demo seed {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("failed to parse demo seed {}", path.display()))
}

/// Seed the demo account row and profile in the database `db`.
async fn seed_demo_account(db: &SqlitePool, account_id: i64, seed: &DemoSeed) -> Result<()> {
    let mut conn = db.acquire().await?;
    seed_demo_account_on_conn(&mut conn, account_id, seed).await
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
    // it. The server takes its grant from its id (`DEMO_ACCOUNT_PERMISSIONS`)
    // and does not read these flags for it. The row still says the same
    // grant, export and neither import nor delete, so the database matches
    // what the server applies
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

/// Delete the demo account, `actor` acting, and its on-disk attachments.
/// Leaves the database and other accounts intact. Its data rows follow via
/// CASCADE; its Audit Trail stays, unlinked, with an `account_deleted` entry,
/// as any account's does (`docs/adr/0016-the-demo-account-is-fixed-not-configured.md`).
async fn wipe_demo_account(
    cfg: &Config,
    db: &SqlitePool,
    account_id: i64,
    actor: AuditActor,
) -> Result<()> {
    println!(
        "Reset demo — clearing account data in {}",
        cfg.paths.db.display()
    );
    let mut conn = db
        .acquire()
        .await
        .with_context(|| format!("open {} for demo account wipe", cfg.paths.db.display()))?;
    let deleted = account_profile::delete_account(&mut conn, account_id, actor).await?;
    println!("  sql:      demo account rows removed (account existed={deleted})");
    drop(conn);

    crate::asset_store::remove_account_dir(&cfg.paths, account_id).with_context(|| {
        format!(
            "remove {}",
            crate::asset_store::account_dir(&cfg.paths, account_id).display()
        )
    })
}

#[cfg(test)]
pub(crate) mod tests;
