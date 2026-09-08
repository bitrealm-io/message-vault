//! Command-line interface for `message-vault-server`.
//!
//! Each subcommand is a `clap` argument struct plus one `run_*` function. The
//! functions here only parse, validate, and print; the work lives in the
//! module each one calls (`import_cli`, `dedupe`, `reset_demo`, and so on).
//! Every command that reads the vault opens it the same way: the config with
//! `--db` and `--db-url` applied, through [`OpenVault`].

use std::fmt::Write as _;
use std::path::PathBuf;

use crate::import::ImportMode;
use anyhow::{Result, bail};
use clap::{Args, Command, CommandFactory, Parser, Subcommand};

use crate::config::{Config, validate_source_id};
use crate::db::contacts as contacts_db;
use crate::dedupe::DedupeStats;
use crate::open_vault::OpenVault;

#[derive(Debug, Parser)]
#[command(name = "message-vault-server")]
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
    /// Import a message-ir JSONL folder (source from export.source unless --source)
    Import(ImportArgs),

    /// Soft-hide the same SMS when it appears under more than one import source
    DedupeCrossSource(DedupeArgs),

    /// Import an address book (VCF or vCard CSV) into an existing database.
    ImportContacts(ImportContactsArgs),

    /// Regenerate demo bundle, clear demo account data, import, and process assets
    ResetDemo(ResetDemoArgs),

    /// Run the HTTP API (`POST /v1/imports/{id}/batches` takes message-ir JSONL)
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

    /// Claim an unclaimed vault by creating its owner. Refuses a vault that
    /// already has one.
    CreateOwner(CreateOwnerArgs),

    /// Set a new password for the vault owner, ending their sessions. Refuses
    /// a vault that has no owner yet.
    ResetOwnerPassword(ResetOwnerPasswordArgs),
}

/// Options for `create-owner`.
#[derive(Debug, Args)]
pub struct CreateOwnerArgs {
    /// Login username for the vault owner
    #[arg(long)]
    pub username: String,

    /// Password for the vault owner; must satisfy the vault's password policy
    #[arg(long)]
    pub password: String,

    /// Path to config.toml
    #[arg(long, default_value = "config/config.toml")]
    pub config: PathBuf,

    /// Connection URL (postgres://… or sqlite://…; overrides `[database]` url)
    #[arg(long)]
    pub db_url: Option<String>,
}

/// Options for `reset-owner-password`.
#[derive(Debug, Args)]
pub struct ResetOwnerPasswordArgs {
    /// New password for the vault owner; must satisfy the password policy
    #[arg(long)]
    pub password: String,

    /// Path to config.toml
    #[arg(long, default_value = "config/config.toml")]
    pub config: PathBuf,

    /// Connection URL (postgres://… or sqlite://…; overrides `[database]` url)
    #[arg(long)]
    pub db_url: Option<String>,
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
    pub mode: ImportMode,

    /// Skip the cross-source soft-dedupe pass after import
    #[arg(long)]
    pub skip_dedupe: bool,

    /// Near-time window in seconds for dedupe Pass B (default 2)
    #[arg(long, default_value_t = 2)]
    pub window_secs: i64,

    /// Account username or id (scopes import to this vault tenant)
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

    /// Connection URL (postgres://… or sqlite://…; overrides `[database]` url)
    #[arg(long)]
    pub db_url: Option<String>,

    /// Near-time window in seconds for Pass B (default 2)
    #[arg(long, default_value_t = 2)]
    pub window_secs: i64,

    /// Account username or id (scopes dedupe to this vault tenant)
    #[arg(long)]
    pub account: String,
}

/// Options for `import-contacts`.
#[derive(Debug, Args)]
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

    /// Connection URL (postgres://… or sqlite://…; overrides `[database]` url)
    #[arg(long)]
    pub db_url: Option<String>,

    /// Account username or id (scopes contacts to this vault tenant)
    #[arg(long)]
    pub account: String,
}

/// Options for `reset-demo`.
#[derive(Debug, Args)]
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
#[derive(Debug, Args)]
pub struct ServeArgs {
    /// Path to config.toml (must include `[server]` with `bind`)
    #[arg(long, default_value = "config/config.toml")]
    pub config: PathBuf,

    /// Connection URL (postgres://… or sqlite://…; overrides `[database]` url)
    #[arg(long)]
    pub db_url: Option<String>,
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
    match cli.command {
        Commands::Import(args) => run_import(args).await,
        Commands::DedupeCrossSource(args) => run_dedupe(args).await,
        Commands::ImportContacts(args) => run_import_contacts(args).await,
        Commands::ResetDemo(args) => run_reset_demo(args).await,
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

/// Claim the vault and report the owner's username.
async fn run_create_owner(args: CreateOwnerArgs) -> Result<()> {
    let cfg = Config::load(&args.config)?.with_db_overrides(None, args.db_url);
    let vault = OpenVault::open(cfg).await?;
    let username = crate::owner_cli::create_owner(&vault, &args.username, &args.password).await?;
    vault.close().await;
    println!("Vault claimed. Sign in as {username}.");
    Ok(())
}

/// Set the vault owner's password and report the username to sign in with.
async fn run_reset_owner_password(args: ResetOwnerPasswordArgs) -> Result<()> {
    let cfg = Config::load(&args.config)?.with_db_overrides(None, args.db_url);
    let vault = OpenVault::open(cfg).await?;
    let username = crate::owner_cli::reset_owner_password(&vault, &args.password).await?;
    vault.close().await;
    println!("Owner password set. Sign in as {username}.");
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
    let cfg = Config::load(&args.config)?.with_db_overrides(args.db, args.db_url);
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
    let vault = OpenVault::open(cfg).await?;
    let account = vault.account_id(&args.account).await?;

    let stats = crate::import_cli::run(
        &vault,
        &crate::import_cli::CliImportOptions {
            account_id: account,
            input_dir: args.input,
            assets_dir: args.assets_dir,
            source_override: args.source,
            mode: args.mode,
            media,
            contacts: args.contacts,
            overwrite_contacts: args.overwrite_contacts,
            skip_dedupe: args.skip_dedupe,
            window_secs: args.window_secs,
        },
    )
    .await?;

    println!();
    println!("Import into {}", vault.location());
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
    vault.close().await;
    Ok(())
}

/// The counts from one import stage, one line each, ready to print.
fn format_import_stats(import: &crate::import::ImportStats) -> String {
    let mut out = String::new();
    if import.contacts_skipped {
        out.push_str(
            "  contacts:      (skipped — already loaded or no --contacts; use --overwrite-contacts)\n",
        );
    } else {
        let _ = writeln!(out, "  contacts:      {}", import.contacts);
        let _ = writeln!(out, "  contact handles:{}", import.contact_handles);
    }
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
            "  phones needing review: {} (ambiguous numbers — fix them in the vault)",
            import.phones_needing_review
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
    let cfg = Config::load(&args.config)?.with_db_overrides(args.db, args.db_url);
    validate_window_secs(args.window_secs)?;
    let vault = OpenVault::open(cfg).await?;
    let account = vault.account_id(&args.account).await?;
    let mut conn = vault.conn().await?;
    let priority = crate::dedupe::source_priority_from_db(&mut conn, account).await?;

    println!("Cross-source dedupe on {}", vault.location());
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
    vault.close().await;
    Ok(())
}

/// Load an address book into an existing vault and print the counts.
async fn run_import_contacts(args: ImportContactsArgs) -> Result<()> {
    let cfg = Config::load(&args.config)?.with_db_overrides(args.db, args.db_url);
    let vault = OpenVault::open(cfg).await?;
    let account = vault.account_id(&args.account).await?;
    let mut conn = vault.conn().await?;
    let stats =
        contacts_db::load_contacts_if_needed(&mut conn, Some(&args.contacts), true, account)
            .await?;

    println!("Imported contacts into {}", vault.location());
    println!("  config:       {}", args.config.display());
    println!("  account:      {account}");
    println!("  contacts:     {}", args.contacts.display());
    println!("  rows:         {}", stats.contacts);
    println!("  phones:       {}", stats.phones);
    drop(conn);
    vault.close().await;
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
    let cfg = Config::load(&args.config)?.with_db_overrides(None, args.db_url);
    let _ = cfg.require_server()?;
    crate::server::run(cfg).await
}

/// Convert stored media into browser previews.
async fn run_process_assets(args: ProcessAssetsArgs) -> Result<()> {
    let cfg = Config::load(&args.config)?.with_db_overrides(args.db, None);
    if let Some(ref source) = args.source {
        validate_source_id(source)?;
    }
    let vault = OpenVault::open(cfg).await?;
    crate::process_assets::run(
        &vault,
        &crate::process_assets::ProcessAssetsOptions {
            force: args.force,
            dry_run: args.dry_run,
            skip_image: args.skip_image,
            skip_video: args.skip_video,
            skip_audio: args.skip_audio,
            source: args.source,
        },
    )
    .await?;
    vault.close().await;
    Ok(())
}

#[cfg(test)]
mod tests;
