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
    sqlx::any::install_default_drivers();
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
    let account_root = data_dir.join(DEMO_ACCOUNT_ID);
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

        let active_account = temp.path().join("data").join(DEMO_ACCOUNT_ID);
        let prepared_account = temp.path().join("prepared-data").join(DEMO_ACCOUNT_ID);
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

    let active_account = temp.path().join("data").join(DEMO_ACCOUNT_ID);
    let prepared_account = temp.path().join("prepared-data").join(DEMO_ACCOUNT_ID);
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
    seed_reset_test_account(&mut conn, "non-demo-account", "non-demo-existing").await;
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

async fn seed_reset_test_account(conn: &mut AnyConnection, account_id: &str, guid: &str) {
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
    for (account_id, guid) in [
        (DEMO_ACCOUNT_ID, "demo-existing"),
        ("non-demo-account", "non-demo-existing"),
    ] {
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
        assert_eq!(username, account_id, "username {account_id}");
        assert_eq!(message_count, 1, "message {guid}");
        assert_eq!(body, "keep me", "message body {guid}");
    }
    close_test_db(pool, conn).await;
}
