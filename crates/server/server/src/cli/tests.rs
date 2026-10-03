//! The command line, driven the way `main` drives it: a parsed [`Cli`] in,
//! effects on the database out. Most tests write a config file into a temp dir
//! and assert on the database afterwards. A few check a file a command writes
//! or the text it prints.

use std::fs;
use std::path::{Path, PathBuf};

use super::*;
use crate::db::account_profile;

const ALICE: i64 = 7;

/// A one-conversation JSON Lines export with no messages, enough for the
/// import to record a conversation under source `imessage`.
const CONVERSATION_JSONL: &str = r#"{"schema_version":4,"export":{"source":"imessage","tool":"t","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+15551230000","conversation_type":"individual","group_title":null,"participants":[],"stats":{"message_count":0,"attachment_count":0,"first_timestamp_unix_ms":null,"last_timestamp_unix_ms":null}}}
"#;

/// A database under `dir`: its config file on disk, the way an operator has
/// one, with absolute paths so the test does not depend on the working
/// directory.
fn server_config(dir: &Path) -> PathBuf {
    let config_dir = dir.join("config");
    fs::create_dir_all(&config_dir).unwrap();
    let text = format!(
        "[paths]\ndb = \"{}\"\ndata_dir = \"{}\"\n",
        dir.join("messagecrate.db").display(),
        dir.join("data").display()
    );
    let path = config_dir.join("config.toml");
    fs::write(&path, text).unwrap();
    path
}

/// The database the config names, made when it is not there yet.
async fn open(config: &Path) -> OpenDb {
    OpenDb::create_or_open(Config::load(config).unwrap())
        .await
        .unwrap()
}

/// An ordinary account named alice, so `--account alice` resolves.
async fn with_alice(config: &Path) {
    let opened = open(config).await;
    let mut conn = opened.conn().await.unwrap();
    account_profile::insert_account_at(&mut conn, ALICE, "alice", None, None)
        .await
        .unwrap();
}

async fn count(config: &Path, sql: &str) -> i64 {
    let opened = open(config).await;
    let mut conn = opened.conn().await.unwrap();
    sqlx::query_scalar(sql).fetch_one(&mut *conn).await.unwrap()
}

fn import_args(config: &Path, input: &Path) -> ImportArgs {
    ImportArgs {
        source: None,
        config: config.to_path_buf(),
        input: input.to_path_buf(),
        db: None,
        assets_dir: None,
        media: "copy".into(),
        mode: ImportMode::Replace,
        skip_dedupe: false,
        window_secs: 2,
        account: "alice".into(),
    }
}

#[tokio::test]
async fn create_owner_claims_the_server_once_and_reset_password_needs_the_claim() {
    let dir = tempfile::tempdir().unwrap();
    let config = server_config(dir.path());
    open(&config).await.close().await;

    let unclaimed = run(Cli {
        command: Commands::ResetOwnerPassword(ResetOwnerPasswordArgs {
            password: "correct horse battery staple".into(),
            config: config.clone(),
        }),
    })
    .await
    .unwrap_err();
    assert_eq!(
        unclaimed.to_string(),
        "this Message Crate has no owner yet; use `create-owner` to claim it"
    );

    run(Cli {
        command: Commands::CreateOwner(CreateOwnerArgs {
            username: "Owner".into(),
            password: "correct horse battery staple".into(),
            config: config.clone(),
        }),
    })
    .await
    .unwrap();

    let twice = run(Cli {
        command: Commands::CreateOwner(CreateOwnerArgs {
            username: "again".into(),
            password: "correct horse battery staple".into(),
            config: config.clone(),
        }),
    })
    .await
    .unwrap_err();
    assert_eq!(
        twice.to_string(),
        "this Message Crate already has an owner; use `reset-owner-password` to set a new password for it"
    );

    run(Cli {
        command: Commands::ResetOwnerPassword(ResetOwnerPasswordArgs {
            password: "a different long enough password".into(),
            config: config.clone(),
        }),
    })
    .await
    .unwrap();

    let opened = open(&config).await;
    let mut conn = opened.conn().await.unwrap();
    assert!(account_profile::is_claimed(&mut conn).await.unwrap());
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
    let config = server_config(dir.path());
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
    assert_eq!(count(&config, "SELECT COUNT(*) FROM imports").await, 1);

    run(Cli {
        command: Commands::DedupeCrossSource(DedupeArgs {
            config: config.clone(),
            db: None,
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
async fn process_assets_fails_when_a_conversion_failed_and_names_the_count() {
    use crate::process_assets::tests::{ACCOUNT, PNG_1X1_RGB, attach_stored_blob, seed_message};

    let dir = tempfile::tempdir().unwrap();
    let config = server_config(dir.path());
    with_alice(&config).await;
    // The process_assets fixtures seed rows for ACCOUNT, which must be alice.
    assert_eq!(ACCOUNT, ALICE);
    // A PNG whose original is gone: the conversion fails whether or not
    // ffmpeg is installed.
    {
        let opened = open(&config).await;
        let mut conn = opened.conn().await.unwrap();
        let message_id = seed_message(&mut conn, "imessage").await;
        let sha = "c".repeat(64);
        attach_stored_blob(
            &opened,
            &mut conn,
            "imessage",
            message_id,
            &sha,
            ".png",
            PNG_1X1_RGB,
        )
        .await;
        let original = opened
            .cfg
            .paths
            .assets_dir_for_account(ALICE, "imessage")
            .join(format!("cc/{sha}.png"));
        fs::remove_file(original).unwrap();
    }

    let err = run(Cli {
        command: Commands::ProcessAssets(ProcessAssetsArgs {
            config: config.clone(),
            force: false,
            dry_run: false,
            skip_image: false,
            skip_video: false,
            skip_audio: false,
            db: None,
            source: None,
        }),
    })
    .await
    .unwrap_err();

    assert_eq!(
        err.to_string(),
        "1 conversion(s) failed; those originals stay without a browser preview"
    );
}

/// S7-12: `process-assets --db` with a path where no database is exits with
/// an error naming it and creates no file, instead of processing an empty
/// new database and exiting 0.
#[tokio::test]
async fn process_assets_with_a_mistyped_db_fails_and_creates_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let config = server_config(dir.path());
    with_alice(&config).await;

    let err = run(Cli {
        command: Commands::ProcessAssets(ProcessAssetsArgs {
            config: config.clone(),
            force: false,
            dry_run: false,
            skip_image: false,
            skip_video: false,
            skip_audio: false,
            db: Some(PathBuf::from("data/messagecrate.db")),
            source: None,
        }),
    })
    .await
    .unwrap_err();

    let mistyped = dir.path().join("data/messagecrate.db");
    assert!(
        err.to_string().contains(&mistyped.display().to_string()),
        "{err}"
    );
    assert!(!mistyped.exists());
}

fn imports_discard_args(config: &Path) -> Cli {
    Cli {
        command: Commands::Imports(ImportsArgs {
            command: ImportsCommand::Discard(ImportsDiscardArgs {
                config: config.to_path_buf(),
                db: None,
                account: "alice".into(),
            }),
        }),
    }
}

#[tokio::test]
async fn imports_discard_clears_a_stranded_session_so_the_next_import_runs() {
    let dir = tempfile::tempdir().unwrap();
    let config = server_config(dir.path());
    with_alice(&config).await;
    let input = dir.path().join("export");
    fs::create_dir_all(&input).unwrap();
    fs::write(input.join("chat.jsonl"), CONVERSATION_JSONL).unwrap();

    // A session the way a killed `import` leaves it: running, never finished.
    let stranded = {
        let opened = open(&config).await;
        let mut conn = opened.conn().await.unwrap();
        crate::db::imports::start_import(
            &mut conn,
            &crate::db::imports::StartImportArgs::new(
                ALICE,
                "imessage",
                "replace",
                Some("message-crate-server"),
            ),
        )
        .await
        .unwrap()
    };

    let blocked = run(Cli {
        command: Commands::Import(import_args(&config, &input)),
    })
    .await
    .unwrap_err();
    assert!(
        blocked
            .to_string()
            .contains("already has a running Import Run"),
        "{blocked}"
    );
    assert!(
        blocked
            .to_string()
            .contains("message-crate-server imports discard"),
        "the error names the way out: {blocked}"
    );

    run(imports_discard_args(&config)).await.unwrap();

    let status: String = {
        let opened = open(&config).await;
        let mut conn = opened.conn().await.unwrap();
        sqlx::query_scalar("SELECT status FROM imports WHERE id = $1")
            .bind(stranded)
            .fetch_one(&mut *conn)
            .await
            .unwrap()
    };
    assert_eq!(status, "cancelled");

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
        count(
            &config,
            "SELECT COUNT(*) FROM imports WHERE status = 'running'"
        )
        .await,
        0
    );
}

#[tokio::test]
async fn imports_discard_with_no_session_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let config = server_config(dir.path());
    with_alice(&config).await;

    run(imports_discard_args(&config)).await.unwrap();

    assert_eq!(count(&config, "SELECT COUNT(*) FROM imports").await, 0);
}

#[test]
fn imports_discard_prints_the_session_or_that_there_was_none() {
    let row = crate::db::imports::ImportRow {
        id: 12,
        account_id: ALICE,
        source: "imessage".into(),
        tool: Some("message-crate-server".into()),
        mode: "replace".into(),
        dedupe: false,
        status: crate::db::imports::ImportStatus::Running,
        started_at: "2026-09-21T10:00:00+00:00".into(),
        finished_at: None,
        message_count: 0,
        attachment_count: 0,
        bytes_uploaded: 0,
        duration_ms: None,
        parse_ms: None,
        attachments_ms: None,
        prepare_ms: None,
        upload_ms: None,
        summary_json: None,
        stage: Some("parse".into()),
        staging_dir: None,
        device_id: None,
        form_json: None,
        source_fingerprint: None,
        source_identities: None,
    };
    assert_eq!(
        format_discarded_import("alice", Some(&row)),
        "Discarded import session 12 for account alice (source imessage, replace mode, started 2026-09-21T10:00:00+00:00, stage parse).\n"
    );
    assert_eq!(
        format_discarded_import("alice", None),
        "Account alice has no active import session.\n"
    );
}

#[tokio::test]
async fn import_refuses_a_negative_window_before_opening_anything() {
    let dir = tempfile::tempdir().unwrap();
    let config = server_config(dir.path());
    let mut args = import_args(&config, dir.path());
    args.window_secs = -1;

    let err = run(Cli {
        command: Commands::Import(args),
    })
    .await
    .unwrap_err();

    assert_eq!(err.to_string(), "--window-secs must be >= 0");
    assert!(!dir.path().join("messagecrate.db").exists());
}

/// Zero is exact-time matching only, and the refusal says ">= 0".
#[test]
fn a_window_of_zero_seconds_is_accepted_and_a_negative_one_refused() {
    assert!(validate_window_secs(0).is_ok());
    assert!(validate_window_secs(-1).is_err());
}

#[tokio::test]
async fn import_refuses_an_unknown_media_mode() {
    let dir = tempfile::tempdir().unwrap();
    let config = server_config(dir.path());
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
    let config = server_config(dir.path());
    open(&config).await.close().await;
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
fn import_stats_print_one_line_for_each_count() {
    let stats = crate::imports_api::ImportStats {
        conversations: 2,
        messages: 5,
        mode: ImportMode::Replace,
        ..Default::default()
    };

    assert_eq!(
        format_import_stats(&stats),
        "  files:         0\n\
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

    let appended = crate::imports_api::ImportStats {
        mode: ImportMode::Append,
        messages_appended: 4,
        phones_needing_review: 1,
        ..Default::default()
    };
    let text = format_import_stats(&appended);
    assert!(text.contains("  messages appended: 4\n"), "{text}");
    assert!(
        text.ends_with(
            "  phones needing review: 1 (ambiguous numbers — fix them in Message Crate)\n"
        ),
        "{text}"
    );

    let incomplete = crate::imports_api::ImportStats {
        other_identities: 2,
        ..Default::default()
    };
    let text = format_import_stats(&incomplete);
    assert!(
        text.ends_with(
            "  identities with no address: 2 (a name the backup gave in place of an address; the export is incomplete)\n"
        ),
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

/// Parse a `serve` command line the way `main` does.
fn serve_args(args: &[&str]) -> ServeArgs {
    let argv = ["message-crate-server", "serve"].iter().chain(args);
    match Cli::try_parse_from(argv)
        .expect("serve arguments parse")
        .command
    {
        Commands::Serve(args) => args,
        other => panic!("expected serve, parsed {other:?}"),
    }
}

/// How the desktop app starts the server (#970): one folder holds the whole
/// Message Crate, no config file is read, and the other settings are the
/// defaults unless a flag says otherwise.
#[test]
fn serve_with_a_data_dir_needs_no_config_file() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("crate");

    let cfg = serve_config(serve_args(&["--data-dir", data.to_str().unwrap()])).unwrap();

    assert_eq!(cfg.paths.db, data.join("messagecrate.db"));
    assert_eq!(cfg.paths.data_dir, data);
    let server = cfg.require_server().expect("a [server] section by default");
    assert_eq!(server.bind, "127.0.0.1:8080");
    assert_eq!(server.static_dir, PathBuf::from("static"));
    assert!(server.cors_origins.is_empty());

    let cfg = serve_config(serve_args(&[
        "--data-dir",
        data.to_str().unwrap(),
        "--bind",
        "0.0.0.0:9000",
        "--static-dir",
        "/opt/site",
        "--cors-origin",
        "http://localhost:5173",
        "--cors-origin",
        "http://127.0.0.1:5173",
    ]))
    .unwrap();
    let server = cfg.require_server().unwrap();
    assert_eq!(server.bind, "0.0.0.0:9000");
    assert_eq!(server.static_dir, PathBuf::from("/opt/site"));
    assert_eq!(
        server.cors_origins,
        ["http://localhost:5173", "http://127.0.0.1:5173"]
    );
}

/// The same two flags apply over a config file, a relative `--static-dir`
/// resolves where the config file's own `static_dir` does, and a relative data folder
/// is made absolute so the server does not depend on where it was started.
#[tokio::test]
async fn serve_flags_override_the_config_file_and_a_relative_data_dir_is_made_absolute() {
    let temp = tempfile::tempdir().unwrap();
    let config = server_config(temp.path());
    fs::write(
        &config,
        format!(
            "{}\n[server]\nbind = \"127.0.0.1:8080\"\nstatic_dir = \"site\"\n",
            fs::read_to_string(&config).unwrap()
        ),
    )
    .unwrap();

    let from_file = serve_config(serve_args(&["--config", config.to_str().unwrap()])).unwrap();
    assert_eq!(
        from_file.require_server().unwrap().static_dir,
        temp.path().join("site")
    );
    let overridden = serve_config(serve_args(&[
        "--config",
        config.to_str().unwrap(),
        "--bind",
        "127.0.0.1:9100",
        "--static-dir",
        "elsewhere",
    ]))
    .unwrap();
    let server = overridden.require_server().unwrap();
    assert_eq!(server.bind, "127.0.0.1:9100");
    assert_eq!(server.static_dir, temp.path().join("elsewhere"));

    let relative = serve_config(serve_args(&["--data-dir", "some/crate"])).unwrap();
    assert!(relative.paths.data_dir.is_absolute());
    assert!(relative.paths.data_dir.ends_with("some/crate"));
}

/// A config file and a data folder are two answers to one question.
#[test]
fn serve_refuses_a_config_file_together_with_a_data_dir() {
    let argv = [
        "message-crate-server",
        "serve",
        "--config",
        "c.toml",
        "--data-dir",
        "d",
    ];
    assert!(Cli::try_parse_from(argv).is_err());
}
