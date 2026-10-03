//! CLI directory import: any JSONL folder; source from IR `export.source` unless overridden.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::config::validate_source_id;
use crate::db::account_profile;
use crate::dedupe::{self, DedupeStats};
use crate::imports_api::{self, ImportMode, ImportOptions, ImportStats};
use crate::jsonl;
use crate::models::ExportRecord;
use crate::open_db::OpenDb;
use media::MediaMode;

/// Options for a CLI directory import.
#[derive(Debug, Clone)]
pub struct CliImportOptions {
    /// Account the import writes into.
    pub account_id: i64,
    /// Folder of `*.jsonl` conversation files (+ attachments).
    pub input_dir: PathBuf,
    /// Originals asset store override; per-account default when `None`.
    pub assets_dir: Option<PathBuf>,
    /// When set, force this source for every conversation (ignore IR export.source).
    pub source_override: Option<String>,
    /// Import mode: replace or append.
    pub mode: ImportMode,
    /// Attachment handling mode: copy, none, convert, compress.
    pub media: MediaMode,
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
    /// Source ids written, one Import Run each.
    pub sources: Vec<String>,
    /// Import stage counts, summed over the runs.
    pub import: ImportStats,
    /// Dedupe counts when the pass ran, `None` when skipped.
    pub dedupe: Option<DedupeStats>,
}

/// Which source ids the import writes, the files of each, and where that
/// decision came from. Each source is one Import Run, so each run has one
/// source, its own counts, its own Contact Group and its own Saved Search.
struct SourcePlan {
    /// Each source id with its files, in source order.
    runs: BTreeMap<String, Vec<PathBuf>>,
    /// True when the ids were read from each conversation's `export.source`.
    from_jsonl: bool,
}

impl SourcePlan {
    /// Use the `--source` override for every file when given, else read
    /// every conversation header and group the files by `export.source`.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid source id, an unreadable file, a file
    /// holding conversations of two sources, or a folder with no
    /// `export.source` anywhere.
    fn resolve(opts: &CliImportOptions, paths: &[PathBuf], input: &Path) -> Result<Self> {
        if let Some(source) = &opts.source_override {
            validate_source_id(source)?;
            return Ok(Self {
                runs: BTreeMap::from([(source.clone(), paths.to_vec())]),
                from_jsonl: false,
            });
        }
        let discovered = files_by_source(paths)?;
        if discovered.is_empty() {
            bail!(
                "no conversation export.source found in {}; each conversation needs \
                 export.source in the message-ir header (or pass --source)",
                input.display()
            );
        }
        for source in discovered.keys() {
            validate_source_id(source)?;
        }
        Ok(Self {
            runs: discovered,
            from_jsonl: true,
        })
    }

    /// The source ids, in the order their runs import.
    fn sources(&self) -> Vec<String> {
        self.runs.keys().cloned().collect()
    }
}

/// Import a folder of JSON Lines files into the database, then optionally run
/// cross-source duplicate hiding.
///
/// # Errors
///
/// Returns an error when the input directory is missing, has no `.jsonl`
/// files, or import / duplicate detection fails.
pub async fn run(opened: &OpenDb, opts: &CliImportOptions) -> Result<CliImportStats> {
    let input = &opts.input_dir;
    if !input.is_dir() {
        bail!("input directory does not exist: {}", input.display());
    }
    let paths = list_jsonl_files(input)?;
    if paths.is_empty() {
        bail!("input {} has no .jsonl files", input.display());
    }
    let plan = SourcePlan::resolve(opts, &paths, input)?;
    print_plan(opts, opened, &plan);

    let mut conn = opened.conn().await?;
    account_profile::ensure_account_row(&mut conn, opts.account_id).await?;

    let mut import_stats = ImportStats {
        mode: opts.mode,
        ..Default::default()
    };
    for (source, files) in &plan.runs {
        let run = import_under_session(&opened.cfg, opts, &mut conn, source, files, &plan).await?;
        import_stats.add_run(&run);
    }
    let dedupe = if opts.skip_dedupe {
        None
    } else {
        let stats =
            dedupe::dedupe_cross_source(&mut conn, opts.account_id, None, opts.window_secs).await?;
        println!(
            "  dedupe:       fingerprints_set={} exact_hidden={} near_flagged={} (fingerprints are one per message, not duplicates)",
            stats.keys_filled, stats.exact_flagged, stats.near_flagged
        );
        Some(stats)
    };

    Ok(CliImportStats {
        input_dir: input.clone(),
        sources: plan.sources(),
        import: import_stats,
        dedupe,
    })
}

/// Echo what the import is about to do so a wrong flag is visible before any
/// row is written.
fn print_plan(opts: &CliImportOptions, opened: &OpenDb, plan: &SourcePlan) {
    println!("Import");
    println!("  account:      {}", opts.account_id);
    println!("  input:        {}", opts.input_dir.display());
    println!("  db:           {}", opened.location().display());
    println!("  sources:      {}", plan.sources().join(", "));
    if plan.from_jsonl {
        println!("  source mode:  from JSONL export.source");
    } else {
        println!("  source mode:  --source override");
    }
    println!("  mode:         {}", opts.mode.as_str());
    println!("  media:        {}", opts.media.as_str());
}

/// Record the Import Run of one source, import that source's `paths` inside
/// it, and mark the run finished either way so the Settings import table
/// never shows a run stuck in progress.
///
/// # Errors
///
/// Returns the import's error after the run has been marked failed.
async fn import_under_session(
    cfg: &crate::config::Config,
    opts: &CliImportOptions,
    conn: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
    source: &str,
    paths: &[PathBuf],
    plan: &SourcePlan,
) -> Result<ImportStats> {
    let account_id = opts.account_id;
    let assets_dir = opts
        .assets_dir
        .clone()
        .unwrap_or_else(|| cfg.paths.assets_dir_for_account(account_id, source));
    let session = imports_api::OwnedSession::start(
        conn,
        account_id,
        source,
        opts.mode,
        "message-crate-server",
    )
    .await?;

    let import_opts = ImportOptions {
        assets_dir: &assets_dir,
        asset_root: &opts.input_dir,
        mode: opts.mode,
        source: opts.source_override.as_deref().unwrap_or(""),
        account_id,
        fill_content_keys: true,
        import_id: Some(session.id),
        source_from_jsonl: plan.from_jsonl,
        paths: plan.from_jsonl.then_some(&cfg.paths),
        media: opts.media,
        wipe_sources: Some(vec![source.to_string()]),
    };
    let result = imports_api::import_jsonl_files_on_conn(
        conn,
        paths,
        &import_opts,
        imports_api::ImportSchemaMode::AssumeReady,
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
/// Returns an error when `dir` cannot be read, or when one of its entries
/// cannot be read, because skipping that entry would leave its conversation
/// out of the import while the run reports success.
pub fn list_jsonl_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let entries = fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))?;
    jsonl_paths(dir, entries.map(|entry| entry.map(|entry| entry.path())))
}

/// The `.jsonl` paths among `entries`, the listing of `dir`, sorted.
fn jsonl_paths(
    dir: &Path,
    entries: impl IntoIterator<Item = std::io::Result<PathBuf>>,
) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in entries {
        let path =
            entry.with_context(|| format!("failed to read an entry of {}", dir.display()))?;
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

/// Group the files by the IR `export.source` of their conversation headers.
///
/// # Errors
///
/// Returns an error when a JSON Lines file cannot be read, a conversation
/// has no `export.source`, or one file holds conversations of two sources.
fn files_by_source(paths: &[PathBuf]) -> Result<BTreeMap<String, Vec<PathBuf>>> {
    let mut runs: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for path in paths {
        let mut file_source: Option<String> = None;
        // `read_records` refuses a file with no conversation header.
        for record in jsonl::read_records(path)? {
            if let ExportRecord::Conversation(c) = record {
                let Some(source) = c.export_source.as_deref().and_then(message_ir::trimmed) else {
                    bail!(
                        "{}: conversation '{}' is missing export.source \
                         (required for CLI directory import; or pass --source)",
                        path.display(),
                        c.chat_identifier
                    );
                };
                match &file_source {
                    Some(first) if first != source => bail!(
                        "{}: holds conversations of two sources, '{first}' and '{source}'; \
                         each file is imported in the Import Run of its one source",
                        path.display()
                    ),
                    Some(_) => {}
                    None => file_source = Some(source.to_string()),
                }
            }
        }
        if let Some(source) = file_source {
            runs.entry(source).or_default().push(path.clone());
        }
    }
    Ok(runs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::account_profile;
    use crate::open_db::fresh_config;
    use tempfile::TempDir;

    const ALICE: i64 = 7;

    /// The number the conversation is with.
    const PHONE: &str = "+14075551234";

    /// One incoming text from `phone`.
    fn conversation_with(phone: &str) -> String {
        format!(
            r#"{{"schema_version":4,"export":{{"source":"sms-backup-restore","tool":"test","tool_version":"0","owner_handle":null,"owner_display_name":null}},"conversation":{{"chat_identifier":"{phone}","conversation_type":"individual","group_title":null,"participants":[{{"handle":"{phone}","display_name":null}}],"stats":{{"message_count":1,"attachment_count":0,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}}}}
{{"guid":"g-contacts-1","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"sms","message_kind":"sms","sender_handle":"{phone}","sender_display_name":null,"subject":null,"text":"hi","attachments":[],"imessage":null,"source":null}}
"#
        )
    }

    /// A database with account alice and an export folder holding one
    /// conversation with `PHONE`.
    async fn fixture_with_export(dir: &Path) -> (OpenDb, CliImportOptions) {
        let opened = OpenDb::open(fresh_config(dir)).await.unwrap();
        let mut conn = opened.conn().await.unwrap();
        account_profile::insert_account_at(&mut conn, ALICE, "alice", None, None)
            .await
            .unwrap();
        drop(conn);

        let input = dir.join("export");
        fs::create_dir_all(&input).unwrap();
        fs::write(input.join("chat.jsonl"), conversation_with(PHONE)).unwrap();

        let opts = CliImportOptions {
            account_id: ALICE,
            input_dir: input,
            assets_dir: None,
            source_override: None,
            mode: ImportMode::Append,
            media: MediaMode::Clone,
            skip_dedupe: true,
            window_secs: 2,
        };
        (opened, opts)
    }

    async fn count(opened: &OpenDb, sql: &str) -> i64 {
        let mut conn = opened.conn().await.unwrap();
        sqlx::query_scalar(sql)
            .bind(ALICE)
            .fetch_one(&mut *conn)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn an_import_makes_a_contact_for_the_conversations_participant() {
        let dir = TempDir::new().unwrap();
        let (opened, opts) = fixture_with_export(dir.path()).await;

        let stats = run(&opened, &opts).await.unwrap();

        assert_eq!(stats.import.messages, 1);
        assert_eq!(stats.import.mode, ImportMode::Append);
        assert_eq!(
            count(
                &opened,
                "SELECT COUNT(*) FROM contacts WHERE account_id = $1"
            )
            .await,
            1,
            "only the conversation's participant became a contact"
        );
        opened.close().await;
    }

    /// Issue #1173: an Apple Messages export and an SMS export in one folder,
    /// each with a chat with `PHONE`, import as one conversation holding both
    /// exports' messages. Both services resolve to the one phone handle.
    #[tokio::test]
    async fn two_exports_of_a_chat_with_one_number_import_as_one_conversation() {
        let dir = TempDir::new().unwrap();
        let (opened, opts) = fixture_with_export(dir.path()).await;
        let apple = conversation_with(PHONE)
            .replace("sms-backup-restore", "imessage")
            .replace("g-contacts-1", "g-apple-1");
        fs::write(opts.input_dir.join("apple.jsonl"), apple).unwrap();

        let stats = run(&opened, &opts).await.unwrap();

        assert_eq!(stats.import.messages, 2);
        assert_eq!(
            count(
                &opened,
                "SELECT COUNT(*) FROM conversations WHERE account_id = $1"
            )
            .await,
            1
        );
        assert_eq!(
            count(
                &opened,
                "SELECT COUNT(DISTINCT conversation_id) FROM messages WHERE account_id = $1"
            )
            .await,
            1,
            "both messages are in the one conversation"
        );
        opened.close().await;
    }

    /// Each Import Run of alice, by run id: its source, the name of its
    /// Contact Group, how many contacts that group holds, and its Saved
    /// Search's query. A run missing either shortcut is left out.
    async fn runs_with_their_shortcuts(opened: &OpenDb) -> Vec<(String, String, i64, String)> {
        let mut conn = opened.conn().await.unwrap();
        sqlx::query_as(
            "SELECT i.source, g.name,
                    (SELECT COUNT(*) FROM contact_group_members m WHERE m.group_id = g.id),
                    s.query
             FROM imports i
             JOIN contact_groups g
               ON g.account_id = i.account_id AND g.kind = 'import'
              AND g.name = i.source || ' import ' || substr(i.finished_at, 1, 10)
             JOIN saved_searches s
               ON s.account_id = i.account_id AND s.query = 'import:#' || i.id
             WHERE i.account_id = $1
             ORDER BY i.id",
        )
        .bind(ALICE)
        .fetch_all(&mut *conn)
        .await
        .unwrap()
    }

    /// Issue #1107: a run of the `import` command makes the run's Contact
    /// Group, holding the contact it created, and its Saved Search, as a run
    /// completed over HTTP does.
    #[tokio::test]
    async fn an_import_makes_the_runs_contact_group_and_saved_search() {
        let dir = TempDir::new().unwrap();
        let (opened, opts) = fixture_with_export(dir.path()).await;

        run(&opened, &opts).await.unwrap();

        let runs = runs_with_their_shortcuts(&opened).await;
        assert_eq!(runs.len(), 1, "{runs:?}");
        let (source, group, members, _) = &runs[0];
        assert_eq!(source, "sms-backup-restore");
        assert!(group.starts_with("sms-backup-restore import "), "{group}");
        assert_eq!(*members, 1, "the group holds the contact the run created");
        opened.close().await;
    }

    /// Issue #1107: a folder holding two sources imports as two Import Runs,
    /// each with one source, its own Contact Group and its own Saved Search.
    #[tokio::test]
    async fn a_folder_of_two_sources_imports_as_one_run_per_source() {
        let dir = TempDir::new().unwrap();
        let (opened, opts) = fixture_with_export(dir.path()).await;
        let apple = conversation_with("+14075550123")
            .replace("sms-backup-restore", "imessage")
            .replace("g-contacts-1", "g-apple-1");
        fs::write(opts.input_dir.join("apple.jsonl"), apple).unwrap();

        let stats = run(&opened, &opts).await.unwrap();

        assert_eq!(stats.import.messages, 2, "both runs' messages are counted");
        assert_eq!(
            count(
                &opened,
                "SELECT COUNT(*) FROM imports WHERE account_id = $1"
            )
            .await,
            2,
            "one run per source, and none names both"
        );
        let runs = runs_with_their_shortcuts(&opened).await;
        let sources: Vec<&str> = runs.iter().map(|run| run.0.as_str()).collect();
        assert_eq!(sources, ["imessage", "sms-backup-restore"], "{runs:?}");
        assert!(
            runs.iter().all(|run| run.2 == 1),
            "each run's group holds the contact that run created: {runs:?}"
        );
        opened.close().await;
    }

    /// Issue #1166: an entry of the folder that cannot be read fails the
    /// listing, naming the folder, instead of being skipped.
    #[test]
    fn an_unreadable_folder_entry_fails_the_listing() {
        let dir = Path::new("/exports/phone");
        let entries = vec![
            Ok(dir.join("a.jsonl")),
            Err(std::io::Error::other("entry unreadable")),
            Ok(dir.join("b.jsonl")),
        ];

        let err = jsonl_paths(dir, entries).unwrap_err();

        let message = format!("{err:#}");
        assert!(message.contains("/exports/phone"), "{message}");
        assert!(message.contains("entry unreadable"), "{message}");
    }

    #[test]
    fn files_are_grouped_by_the_source_in_their_headers() {
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
        let runs = files_by_source(&paths).unwrap();
        assert_eq!(
            runs,
            BTreeMap::from([
                ("go-sms-pro".to_string(), vec![tmp.path().join("b.jsonl")]),
                ("imessage".to_string(), vec![tmp.path().join("a.jsonl")]),
            ])
        );
    }
}
