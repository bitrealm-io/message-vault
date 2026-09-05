//! Search parity: the same committed message corpus in a fresh vault must
//! return identical result id sets on SQLite and on Postgres.
//!
//! The corpus lives in `tests/fixtures/search/parity-messages.json`; each
//! message carries a stable integer key `k` that the test binds as the
//! message id, so the expected id sets below are exactly the fixture keys.
//! The queries and expected sets are the parity contract committed by the
//! sqlx Any migration (#148): both engines must return exactly these sets.
//!
//! Runs on SQLite always and, when `MV_TEST_POSTGRES_URL` is set (CI and
//! the local compose service), on Postgres too, in a schema of its own. Both
//! pools come from the crate's test-support re-exports.

use message_vault_server::{
    ExportPageOpts, ensure_vault_schema, export_messages, pg_test_schema_pool, sqlite_test_pool,
};
use serde::Deserialize;
use sqlx::AnyConnection;

/// Account used for the corpus vault (the same id the crate's unit tests use).
const ACCOUNT_ID: &str = "11111111-1111-1111-1111-111111111111";

/// One corpus message; `k` is bound as the message id (both engines accept
/// explicit ids: SQLite `INTEGER PRIMARY KEY`, Postgres `BY DEFAULT AS IDENTITY`).
#[derive(Deserialize)]
struct FixtureMessage {
    k: i64,
    source: String,
    guid: String,
    body: Option<String>,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    attachments: Vec<FixtureAttachment>,
}

#[derive(Deserialize)]
struct FixtureAttachment {
    original_name: String,
    #[serde(default)]
    transcription: Option<String>,
}

/// Committed queries and expected id sets (message keys from the fixture).
/// Format: (query string, expected keys). These are the parity contract.
const CASES: &[(&str, &[i64])] = &[
    ("vault", &[1]),
    ("hello", &[1, 2]),         // case-insensitive on both engines
    ("report*", &[3]),          // prefix
    ("\"two words\"", &[4, 5]), // phrase (exact adjacency; k=5 "two words apart" matches too)
    ("red AND apple", &[6]),
    ("red apple", &[6]), // implicit AND
    ("red OR green", &[6, 7, 8]),
    ("apple NOT red", &[7]),
    ("secret", &[12]),         // attachment transcription
    ("IMG_0001", &[13]),       // attachment filename
    ("dinner", &[11]),         // subject
    ("dash-separated", &[14]), // punctuation tokenization
    ("alpha beta", &[15]),
];
// Diacritics: FTS5 strips them, Postgres 'simple' does not — the documented
// exception. "cafe" matches k=9 and k=10 on SQLite, only k=9 on Postgres;
// asserted per engine below, not in CASES.

/// Which engine a run targets (drives the diacritics expectation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Engine {
    Sqlite,
    Postgres,
}

/// Load the committed corpus fixture.
fn corpus() -> Vec<FixtureMessage> {
    serde_json::from_str(include_str!(
        "../../../../tests/fixtures/search/parity-messages.json"
    ))
    .expect("committed parity corpus parses")
}

/// Create a fresh vault: schema, one account, one conversation (a handle row
/// is required for `chat_handle_id`), then the corpus messages with their
/// keys bound as ids.
async fn setup_vault(conn: &mut AnyConnection) {
    ensure_vault_schema(conn)
        .await
        .expect("fresh vault schema applies");
    sqlx::query("INSERT INTO accounts (id, username) VALUES ($1, 'alice')")
        .bind(ACCOUNT_ID)
        .execute(&mut *conn)
        .await
        .unwrap();
    let handle_id: i64 = sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, '+15555550100', '+15555550100', 'phone', 'phone')
         RETURNING id",
    )
    .bind(ACCOUNT_ID)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let conversation_id: i64 = sqlx::query_scalar(
        r"
        INSERT INTO conversations (
            account_id, chat_handle_id, conversation_type,
            group_title, exported_at, source_file
        ) VALUES ($1, $2, 'individual', NULL, NULL, 'parity.json')
        RETURNING id
        ",
    )
    .bind(ACCOUNT_ID)
    .bind(handle_id)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    for m in corpus() {
        sqlx::query(
            r"
            INSERT INTO messages (
                id, conversation_id, account_id, source, guid, timestamp,
                is_from_me, sort_order, body, subject
            ) VALUES ($1, $2, $3, $4, $5, $6, 0, $1, $7, $8)
            ",
        )
        .bind(m.k)
        .bind(conversation_id)
        .bind(ACCOUNT_ID)
        .bind(&m.source)
        .bind(&m.guid)
        .bind("2020-01-01T00:00:00Z")
        .bind(&m.body)
        .bind(&m.subject)
        .execute(&mut *conn)
        .await
        .unwrap();
        for a in &m.attachments {
            sqlx::query(
                "INSERT INTO attachments (message_id, original_name, transcription)
                 VALUES ($1, $2, $3)",
            )
            .bind(m.k)
            .bind(&a.original_name)
            .bind(&a.transcription)
            .execute(&mut *conn)
            .await
            .unwrap();
        }
    }
}

/// Run the committed query list through the same search entry point the API
/// uses ([`export_messages`] on `/v1/export/messages`). Returns (query, id set)
/// pairs in `CASES` order.
async fn run_against(conn: &mut AnyConnection) -> Vec<(&'static str, Vec<i64>)> {
    setup_vault(conn).await;
    let mut results = Vec::with_capacity(CASES.len());
    for &(query, _expected) in CASES {
        let resp = export_messages(
            conn,
            ExportPageOpts {
                account_id: ACCOUNT_ID,
                query,
                limit: 100,
                offset: 0,
                clock: (
                    chrono_tz::UTC,
                    chrono::NaiveDate::from_ymd_opt(2026, 9, 2).unwrap(),
                ),
            },
        )
        .await
        .expect("committed parity query executes");
        let mut ids: Vec<i64> = resp.items.iter().map(|m| m.id).collect();
        ids.sort_unstable();
        ids.dedup();
        results.push((query, ids));
    }
    results
}

/// Assert every committed case returned its committed id set on this engine.
fn assert_committed_cases(results: &[(&str, Vec<i64>)], engine: Engine) {
    assert_eq!(results.len(), CASES.len());
    for ((query, expected), (actual_query, actual_ids)) in CASES.iter().zip(results) {
        assert_eq!(actual_query, query);
        assert_eq!(
            actual_ids, expected,
            "{engine:?}: query {query:?} returned {actual_ids:?}, expected {expected:?}"
        );
    }
}

/// The one documented engine divergence, asserted per engine: FTS5 strips
/// diacritics, Postgres 'simple' does not.
async fn assert_diacritics_exception(conn: &mut AnyConnection, engine: Engine) {
    let expected: Vec<i64> = match engine {
        Engine::Sqlite => vec![9, 10],
        Engine::Postgres => vec![9],
    };
    let resp = export_messages(
        conn,
        ExportPageOpts {
            account_id: ACCOUNT_ID,
            query: "cafe",
            limit: 100,
            offset: 0,
            clock: (
                chrono_tz::UTC,
                chrono::NaiveDate::from_ymd_opt(2026, 9, 2).unwrap(),
            ),
        },
    )
    .await
    .expect("diacritics query executes");
    let mut ids: Vec<i64> = resp.items.iter().map(|m| m.id).collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        expected,
        "{engine:?}: 'cafe' must match k=9{} (diacritics: {})",
        if engine == Engine::Postgres {
            " only"
        } else {
            " and k=10"
        },
        match engine {
            Engine::Sqlite => "FTS5 strips accents",
            Engine::Postgres => "Postgres 'simple' does not",
        }
    );
}

#[tokio::test]
async fn search_parity_across_engines() {
    // SQLite always.
    let (pool, _dir) = sqlite_test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    let sqlite = run_against(&mut conn).await;
    assert_committed_cases(&sqlite, Engine::Sqlite);
    assert_diacritics_exception(&mut conn, Engine::Sqlite).await;

    // Postgres when the gated suite is enabled (CI sets MV_TEST_POSTGRES_URL).
    let Some(url) = message_vault_server::pg_test_url() else {
        return;
    };
    let pool = pg_test_schema_pool(&url).await;
    let mut conn = pool.acquire().await.unwrap();
    let postgres = run_against(&mut conn).await;
    assert_committed_cases(&postgres, Engine::Postgres);
    assert_diacritics_exception(&mut conn, Engine::Postgres).await;

    // The parity claim: both engines agree on every committed case.
    assert_eq!(
        sqlite, postgres,
        "SQLite and Postgres must return identical result id sets"
    );
}
