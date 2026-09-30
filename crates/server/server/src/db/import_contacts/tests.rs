use super::*;
use crate::db::schema;
use crate::db::vault_imports::{StartImportArgs, start_import};

const ACCOUNT: i64 = 7;

async fn vault_with_a_run() -> (sqlx::AnyPool, tempfile::TempDir, i64) {
    let (pool, dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    schema::ensure_vault_schema(&mut conn).await.unwrap();
    crate::db::account_profile::ensure_account_row(&mut conn, ACCOUNT)
        .await
        .unwrap();
    let import_id = start_import(
        &mut conn,
        &StartImportArgs::new(ACCOUNT, "imessage", "full", None),
    )
    .await
    .unwrap();
    (pool, dir, import_id)
}

async fn contact(conn: &mut AnyConnection, name: &str) -> i64 {
    crate::db::contacts::create_contact(conn, ACCOUNT, name, crate::db::contacts::Origin::Import)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_run_keeps_the_most_consequential_reason_for_a_contact() {
    let (pool, _dir, import_id) = vault_with_a_run().await;
    let mut conn = pool.acquire().await.unwrap();
    let ada = contact(&mut conn, "Ada").await;

    record(&mut conn, Some(import_id), ada, ContactReason::HandleAdded)
        .await
        .unwrap();
    record(&mut conn, Some(import_id), ada, ContactReason::Named)
        .await
        .unwrap();
    let (rows, total) = page(&mut conn, import_id, 40, 0).await.unwrap();
    assert_eq!(total, 1);
    assert_eq!(
        rows[0].reason,
        ContactReason::Named,
        "named outranks a handle"
    );

    record(&mut conn, Some(import_id), ada, ContactReason::HandleAdded)
        .await
        .unwrap();
    let (rows, _) = page(&mut conn, import_id, 40, 0).await.unwrap();
    assert_eq!(
        rows[0].reason,
        ContactReason::Named,
        "a later, lesser reason does not demote the record"
    );
}

#[tokio::test]
async fn the_tally_and_the_page_read_what_the_run_recorded() {
    let (pool, _dir, import_id) = vault_with_a_run().await;
    let mut conn = pool.acquire().await.unwrap();
    let ada = contact(&mut conn, "Ada").await;
    let grace = contact(&mut conn, "Grace").await;
    let nameless = contact(&mut conn, "").await;
    let untouched = contact(&mut conn, "Nobody").await;

    record(&mut conn, Some(import_id), ada, ContactReason::Created)
        .await
        .unwrap();
    record(
        &mut conn,
        Some(import_id),
        grace,
        ContactReason::ReplacedTrashed,
    )
    .await
    .unwrap();
    record(
        &mut conn,
        Some(import_id),
        nameless,
        ContactReason::HandleAdded,
    )
    .await
    .unwrap();

    assert_eq!(
        counts(&mut conn, import_id).await.unwrap(),
        ContactCounts {
            new_count: 2,
            changed_count: 1
        }
    );
    let (rows, total) = page(&mut conn, import_id, 40, 0).await.unwrap();
    assert_eq!(total, 3);
    assert_eq!(
        rows,
        vec![
            ImportContact {
                id: grace,
                name: "Grace".into(),
                reason: ContactReason::ReplacedTrashed
            },
            ImportContact {
                id: ada,
                name: "Ada".into(),
                reason: ContactReason::Created
            },
            ImportContact {
                id: nameless,
                name: String::new(),
                reason: ContactReason::HandleAdded
            },
        ],
        "most consequential first, and the untouched contact is absent"
    );
    let mut ids = contact_ids(&mut conn, import_id).await.unwrap();
    ids.sort_unstable();
    let mut expected = vec![ada, grace, nameless];
    expected.sort_unstable();
    assert_eq!(ids, expected);
    assert!(!ids.contains(&untouched));
}

#[tokio::test]
async fn nothing_is_recorded_without_a_run() {
    let (pool, _dir, import_id) = vault_with_a_run().await;
    let mut conn = pool.acquire().await.unwrap();
    let ada = contact(&mut conn, "Ada").await;
    record(&mut conn, None, ada, ContactReason::Created)
        .await
        .unwrap();
    assert_eq!(
        counts(&mut conn, import_id).await.unwrap(),
        ContactCounts::default()
    );
}
