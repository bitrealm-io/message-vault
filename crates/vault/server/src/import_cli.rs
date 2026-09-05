//! CLI directory import: any JSONL folder; source from IR `export.source` unless overridden.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::config::{Config, validate_source_id};
use crate::db::account_profile;
use crate::db::engine::DbTarget;
use crate::db::schema;
use crate::dedupe::{self, DedupeStats};
use crate::import::{self, ImportMode, ImportOptions, ImportStats};
use crate::jsonl;
use crate::models::ExportRecord;
use media::MediaMode;

/// Options for a CLI directory import.
#[derive(Debug, Clone)]
pub struct CliImportOptions {
    /// Vault account the import writes into.
    pub account_id: String,
    /// Folder of `*.jsonl` conversation files (+ attachments).
    pub input_dir: PathBuf,
    /// Database path override; falls back to config when `None`.
    pub db_path: Option<PathBuf>,
    /// Database URL override (`sqlite:...` / `postgres://...`); wins over `db_path`.
    pub db_url: Option<String>,
    /// Originals asset store override; per-account default when `None`.
    pub assets_dir: Option<PathBuf>,
    /// When set, force this source for every conversation (ignore IR export.source).
    pub source_override: Option<String>,
    /// Import mode: replace or append.
    pub mode: ImportMode,
    /// Attachment handling mode: copy, none, convert, compress.
    pub media: MediaMode,
    /// Optional address book to load: VCF or vCard CSV export.
    pub contacts: Option<PathBuf>,
    /// Reload contacts even when the table is non-empty.
    pub overwrite_contacts: bool,
    /// Skip the cross-source soft-dedupe pass after import.
    pub skip_dedupe: bool,
    /// Near-time window in seconds for dedupe Pass B.
    pub window_secs: i64,
}

/// Counts and inputs reported by a CLI directory import.
#[derive(Debug)]
pub struct CliImportStats {
    /// Input folder that was imported.
    pub input_dir: PathBuf,
    /// Source ids written (one per conversation unless overridden).
    pub sources: Vec<String>,
    /// Import stage counts.
    pub import: ImportStats,
    /// Dedupe counts when the pass ran, `None` when skipped.
    pub dedupe: Option<DedupeStats>,
}

/// Which source ids the import writes, and where that decision came from.
struct SourcePlan {
    /// Source ids written (one per conversation unless overridden).
    sources: Vec<String>,
    /// True when the ids were read from each conversation's `export.source`.
    from_jsonl: bool,
}

impl SourcePlan {
    /// Use the `--source` override when given, else read every conversation
    /// header and collect the distinct `export.source` values.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid source id, an unreadable file, or a
    /// folder with no `export.source` anywhere.
    fn resolve(opts: &CliImportOptions, paths: &[PathBuf], input: &Path) -> Result<Self> {
        if let Some(source) = &opts.source_override {
            validate_source_id(source)?;
            return Ok(Self {
                sources: vec![source.clone()],
                from_jsonl: false,
            });
        }
        let discovered = discover_sources(paths)?;
        if discovered.is_empty() {
            bail!(
                "no conversation export.source found in {}; each conversation needs \
                 export.source in the message-ir header (or pass --source)",
                input.display()
            );
        }
        for source in &discovered {
            validate_source_id(source)?;
        }
        Ok(Self {
            sources: discovered,
            from_jsonl: true,
        })
    }
}

/// Import a folder of JSON Lines files into the vault, then optionally run
/// cross-source duplicate hiding.
///
/// # Errors
///
/// Returns an error when the input directory is missing, has no `.jsonl`
/// files, or import / duplicate detection fails.
pub async fn run(cfg: &Config, opts: &CliImportOptions) -> Result<CliImportStats> {
    let input = &opts.input_dir;
    if !input.is_dir() {
        bail!("input directory does not exist: {}", input.display());
    }
    let paths = list_jsonl_files(input)?;
    if paths.is_empty() {
        bail!("input {} has no .jsonl files", input.display());
    }
    let plan = SourcePlan::resolve(opts, &paths, input)?;
    let db_path = opts.db_path.clone().unwrap_or_else(|| cfg.paths.db.clone());
    let target = DbTarget::new(opts.db_url.as_deref(), &db_path);
    print_plan(opts, target, &plan);

    let pool = target.open().await?;
    let mut conn = pool.acquire().await?;
    schema::ensure_vault_schema(&mut conn).await?;
    account_profile::ensure_account_row(&mut conn, &opts.account_id).await?;

    let import_stats = import_under_session(cfg, opts, &mut conn, &paths, &plan).await?;
    let dedupe = if opts.skip_dedupe {
        None
    } else {
        let stats =
            dedupe::dedupe_cross_source(&mut conn, &opts.account_id, None, opts.window_secs)
                .await?;
        println!(
            "  dedupe:       fingerprints_set={} exact_hidden={} near_flagged={} (fingerprints are one per message, not duplicates)",
            stats.keys_filled, stats.exact_flagged, stats.near_flagged
        );
        Some(stats)
    };

    Ok(CliImportStats {
        input_dir: input.clone(),
        sources: plan.sources,
        import: import_stats,
        dedupe,
    })
}

/// Echo what the import is about to do so a wrong flag is visible before any
/// row is written.
fn print_plan(opts: &CliImportOptions, target: DbTarget<'_>, plan: &SourcePlan) {
    println!("Import");
    println!("  account:      {}", opts.account_id);
    println!("  input:        {}", opts.input_dir.display());
    println!("  db:           {target}");
    println!("  sources:      {}", plan.sources.join(", "));
    if plan.from_jsonl {
        println!("  source mode:  from JSONL export.source");
    } else {
        println!("  source mode:  --source override");
    }
    println!("  mode:         {}", opts.mode.as_str());
    println!("  media:        {}", opts.media.as_str());
    match &opts.contacts {
        Some(path) => println!("  contacts:     {}", path.display()),
        None => println!("  contacts:     (none — use --contacts for VCF or vCard CSV)"),
    }
}

/// Record an import session, run the import inside it, and mark the session
/// finished either way so the Settings import table never shows a run stuck
/// in progress.
///
/// # Errors
///
/// Returns the import's error after the session has been marked failed.
async fn import_under_session(
    cfg: &Config,
    opts: &CliImportOptions,
    conn: &mut sqlx::pool::PoolConnection<sqlx::Any>,
    paths: &[PathBuf],
    plan: &SourcePlan,
) -> Result<ImportStats> {
    let account_id = &opts.account_id;
    let assets_dir = opts.assets_dir.clone().unwrap_or_else(|| {
        cfg.paths
            .assets_dir_for_account(account_id, plan.sources.first().expect("sources non-empty"))
    });
    let session = import::OwnedSession::start(
        conn,
        account_id,
        &plan.sources.join(","),
        opts.mode,
        "message-vault-server",
    )
    .await?;

    let import_opts = ImportOptions {
        assets_dir: &assets_dir,
        asset_root: &opts.input_dir,
        contacts: opts.contacts.as_deref(),
        overwrite_contacts: opts.overwrite_contacts,
        mode: opts.mode,
        source: opts.source_override.as_deref().unwrap_or(""),
        account_id,
        fill_content_keys: true,
        import_id: Some(session.id),
        source_from_jsonl: plan.from_jsonl,
        paths: plan.from_jsonl.then_some(&cfg.paths),
        media: opts.media,
        wipe_sources: Some(plan.sources.clone()),
    };
    let result = import::import_jsonl_files_on_conn(
        conn,
        paths,
        &import_opts,
        import::ImportSchemaMode::AssumeReady,
    )
    .await;
    session.finish(conn, &result).await;
    result
}

/// Every JSON Lines file (`.jsonl`, one JSON object per line) directly inside
/// `dir`, sorted by path.
///
/// # Errors
///
/// Returns an error when `dir` cannot be read.
pub fn list_jsonl_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let entries = fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))?;
    let mut paths = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
            continue;
        };
        if ext.eq_ignore_ascii_case("jsonl") {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

/// Collect distinct IR `export.source` values from conversation headers.
///
/// # Errors
///
/// Returns an error when a JSON Lines file cannot be read.
pub fn discover_sources(paths: &[PathBuf]) -> Result<Vec<String>> {
    let mut set = std::collections::BTreeSet::new();
    for path in paths {
        let records = jsonl::read_records(path)?;
        let mut saw_conversation = false;
        for record in records {
            if let ExportRecord::Conversation(c) = record {
                saw_conversation = true;
                let Some(source) = c.export_source.as_deref().and_then(message_ir::trimmed) else {
                    bail!(
                        "{}: conversation '{}' is missing export.source \
                         (required for CLI directory import; or pass --source)",
                        path.display(),
                        c.chat_identifier
                    );
                };
                set.insert(source.to_string());
            }
        }
        let is_orphaned = import::is_orphaned_export(path);
        if !saw_conversation && !is_orphaned {
            bail!(
                "{}: no conversation header (cannot determine export.source)",
                path.display()
            );
        }
        if !saw_conversation && is_orphaned {
            bail!(
                "{}: orphaned.jsonl without a conversation header cannot supply export.source; \
                 pass --source, or add a conversation header with export.source",
                path.display()
            );
        }
    }
    Ok(set.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn discover_sources_from_ir_headers() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("a.jsonl"),
            r#"{"schema_version":4,"export":{"source":"imessage","tool":"t","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+1","conversation_type":"individual","group_title":null,"participants":[],"stats":{"message_count":0,"attachment_count":0,"first_timestamp_unix_ms":null,"last_timestamp_unix_ms":null}}}
"#,
        )
        .unwrap();
        fs::write(
            tmp.path().join("b.jsonl"),
            r#"{"schema_version":4,"export":{"source":"go-sms-pro","tool":"t","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+2","conversation_type":"individual","group_title":null,"participants":[],"stats":{"message_count":0,"attachment_count":0,"first_timestamp_unix_ms":null,"last_timestamp_unix_ms":null}}}
"#,
        )
        .unwrap();
        let paths = list_jsonl_files(tmp.path()).unwrap();
        let sources = discover_sources(&paths).unwrap();
        assert_eq!(
            sources,
            vec!["go-sms-pro".to_string(), "imessage".to_string()]
        );
    }
}
