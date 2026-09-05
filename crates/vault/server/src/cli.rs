//! Command-line interface for `message-vault-server`.
//!
//! Each subcommand is a `clap` argument struct plus one `run_*` function. The
//! functions here only parse, validate, and print; the work lives in the
//! module each one calls (`import_cli`, `dedupe`, `reset_demo`, and so on).

use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Args, Command, CommandFactory, Parser, Subcommand};

use crate::config::{Config, validate_source_id};
use crate::db::engine::DbTarget;
use crate::db::{account_profile, contacts as contacts_db};
use crate::dedupe::DedupeStats;

#[derive(Parser)]
#[command(name = "message-vault-server")]
#[command(about = "Import and view messages in SQLite")]
/// Command-line entry point parsed from argv.
pub struct Cli {
    /// Chosen subcommand and its options.
    #[command(subcommand)]
    pub command: Commands,
}

/// One subcommand per CLI operation: import, serve, and maintenance.
#[derive(Subcommand)]
pub enum Commands {
    /// Import a message-ir JSONL folder (source from export.source unless --source)
    Import(ImportArgs),

    /// Soft-hide the same SMS when it appears under more than one import source
    DedupeCrossSource(DedupeArgs),

    /// Import an address book (VCF or vCard CSV) into an existing database.
    ImportContacts(ImportContactsArgs),

    /// Regenerate demo bundle, clear demo account data, import, and process assets
    ResetDemo(ResetDemoArgs),

    /// Run HTTP ingest API (`POST /v1/import` with message-ir JSONL)
    Serve(ServeArgs),

    /// Write the OpenAPI document (JSON) to stdout or --output. Does not open the database.
    DumpOpenapi(DumpArgs),

    /// Write this CLI's docs-site reference page (Markdown) to stdout or
    /// --output. Does not open the database.
    DumpCliDocs(DumpArgs),

    /// Convert media under assets/ into browser previews under `assets_converted/`
    ProcessAssets(ProcessAssetsArgs),
}

/// Options for `import`.
#[derive(Args)]
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

    /// Connection URL (postgres://… or sqlite://…; overrides `[database]` url)
    #[arg(long)]
    pub db_url: Option<String>,

    /// Originals asset store directory (overrides account/source default; fixed-source only)
    #[arg(long)]
    pub assets_dir: Option<PathBuf>,

    /// Address book to load: VCF or vCard CSV export
    #[arg(long = "contacts", alias = "contacts-csv")]
    pub contacts: Option<PathBuf>,

    /// Reload contacts from --contacts even if the table is non-empty
    #[arg(long)]
    pub overwrite_contacts: bool,

    /// Attachment handling: copy (default), none, convert, compress
    #[arg(long, default_value = "copy")]
    pub media: String,

    /// Import mode: replace (wipe sources found in input) or append
    #[arg(long, default_value = "replace")]
    pub mode: String,

    /// Skip the cross-source soft-dedupe pass after import
    #[arg(long)]
    pub skip_dedupe: bool,

    /// Near-time window in seconds for dedupe Pass B (default 2)
    #[arg(long, default_value_t = 2)]
    pub window_secs: i64,

    /// Account username or UUID (scopes import to this vault tenant)
    #[arg(long)]
    pub account: String,
}

/// Options for `dedupe-cross-source`.
#[derive(Args)]
pub struct DedupeArgs {
    /// Path to config.toml
    #[arg(long, default_value = "config/config.toml")]
    pub config: PathBuf,

    /// Output SQLite database path (overrides config)
    #[arg(long)]
    pub db: Option<PathBuf>,

    /// Connection URL (postgres://… or sqlite://…; overrides `[database]` url)
    #[arg(long)]
    pub db_url: Option<String>,

    /// Near-time window in seconds for Pass B (default 2)
    #[arg(long, default_value_t = 2)]
    pub window_secs: i64,

    /// Account username or UUID (scopes dedupe to this vault tenant)
    #[arg(long)]
    pub account: String,
}

/// Options for `import-contacts`.
#[derive(Args)]
pub struct ImportContactsArgs {
    /// Path to config.toml
    #[arg(long, default_value = "config/config.toml")]
    pub config: PathBuf,

    /// Address book: VCF, or vCard CSV (First Name, Last Name, Phone columns)
    #[arg(long = "contacts", alias = "contacts-csv")]
    pub contacts: PathBuf,

    /// Output SQLite database path (overrides config)
    #[arg(long)]
    pub db: Option<PathBuf>,

    /// Account username or UUID (scopes contacts to this vault tenant)
    #[arg(long)]
    pub account: String,
}

/// Options for `reset-demo`.
#[derive(Args)]
pub struct ResetDemoArgs {
    /// Demo bundle directory (rewritten by demo-seed, then imported)
    #[arg(long, default_value = "crates/vault/demo-seed")]
    pub bundle: PathBuf,

    /// Active config path. Overwritten on the SQLite path; only read for
    /// attachment paths when `--db-url` is set (default config/config.toml)
    #[arg(long, default_value = "config/config.toml")]
    pub config: PathBuf,

    /// Connection URL (postgres://… or sqlite://…); seeds that database
    /// instead of replacing paths.db
    #[arg(long)]
    pub db_url: Option<String>,
}

/// Options for `serve`.
#[derive(Args)]
pub struct ServeArgs {
    /// Path to config.toml (must include `[server]` with `bind`)
    #[arg(long, default_value = "config/config.toml")]
    pub config: PathBuf,

    /// Connection URL (postgres://… or sqlite://…; overrides `[database]` url)
    #[arg(long)]
    pub db_url: Option<String>,
}

/// Options shared by `dump-openapi` and `dump-cli-docs`.
#[derive(Args)]
pub struct DumpArgs {
    /// Destination file. Omit to print stdout.
    #[arg(long)]
    pub output: Option<PathBuf>,
}

/// Options for `process-assets`.
#[derive(Args)]
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

    /// Only process this source id
    #[arg(long)]
    pub source: Option<String>,
}

/// Build the clap [`Command`] definition for `message-vault-server`.
pub fn clap_command() -> Command {
    Cli::command()
}

/// Execute a parsed [`Cli`], dispatching to the matching subcommand.
///
/// # Errors
///
/// Returns the subcommand's error, or a validation error for bad flag values.
pub async fn run(cli: Cli) -> Result<()> {
    // Register the sqlx Any drivers once before any pool connects.
    sqlx::any::install_default_drivers();
    match cli.command {
        Commands::Import(args) => run_import(args).await,
        Commands::DedupeCrossSource(args) => run_dedupe(args).await,
        Commands::ImportContacts(args) => run_import_contacts(args).await,
        Commands::ResetDemo(args) => run_reset_demo(args).await,
        Commands::Serve(args) => run_serve(args).await,
        Commands::DumpOpenapi(args) => crate::openapi::write_openapi(args.output.as_deref()),
        Commands::DumpCliDocs(args) => crate::cli_docs::write_cli_docs(args.output.as_deref()),
        Commands::ProcessAssets(args) => run_process_assets(args).await,
    }
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
    let cfg = Config::load(&args.config)?;
    validate_window_secs(args.window_secs)?;
    if let Some(ref source) = args.source {
        validate_source_id(source)?;
    }
    let mode = crate::import::ImportMode::parse(&args.mode)?;
    let media = media::MediaMode::parse(&args.media).ok_or_else(|| {
        anyhow::anyhow!(
            "invalid --media '{}' (expected copy, none, convert, or compress)",
            args.media
        )
    })?;
    let db_path = args.db.clone().unwrap_or_else(|| cfg.paths.db.clone());
    let target = DbTarget::new(args.db_url.as_deref(), &db_path);
    let account = account_profile::resolve_account_ref_at(target, &args.account).await?;
    let target_label = target.to_string();

    let stats = crate::import_cli::run(
        &cfg,
        &crate::import_cli::CliImportOptions {
            account_id: account,
            input_dir: args.input,
            db_path: args.db,
            db_url: args.db_url,
            assets_dir: args.assets_dir,
            source_override: args.source,
            mode,
            media,
            contacts: args.contacts,
            overwrite_contacts: args.overwrite_contacts,
            skip_dedupe: args.skip_dedupe,
            window_secs: args.window_secs,
        },
    )
    .await?;

    println!();
    println!("Import into {target_label}");
    println!("  input:         {}", stats.input_dir.display());
    println!("  sources:       {}", stats.sources.join(", "));
    print_import_stats(&stats.import);
    match stats.dedupe {
        Some(dedupe) => {
            println!("Cross-source soft-dedupe (hide the same SMS across sources)");
            print_dedupe_stats(&dedupe);
        }
        None => println!("Cross-source soft-dedupe skipped (--skip-dedupe)"),
    }
    Ok(())
}

/// Print the counts from one import stage.
fn print_import_stats(import: &crate::import::ImportStats) {
    if import.contacts_skipped {
        println!(
            "  contacts:      (skipped — already loaded or no --contacts; use --overwrite-contacts)"
        );
    } else {
        println!("  contacts:      {}", import.contacts);
        println!("  contact handles:{}", import.contact_handles);
    }
    println!("  files:         {}", import.files);
    println!("  conversations: {}", import.conversations);
    println!("  participants:  {}", import.participants);
    println!("  messages:      {}", import.messages);
    println!("  messages deduped: {}", import.messages_deduped);
    if import.mode == "append" {
        println!("  messages appended: {}", import.messages_appended);
    }
    println!(
        "  attachment records: {} (message↔media links in the database)",
        import.attachments
    );
    println!("  tapbacks:      {}", import.tapbacks);
    println!(
        "  media files stored:  {} (unique blobs under assets/)",
        import.assets_copied
    );
    println!(
        "  media files reused:  {} (same content hash already on disk)",
        import.assets_deduped
    );
    println!(
        "  media files missing: {} (attachment path not found on disk)",
        import.assets_missing
    );
    if import.phones_needing_review > 0 {
        println!(
            "  phones needing review: {} (ambiguous numbers — fix them in the vault)",
            import.phones_needing_review
        );
    }
}

/// Print the counts from a cross-source dedupe pass.
fn print_dedupe_stats(stats: &DedupeStats) {
    println!(
        "  fingerprints set:   {} (one per message; not a duplicate count)",
        stats.keys_filled
    );
    println!("  exact duplicate groups: {}", stats.exact_groups);
    println!("  exact duplicates hidden: {}", stats.exact_flagged);
    println!("  near duplicates flagged: {}", stats.near_flagged);
}

/// Run the cross-source dedupe pass on its own and print the counts.
async fn run_dedupe(args: DedupeArgs) -> Result<()> {
    let cfg = Config::load(&args.config)?;
    validate_window_secs(args.window_secs)?;
    let db = args.db.unwrap_or_else(|| cfg.paths.db.clone());
    let target = DbTarget::new(args.db_url.as_deref(), &db);
    let account = account_profile::resolve_account_ref_at(target, &args.account).await?;

    let priority = {
        let pool = target.open().await?;
        let mut conn = pool.acquire().await?;
        crate::dedupe::source_priority_from_db(&mut conn, &account).await?
    };

    println!("Cross-source dedupe on {target}");
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

    let stats = crate::dedupe::run_dedupe(target, &account, args.window_secs).await?;
    print_dedupe_stats(&stats);
    Ok(())
}

/// Load an address book into an existing SQLite vault and print the counts.
async fn run_import_contacts(args: ImportContactsArgs) -> Result<()> {
    let cfg = Config::load(&args.config)?;
    let db = args.db.unwrap_or_else(|| cfg.paths.db.clone());
    let target = DbTarget::Path(&db);
    let account = account_profile::resolve_account_ref_at(target, &args.account).await?;

    if let Some(parent) = db.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }

    let pool = target.open().await?;
    let mut conn = pool.acquire().await?;
    let stats =
        contacts_db::load_contacts_if_needed(&mut conn, Some(&args.contacts), true, &account)
            .await?;

    println!("Imported contacts into {}", db.display());
    println!("  config:       {}", args.config.display());
    println!("  account:      {account}");
    println!("  contacts:     {}", args.contacts.display());
    println!("  rows:         {}", stats.contacts);
    println!("  phones:       {}", stats.phones);
    Ok(())
}

/// Rebuild the demo account from the bundle and print what landed.
async fn run_reset_demo(args: ResetDemoArgs) -> Result<()> {
    let stats =
        crate::reset_demo::run_reset_demo(&args.bundle, &args.config, args.db_url.as_deref())
            .await?;
    println!();
    println!("Demo reset complete");
    if stats.seed.messages > 0 {
        println!("  generated messages: {}", stats.seed.messages);
    }
    println!();
    println!("Imported into vault");
    println!("  conversations:        {}", stats.import.conversations);
    println!("  messages:             {}", stats.import.messages);
    println!(
        "  attachment records:    {} (message↔media links; not unique files)",
        stats.import.attachments
    );
    println!("  tapbacks:             {}", stats.import.tapbacks);
    println!("  contacts:             {}", stats.import.contacts);
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

/// Start the HTTP server with the config, honouring a `--db-url` override.
async fn run_serve(args: ServeArgs) -> Result<()> {
    let mut cfg = Config::load(&args.config)?;
    if let Some(url) = args.db_url {
        cfg.database.url = Some(url);
    }
    let _ = cfg.require_server()?;
    crate::server::run(cfg).await
}

/// Convert stored media into browser previews.
async fn run_process_assets(args: ProcessAssetsArgs) -> Result<()> {
    let cfg = Config::load(&args.config)?;
    if let Some(ref source) = args.source {
        validate_source_id(source)?;
    }
    crate::process_assets::run(
        &cfg,
        &crate::process_assets::ProcessAssetsOptions {
            force: args.force,
            dry_run: args.dry_run,
            skip_image: args.skip_image,
            skip_video: args.skip_video,
            skip_audio: args.skip_audio,
            db: args.db,
            source: args.source,
            db_url: None,
        },
    )
    .await?;
    Ok(())
}
