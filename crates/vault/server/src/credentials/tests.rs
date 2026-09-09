use super::*;
use crate::db::engine;
use crate::db::permissions::Permissions;
use crate::db::schema;

const TEST_ACCOUNT: i64 = 7;
const OTHER_ACCOUNT: i64 = 8;

/// Passwords hashed before the argon2 0.6 upgrade must still let people in.
///
/// These three strings were produced by argon2 0.5.3, the version the vault
/// shipped with, using the salt scheme it used then: sixteen bytes from the
/// system RNG, base64-encoded, passed to `Argon2::default()`. If a future
/// upgrade stops them verifying, every account created before that upgrade
/// is locked out, and no other test in this suite would notice — the rest
/// all hash and verify with the same version in the same process.
#[test]
fn hashes_written_by_argon2_0_5_still_verify() {
    const LEGACY: &[(&str, &str)] = &[
        (
            "hunter2hunter2",
            "$argon2id$v=19$m=19456,t=2,p=1$/ic5l4xy5HAgEHBiuv0t3A$iI1c7vmoqfa79pGmE3/iquM09ezwKoYA8U1dxtWH/rg",
        ),
        (
            "",
            "$argon2id$v=19$m=19456,t=2,p=1$Mp73mogaQlz3ZqmokZzY/A$CVBX4QUiJTjS5u4sLQ7rvMEuSo8e6c1izYXZTtJo+RQ",
        ),
        (
            "pässwörd with spaces 🔐",
            "$argon2id$v=19$m=19456,t=2,p=1$l9S0iAuO7kK+iuZf99EN9g$FQ35n77YztPzFZAYqhZnyRLK6TUg7nuXUGiO7Nx1s90",
        ),
    ];
    for (password, hash) in LEGACY {
        assert!(
            verify_password(hash, password),
            "argon2 0.5 hash must still verify for {password:?}"
        );
        assert!(
            !verify_password(hash, "wrong-password"),
            "the wrong password must still be refused for {password:?}"
        );
    }
}

/// The parameters the vault writes must not drift silently. A weaker
/// memory or time cost would be a security regression that still passes
/// every round-trip test, because hashing and verifying would agree.
#[test]
fn a_new_hash_keeps_argon2id_and_its_default_cost() {
    let hash = hash_password("hunter2hunter2").expect("hash");
    assert!(
        hash.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"),
        "unexpected argon2 parameters: {hash}"
    );
    assert!(verify_password(&hash, "hunter2hunter2"));
    assert!(!verify_password(&hash, "hunter2hunter3"));
}

/// Two hashes of the same password must differ, or the salt is not doing
/// its job. argon2 0.6 generates the salt itself now, so this is the check
/// that the generated salt is actually random rather than fixed.
#[test]
fn the_same_password_hashes_differently_every_time() {
    let a = hash_password("hunter2hunter2").expect("hash");
    let b = hash_password("hunter2hunter2").expect("hash");
    assert_ne!(a, b, "a repeated hash means the salt is not random");
    assert!(verify_password(&a, "hunter2hunter2"));
    assert!(verify_password(&b, "hunter2hunter2"));
}

#[test]
fn a_username_is_trimmed_and_checked() {
    assert_eq!(require_valid_username("  alice ").unwrap(), "alice");
    assert!(matches!(
        require_valid_username("no spaces here"),
        Err(ApiError::ValidationFailed(_))
    ));
    assert!(matches!(
        require_valid_username("   "),
        Err(ApiError::ValidationFailed(_))
    ));
}

#[test]
fn auth_rate_limit_forgets_idle_buckets() {
    let limits: AuthRateLimits = Arc::new(Mutex::new(HashMap::new()));
    let start = Instant::now();
    check_auth_rate_limit_at(&limits, "login:alice", start).unwrap();
    check_auth_rate_limit_at(&limits, "login:bob", start).unwrap();
    assert_eq!(limits.lock().unwrap().len(), 2);

    let later = start + AUTH_RATE_WINDOW + Duration::from_secs(1);
    check_auth_rate_limit_at(&limits, "login:carol", later).unwrap();
    let map = limits.lock().unwrap();
    assert_eq!(map.len(), 1);
    assert!(map.contains_key("login:carol"));
}

#[test]
fn auth_rate_limit_trips_after_max() {
    let limits: AuthRateLimits = Arc::new(Mutex::new(HashMap::new()));
    let bucket = "register:someone";
    for _ in 0..AUTH_RATE_MAX {
        check_auth_rate_limit(&limits, bucket).unwrap();
    }
    let err = check_auth_rate_limit(&limits, bucket).unwrap_err();
    match err {
        ApiError::RateLimited { retry_after_secs } => {
            assert!((1..=AUTH_RATE_WINDOW.as_secs()).contains(&retry_after_secs));
        }
        other => panic!("expected RateLimited, got {other:?}"),
    }
}

#[test]
fn auth_rate_limits_do_not_cross_vaults() {
    let one: AuthRateLimits = Arc::new(Mutex::new(HashMap::new()));
    let two: AuthRateLimits = Arc::new(Mutex::new(HashMap::new()));
    let bucket = "register:someone";
    for _ in 0..AUTH_RATE_MAX {
        check_auth_rate_limit(&one, bucket).unwrap();
    }
    check_auth_rate_limit(&one, bucket).unwrap_err();
    check_auth_rate_limit(&two, bucket)
        .expect("a second vault's limiter must not see the first vault's hits");
}

async fn password_change_setup() -> (
    tempfile::TempDir,
    sqlx::pool::PoolConnection<sqlx::Any>,
    String,
    Vec<String>,
    String,
) {
    let (pool, dir) = engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    schema::ensure_vault_schema(&mut conn).await.unwrap();
    let old_hash = hash_password("old-password").unwrap();
    account_profile::insert_account_at(&mut conn, TEST_ACCOUNT, "alice", Some(&old_hash), None)
        .await
        .unwrap();
    account_profile::insert_account_at(&mut conn, OTHER_ACCOUNT, "bob", Some(&old_hash), None)
        .await
        .unwrap();
    let old_session = session_tokens::insert_account_session_token(&mut conn, TEST_ACCOUNT)
        .await
        .unwrap();
    let first_api_token = api_tokens::create_api_token(
        &mut conn,
        TEST_ACCOUNT,
        "backup client",
        Permissions::all(),
        None,
    )
    .await
    .unwrap()
    .token;
    let second_api_token = api_tokens::create_api_token(
        &mut conn,
        TEST_ACCOUNT,
        "export client",
        Permissions {
            import: false,
            export: true,
            delete: false,
        },
        None,
    )
    .await
    .unwrap()
    .token;
    let other_account_token = api_tokens::create_api_token(
        &mut conn,
        OTHER_ACCOUNT,
        "other account client",
        Permissions::all(),
        None,
    )
    .await
    .unwrap()
    .token;
    (
        dir,
        conn,
        old_session,
        vec![first_api_token, second_api_token],
        other_account_token,
    )
}

#[tokio::test]
async fn change_password_transaction_updates_all_credentials() {
    let (_dir, mut conn, old_session, api_tokens, other_account_token) =
        password_change_setup().await;
    let new_hash = hash_password("new-password").unwrap();

    let new_session = change_password_on_conn(&mut conn, TEST_ACCOUNT, "old-password", &new_hash)
        .await
        .unwrap();

    let stored_hash = account_profile::load_password_hash(&mut conn, TEST_ACCOUNT)
        .await
        .unwrap()
        .unwrap();
    assert!(passwords_match(Some(&stored_hash), "new-password"));
    assert!(
        session_tokens::lookup_account_for_token(&mut conn, &old_session)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        session_tokens::lookup_account_for_token(&mut conn, &new_session)
            .await
            .unwrap(),
        Some(TEST_ACCOUNT)
    );
    for api_token in api_tokens {
        assert!(
            crate::db::api_tokens::lookup_account_for_api_token(&mut conn, &api_token)
                .await
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(
        crate::db::api_tokens::lookup_account_for_api_token(&mut conn, &other_account_token)
            .await
            .unwrap()
            .unwrap()
            .account_id,
        OTHER_ACCOUNT
    );
}

#[tokio::test]
async fn change_password_transaction_rolls_back_every_credential() {
    if crate::test_support::on_postgres() {
        return; // SQLite-only: the failure is injected with a trigger in SQLite's RAISE syntax
    }
    let (_dir, mut conn, old_session, api_tokens, other_account_token) =
        password_change_setup().await;
    sqlx::query(
        "CREATE TRIGGER fail_session_rotation
         BEFORE UPDATE ON account_session_tokens
         BEGIN
             SELECT RAISE(FAIL, 'injected session rotation failure');
         END",
    )
    .execute(&mut *conn)
    .await
    .unwrap();
    let new_hash = hash_password("new-password").unwrap();

    assert!(
        change_password_on_conn(&mut conn, TEST_ACCOUNT, "old-password", &new_hash)
            .await
            .is_err()
    );

    let stored_hash = account_profile::load_password_hash(&mut conn, TEST_ACCOUNT)
        .await
        .unwrap()
        .unwrap();
    assert!(passwords_match(Some(&stored_hash), "old-password"));
    assert_eq!(
        session_tokens::lookup_account_for_token(&mut conn, &old_session)
            .await
            .unwrap(),
        Some(TEST_ACCOUNT)
    );
    for api_token in api_tokens {
        assert!(
            crate::db::api_tokens::lookup_account_for_api_token(&mut conn, &api_token)
                .await
                .unwrap()
                .is_some()
        );
    }
    assert_eq!(
        crate::db::api_tokens::lookup_account_for_api_token(&mut conn, &other_account_token)
            .await
            .unwrap()
            .unwrap()
            .account_id,
        OTHER_ACCOUNT
    );
}

/// The Postgres half of `change_password_transaction_rolls_back_every_credential`.
///
/// The SQLite test injects its failure with `RAISE(FAIL, …)`, which Postgres
/// does not have, so the twin injects one with a `CHECK (false) NOT VALID`
/// constraint instead: `NOT VALID` leaves the rows the setup already wrote
/// alone and rejects the rotation's upsert. Without this twin nothing proves
/// the transaction rolls back on the engine the vault runs on when it is not
/// running on SQLite, and the two engines treat a failed statement inside a
/// transaction differently — which is the thing at issue.
#[tokio::test]
async fn change_password_transaction_rolls_back_every_credential_pg() {
    if !crate::test_support::on_postgres() {
        return; // Postgres-only: the SQLite twin above injects the same failure with a trigger
    }
    let (_dir, mut conn, old_session, api_tokens, other_account_token) =
        password_change_setup().await;
    sqlx::query(
        "ALTER TABLE account_session_tokens
         ADD CONSTRAINT fail_session_rotation CHECK (false) NOT VALID",
    )
    .execute(&mut *conn)
    .await
    .unwrap();
    let new_hash = hash_password("new-password").unwrap();

    assert!(
        change_password_on_conn(&mut conn, TEST_ACCOUNT, "old-password", &new_hash)
            .await
            .is_err()
    );

    sqlx::query("ALTER TABLE account_session_tokens DROP CONSTRAINT fail_session_rotation")
        .execute(&mut *conn)
        .await
        .unwrap();

    let stored_hash = account_profile::load_password_hash(&mut conn, TEST_ACCOUNT)
        .await
        .unwrap()
        .unwrap();
    assert!(
        passwords_match(Some(&stored_hash), "old-password"),
        "the old password must still be the stored one"
    );
    assert_eq!(
        session_tokens::lookup_account_for_token(&mut conn, &old_session)
            .await
            .unwrap(),
        Some(TEST_ACCOUNT),
        "the old session must survive the failed change"
    );
    for api_token in api_tokens {
        assert!(
            crate::db::api_tokens::lookup_account_for_api_token(&mut conn, &api_token)
                .await
                .unwrap()
                .is_some(),
            "the account's API tokens must survive the failed change"
        );
    }
    assert_eq!(
        crate::db::api_tokens::lookup_account_for_api_token(&mut conn, &other_account_token)
            .await
            .unwrap()
            .unwrap()
            .account_id,
        OTHER_ACCOUNT
    );
}
