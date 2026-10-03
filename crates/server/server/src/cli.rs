//! Command-line interface for `message-crate-server`.
//!
//! Each subcommand is a `clap` argument struct plus one `run_*` function. The
//! functions here only parse, validate, and print; the work lives in the
//! module each one calls (`import_cli`, `dedupe`, `reset_demo`, and so on).
//! Every command that reads the database opens it the same way: the config with
//! `--db` applied, through [`OpenDb`].

use std::fmt::Write as _;
use std::path::PathBuf;

use crate::imports_api::ImportMode;
use anyhow::{Result, bail};
use clap::{Args, Command, CommandFactory, Parser, Subcommand};

use crate::config::{Config, validate_source_id};
use crate::dedupe::DedupeStats;
use crate::open_db::OpenDb;
use demo_seed::DemoSize;

#[derive(Debug, Parser)]
#[command(name = "message-crate-server")]
#[command(about = "Import and view messages in SQLite")]
/// Command-line entry point parsed from argv.
pub struct Cli {
    /// Chosen subcommand and its options.
    #[command(subcommand)]
    pub command: Commands,
}

/// One subcommand per CLI operation: import, serve, and maintenance.
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Import a message-ir JSONL folder, one Import Run per source (source from export.source unless --source)
    Import(ImportArgs),

    /// Work on an account's import sessions (`discard` clears a stranded one)
    Imports(ImportsArgs),

    /// Soft-hide the same SMS when it appears under more than one import source
    DedupeCrossSource(DedupeArgs),

    /// Rebuild the Demo Account: generate Demo Data, clear the account,
    /// import, and process assets. Adds the account when it is not there.
    ResetDemo(ResetDemoArgs),

    /// Create an empty database, with no Demo Account. `serve` adds the Demo
    /// Account only to a database that does not exist yet, so this is how a
    /// Message Crate starts empty.
    CreateDatabase(CreateDatabaseArgs),

    /// Run the HTTP API. A database that does not exist yet is created with
    /// the Demo Account before the server listens.
    Serve(ServeArgs),

    /// Write the OpenAPI document (JSON) to stdout or --output. Does not open the database.
    DumpOpenapi(DumpArgs),

    /// Write this CLI's docs-site reference page (Markdown) to stdout or
    /// --output. Does not open the database.
    DumpCliDocs(DumpArgs),

    /// Write one docs-site page per HTTP problem type (Markdown) into the
    /// --output directory, or all of them to stdout. Does not open the database.
    DumpErrorDocs(DumpArgs),

    /// Convert media under assets/ into browser previews under `assets_converted/`
    ProcessAssets(ProcessAssetsArgs),

    /// Claim an unclaimed Message Crate by creating its owner. Refuses one
    /// that already has an owner.
    CreateOwner(CreateOwnerArgs),

    /// Set a new password for the owner, ending their sessions. Refuses a
    /// Message Crate that has no owner yet.
    ResetOwnerPassword(ResetOwnerPasswordArgs),
}

/// Options for `create-owner`.
#[derive(Debug, Args)]
pub struct CreateOwnerArgs {
    /// Login username for the owner
    #[arg(long)]
    pub username: String,

    /// Password for the owner; must satisfy the server's password policy
    #[arg(long)]
    pub password: String,

    /// Path to config.toml
    #[arg(long, default_value = "config/config.toml")]
    pub config: PathBuf,
}

/// Options for `reset-owner-password`.
#[derive(Debug, Args)]
pub struct ResetOwnerPasswordArgs {
    /// New password for the owner; must satisfy the password policy
    #[arg(long)]
    pub password: String,

    /// Path to config.toml
    #[arg(long, default_value = "config/config.toml")]
    pub config: PathBuf,
}

/// Options for `import`.
#[derive(Debug, Args)]
pub struct ImportArgs {
    /// Optional source override (forces one source; skips IR export.source)
    #[arg(long)]
    pub source: Option<String>,

    /// Path to config.toml
    #[arg(long, default_value = "config/config.toml")]
    pub config: PathBuf,

    /// Folder of `*.jsonl` conversation files (+ attachments)
    #[arg(long = "input", visible_aliases = ["dir", "staging-dir", "export-dir"])]
    pub input: PathBuf,

    /// Output SQLite database path (overrides config)
    #[arg(long)]
    pub db: Option<PathBuf>,

    /// Originals asset store directory (overrides the account's default)
    #[arg(long)]
    pub assets_dir: Option<PathBuf>,

    /// Attachment handling: copy (default), none, convert, compress
    #[arg(long, default_value = "copy")]
    pub media: String,

    /// Import mode: replace (wipe sources found in input) or append
    #[arg(long, default_value = "replace")]
    pub mode: ImportMode,

    /// Skip the cross-source soft-dedupe pass after import
    #[arg(long)]
    pub skip_dedupe: bool,

    /// Near-time window in seconds for dedupe Pass B (default 2)
    #[arg(long, default_value_t = 2)]
    pub window_secs: i64,

    /// Account username or id (scopes import to this account)
    #[arg(long)]
    pub account: String,
}

/// The `imports` group: one subcommand per operation on import sessions.
#[derive(Debug, Args)]
pub struct ImportsArgs {
    /// Which operation to run.
    #[command(subcommand)]
    pub command: ImportsCommand,
}

/// Operations on an account's import sessions.
#[derive(Debug, Subcommand)]
pub enum ImportsCommand {
    /// Discard the account's active import session, if it has one. A killed
    /// `import` leaves its session open, and no later import can start until
    /// it is discarded.
    Discard(ImportsDiscardArgs),
}

/// Options for `imports discard`.
#[derive(Debug, Args)]
pub struct ImportsDiscardArgs {
    /// Path to config.toml
    #[arg(long, default_value = "config/config.toml")]
    pub config: PathBuf,

    /// Output SQLite database path (overrides config)
    #[arg(long)]
    pub db: Option<PathBuf>,

    /// Account username or id whose active session is discarded
    #[arg(long)]
    pub account: String,
}

/// Options for `dedupe-cross-source`.
#[derive(Debug, Args)]
pub struct DedupeArgs {
    /// Path to config.toml
    #[arg(long, default_value = "config/config.toml")]
    pub config: PathBuf,

    /// Output SQLite database path (overrides config)
    #[arg(long)]
    pub db: Option<PathBuf>,

    /// Near-time window in seconds for Pass B (default 2)
    #[arg(long, default_value_t = 2)]
    pub window_secs: i64,

    /// Account username or id (scopes dedupe to this account)
    #[arg(long)]
    pub account: String,
}

/// Options for `reset-demo`.
#[derive(Debug, Args)]
pub struct ResetDemoArgs {
    /// How much Demo Data: medium is about 54,000 messages, large about 613,000
    #[arg(long, value_enum, default_value_t = DemoSize::Medium)]
    pub size: DemoSize,

    /// Path to config.toml; the Demo Account is rebuilt in the database it names
    #[arg(long, default_value = "config/config.toml")]
    pub config: PathBuf,
}

/// Options for `create-database`.
#[derive(Debug, Args)]
pub struct CreateDatabaseArgs {
    /// Path to config.toml
    #[arg(long, default_value = "config/config.toml")]
    pub config: PathBuf,
}

/// Options for `serve`.
#[derive(Debug, Args)]
pub struct ServeArgs {
    /// Path to config.toml (must include a `[server]` section)
    #[arg(long, default_value = "config/config.toml")]
    pub config: PathBuf,

    /// Keep the whole Message Crate in this folder and read no config file:
    /// the database is `messagecrate.db` inside it, with every other setting
    /// at its default
    #[arg(long, conflicts_with = "config")]
    pub data_dir: Option<PathBuf>,

    /// Address to listen on (overrides `[server] bind`; default 127.0.0.1:8080)
    #[arg(long)]
    pub bind: Option<String>,

    /// Folder holding the built website (overrides `[server] static_dir`;
    /// default `static`)
    #[arg(long)]
    pub static_dir: Option<PathBuf>,

    /// Another website allowed to call this API, added to `[server]
    /// cors_origins`; repeat for more than one. The packaged desktop app's
    /// own origins are always allowed
    #[arg(long = "cors-origin", value_name = "ORIGIN")]
    pub cors_origins: Vec<String>,
}

/// Options shared by `dump-openapi`, `dump-cli-docs` and `dump-error-docs`.
#[derive(Debug, Args)]
pub struct DumpArgs {
    /// Destination file (a directory for `dump-error-docs`). Omit to print stdout.
    #[arg(long)]
    pub output: Option<PathBuf>,
}

/// Options for `process-assets`.
#[derive(Debug, Args)]
pub struct ProcessAssetsArgs {
    /// Path to config.toml
    #[arg(long, default_value = "config/config.toml")]
    pub config: PathBuf,

    /// Re-convert even when a browser preview already exists
    #[arg(long)]
    pub force: bool,

    /// Convert and log without writing files or updating the DB
    #[arg(long)]
    pub dry_run: bool,

    /// Skip image conversion
    #[arg(long)]
    pub skip_image: bool,

    /// Skip video conversion
    #[arg(long)]
    pub skip_video: bool,

    /// Skip audio conversion
    #[arg(long)]
    pub skip_audio: bool,

    /// Override SQLite database path from config
    #[arg(long)]
    pub db: Option<PathBuf>,
}

/// Build the clap [`Command`] definition for `message-crate-server`.
pub fn clap_command() -> Command {
    Cli::command()
}

/// Execute a parsed [`Cli`], dispatching to the matching subcommand.
///
/// # Errors
///
/// Returns the subcommand's error, or a validation error for bad flag values.
pub async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Import(args) => run_import(args).await,
        Commands::Imports(args) => match args.command {
            ImportsCommand::Discard(args) => run_imports_discard(args).await,
        },
        Commands::DedupeCrossSource(args) => run_dedupe(args).await,
        Commands::ResetDemo(args) => run_reset_demo(args).await,
        Commands::CreateDatabase(args) => run_create_database(args).await,
        Commands::Serve(args) => run_serve(args).await,
        Commands::DumpOpenapi(args) => crate::openapi::write_openapi(args.output.as_deref()),
        Commands::DumpCliDocs(args) => crate::cli_docs::write_cli_docs(args.output.as_deref()),
        Commands::DumpErrorDocs(args) => {
            crate::error_docs::write_error_docs(args.output.as_deref())
        }
        Commands::ProcessAssets(args) => run_process_assets(args).await,
        Commands::CreateOwner(args) => run_create_owner(args).await,
        Commands::ResetOwnerPassword(args) => run_reset_owner_password(args).await,
    }
}

/// Claim this Message Crate and report the owner's username.
async fn run_create_owner(args: CreateOwnerArgs) -> Result<()> {
    let cfg = Config::load(&args.config)?;
    let opened = OpenDb::open(cfg).await?;
    let username = crate::owner_cli::create_owner(&opened, &args.username, &args.password).await?;
    opened.close().await;
    println!("Message Crate claimed. Log in as {username}.");
    Ok(())
}

/// Set the owner's password and report the username to log in with.
async fn run_reset_owner_password(args: ResetOwnerPasswordArgs) -> Result<()> {
    let cfg = Config::load(&args.config)?;
    let opened = OpenDb::open(cfg).await?;
    let username = crate::owner_cli::reset_owner_password(&opened, &args.password).await?;
    opened.close().await;
    println!("Owner password set. Log in as {username}.");
    Ok(())
}

/// Reject a negative dedupe window before any database is opened.
fn validate_window_secs(window_secs: i64) -> Result<()> {
    if window_secs < 0 {
        bail!("--window-secs must be >= 0");
    }
    Ok(())
}

/// Import a folder of conversation files, then print the counts.
async fn run_import(args: ImportArgs) -> Result<()> {
    let cfg = Config::load_with_db(&args.config, args.db)?;
    validate_window_secs(args.window_secs)?;
    if let Some(ref source) = args.source {
        validate_source_id(source)?;
    }
    let media = media::MediaMode::parse(&args.media).ok_or_else(|| {
        anyhow::anyhow!(
            "invalid --media '{}' (expected copy, none, convert, or compress)",
            args.media
        )
    })?;
    let opened = OpenDb::open(cfg).await?;
    let account = opened.account_id(&args.account).await?;

    let stats = crate::import_cli::run(
        &opened,
        &crate::import_cli::CliImportOptions {
            account_id: account,
            input_dir: args.input,
            assets_dir: args.assets_dir,
            source_override: args.source,
            mode: args.mode,
            media,
            skip_dedupe: args.skip_dedupe,
            window_secs: args.window_secs,
        },
    )
    .await?;

    println!();
    println!("Import into {}", opened.location().display());
    println!("  input:         {}", stats.input_dir.display());
    println!("  sources:       {}", stats.sources.join(", "));
    print!("{}", format_import_stats(&stats.import));
    match stats.dedupe {
        Some(dedupe) => {
            println!("Cross-source soft-dedupe (hide the same SMS across sources)");
            print!("{}", format_dedupe_stats(&dedupe));
        }
        None => println!("Cross-source soft-dedupe skipped (--skip-dedupe)"),
    }
    opened.close().await;
    Ok(())
}

/// Discard the account's active import session and say which one it was,
/// or that there was none.
async fn run_imports_discard(args: ImportsDiscardArgs) -> Result<()> {
    let cfg = Config::load_with_db(&args.config, args.db)?;
    let opened = OpenDb::open(cfg).await?;
    let account = opened.account_id(&args.account).await?;
    let mut conn = opened.conn().await?;
    let discarded = crate::db::imports::discard_running_import(&mut conn, account).await?;
    drop(conn);
    if discarded.is_some() {
        crate::asset_store::sweep_after_run(&opened.db, &opened.cfg.paths, account).await;
    }
    opened.close().await;
    print!(
        "{}",
        format_discarded_import(&args.account, discarded.as_ref())
    );
    Ok(())
}

/// What `imports discard` prints: the session it discarded, or that the
/// account had none.
fn format_discarded_import(
    account: &str,
    discarded: Option<&crate::db::imports::ImportRow>,
) -> String {
    match discarded {
        Some(row) => format!(
            "Discarded import session {} for account {account} (source {}, {} mode, started {}, stage {}).\n",
            row.id,
            row.source,
            row.mode,
            row.started_at,
            row.stage.map_or("none", |stage| stage.as_str()),
        ),
        None => format!("Account {account} has no active import session.\n"),
    }
}

/// The counts from one import stage, one line each, ready to print.
fn format_import_stats(import: &crate::imports_api::ImportStats) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "  files:         {}", import.files);
    let _ = writeln!(out, "  conversations: {}", import.conversations);
    let _ = writeln!(out, "  participants:  {}", import.participants);
    let _ = writeln!(out, "  messages:      {}", import.messages);
    let _ = writeln!(out, "  messages deduped: {}", import.messages_deduped);
    if import.mode == ImportMode::Append {
        let _ = writeln!(out, "  messages appended: {}", import.messages_appended);
    }
    let _ = writeln!(
        out,
        "  attachment records: {} (message↔media links in the database)",
        import.attachments
    );
    let _ = writeln!(out, "  tapbacks:      {}", import.tapbacks);
    let _ = writeln!(
        out,
        "  media files stored:  {} (unique blobs under assets/)",
        import.assets_copied
    );
    let _ = writeln!(
        out,
        "  media files reused:  {} (same content hash already on disk)",
        import.assets_deduped
    );
    let _ = writeln!(
        out,
        "  media files missing: {} (attachment path not found on disk)",
        import.assets_missing
    );
    if import.phones_needing_review > 0 {
        let _ = writeln!(
            out,
            "  phones needing review: {} (ambiguous numbers — fix them in Message Crate)",
            import.phones_needing_review
        );
    }
    if import.other_identities > 0 {
        let _ = writeln!(
            out,
            "  identities with no address: {} (a name the backup gave in place of an address; the export is incomplete)",
            import.other_identities
        );
    }
    out
}

/// The counts from a cross-source dedupe pass, one line each, ready to print.
fn format_dedupe_stats(stats: &DedupeStats) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "  fingerprints set:   {} (one per message; not a duplicate count)",
        stats.keys_filled
    );
    let _ = writeln!(out, "  exact duplicate groups: {}", stats.exact_groups);
    let _ = writeln!(out, "  exact duplicates hidden: {}", stats.exact_flagged);
    let _ = writeln!(out, "  near duplicates flagged: {}", stats.near_flagged);
    out
}

/// Run the cross-source dedupe pass on its own and print the counts.
async fn run_dedupe(args: DedupeArgs) -> Result<()> {
    let cfg = Config::load_with_db(&args.config, args.db)?;
    validate_window_secs(args.window_secs)?;
    let opened = OpenDb::open(cfg).await?;
    let account = opened.account_id(&args.account).await?;
    let mut conn = opened.conn().await?;
    let priority = crate::dedupe::source_priority_from_db(&mut conn, account).await?;

    println!("Cross-source dedupe on {}", opened.location().display());
    println!("  config:       {}", args.config.display());
    println!("  account:      {account}");
    println!("  window_secs:  {}", args.window_secs);
    println!(
        "  priority:     {}",
        if priority.is_empty() {
            "(none)".to_string()
        } else {
            priority.join(", ")
        }
    );

    let stats =
        crate::dedupe::dedupe_cross_source(&mut conn, account, None, args.window_secs).await?;
    print!("{}", format_dedupe_stats(&stats));
    drop(conn);
    opened.close().await;
    Ok(())
}

/// Rebuild the demo account from the bundle and print what landed.
async fn run_reset_demo(args: ResetDemoArgs) -> Result<()> {
    let cfg = Config::load(&args.config)?;
    let stats = crate::reset_demo::run_reset_demo(args.size, &cfg).await?;
    println!();
    println!("Demo reset complete");
    if stats.seed.messages > 0 {
        println!("  generated messages: {}", stats.seed.messages);
    }
    println!();
    println!("Imported into the database");
    println!("  conversations:        {}", stats.import.conversations);
    println!("  messages:             {}", stats.import.messages);
    println!(
        "  attachment records:    {} (message↔media links; not unique files)",
        stats.import.attachments
    );
    println!("  tapbacks:             {}", stats.import.tapbacks);
    println!(
        "  contacts named:       {} (from the demo address book)",
        stats.address_book.contacts_changed()
    );
    println!();
    println!("Media files on disk (assets/)");
    println!(
        "  unique files stored:   {} (content-addressed blobs)",
        stats.import.assets_copied
    );
    println!(
        "  files missing:         {} (referenced by attachments but not found)",
        stats.import.assets_missing
    );
    println!();
    println!("Duplicate detection across sources");
    println!(
        "  fingerprints set:      {} (one per message; used to match the same SMS)",
        stats.dedupe_keys_filled
    );
    println!();
    println!("Browser previews (assets_converted/; needs ffmpeg)");
    println!(
        "  converted for web:     {} (JPEG/MP4/MP3 written)",
        stats.process_assets.derived
    );
    println!(
        "  left as-is:            {} (already converted, non-media, or small JPEG)",
        stats.process_assets.skipped
    );
    println!("  conversion failures:   {}", stats.process_assets.errors);
    Ok(())
}

/// Create the database the config names, empty, or say it is already there.
async fn run_create_database(args: CreateDatabaseArgs) -> Result<()> {
    let cfg = Config::load(&args.config)?;
    let is_new = crate::reset_demo::database_is_new(&cfg).await?;
    let opened = OpenDb::create_or_open(cfg).await?;
    if is_new {
        println!("Empty database created at {}.", opened.location().display());
    } else {
        println!(
            "Database at {} already exists; left as it is.",
            opened.location().display()
        );
    }
    opened.close().await;
    Ok(())
}

/// Start the HTTP server with the config.
async fn run_serve(args: ServeArgs) -> Result<()> {
    let cfg = serve_config(args)?;
    let _ = cfg.require_server()?;
    crate::server::run(cfg).await
}

/// The config `serve` runs on: the folder `--data-dir` names, or else the
/// config file, with the other flags applied over either. A relative
/// `--static-dir` resolves where the config file's paths do; with
/// `--data-dir`, which reads no config file, that is the working directory,
/// as for `--data-dir` itself.
fn serve_config(args: ServeArgs) -> Result<Config> {
    let (cfg, root) = match &args.data_dir {
        Some(data_dir) => {
            // Made absolute so nothing later depends on the directory the
            // server was started in.
            let cwd = std::env::current_dir()?;
            let data_dir = if data_dir.is_absolute() {
                data_dir.clone()
            } else {
                cwd.join(data_dir)
            };
            (Config::for_data_dir(&data_dir), cwd)
        }
        None => (
            Config::load(&args.config)?,
            crate::config::config_root(&args.config)?,
        ),
    };
    Ok(cfg.with_serve_overrides(&root, args.bind, args.static_dir, args.cors_origins))
}

/// Convert stored media into browser previews.
///
/// # Errors
///
/// Returns an error after the summary line when any conversion failed, so a
/// cron job or script that runs the command sees a non-zero exit status.
async fn run_process_assets(args: ProcessAssetsArgs) -> Result<()> {
    let cfg = Config::load_with_db(&args.config, args.db)?;
    let opened = OpenDb::open(cfg).await?;
    let stats = crate::process_assets::run(
        &opened,
        &crate::process_assets::ProcessAssetsOptions {
            force: args.force,
            dry_run: args.dry_run,
            skip_image: args.skip_image,
            skip_video: args.skip_video,
            skip_audio: args.skip_audio,
            account: None,
        },
    )
    .await?;
    opened.close().await;
    if stats.errors > 0 {
        bail!(
            "{} conversion(s) failed; those originals stay without a browser preview",
            stats.errors
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;
