//! The command line, driven the way `main` drives it: a parsed [`Cli`] in,
//! effects on the vault out. Each test writes a config file into a temp dir
//! and asserts on the database afterwards, never on what was printed.

use std::fs;
use std::path::{Path, PathBuf};

use super::*;
use crate::db::account_profile;

const ALICE: i64 = 7;

/// A one-conversation JSON Lines export with no messages, enough for the
/// import to record a conversation under source `imessage`.
const CONVERSATION_JSONL: &str = r#"{"schema_version":4,"export":{"source":"imessage","tool":"t","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+15551230000","conversation_type":"individual","group_title":null,"participants":[],"stats":{"message_count":0,"attachment_count":0,"first_timestamp_unix_ms":null,"last_timestamp_unix_ms":null}}}
"#;

/// A vault under `dir`: its config file on disk, the way an operator has
/// one, with absolute paths so the test does not depend on the working
/// directory. On a Postgres run the database is a schema of its own on that
/// server, as every other test's is.
async fn vault_config(dir: &Path) -> PathBuf {
    let config_dir = dir.join("config");
    fs::create_dir_all(&config_dir).unwrap();
    let mut text = format!(
        "[paths]\ndb = \"{}\"\ndata_dir = \"{}\"\n",
        dir.join("vault.db").display(),
        dir.join("data").display()
    );
    if let Some(url) = crate::pg_test_url() {
        let scoped = crate::db::engine::pg_test_schema_url(&url).await;
        text.push_str(&format!("\n[database]\nurl = \"{scoped}\"\n"));
    }
    let path = config_dir.join("config.toml");
    fs::write(&path, text).unwrap();
    path
}

/// The vault the config names, opened the way the commands open it.
async fn open(config: &Path) -> OpenVault {
    OpenVault::open(Config::load(config).unwrap())
        .await
        .unwrap()
}

/// An ordinary account named alice, so `--account alice` resolves.
async fn with_alice(config: &Path) {
    let vault = open(config).await;
    let mut conn = vault.conn().await.unwrap();
    account_profile::insert_account_at(&mut conn, ALICE, "alice", None, None)
        .await
        .unwrap();
}

async fn count(config: &Path, sql: &str) -> i64 {
    let vault = open(config).await;
    let mut conn = vault.conn().await.unwrap();
    sqlx::query_scalar(sql).fetch_one(&mut *conn).await.unwrap()
}

fn import_args(config: &Path, input: &Path) -> ImportArgs {
    ImportArgs {
        source: None,
        config: config.to_path_buf(),
        input: input.to_path_buf(),
        db: None,
        db_url: None,
        assets_dir: None,
        contacts: None,
        overwrite_contacts: false,
        media: "copy".into(),
        mode: ImportMode::Replace,
        skip_dedupe: false,
        window_secs: 2,
        account: "alice".into(),
    }
}

#[tokio::test]
async fn create_owner_claims_the_vault_once_and_reset_password_needs_the_claim() {
    let dir = tempfile::tempdir().unwrap();
    let config = vault_config(dir.path()).await;

    let unclaimed = run(Cli {
        command: Commands::ResetOwnerPassword(ResetOwnerPasswordArgs {
            password: "correct horse battery staple".into(),
            config: config.clone(),
            db_url: None,
        }),
    })
    .await
    .unwrap_err();
    assert_eq!(
        unclaimed.to_string(),
        "this vault has no owner yet; use `create-owner` to claim it"
    );

    run(Cli {
        command: Commands::CreateOwner(CreateOwnerArgs {
            username: "Owner".into(),
            password: "correct horse battery staple".into(),
            config: config.clone(),
            db_url: None,
        }),
    })
    .await
    .unwrap();

    let twice = run(Cli {
        command: Commands::CreateOwner(CreateOwnerArgs {
            username: "again".into(),
            password: "correct horse battery staple".into(),
            config: config.clone(),
            db_url: None,
        }),
    })
    .await
    .unwrap_err();
    assert_eq!(
        twice.to_string(),
        "this vault already has an owner; use `reset-owner-password` to set a new password for it"
    );

    run(Cli {
        command: Commands::ResetOwnerPassword(ResetOwnerPasswordArgs {
            password: "a different long enough password".into(),
            config: config.clone(),
            db_url: None,
        }),
    })
    .await
    .unwrap();

    let vault = open(&config).await;
    let mut conn = vault.conn().await.unwrap();
    assert!(account_profile::vault_is_claimed(&mut conn).await.unwrap());
    assert_eq!(
        account_profile::username_for_account(&mut conn, account_profile::OWNER_ACCOUNT_ID)
            .await
            .unwrap(),
        Some("Owner".to_string())
    );
}

#[tokio::test]
async fn import_records_the_conversation_then_dedupe_and_process_assets_run_on_it() {
    let dir = tempfile::tempdir().unwrap();
    let config = vault_config(dir.path()).await;
    with_alice(&config).await;
    let input = dir.path().join("export");
    fs::create_dir_all(&input).unwrap();
    fs::write(input.join("chat.jsonl"), CONVERSATION_JSONL).unwrap();

    run(Cli {
        command: Commands::Import(import_args(&config, &input)),
    })
    .await
    .unwrap();

    assert_eq!(
        count(&config, "SELECT COUNT(*) FROM conversations").await,
        1
    );
    assert_eq!(
        count(&config, "SELECT COUNT(*) FROM vault_imports").await,
        1
    );

    run(Cli {
        command: Commands::DedupeCrossSource(DedupeArgs {
            config: config.clone(),
            db: None,
            db_url: None,
            window_secs: 2,
            account: "alice".into(),
        }),
    })
    .await
    .unwrap();

    run(Cli {
        command: Commands::ProcessAssets(ProcessAssetsArgs {
            config: config.clone(),
            force: false,
            dry_run: true,
            skip_image: false,
            skip_video: false,
            skip_audio: false,
            db: None,
            source: None,
        }),
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn import_contacts_loads_the_address_book_for_the_account() {
    let dir = tempfile::tempdir().unwrap();
    let config = vault_config(dir.path()).await;
    with_alice(&config).await;
    let vcf = dir.path().join("book.vcf");
    fs::write(
        &vcf,
        "BEGIN:VCARD\nVERSION:3.0\nFN:Ada Lovelace\nTEL:+15551234567\nEND:VCARD\n",
    )
    .unwrap();

    run(Cli {
        command: Commands::ImportContacts(ImportContactsArgs {
            config: config.clone(),
            contacts: vcf,
            db: None,
            db_url: None,
            account: "alice".into(),
        }),
    })
    .await
    .unwrap();

    assert_eq!(
        count(
            &config,
            "SELECT COUNT(*) FROM contacts WHERE account_id = 7"
        )
        .await,
        1
    );
}

#[tokio::test]
async fn the_db_url_flag_moves_the_vault_to_another_file() {
    let dir = tempfile::tempdir().unwrap();
    let config = vault_config(dir.path()).await;
    if crate::pg_test_url().is_some() {
        // Every vault on a Postgres run is a schema on that server; the
        // point here is the SQLite file the flag names.
        return;
    }
    let other = dir.path().join("elsewhere").join("other.db");
    fs::create_dir_all(other.parent().unwrap()).unwrap();

    run(Cli {
        command: Commands::CreateOwner(CreateOwnerArgs {
            username: "owner".into(),
            password: "correct horse battery staple".into(),
            config: config.clone(),
            db_url: Some(format!("sqlite://{}", other.display())),
        }),
    })
    .await
    .unwrap();

    assert!(other.is_file(), "the owner went into {}", other.display());
    assert!(!dir.path().join("vault.db").exists());
}

#[tokio::test]
async fn import_refuses_a_negative_window_before_opening_anything() {
    let dir = tempfile::tempdir().unwrap();
    let config = vault_config(dir.path()).await;
    let mut args = import_args(&config, dir.path());
    args.window_secs = -1;

    let err = run(Cli {
        command: Commands::Import(args),
    })
    .await
    .unwrap_err();

    assert_eq!(err.to_string(), "--window-secs must be >= 0");
    assert!(!dir.path().join("vault.db").exists());
}

#[tokio::test]
async fn import_refuses_an_unknown_media_mode() {
    let dir = tempfile::tempdir().unwrap();
    let config = vault_config(dir.path()).await;
    let mut args = import_args(&config, dir.path());
    args.media = "shrink".into();

    let err = run(Cli {
        command: Commands::Import(args),
    })
    .await
    .unwrap_err();

    assert_eq!(
        err.to_string(),
        "invalid --media 'shrink' (expected copy, none, convert, or compress)"
    );
}

#[tokio::test]
async fn import_refuses_an_unknown_account() {
    let dir = tempfile::tempdir().unwrap();
    let config = vault_config(dir.path()).await;
    let input = dir.path().join("export");
    fs::create_dir_all(&input).unwrap();
    fs::write(input.join("chat.jsonl"), CONVERSATION_JSONL).unwrap();

    let err = run(Cli {
        command: Commands::Import(import_args(&config, &input)),
    })
    .await
    .unwrap_err();

    assert_eq!(
        err.to_string(),
        "account not found: alice (use an existing username or account id)"
    );
}

#[tokio::test]
async fn dump_openapi_writes_the_document_to_the_output_path() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("openapi.json");

    run(Cli {
        command: Commands::DumpOpenapi(DumpArgs {
            output: Some(output.clone()),
        }),
    })
    .await
    .unwrap();

    let text = fs::read_to_string(&output).unwrap();
    assert!(text.contains("\"openapi\""), "{text}");
}

#[test]
fn import_stats_print_the_contacts_lines_unless_they_were_skipped() {
    let stats = crate::import::ImportStats {
        conversations: 2,
        messages: 5,
        contacts: 1,
        contact_handles: 3,
        mode: ImportMode::Replace,
        ..Default::default()
    };

    assert_eq!(
        format_import_stats(&stats),
        "  contacts:      1\n\
         \x20 contact handles:3\n\
         \x20 files:         0\n\
         \x20 conversations: 2\n\
         \x20 participants:  0\n\
         \x20 messages:      5\n\
         \x20 messages deduped: 0\n\
         \x20 attachment records: 0 (message↔media links in the database)\n\
         \x20 tapbacks:      0\n\
         \x20 media files stored:  0 (unique blobs under assets/)\n\
         \x20 media files reused:  0 (same content hash already on disk)\n\
         \x20 media files missing: 0 (attachment path not found on disk)\n"
    );

    let skipped = crate::import::ImportStats {
        contacts_skipped: true,
        mode: ImportMode::Append,
        messages_appended: 4,
        phones_needing_review: 1,
        ..Default::default()
    };
    let text = format_import_stats(&skipped);
    assert!(text.starts_with("  contacts:      (skipped"), "{text}");
    assert!(text.contains("  messages appended: 4\n"), "{text}");
    assert!(
        text.ends_with("  phones needing review: 1 (ambiguous numbers — fix them in the vault)\n"),
        "{text}"
    );
}

#[test]
fn dedupe_stats_print_one_line_per_count() {
    let stats = DedupeStats {
        keys_filled: 10,
        exact_groups: 2,
        exact_flagged: 3,
        near_flagged: 1,
    };

    assert_eq!(
        format_dedupe_stats(&stats),
        "  fingerprints set:   10 (one per message; not a duplicate count)\n\
         \x20 exact duplicate groups: 2\n\
         \x20 exact duplicates hidden: 3\n\
         \x20 near duplicates flagged: 1\n"
    );
}
