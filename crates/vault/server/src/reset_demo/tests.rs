use super::*;
use crate::config::PathsConfig;
use sqlx::AnyConnection;

fn url_config_for_refuse_tests() -> Config {
    Config {
        paths: PathsConfig {
            db: PathBuf::from("data/vault.db"),
            data_dir: PathBuf::from("data"),
            assets_dir: "assets".into(),
            assets_converted_dir: "assets_converted".into(),
        },
        server: None,
        database: crate::config::DatabaseConfig {
            url: Some("postgres://vault:vault@127.0.0.1:5432/vault".into()),
        },
    }
}

#[test]
fn refuse_url_config_errors_when_config_has_url() {
    let err = refuse_url_config(&url_config_for_refuse_tests())
        .expect_err("config URL without --db-url must fail");
    assert!(
        err.to_string()
            .contains("URL-served databases cannot be reset"),
        "{err}"
    );
}

fn write_tiny_reset_bundle(root: &Path) {
    fs::create_dir_all(root.join("config")).expect("create bundle config");
    fs::create_dir_all(root.join("staging").join(IMESSAGE_SOURCE)).expect("imessage dir");
    fs::create_dir_all(root.join("staging").join(SBR_SOURCE)).expect("sbr dir");
    fs::create_dir_all(root.join("staging").join(WHATSAPP_SOURCE)).expect("whatsapp dir");
    fs::write(
        root.join("config/config.toml"),
        "[paths]\ndb = \"data/vault.db\"\ndata_dir = \"data\"\n",
    )
    .expect("write bundle config");
    fs::write(
        root.join("config/seed.toml"),
        r#"
[owner]
display_name = "Demo User"
handle_specs = [["+14155559000", "phone"]]
emails = ["demo.ingest@example.com"]

[account]
username = "demo"
"#,
    )
    .expect("write seed.toml");
    fs::write(
        root.join("config/contacts.vcf"),
        "BEGIN:VCARD\nVERSION:3.0\nFN:Test\nTEL:+15555550100\nEND:VCARD\n",
    )
    .expect("write contacts");
    let conversation = |source: &str, chat: &str, guid: &str| {
        format!(
            r#"{{"schema_version":4,"export":{{"source":"{source}","tool":"t","tool_version":"0","owner_handle":null,"owner_display_name":null}},"conversation":{{"chat_identifier":"{chat}","conversation_type":"individual","group_title":null,"participants":[{{"handle":"{chat}","display_name":null}}],"stats":{{"message_count":1,"attachment_count":0,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}}}}
{{"guid":"{guid}","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"sms","message_kind":"sms","sender_handle":"{chat}","sender_display_name":null,"subject":null,"text":"hello","attachments":[],"imessage":null,"source":null}}
"#
        )
    };
    fs::write(
        root.join("staging").join(IMESSAGE_SOURCE).join("a.jsonl"),
        conversation(IMESSAGE_SOURCE, "+15555550101", "pg-demo-imessage"),
    )
    .expect("write imessage jsonl");
    fs::write(
        root.join("staging").join(SBR_SOURCE).join("a.jsonl"),
        conversation(SBR_SOURCE, "+15555550102", "pg-demo-sbr"),
    )
    .expect("write sbr jsonl");
    fs::write(
        root.join("staging").join(WHATSAPP_SOURCE).join("a.jsonl"),
        conversation(WHATSAPP_SOURCE, "+15555550103", "pg-demo-wa"),
    )
    .expect("write whatsapp jsonl");
}

#[tokio::test]
async fn reset_demo_db_url_creates_demo_account_on_postgres() {
    let Some(url) = crate::pg_test_url() else {
        return;
    };
    // A schema of this test's own. `reset_prepared_bundle_at_url` takes a
    // URL rather than a pool, so the schema rides in the URL's search_path
    // and everything the reset writes lands there (#435).
    let url = crate::db::engine::pg_test_schema_url(&url).await;

    let temp = tempfile::tempdir().expect("temp dir");
    let bundle = temp.path().join("bundle");
    write_tiny_reset_bundle(&bundle);
    let data_dir = temp.path().join("data");
    fs::create_dir_all(&data_dir).expect("data dir");
    let unused_db = temp.path().join("unused.db");
    let config_dest = temp.path().join("config.toml");
    fs::write(
        &config_dest,
        format!(
            "[paths]\ndb = \"{}\"\ndata_dir = \"{}\"\n",
            unused_db.display(),
            data_dir.display()
        ),
    )
    .expect("write host config");

    let pool = engine::open_pool_from_url(&url)
        .await
        .expect("open postgres");
    let mut conn = pool.acquire().await.expect("acquire");
    schema::ensure_vault_schema(&mut conn)
        .await
        .expect("schema");
    conn.close().await.expect("close schema conn");
    pool.close().await;

    let host_config_before = fs::read(&config_dest).expect("read host config");
    let cfg = Config::load(&config_dest).expect("load host config");
    reset_prepared_bundle_at_url(&cfg, &bundle, DEMO_ACCOUNT_ID, &url)
        .await
        .expect("reset at url");
    assert!(
        !unused_db.exists(),
        "reset-demo --db-url must not create or replace paths.db"
    );
    assert_eq!(
        fs::read(&config_dest).expect("reread host config"),
        host_config_before,
        "reset-demo --db-url must leave the host config file unchanged"
    );

    let pool = engine::open_pool_from_url(&url)
        .await
        .expect("reopen postgres");
    let mut conn = pool.acquire().await.expect("acquire");
    let username: Option<String> =
        sqlx::query_scalar("SELECT username FROM accounts WHERE id = $1")
            .bind(DEMO_ACCOUNT_ID)
            .fetch_optional(&mut *conn)
            .await
            .expect("username");
    assert_eq!(username.as_deref(), Some("demo"));
    let hash: Option<String> =
        sqlx::query_scalar("SELECT password_hash FROM accounts WHERE id = $1")
            .bind(DEMO_ACCOUNT_ID)
            .fetch_one(&mut *conn)
            .await
            .expect("password hash");
    assert!(hash.is_none(), "demo account must have no password hash");
    let conversations: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM conversations WHERE account_id = $1")
            .bind(DEMO_ACCOUNT_ID)
            .fetch_one(&mut *conn)
            .await
            .expect("conversations");
    assert!(conversations >= 1, "expected imported conversations");
    conn.close().await.expect("close");
    pool.close().await;
}

/// Open `db` with the vault schema applied and one connection checked out.
async fn test_db_conn(db: &Path) -> sqlx::pool::PoolConnection<sqlx::Any> {
    let (_pool, conn) = test_db(db).await;
    conn
}

/// Open `db` with the vault schema applied; returns the pool alongside the
/// connection so the caller can close the pool deterministically before
/// copying or replacing the database file.
async fn test_db(db: &Path) -> (sqlx::AnyPool, sqlx::pool::PoolConnection<sqlx::Any>) {
    let pool = engine::open_pool_for_path(db)
        .await
        .expect("open test database");
    let mut conn = pool.acquire().await.expect("acquire test connection");
    schema::ensure_vault_schema(&mut conn)
        .await
        .expect("create vault schema");
    (pool, conn)
}

/// Close the pool so no connection stays attached to the database file.
async fn close_test_db(pool: sqlx::AnyPool, conn: sqlx::pool::PoolConnection<sqlx::Any>) {
    // Await the real close: `pool.close()` alone only waits for the
    // connection to be returned, and the sqlx worker thread closes it
    // later — racing the checkpoint/copy that follows can SIGBUS.
    conn.close().await.expect("close test connection");
    pool.close().await;
}

/// The committed demo bundle ships a `seed.toml`; it must parse with the
/// current `DemoOwner` (handle_specs) format or `reset-demo` fails on
/// release images that skip bundle regeneration.
#[test]
fn committed_demo_seed_toml_parses() {
    let text = include_str!("../../../demo-seed/config/seed.toml");
    let seed: DemoSeed = toml::from_str(text).expect("committed demo seed.toml must parse");
    assert_eq!(seed.owner.display_name, "Demo User");
    assert_eq!(seed.owner.handle_specs.len(), 1);
    let (raw, handle_type) = &seed.owner.handle_specs[0];
    assert_eq!(raw, "+14155559000");
    assert_eq!(*handle_type, HandleType::Phone);
    assert_eq!(seed.owner.emails, vec!["demo.ingest@example.com"]);
    assert_eq!(seed.account.username, "demo");
}

#[test]
fn a_complete_bundle_is_used_as_it_is_when_there_is_no_seed_file() {
    let temp = tempfile::tempdir().expect("create test directory");
    let bundle = temp.path().join("bundle");
    write_tiny_reset_bundle(&bundle);
    let seed_toml = temp.path().join("demo_seed.toml");

    let stats = maybe_regenerate_bundle(&bundle, &seed_toml).expect("the image bundle is complete");

    assert_eq!(stats, demo_seed::GenStats::default());
    assert!(
        bundle.join("staging/imessage/a.jsonl").is_file(),
        "the bundle's own conversations stay in place"
    );
}

#[test]
fn an_incomplete_bundle_without_a_seed_file_cannot_be_reset() {
    let temp = tempfile::tempdir().expect("create test directory");
    let bundle = temp.path().join("bundle");
    fs::create_dir_all(bundle.join("staging").join(IMESSAGE_SOURCE)).expect("imessage dir");
    let seed_toml = temp.path().join("demo_seed.toml");

    let error = maybe_regenerate_bundle(&bundle, &seed_toml)
        .expect_err("no seed file and no complete bundle");

    assert!(
        error.to_string().contains("is not a complete demo bundle"),
        "{error:#}"
    );
}

#[tokio::test]
async fn the_demo_account_may_import_export_and_delete() {
    let temp = tempfile::tempdir().expect("create test directory");
    let db = temp.path().join("vault.db");
    let (pool, mut conn) = test_db(&db).await;
    let seed = DemoSeed {
        owner: DemoOwner {
            display_name: "Demo User".into(),
            handle_specs: Vec::new(),
            emails: Vec::new(),
        },
        account: DemoAccount {
            username: "demo".into(),
        },
    };

    seed_demo_account_on_conn(&mut conn, DEMO_ACCOUNT_ID, &seed)
        .await
        .expect("seed the demo account");

    let (import, export, delete): (i64, i64, i64) =
        sqlx::query_as("SELECT can_import, can_export, can_delete FROM accounts WHERE id = $1")
            .bind(DEMO_ACCOUNT_ID)
            .fetch_one(&mut *conn)
            .await
            .expect("read the demo account");
    assert_eq!(
        (import, export, delete),
        (1, 1, 1),
        "the demo account is there to try the whole vault, so it may import, export, and delete"
    );

    close_test_db(pool, conn).await;
}

#[tokio::test]
async fn failed_reset_preserves_existing_demo_account() {
    let temp = tempfile::tempdir().expect("create test directory");
    let db = temp.path().join("vault.db");
    let data_dir = temp.path().join("data");
    let account_root = data_dir.join(DEMO_ACCOUNT_ID.to_string());
    fs::create_dir_all(&account_root).expect("create account data directory");
    let sentinel = account_root.join("existing.bin");
    let original_data = b"existing account data\n";
    fs::write(&sentinel, original_data).expect("write account data sentinel");

    {
        let (pool, mut conn) = test_db(&db).await;
        account_profile::ensure_account_row(&mut conn, DEMO_ACCOUNT_ID)
            .await
            .expect("seed account");
        let handle_id: i64 = sqlx::query_scalar(
            "INSERT INTO handles (
                account_id, raw, normalized, handle_type, service
             ) VALUES ($1, '+15555550100', '+15555550100', 'phone', 'phone')
             RETURNING id",
        )
        .bind(DEMO_ACCOUNT_ID)
        .fetch_one(&mut *conn)
        .await
        .expect("insert handle");
        let conversation_id: i64 = sqlx::query_scalar(
            "INSERT INTO conversations (
                account_id, chat_handle_id, conversation_type, source_file
             ) VALUES ($1, $2, 'individual', 'existing.jsonl')
             RETURNING id",
        )
        .bind(DEMO_ACCOUNT_ID)
        .bind(handle_id)
        .fetch_one(&mut *conn)
        .await
        .expect("insert conversation");
        sqlx::query(
            "INSERT INTO messages (
                conversation_id, account_id, source, guid, timestamp,
                is_from_me, body, sort_order
             ) VALUES ($1, $2, 'imessage', 'existing-message',
                       '2026-01-01T00:00:00Z', 0, 'keep me', 0)",
        )
        .bind(conversation_id)
        .bind(DEMO_ACCOUNT_ID)
        .execute(&mut *conn)
        .await
        .expect("insert message");
        close_test_db(pool, conn).await;
    }

    let cfg = Config {
        paths: PathsConfig {
            db: db.clone(),
            data_dir,
            assets_dir: "assets".into(),
            assets_converted_dir: "assets_converted".into(),
        },
        server: None,
        database: crate::config::DatabaseConfig::default(),
    };
    let invalid_bundle = temp.path().join("invalid-bundle");
    fs::create_dir_all(invalid_bundle.join("staging").join(IMESSAGE_SOURCE))
        .expect("create iMessage tree");
    fs::create_dir_all(invalid_bundle.join("staging").join(SBR_SOURCE))
        .expect("create Android tree");

    let result = reset_prepared_bundle(
        &cfg,
        &invalid_bundle,
        DEMO_ACCOUNT_ID,
        &temp.path().join("config/config.toml"),
        &temp.path().join("prepared-config.toml"),
    )
    .await;

    assert!(result.is_err());
    let mut conn = test_db_conn(&db).await;
    let account_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts WHERE id = $1")
        .bind(DEMO_ACCOUNT_ID)
        .fetch_one(&mut *conn)
        .await
        .expect("count account");
    let message_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE guid = 'existing-message'")
            .fetch_one(&mut *conn)
            .await
            .expect("count message");
    assert_eq!(account_count, 1);
    assert_eq!(message_count, 1);
    assert_eq!(
        fs::read(&sentinel).expect("read account sentinel"),
        original_data
    );
}

#[tokio::test]
async fn failed_preparation_preserves_active_config() {
    let temp = tempfile::tempdir().expect("create test directory");
    let config_dest = temp.path().join("config/config.toml");
    fs::create_dir_all(config_dest.parent().expect("config parent")).expect("create config parent");
    let original = b"active configuration\n";
    fs::write(&config_dest, original).expect("write active config");
    let invalid_bundle = temp.path().join("invalid-bundle");
    fs::create_dir_all(&invalid_bundle).expect("create invalid bundle");

    let result =
        prepare_config_and_reset(&invalid_bundle, &config_dest, DEMO_ACCOUNT_ID, None).await;

    assert!(result.is_err());
    assert_eq!(
        fs::read(&config_dest).expect("read active config"),
        original
    );
}

#[tokio::test]
async fn vault_db_without_accounts_table_does_not_block_reset_check() {
    let temp = tempfile::tempdir().expect("create test directory");
    let active = temp.path().join("vault.db");
    fs::write(&active, []).expect("create empty sqlite file");
    let prepared = temp.path().join("prepared.db");
    drop(test_db_conn(&prepared).await);

    verify_non_demo_state_preserved(&active, &prepared, DEMO_ACCOUNT_ID)
        .await
        .expect("a vault.db with no accounts table must not block reset-demo");
}

/// Copy a seeded active database (demo account plus account 9, one message
/// each) to a prepared one beside it, and return both paths.
async fn active_and_prepared_reset_databases(root: &Path) -> (PathBuf, PathBuf) {
    let active = root.join("vault.db");
    seed_reset_test_database(&active).await;
    let prepared = root.join("prepared.db");
    fs::copy(&active, &prepared).expect("copy prepared database");
    (active, prepared)
}

#[tokio::test]
async fn reset_check_accepts_a_prepared_database_that_changed_only_the_demo_account() {
    let temp = tempfile::tempdir().expect("create test directory");
    let (active, prepared) = active_and_prepared_reset_databases(temp.path()).await;
    make_prepared_reset_database_observably_different(&prepared).await;

    verify_non_demo_state_preserved(&active, &prepared, DEMO_ACCOUNT_ID)
        .await
        .expect("a reset that only changed the demo account must be accepted");
}

#[tokio::test]
async fn reset_check_refuses_a_prepared_database_with_fewer_non_demo_messages() {
    let temp = tempfile::tempdir().expect("create test directory");
    let (active, prepared) = active_and_prepared_reset_databases(temp.path()).await;
    let (pool, mut conn) = test_db(&prepared).await;
    sqlx::query("DELETE FROM messages WHERE account_id = 9")
        .execute(&mut *conn)
        .await
        .expect("delete account 9's message");
    close_test_db(pool, conn).await;

    let error = verify_non_demo_state_preserved(&active, &prepared, DEMO_ACCOUNT_ID)
        .await
        .expect_err("a reset that lost a non-demo account's message must be refused")
        .to_string();

    assert!(
        error.contains("active={9: 1}, prepared={9: 0}"),
        "the error must name account 9 and both counts: {error}"
    );
}

#[tokio::test]
async fn reset_check_refuses_a_prepared_database_with_more_non_demo_messages() {
    let temp = tempfile::tempdir().expect("create test directory");
    let (active, prepared) = active_and_prepared_reset_databases(temp.path()).await;
    let (pool, mut conn) = test_db(&prepared).await;
    seed_reset_test_account(&mut conn, 10, "new-non-demo").await;
    close_test_db(pool, conn).await;

    let error = verify_non_demo_state_preserved(&active, &prepared, DEMO_ACCOUNT_ID)
        .await
        .expect_err("a reset that added a non-demo account's message must be refused")
        .to_string();

    assert!(
        error.contains("active={9: 1}, prepared={9: 1, 10: 1}"),
        "the error must name account 10: {error}"
    );
}

#[test]
fn reset_refuses_while_server_holds_database_lock() {
    let temp = tempfile::tempdir().expect("create test directory");
    let db = temp.path().join("vault.db");
    let _serve_lock = crate::operation_lock::acquire_for_serve(&db).expect("acquire server lock");

    let error = crate::operation_lock::acquire_for_reset(&db)
        .expect_err("reset lock must conflict with active server")
        .to_string();

    assert!(error.contains("serve is active"), "{error}");
    assert!(error.contains("offline"), "{error}");
}

#[tokio::test]
async fn failures_after_database_and_account_install_restore_all_active_state() {
    for failure_point in [
        ResetInstallFailure::AfterDatabase,
        ResetInstallFailure::AfterAccount,
    ] {
        let temp = tempfile::tempdir().expect("create test directory");
        let active_db = temp.path().join("active/vault.db");
        fs::create_dir_all(active_db.parent().expect("database parent"))
            .expect("create database parent");
        seed_reset_test_database(&active_db).await;
        let prepared_db = temp.path().join("prepared/vault.db");
        fs::create_dir_all(prepared_db.parent().expect("prepared database parent"))
            .expect("create prepared database parent");
        fs::copy(&active_db, &prepared_db).expect("copy prepared database");
        make_prepared_reset_database_observably_different(&prepared_db).await;

        let active_account = temp.path().join("data").join(DEMO_ACCOUNT_ID.to_string());
        let prepared_account = temp
            .path()
            .join("prepared-data")
            .join(DEMO_ACCOUNT_ID.to_string());
        fs::create_dir_all(&active_account).expect("create active account");
        fs::create_dir_all(&prepared_account).expect("create prepared account");
        fs::write(active_account.join("sentinel"), b"old data").expect("write old data");
        fs::write(prepared_account.join("sentinel"), b"new data").expect("write new data");

        let active_config = temp.path().join("config/config.toml");
        let prepared_config = temp.path().join("prepared-config/config.toml");
        fs::create_dir_all(active_config.parent().expect("active config parent"))
            .expect("create active config parent");
        fs::create_dir_all(prepared_config.parent().expect("prepared config parent"))
            .expect("create prepared config parent");
        fs::write(&active_config, b"old config").expect("write old config");
        fs::write(&prepared_config, b"new config").expect("write new config");

        let result = replace_reset_state_with(
            &ResetPaths {
                active_db: &active_db,
                prepared_db: &prepared_db,
                active_account: &active_account,
                prepared_account: &prepared_account,
                active_config: &active_config,
                prepared_config: &prepared_config,
            },
            |source, destination| {
                if failure_point == ResetInstallFailure::AfterDatabase && source == prepared_account
                {
                    bail!("injected failure after database rename");
                }
                if failure_point == ResetInstallFailure::AfterAccount && source == prepared_config {
                    bail!("injected failure after account-directory rename");
                }
                fs::rename(source, destination).map_err(Into::into)
            },
        );

        assert!(result.is_err());
        assert_reset_test_database(&active_db).await;
        assert_eq!(
            fs::read(active_account.join("sentinel")).expect("read data sentinel"),
            b"old data"
        );
        assert_eq!(
            fs::read(&active_config).expect("read active config"),
            b"old config"
        );
    }
}

#[tokio::test]
async fn active_sidecars_are_cleaned_immediately_before_database_rename() {
    let temp = tempfile::tempdir().expect("create test directory");
    let active_db = temp.path().join("active/vault.db");
    fs::create_dir_all(active_db.parent().expect("database parent"))
        .expect("create database parent");
    seed_reset_test_database(&active_db).await;
    let prepared_db = temp.path().join("prepared/vault.db");
    fs::create_dir_all(prepared_db.parent().expect("prepared database parent"))
        .expect("create prepared database parent");
    fs::copy(&active_db, &prepared_db).expect("copy prepared database");

    let active_account = temp.path().join("data").join(DEMO_ACCOUNT_ID.to_string());
    let prepared_account = temp
        .path()
        .join("prepared-data")
        .join(DEMO_ACCOUNT_ID.to_string());
    fs::create_dir_all(&active_account).expect("create active account");
    fs::create_dir_all(&prepared_account).expect("create prepared account");
    let active_config = temp.path().join("config/config.toml");
    let prepared_config = temp.path().join("prepared-config/config.toml");
    fs::create_dir_all(active_config.parent().expect("active config parent"))
        .expect("create active config parent");
    fs::create_dir_all(prepared_config.parent().expect("prepared config parent"))
        .expect("create prepared config parent");
    fs::write(&active_config, b"old config").expect("write active config");
    fs::write(&prepared_config, b"new config").expect("write prepared config");

    {
        let (pool, mut conn) = test_db(&active_db).await;
        sqlx::query("UPDATE accounts SET preferred_name = 'reopened' WHERE id = $1")
            .bind(DEMO_ACCOUNT_ID)
            .execute(&mut *conn)
            .await
            .expect("write through reopened active database");
        close_test_db(pool, conn).await;
    }
    let active_wal = sqlite_sidecar(&active_db, "-wal");
    let active_shm = sqlite_sidecar(&active_db, "-shm");
    fs::write(&active_wal, b"").expect("create empty WAL sidecar");
    fs::write(&active_shm, b"").expect("create empty shared-memory sidecar");
    let mut observed_clean_boundary = false;

    let result = install_reset_state_with(
        &ResetPaths {
            active_db: &active_db,
            prepared_db: &prepared_db,
            active_account: &active_account,
            prepared_account: &prepared_account,
            active_config: &active_config,
            prepared_config: &prepared_config,
        },
        |source, destination| {
            if source == active_db {
                observed_clean_boundary = !active_wal.exists() && !active_shm.exists();
            }
            if source == prepared_db {
                bail!("stop after observing active database rename boundary");
            }
            fs::rename(source, destination).map_err(Into::into)
        },
    )
    .await;

    assert!(result.is_err());
    assert!(
        observed_clean_boundary,
        "active WAL and shared-memory sidecars must be absent at rename"
    );
}

#[test]
fn reset_rollback_attempts_remaining_restorations_after_one_fails() {
    let temp = tempfile::tempdir().expect("create test directory");
    let active_db = temp.path().join("active/vault.db");
    let prepared_db = temp.path().join("prepared/vault.db");
    let active_account = temp.path().join("data/demo");
    let prepared_account = temp.path().join("prepared-data/demo");
    let active_config = temp.path().join("config/config.toml");
    let prepared_config = temp.path().join("prepared-config/config.toml");
    for parent in [
        active_db.parent().expect("active db parent"),
        prepared_db.parent().expect("prepared db parent"),
        &active_account,
        &prepared_account,
        active_config.parent().expect("active config parent"),
        prepared_config.parent().expect("prepared config parent"),
    ] {
        fs::create_dir_all(parent).expect("create replacement fixture directory");
    }
    fs::write(&active_db, b"old db").expect("write active db");
    fs::write(&prepared_db, b"new db").expect("write prepared db");
    fs::write(active_account.join("sentinel"), b"old").expect("write active account");
    fs::write(prepared_account.join("sentinel"), b"new").expect("write prepared account");
    fs::write(&active_config, b"old config").expect("write active config");
    fs::write(&prepared_config, b"new config").expect("write prepared config");
    let mut database_restore_attempted = false;

    let result = replace_reset_state_with(
        &ResetPaths {
            active_db: &active_db,
            prepared_db: &prepared_db,
            active_account: &active_account,
            prepared_account: &prepared_account,
            active_config: &active_config,
            prepared_config: &prepared_config,
        },
        |source, destination| {
            if source == prepared_config {
                bail!("injected config install failure");
            }
            if source.ends_with("previous-account") {
                bail!("injected account restore failure");
            }
            if source.ends_with("previous-vault.db") {
                database_restore_attempted = true;
            }
            fs::rename(source, destination).map_err(Into::into)
        },
    );

    let error = result.expect_err("replacement must fail").to_string();
    assert!(
        database_restore_attempted,
        "database restoration must be attempted after account restoration fails"
    );
    assert!(error.contains("injected account restore failure"));
    assert!(
        prepared_account
            .parent()
            .unwrap()
            .join("previous-account")
            .exists()
    );
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResetInstallFailure {
    AfterDatabase,
    AfterAccount,
}

async fn seed_reset_test_database(path: &Path) {
    let (pool, mut conn) = test_db(path).await;
    schema::ensure_vault_schema(&mut conn)
        .await
        .expect("create reset test schema");
    seed_reset_test_account(&mut conn, DEMO_ACCOUNT_ID, "demo-existing").await;
    seed_reset_test_account(&mut conn, 9, "non-demo-existing").await;
    close_test_db(pool, conn).await;
    // Pool close does not reliably checkpoint WAL sidecars, so an
    // fs::copy of this file would miss everything written to the -wal.
    // Checkpoint explicitly so copies see the seeded rows.
    checkpoint_and_clean_sidecars(path, "while seeding reset test database")
        .await
        .expect("checkpoint seeded reset test database");
}

async fn make_prepared_reset_database_observably_different(path: &Path) {
    let (pool, mut conn) = test_db(path).await;
    sqlx::query("UPDATE accounts SET username = 'prepared-demo' WHERE id = $1")
        .bind(DEMO_ACCOUNT_ID)
        .execute(&mut *conn)
        .await
        .expect("change prepared demo account");
    sqlx::query("DELETE FROM messages WHERE account_id = $1")
        .bind(DEMO_ACCOUNT_ID)
        .execute(&mut *conn)
        .await
        .expect("delete prepared demo message");
    sqlx::query("DELETE FROM accounts WHERE id = 'non-demo-account'")
        .execute(&mut *conn)
        .await
        .expect("delete prepared non-demo marker");
    close_test_db(pool, conn).await;
    checkpoint_and_clean_sidecars(path, "while preparing reset test database")
        .await
        .expect("checkpoint prepared reset test database");

    let (pool, mut conn) = test_db(path).await;
    let demo_username: String = sqlx::query_scalar("SELECT username FROM accounts WHERE id = $1")
        .bind(DEMO_ACCOUNT_ID)
        .fetch_one(&mut *conn)
        .await
        .expect("read changed prepared demo account");
    let demo_messages: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE account_id = $1")
            .bind(DEMO_ACCOUNT_ID)
            .fetch_one(&mut *conn)
            .await
            .expect("count prepared demo messages");
    let non_demo_accounts: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM accounts WHERE id = 'non-demo-account'")
            .fetch_one(&mut *conn)
            .await
            .expect("count prepared non-demo marker");
    assert_eq!(demo_username, "prepared-demo");
    assert_eq!(demo_messages, 0);
    assert_eq!(non_demo_accounts, 0);
    close_test_db(pool, conn).await;
}

async fn seed_reset_test_account(conn: &mut AnyConnection, account_id: i64, guid: &str) {
    account_profile::ensure_account_row(conn, account_id)
        .await
        .expect("seed reset test account");
    let handle_id: i64 = sqlx::query_scalar(
        "INSERT INTO handles (
            account_id, raw, normalized, handle_type, service
         ) VALUES ($1, $2, $2, 'username', 'phone')
         RETURNING id",
    )
    .bind(account_id)
    .bind(format!("{account_id}-handle"))
    .fetch_one(&mut *conn)
    .await
    .expect("insert reset test handle");
    let conversation_id: i64 = sqlx::query_scalar(
        "INSERT INTO conversations (
            account_id, chat_handle_id, conversation_type, source_file
         ) VALUES ($1, $2, 'individual', 'existing.jsonl')
         RETURNING id",
    )
    .bind(account_id)
    .bind(handle_id)
    .fetch_one(&mut *conn)
    .await
    .expect("insert reset test conversation");
    sqlx::query(
        "INSERT INTO messages (
            conversation_id, account_id, source, guid, timestamp,
            is_from_me, body, sort_order
         ) VALUES ($1, $2, 'imessage', $3,
                   '2026-01-01T00:00:00Z', 0, 'keep me', 0)",
    )
    .bind(conversation_id)
    .bind(account_id)
    .bind(guid)
    .execute(&mut *conn)
    .await
    .expect("insert reset test message");
}

async fn assert_reset_test_database(path: &Path) {
    let (pool, mut conn) = test_db(path).await;
    for (account_id, guid) in [(DEMO_ACCOUNT_ID, "demo-existing"), (9, "non-demo-existing")] {
        let account_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts WHERE id = $1")
            .bind(account_id)
            .fetch_one(&mut *conn)
            .await
            .expect("count restored account");
        let username: String = sqlx::query_scalar("SELECT username FROM accounts WHERE id = $1")
            .bind(account_id)
            .fetch_one(&mut *conn)
            .await
            .expect("read restored username");
        let (message_count, body): (i64, String) = sqlx::query_as(
            "SELECT COUNT(*), MIN(body)
             FROM messages WHERE account_id = $1 AND guid = $2",
        )
        .bind(account_id)
        .bind(guid)
        .fetch_one(&mut *conn)
        .await
        .expect("count restored message");
        assert_eq!(account_count, 1, "account {account_id}");
        assert_eq!(username, account_id.to_string(), "username {account_id}");
        assert_eq!(message_count, 1, "message {guid}");
        assert_eq!(body, "keep me", "message body {guid}");
    }
    close_test_db(pool, conn).await;
}

#[test]
fn parent_dir_or_cwd_returns_the_parent_or_the_current_directory() {
    assert_eq!(
        parent_dir_or_cwd(Path::new("data/vault.db")),
        Path::new("data")
    );
    assert_eq!(parent_dir_or_cwd(Path::new("vault.db")), Path::new("."));
    assert_eq!(parent_dir_or_cwd(Path::new("/")), Path::new("."));
}

#[test]
fn the_reset_work_directory_is_created_inside_the_data_directory() {
    let temp = tempfile::tempdir().expect("create test directory");
    let data_dir = temp.path().join("data");

    let work = reset_account_work_dir(&data_dir).expect("create work directory");

    assert!(
        data_dir.is_dir(),
        "a missing data directory is created first"
    );
    assert_eq!(work.path().parent(), Some(data_dir.as_path()));
    assert!(work.path().is_dir());
    let name = work
        .path()
        .file_name()
        .and_then(|name| name.to_str())
        .expect("work directory name");
    assert!(name.starts_with(".reset-demo-data-"), "{name}");
}

/// The SQLite tables in `db`, by name, without touching the schema.
async fn sqlite_table_names(db: &Path) -> Vec<String> {
    let pool = engine::open_pool_for_path(db).await.expect("open database");
    let mut conn = pool.acquire().await.expect("acquire");
    let names: Vec<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .fetch_all(&mut *conn)
            .await
            .expect("list tables");
    conn.close().await.expect("close");
    pool.close().await;
    names
}

#[tokio::test]
async fn the_database_snapshot_carries_the_active_tables_and_rows() {
    let temp = tempfile::tempdir().expect("create test directory");
    let active = temp.path().join("active/vault.db");
    fs::create_dir_all(active.parent().expect("database parent")).expect("create database parent");
    seed_reset_test_database(&active).await;
    let prepared = temp.path().join("prepared/vault.db");
    fs::create_dir_all(prepared.parent().expect("prepared parent"))
        .expect("create prepared parent");

    prepare_database_snapshot(&active, &prepared)
        .await
        .expect("snapshot the active database");

    assert!(prepared.is_file());
    let active_tables = sqlite_table_names(&active).await;
    let prepared_tables = sqlite_table_names(&prepared).await;
    assert!(
        active_tables.iter().any(|table| table == "accounts"),
        "{active_tables:?}"
    );
    assert_eq!(prepared_tables, active_tables);
    assert_reset_test_database(&prepared).await;
    assert_reset_test_database(&active).await;
}

#[tokio::test]
async fn the_snapshot_of_a_missing_database_is_an_empty_vault_with_the_schema() {
    let temp = tempfile::tempdir().expect("create test directory");
    let active = temp.path().join("missing/vault.db");
    let prepared = temp.path().join("prepared.db");

    prepare_database_snapshot(&active, &prepared)
        .await
        .expect("create the prepared database");

    assert!(!active.exists(), "nothing is written at the active path");
    let tables = sqlite_table_names(&prepared).await;
    assert!(tables.iter().any(|table| table == "accounts"), "{tables:?}");
    assert!(tables.iter().any(|table| table == "messages"), "{tables:?}");
    let (pool, mut conn) = test_db(&prepared).await;
    let accounts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts")
        .fetch_one(&mut *conn)
        .await
        .expect("count accounts");
    assert_eq!(accounts, 0);
    close_test_db(pool, conn).await;
}

/// The work directories a reset hands to the install step, created inside
/// `root` with the prefixes the reset uses.
fn reset_work_dirs(root: &Path) -> (tempfile::TempDir, tempfile::TempDir) {
    let db_work = tempfile::Builder::new()
        .prefix(".reset-demo-db-")
        .tempdir_in(root)
        .expect("create database work directory");
    let data_work = tempfile::Builder::new()
        .prefix(".reset-demo-data-")
        .tempdir_in(root)
        .expect("create account work directory");
    (db_work, data_work)
}

#[tokio::test]
async fn a_successful_install_removes_the_work_directories() {
    let temp = tempfile::tempdir().expect("create test directory");
    let (db_work, data_work) = reset_work_dirs(temp.path());
    let db_work_path = db_work.path().to_path_buf();
    let data_work_path = data_work.path().to_path_buf();
    let prepared_db = db_work.path().join("vault.db");
    seed_reset_test_database(&prepared_db).await;
    let prepared_account = data_work.path().join(DEMO_ACCOUNT_ID.to_string());
    fs::create_dir_all(&prepared_account).expect("create prepared account");
    fs::write(prepared_account.join("sentinel"), b"new data").expect("write new data");
    let prepared_config = temp.path().join("prepared-config/config.toml");
    fs::create_dir_all(prepared_config.parent().expect("prepared config parent"))
        .expect("create prepared config parent");
    fs::write(&prepared_config, b"new config").expect("write prepared config");
    let active_db = temp.path().join("active/vault.db");
    fs::create_dir_all(active_db.parent().expect("active database parent"))
        .expect("create active database parent");
    let active_account = temp.path().join("data").join(DEMO_ACCOUNT_ID.to_string());
    let active_config = temp.path().join("config/config.toml");
    fs::create_dir_all(active_config.parent().expect("active config parent"))
        .expect("create active config parent");

    install_reset_state_or_keep_work(
        &ResetPaths {
            active_db: &active_db,
            prepared_db: &prepared_db,
            active_account: &active_account,
            prepared_account: &prepared_account,
            active_config: &active_config,
            prepared_config: &prepared_config,
        },
        db_work,
        data_work,
    )
    .await
    .expect("install the prepared state");

    assert_reset_test_database(&active_db).await;
    assert_eq!(
        fs::read(active_account.join("sentinel")).expect("read installed account"),
        b"new data"
    );
    assert_eq!(
        fs::read(&active_config).expect("read installed config"),
        b"new config"
    );
    assert!(
        !db_work_path.exists(),
        "the database work directory is removed after a successful install"
    );
    assert!(
        !data_work_path.exists(),
        "the account work directory is removed after a successful install"
    );
}

#[tokio::test]
async fn a_failed_install_with_nothing_left_in_the_work_directories_removes_them() {
    let temp = tempfile::tempdir().expect("create test directory");
    let (db_work, data_work) = reset_work_dirs(temp.path());
    let db_work_path = db_work.path().to_path_buf();
    let data_work_path = data_work.path().to_path_buf();
    // No prepared database, account or config: the install refuses before
    // any rename, so there is no rollback and nothing to keep.
    let prepared_db = db_work.path().join("vault.db");
    let prepared_account = data_work.path().join(DEMO_ACCOUNT_ID.to_string());
    let prepared_config = temp.path().join("prepared-config/config.toml");
    let active_db = temp.path().join("active/vault.db");
    let active_account = temp.path().join("data").join(DEMO_ACCOUNT_ID.to_string());
    let active_config = temp.path().join("config/config.toml");

    let error = install_reset_state_or_keep_work(
        &ResetPaths {
            active_db: &active_db,
            prepared_db: &prepared_db,
            active_account: &active_account,
            prepared_account: &prepared_account,
            active_config: &active_config,
            prepared_config: &prepared_config,
        },
        db_work,
        data_work,
    )
    .await
    .expect_err("an incomplete prepared state must fail");

    let text = format!("{error:#}");
    assert!(
        text.contains("prepared reset state is incomplete"),
        "{text}"
    );
    assert!(!text.contains("rollback was incomplete"), "{text}");
    assert!(!db_work_path.exists(), "{}", db_work_path.display());
    assert!(!data_work_path.exists(), "{}", data_work_path.display());
}

#[tokio::test]
async fn a_failed_install_that_left_previous_state_in_the_work_directories_keeps_them() {
    let temp = tempfile::tempdir().expect("create test directory");
    let (db_work, data_work) = reset_work_dirs(temp.path());
    let db_work_path = db_work.path().to_path_buf();
    let data_work_path = data_work.path().to_path_buf();
    // A backup the rollback could not put back stands in the database work
    // directory, the way a rename that failed midway would leave it.
    fs::write(
        db_work.path().join("previous-vault.db"),
        b"previous database",
    )
    .expect("write leftover backup");
    let prepared_db = db_work.path().join("vault.db");
    let prepared_account = data_work.path().join(DEMO_ACCOUNT_ID.to_string());
    let prepared_config = temp.path().join("prepared-config/config.toml");
    let active_db = temp.path().join("active/vault.db");
    let active_account = temp.path().join("data").join(DEMO_ACCOUNT_ID.to_string());
    let active_config = temp.path().join("config/config.toml");

    let error = install_reset_state_or_keep_work(
        &ResetPaths {
            active_db: &active_db,
            prepared_db: &prepared_db,
            active_account: &active_account,
            prepared_account: &prepared_account,
            active_config: &active_config,
            prepared_config: &prepared_config,
        },
        db_work,
        data_work,
    )
    .await
    .expect_err("an incomplete prepared state must fail");

    let text = format!("{error:#}");
    assert!(
        text.contains("reset-demo rollback was incomplete"),
        "{text}"
    );
    assert!(text.contains(&db_work_path.display().to_string()), "{text}");
    assert!(
        text.contains(&data_work_path.display().to_string()),
        "{text}"
    );
    assert!(
        text.contains("prepared reset state is incomplete"),
        "{text}"
    );
    assert_eq!(
        fs::read(db_work_path.join("previous-vault.db")).expect("read kept backup"),
        b"previous database"
    );
    assert!(
        data_work_path.is_dir(),
        "the account work directory is kept alongside the database one"
    );
}

/// What a generated bundle holds, read back from its files rather than taken
/// from the generator's own counts, so the import is checked against what
/// was on disk.
#[derive(Debug, Default)]
struct BundleContents {
    files: usize,
    messages: usize,
    replies: usize,
    tapbacks: usize,
}

fn read_generated_bundle(bundle: &Path) -> BundleContents {
    let mut contents = BundleContents::default();
    for source in [IMESSAGE_SOURCE, SBR_SOURCE, WHATSAPP_SOURCE] {
        let staging = bundle.join("staging").join(source);
        for path in crate::import_cli::list_jsonl_files(&staging).expect("list staging files") {
            contents.files += 1;
            let text = fs::read_to_string(&path).expect("read conversation file");
            for line in text.lines().skip(1) {
                let message: message_ir::IrMessage = serde_json::from_str(line)
                    .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()));
                contents.messages += 1;
                let Some(im) = message.imessage.as_ref() else {
                    continue;
                };
                if im.is_reply {
                    contents.replies += 1;
                }
                if let Some(serde_json::Value::Array(items)) = &im.tapbacks {
                    contents.tapbacks += items.len();
                }
            }
        }
    }
    contents
}

async fn count(conn: &mut AnyConnection, sql: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(DEMO_ACCOUNT_ID)
        .fetch_one(&mut *conn)
        .await
        .unwrap_or_else(|error| panic!("{sql}: {error}"))
}

/// The reset imports a bundle the generator wrote, three sources in turn,
/// and dedupes what the overlap conversations carry twice. Until now only a
/// three-line hand-written bundle went through this path in a test, so a
/// generator change that the import could not read, or a stride the import
/// dropped, showed up first in the demo vault.
///
/// Runs on SQLite by file path, and on Postgres by schema URL when
/// `MV_TEST_POSTGRES_URL` is set, the two transports `reset-demo` takes.
#[tokio::test]
async fn a_generated_demo_bundle_imports_whole_and_its_overlap_dedupes() {
    let temp = tempfile::tempdir().expect("create test directory");
    let seed_cfg = demo_seed::testutil::small_config(temp.path());
    let generated = demo_seed::generate(&seed_cfg).expect("generate the small bundle");
    let bundle = Path::new(&seed_cfg.out);
    let contents = read_generated_bundle(bundle);
    assert_eq!(contents.messages, generated.messages);
    assert!(
        contents.replies > 0 && contents.tapbacks > 0,
        "{contents:?}"
    );

    let db_path = temp.path().join("vault.db");
    let pg_url = match crate::pg_test_url() {
        Some(url) => Some(crate::db::engine::pg_test_schema_url(&url).await),
        None => None,
    };
    let target = match pg_url.as_deref() {
        Some(url) => DbTarget::Url(url),
        None => DbTarget::Path(&db_path),
    };
    let cfg = Config {
        paths: PathsConfig {
            db: db_path.clone(),
            data_dir: temp.path().join("data"),
            assets_dir: "assets".into(),
            assets_converted_dir: "assets_converted".into(),
        },
        server: None,
        database: crate::config::DatabaseConfig::default(),
    };

    let prepared = validate_prepared_bundle(bundle).expect("the generator wrote a complete bundle");
    seed_demo_account(target, DEMO_ACCOUNT_ID, &prepared.seed)
        .await
        .expect("seed the demo account");
    // The import stops at the first row it cannot read, so an `Ok` here is
    // the "no failed rows" of the whole bundle; the counts below say that
    // nothing was skipped on the way in either.
    let import = import_demo_sources(&cfg, &prepared, DEMO_ACCOUNT_ID, target)
        .await
        .expect("import every source of the generated bundle");
    assert_eq!(import.files as usize, contents.files, "every file imported");
    assert_eq!(
        import.messages as usize, contents.messages,
        "every message row imported"
    );
    assert_eq!(
        import.attachments as usize, generated.attachment_refs,
        "every attachment reference imported"
    );
    assert_eq!(import.assets_missing, 0, "every attachment file was found");
    assert_eq!(
        import.tapbacks as usize, contents.tapbacks,
        "every tapback imported"
    );

    let pool = target.open().await.expect("open the imported vault");
    let mut conn = pool.acquire().await.expect("acquire");
    let dedupe = dedupe::dedupe_cross_source(&mut conn, DEMO_ACCOUNT_ID, None, 2)
        .await
        .expect("dedupe across sources");

    // The overlap conversations are the only messages written to two
    // backups, so they are the only duplicates the dedupe may find.
    let hidden = count(
        &mut conn,
        "SELECT COUNT(*) FROM messages WHERE account_id = $1 AND duplicate_of IS NOT NULL",
    )
    .await;
    assert_eq!(
        hidden as usize, generated.shared_messages,
        "exactly the shared overlap rows are hidden as duplicates ({dedupe:?})"
    );
    assert_eq!(
        dedupe.exact_flagged as usize, generated.shared_messages,
        "the dedupe found them as exact duplicates ({dedupe:?})"
    );
    assert_eq!(
        dedupe.near_flagged, 0,
        "and nothing else as near duplicates"
    );

    // Every reply names a message the import holds in the same conversation,
    // so every thread the generator wrote can be shown.
    let replies = count(
        &mut conn,
        "SELECT COUNT(*) FROM messages WHERE account_id = $1 AND is_reply = 1",
    )
    .await;
    assert_eq!(replies as usize, contents.replies);
    let unresolved = count(
        &mut conn,
        "SELECT COUNT(*) FROM messages r
         WHERE r.account_id = $1 AND r.is_reply = 1
           AND NOT EXISTS (
             SELECT 1 FROM messages o
             WHERE o.conversation_id = r.conversation_id
               AND o.guid = r.thread_originator_guid
           )",
    )
    .await;
    assert_eq!(unresolved, 0, "every reply target is an imported message");

    // Tapbacks ride on the message they react to, so each row here is one
    // the import attached to its target.
    let tapbacks = count(
        &mut conn,
        "SELECT COUNT(*) FROM tapbacks t JOIN messages m ON m.id = t.message_id
         WHERE m.account_id = $1",
    )
    .await;
    assert_eq!(tapbacks as usize, contents.tapbacks);

    // The seed linked the owner's number as the demo account's identity.
    let owner_handles = count(
        &mut conn,
        "SELECT COUNT(*) FROM account_handles ah JOIN handles h ON h.id = ah.handle_id
         WHERE ah.account_id = $1 AND h.normalized = '+14155559000'",
    )
    .await;
    assert_eq!(owner_handles, 1);
    // Every demo header names that number as the owner, so every message is
    // held at it and the identity's counts are not zero (#690). WhatsApp's
    // are held at the same number as a WhatsApp address, which the demo
    // account has not added, so they count toward no identity.
    let not_held = count(
        &mut conn,
        "SELECT COUNT(*) FROM messages m
         WHERE m.account_id = $1 AND m.source != 'whatsapp'
           AND NOT EXISTS (
             SELECT 1 FROM account_handles ah
             WHERE ah.account_id = m.account_id AND ah.handle_id = m.owner_handle_id
           )",
    )
    .await;
    assert_eq!(
        not_held, 0,
        "every demo message is held at the owner's number"
    );
    conn.close().await.expect("close");
    pool.close().await;
}

/// Add a second copy of the tiny bundle's iMessage conversation to the SBR
/// staging folder, so the two sources carry one message twice and the
/// dedupe has something to hide.
fn write_overlap_conversation(bundle: &Path) {
    let overlap = concat!(
        r#"{"schema_version":4,"export":{"source":"sms-backup-restore","tool":"t","tool_version":"0","owner_handle":null,"owner_display_name":null},"conversation":{"chat_identifier":"+15555550101","conversation_type":"individual","group_title":null,"participants":[{"handle":"+15555550101","display_name":null}],"stats":{"message_count":1,"attachment_count":0,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}"#,
        "\n",
        r#"{"guid":"pg-demo-sbr-overlap","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"sms","message_kind":"sms","sender_handle":"+15555550101","sender_display_name":null,"subject":null,"text":"hello","attachments":[],"imessage":null,"source":null}"#,
        "\n",
    );
    fs::write(
        bundle
            .join("staging")
            .join(SBR_SOURCE)
            .join("overlap.jsonl"),
        overlap,
    )
    .expect("write overlap jsonl");
}

/// Seed the account `demo` had before this reset: a WhatsApp message (the
/// reset appends WhatsApp, so only the wipe removes it) and a file under its
/// data folder. Returns the file's path.
async fn seed_previous_demo(db: &Path, data_dir: &Path) -> PathBuf {
    let (pool, mut conn) = test_db(db).await;
    account_profile::ensure_account_row(&mut conn, DEMO_ACCOUNT_ID)
        .await
        .expect("seed the previous demo account");
    let handle_id: i64 = sqlx::query_scalar(
        "INSERT INTO handles (
            account_id, raw, normalized, handle_type, service
         ) VALUES ($1, '+15555550100', '+15555550100', 'phone', 'phone')
         RETURNING id",
    )
    .bind(DEMO_ACCOUNT_ID)
    .fetch_one(&mut *conn)
    .await
    .expect("insert previous handle");
    let conversation_id: i64 = sqlx::query_scalar(
        "INSERT INTO conversations (
            account_id, chat_handle_id, conversation_type, source_file
         ) VALUES ($1, $2, 'individual', 'previous.jsonl')
         RETURNING id",
    )
    .bind(DEMO_ACCOUNT_ID)
    .bind(handle_id)
    .fetch_one(&mut *conn)
    .await
    .expect("insert previous conversation");
    sqlx::query(
        "INSERT INTO messages (
            conversation_id, account_id, source, guid, timestamp,
            is_from_me, body, sort_order
         ) VALUES ($1, $2, 'whatsapp', 'previous-demo-message',
                   '2026-01-01T00:00:00Z', 0, 'from the previous demo', 0)",
    )
    .bind(conversation_id)
    .bind(DEMO_ACCOUNT_ID)
    .execute(&mut *conn)
    .await
    .expect("insert previous message");
    close_test_db(pool, conn).await;
    checkpoint_and_clean_sidecars(db, "while seeding the previous demo")
        .await
        .expect("checkpoint the previous demo");

    let stale = data_dir
        .join(DEMO_ACCOUNT_ID.to_string())
        .join("previous.bin");
    fs::create_dir_all(stale.parent().expect("account folder")).expect("create account folder");
    fs::write(&stale, b"previous demo attachment").expect("write previous attachment");
    stale
}

/// The wipe removes the demo account's rows and its data folder, and nothing
/// else. On the SQLite path the folder wiped is the empty work directory, so
/// this is the one place the folder removal is observed (#780).
#[tokio::test]
async fn the_wipe_removes_the_demo_rows_and_folder_and_leaves_other_accounts() {
    let temp = tempfile::tempdir().expect("create test directory");
    let db = temp.path().join("vault.db");
    let data_dir = temp.path().join("data");
    seed_reset_test_database(&db).await;
    let demo_folder = data_dir.join(DEMO_ACCOUNT_ID.to_string());
    fs::create_dir_all(demo_folder.join(IMESSAGE_SOURCE).join("assets"))
        .expect("create demo assets folder");
    fs::write(
        demo_folder
            .join(IMESSAGE_SOURCE)
            .join("assets")
            .join("a.bin"),
        b"demo",
    )
    .expect("write demo attachment");
    let other_folder = data_dir.join("9");
    fs::create_dir_all(&other_folder).expect("create other account folder");
    fs::write(other_folder.join("keep.bin"), b"keep").expect("write other attachment");
    let cfg = Config {
        paths: PathsConfig {
            db: db.clone(),
            data_dir: data_dir.clone(),
            assets_dir: "assets".into(),
            assets_converted_dir: "assets_converted".into(),
        },
        server: None,
        database: crate::config::DatabaseConfig::default(),
    };

    wipe_demo_account(&cfg, DEMO_ACCOUNT_ID, DbTarget::Path(&db))
        .await
        .expect("wipe the demo account");

    let (pool, mut conn) = test_db(&db).await;
    let demo_rows: i64 = sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) FROM accounts WHERE id = $1)
              + (SELECT COUNT(*) FROM messages WHERE account_id = $1)
              + (SELECT COUNT(*) FROM conversations WHERE account_id = $1)
              + (SELECT COUNT(*) FROM handles WHERE account_id = $1)",
    )
    .bind(DEMO_ACCOUNT_ID)
    .fetch_one(&mut *conn)
    .await
    .expect("count demo rows");
    assert_eq!(demo_rows, 0, "the demo account and its rows are gone");
    let other_messages: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM messages WHERE account_id = 9 AND guid = 'non-demo-existing'",
    )
    .fetch_one(&mut *conn)
    .await
    .expect("count other messages");
    assert_eq!(other_messages, 1, "the other account keeps its message");
    close_test_db(pool, conn).await;
    assert!(!demo_folder.exists(), "the demo data folder is removed");
    assert_eq!(
        fs::read(other_folder.join("keep.bin")).expect("read other attachment"),
        b"keep",
        "the other account's folder is untouched"
    );
}

/// A reset on the SQLite path leaves a claimed vault where `demo` logs in
/// with an empty password and the owner with `admin`/`admin`, holds none of
/// the previous demo's rows or files, and has deduped the new demo data
/// across its sources (#780).
#[tokio::test]
async fn a_reset_leaves_a_demo_that_logs_in_and_holds_nothing_old() {
    let temp = tempfile::tempdir().expect("create test directory");
    let db = temp.path().join("active").join("vault.db");
    fs::create_dir_all(db.parent().expect("database parent")).expect("create database parent");
    let data_dir = temp.path().join("data");
    let previous_file = seed_previous_demo(&db, &data_dir).await;
    let bundle = temp.path().join("bundle");
    write_tiny_reset_bundle(&bundle);
    write_overlap_conversation(&bundle);
    let config_dest = temp.path().join("config").join("config.toml");
    fs::create_dir_all(config_dest.parent().expect("config parent")).expect("create config parent");
    let prepared_config = temp.path().join("prepared-config.toml");
    let config_text = format!(
        "[paths]\ndb = \"{}\"\ndata_dir = \"{}\"\n",
        db.display(),
        data_dir.display()
    );
    fs::write(&prepared_config, &config_text).expect("write prepared config");
    let cfg = Config {
        paths: PathsConfig {
            db: db.clone(),
            data_dir: data_dir.clone(),
            assets_dir: "assets".into(),
            assets_converted_dir: "assets_converted".into(),
        },
        server: None,
        database: crate::config::DatabaseConfig::default(),
    };
    {
        let (pool, mut conn) = test_db(&db).await;
        assert!(
            !account_profile::vault_is_claimed(&mut conn)
                .await
                .expect("read claim state"),
            "the vault starts unclaimed, so the claim below is the reset's doing"
        );
        close_test_db(pool, conn).await;
    }

    let stats = reset_prepared_bundle(
        &cfg,
        &bundle,
        DEMO_ACCOUNT_ID,
        &config_dest,
        &prepared_config,
    )
    .await
    .expect("reset the demo");

    assert_eq!(
        fs::read_to_string(&config_dest).expect("read installed config"),
        config_text,
        "the prepared config is installed as the active config"
    );
    assert!(
        !previous_file.exists(),
        "the previous demo's file is gone: {}",
        previous_file.display()
    );

    let (pool, mut conn) = test_db(&db).await;
    assert!(
        account_profile::vault_is_claimed(&mut conn)
            .await
            .expect("read claim state"),
        "the reset claims the vault"
    );
    let previous_messages: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE guid = 'previous-demo-message'")
            .fetch_one(&mut *conn)
            .await
            .expect("count previous messages");
    assert_eq!(previous_messages, 0, "the previous demo's message is gone");
    let previous_conversations: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM conversations WHERE source_file = 'previous.jsonl'",
    )
    .fetch_one(&mut *conn)
    .await
    .expect("count previous conversations");
    assert_eq!(
        previous_conversations, 0,
        "the previous demo's conversation is gone"
    );
    let messages = count(
        &mut conn,
        "SELECT COUNT(*) FROM messages WHERE account_id = $1",
    )
    .await;
    assert_eq!(
        messages, 4,
        "the three tiny conversations and the overlap copy are imported"
    );
    // The import fills content keys as it promotes, so the dedupe has none
    // left to fill; what shows it ran is the hidden duplicate below.
    assert_eq!(stats.dedupe_keys_filled, 0);
    assert_eq!(stats.import.messages, 4);
    assert_eq!(stats.process_assets.errors, 0);
    let hidden = count(
        &mut conn,
        "SELECT COUNT(*) FROM messages WHERE account_id = $1 AND duplicate_of IS NOT NULL",
    )
    .await;
    assert_eq!(
        hidden, 1,
        "the overlap copy is hidden as a duplicate of the iMessage message"
    );
    close_test_db(pool, conn).await;

    let pool = engine::open_pool_for_path(&db)
        .await
        .expect("open the reset vault");
    let state = crate::server::test_app_state(pool, &data_dir);
    let session = crate::test_support::log_in(&state, "demo", "").await;
    assert_eq!(session["username"], "demo");
    assert_eq!(session["account_id"], DEMO_ACCOUNT_ID);
    assert_eq!(
        crate::test_support::login_status(&state, "demo", "not-empty").await,
        axum::http::StatusCode::UNAUTHORIZED,
        "demo has no password, so only the empty password logs in"
    );
    let owner = crate::test_support::log_in(&state, DEMO_OWNER_USERNAME, DEMO_OWNER_PASSWORD).await;
    assert_eq!(owner["account_id"], account_profile::OWNER_ACCOUNT_ID);
}
