//! Cross-source content fingerprint and soft-hide dedupe.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::io::{self, Write};
use std::time::Instant;

use anyhow::{Context, Result};
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use sqlx::AnyConnection;
use sqlx::Connection;

use crate::db::schema;
use crate::db::sql::SQLITE_IN_CHUNK;

const CONTENT_KEY_WRITE_LOG_EVERY: usize = 50_000;

/// One production message that still needs a content fingerprint.
///
/// Column order matches the SELECT in [`recompute_content_keys`]:
/// `id`, `conversation_id`, `chat_id` (chat handle `normalized`),
/// `conversation_type`, `is_from_me`, `timestamp`, `body`,
/// `sender_normalized`. Two SQL columns are both named `normalized`, so
/// this stays a positional tuple rather than `FromRow`.
type ContentKeyRow = (
    i64,
    i64,
    String,
    String,
    i64,
    String,
    Option<String>,
    Option<String>,
);

/// Collapse whitespace so minor text differences do not split the same SMS.
pub fn normalize_body(body: Option<&str>) -> String {
    body.unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Stable chat identity for content keys.
///
/// 1:1 chats use the conversation handle's normalized form (E.164 phones,
/// lowercased emails, verbatim usernames). Groups use sorted normalized
/// participant handles so the same people across exporters (different
/// `chat_identifier`s) share one fingerprint.
pub fn chat_identity_for_content_key(
    chat_identifier: &str,
    group_handles: Option<&[String]>,
) -> String {
    match group_handles {
        Some(handles) if !handles.is_empty() => {
            let mut sorted: Vec<&str> = handles
                .iter()
                .map(|h| h.as_str())
                .filter(|h| !h.is_empty())
                .collect();
            sorted.sort_unstable();
            sorted.dedup();
            format!("group:{}", sorted.join("|"))
        }
        _ => chat_identifier.to_string(),
    }
}

/// Build a content key from chat + direction + sender + UTC epoch + body + attachment hashes.
///
/// `timestamp` is the stored UTC instant; a value that does not parse is hashed as text.
/// For groups, pass the sorted-participant identity from [`chat_identity_for_content_key`].
/// Incoming group messages include the normalized sender so two peers sending the same
/// text at the same second do not collide; outgoing (`is_from_me`) uses an empty sender.
pub fn compute_content_key(
    chat_identifier: &str,
    is_from_me: bool,
    sender_normalized: Option<&str>,
    timestamp: &str,
    body: Option<&str>,
    attachment_shas: &[String],
) -> String {
    let epoch = parse_rfc3339_utc_secs(timestamp.trim())
        .map_or_else(|| timestamp.trim().to_string(), |s| s.to_string());

    let mut shas: Vec<&str> = attachment_shas
        .iter()
        .map(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .collect();
    shas.sort_unstable();
    shas.dedup();

    let sender = if is_from_me {
        ""
    } else {
        sender_normalized.map_or("", str::trim)
    };

    let mut hasher = Sha256::new();
    hasher.update(chat_identifier.as_bytes());
    hasher.update(b"|");
    hasher.update(if is_from_me { b"1" } else { b"0" });
    hasher.update(b"|");
    hasher.update(sender.as_bytes());
    hasher.update(b"|");
    hasher.update(epoch.as_bytes());
    hasher.update(b"|");
    hasher.update(normalize_body(body).as_bytes());
    for sha in shas {
        hasher.update(b"|");
        hasher.update(sha.as_bytes());
    }
    crate::assets::hex_encode(&hasher.finalize())
}

/// Fingerprint one message row from its chat identity, direction, sender, time, body, and attachment hashes.
fn content_key_for_row(
    row: &ContentKeyRow,
    group_handles: &HashMap<i64, Vec<String>>,
    shas_by_msg: &HashMap<i64, Vec<String>>,
) -> (i64, String) {
    let (id, conversation_id, chat_id, conversation_type, is_from_me, ts, body, sender_norm) = row;
    let empty: &[String] = &[];
    let shas = shas_by_msg.get(id).map_or(empty, Vec::as_slice);
    let group_identity = if conversation_type == "group" {
        Some(chat_identity_for_content_key(
            chat_id,
            group_handles.get(conversation_id).map(Vec::as_slice),
        ))
    } else {
        None
    };
    let identity = group_identity.as_deref().unwrap_or(chat_id);
    let key = compute_content_key(
        identity,
        *is_from_me != 0,
        sender_norm.as_deref(),
        ts,
        body.as_deref(),
        shas,
    );
    (*id, key)
}

/// Fingerprint every row in parallel.
fn hash_content_keys(
    rows: &[ContentKeyRow],
    group_handles: &HashMap<i64, Vec<String>>,
    shas_by_msg: &HashMap<i64, Vec<String>>,
) -> Vec<(i64, String)> {
    rows.par_iter()
        .map(|row| content_key_for_row(row, group_handles, shas_by_msg))
        .collect()
}

/// Counts reported by one cross-source dedupe pass.
#[derive(Debug, Default)]
pub struct DedupeStats {
    /// Content keys written (one per message; not a duplicate count).
    pub keys_filled: u64,
    /// Groups of messages sharing one content key.
    pub exact_groups: u64,
    /// Messages hidden as exact duplicates (all but the survivor per group).
    pub exact_flagged: u64,
    /// Messages flagged as near duplicates.
    pub near_flagged: u64,
}

/// Source preference for survivors: first imported source (min message id), then name.
pub async fn source_priority_from_db(
    conn: &mut AnyConnection,
    account_id: i64,
) -> Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        r"
        SELECT m.source, MIN(m.id) AS first_id
        FROM messages m
        JOIN conversations c ON c.id = m.conversation_id
        WHERE c.account_id = $1
          AND m.source IS NOT NULL
          AND TRIM(m.source) != ''
        GROUP BY m.source
        ORDER BY first_id ASC, m.source ASC
        ",
    )
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows.into_iter().map(|(source,)| source).collect())
}

/// Fill any missing content keys, clear prior flags, then soft-hide cross-source duplicates.
///
/// Survivor preference: source imported first (min message id), then source name.
/// Optional `source_priority` overrides (tests); `None` loads order from the DB.
pub async fn dedupe_cross_source(
    conn: &mut AnyConnection,
    account_id: i64,
    source_priority: Option<&[String]>,
    near_window_secs: i64,
) -> Result<DedupeStats> {
    let owned_priority;
    let priority = if let Some(p) = source_priority {
        p
    } else {
        owned_priority = source_priority_from_db(conn, account_id).await?;
        owned_priority.as_slice()
    };
    let mut stats = DedupeStats::default();
    let prio: HashMap<&str, usize> = priority
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect();
    let started = Instant::now();

    {
        println!("  dedupe:   filling missing content keys…");
        let _ = io::stdout().flush();
        let mut tx = conn.begin().await?;
        stats.keys_filled = fill_missing_content_keys(&mut tx, account_id).await?;
        sqlx::query(
            r"
            UPDATE messages
            SET duplicate_of = NULL
            WHERE conversation_id IN (
                SELECT id FROM conversations WHERE account_id = $1
            )
            ",
        )
        .bind(account_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        println!(
            "  dedupe:   keys filled={}  ({:.1}s)",
            stats.keys_filled,
            started.elapsed().as_secs_f64()
        );
    }

    {
        println!("  dedupe:   pass A exact content_key…");
        let _ = io::stdout().flush();
        let mut tx = conn.begin().await?;
        let (groups, flagged) = flag_exact_content_key_dupes(&mut tx, account_id, &prio).await?;
        stats.exact_groups = groups;
        stats.exact_flagged = flagged;
        tx.commit().await?;
        println!(
            "  dedupe:   exact groups={} flagged={}  ({:.1}s)",
            stats.exact_groups,
            stats.exact_flagged,
            started.elapsed().as_secs_f64()
        );
    }

    {
        println!("  dedupe:   pass B near-time (±{near_window_secs}s)…");
        let _ = io::stdout().flush();
        let mut tx = conn.begin().await?;
        stats.near_flagged =
            flag_near_time_dupes(&mut tx, account_id, &prio, near_window_secs).await?;
        tx.commit().await?;
        println!(
            "  dedupe:   near flagged={}  ({:.1}s total)",
            stats.near_flagged,
            started.elapsed().as_secs_f64()
        );
    }

    Ok(stats)
}

/// Compute `content_key` for production rows that still lack one (after attachments exist).
pub async fn fill_missing_content_keys(conn: &mut AnyConnection, account_id: i64) -> Result<u64> {
    recompute_content_keys(conn, true, account_id).await
}

/// Bulk-insert fingerprints into the `_content_keys` temp table in chunks that fit the bind limit.
async fn insert_content_key_rows(conn: &mut AnyConnection, keys: &[(i64, String)]) -> Result<()> {
    let total = keys.len();
    let mut written = 0usize;
    for chunk in keys.chunks(SQLITE_IN_CHUNK) {
        let mut sql = "INSERT INTO _content_keys (id, content_key) VALUES ".to_string();
        for (i, _) in chunk.iter().enumerate() {
            if i > 0 {
                sql.push(',');
            }
            let _ = write!(sql, "(${}, ${})", i * 2 + 1, i * 2 + 2);
        }
        let mut q = sqlx::query(&sql);
        for (id, key) in chunk {
            q = q.bind(*id).bind(key);
        }
        q.execute(&mut *conn).await?;
        let previous = written;
        written += chunk.len();
        let crossed_log_mark =
            written / CONTENT_KEY_WRITE_LOG_EVERY != previous / CONTENT_KEY_WRITE_LOG_EVERY;
        if written == total || crossed_log_mark {
            println!("  sql:      writing content keys … running={written}/{total}");
            let _ = io::stdout().flush();
        }
    }
    Ok(())
}

/// Compute and store content keys for the account's messages: every message,
/// or only those without a key. Returns how many were written.
///
/// # Errors
///
/// Returns an error when a query fails or the hashing task panics.
async fn recompute_content_keys(
    conn: &mut AnyConnection,
    missing_only: bool,
    account_id: i64,
) -> Result<u64> {
    let Some(inputs) = ContentKeyInputs::load(conn, account_id, missing_only).await? else {
        return Ok(0);
    };
    println!(
        "  sql:      hashing content keys ({} messages)…",
        inputs.rows.len()
    );
    let _ = io::stdout().flush();
    let keys = tokio::task::spawn_blocking(move || inputs.hash())
        .await
        .context("content-key hash task panicked")?;
    let filled = keys.len() as u64;
    apply_content_keys(conn, &keys).await?;
    Ok(filled)
}

/// Everything the content-key hash reads, loaded in three queries so the
/// hashing runs off the database thread with no lookups of its own.
struct ContentKeyInputs {
    rows: Vec<ContentKeyRow>,
    /// Sorted participant handles per group conversation: one shared identity
    /// across import sources.
    group_handles: HashMap<i64, Vec<String>>,
    /// Attachment digests per message.
    shas_by_msg: HashMap<i64, Vec<String>>,
}

impl ContentKeyInputs {
    /// `None` when no message needs a key.
    async fn load(
        conn: &mut AnyConnection,
        account_id: i64,
        missing_only: bool,
    ) -> Result<Option<Self>> {
        let filter = if missing_only {
            "WHERE (m.content_key IS NULL OR m.content_key = '') AND c.account_id = $1"
        } else {
            "WHERE c.account_id = $1"
        };
        let sql = format!(
            r"
            SELECT m.id, m.conversation_id, h.normalized, c.conversation_type,
                   m.is_from_me, m.timestamp, m.body,
                   hs.normalized
            FROM messages m
            JOIN conversations c ON c.id = m.conversation_id
            JOIN handles h ON h.id = c.chat_handle_id
            LEFT JOIN handles hs ON hs.id = m.sender_handle_id
            {filter}
            ORDER BY m.id
            "
        );
        let rows: Vec<ContentKeyRow> = sqlx::query_as(&sql)
            .bind(account_id)
            .fetch_all(&mut *conn)
            .await?;
        if rows.is_empty() {
            return Ok(None);
        }

        let participant_rows: Vec<(i64, String)> = sqlx::query_as(
            r"
            SELECT p.conversation_id, h.normalized
            FROM participants p
            JOIN conversations c ON c.id = p.conversation_id
            JOIN handles h ON h.id = p.handle_id
            WHERE c.account_id = $1
              AND h.normalized IS NOT NULL AND h.normalized != ''
            ORDER BY p.conversation_id, h.normalized
            ",
        )
        .bind(account_id)
        .fetch_all(&mut *conn)
        .await?;
        let mut group_handles: HashMap<i64, Vec<String>> = HashMap::new();
        for (conversation_id, handle) in participant_rows {
            group_handles
                .entry(conversation_id)
                .or_default()
                .push(handle);
        }

        // One scan for attachment hashes belonging to this account's message id range.
        let min_id = rows.first().map_or(0, |r| r.0);
        let max_id = rows.last().map_or(0, |r| r.0);
        let att_rows: Vec<(i64, String)> = sqlx::query_as(
            r"
            SELECT a.message_id, a.sha256
            FROM attachments a
            JOIN messages m ON m.id = a.message_id
            JOIN conversations c ON c.id = m.conversation_id
            WHERE c.account_id = $1
              AND a.message_id BETWEEN $2 AND $3
              AND a.sha256 IS NOT NULL AND a.sha256 != ''
            ORDER BY a.message_id
            ",
        )
        .bind(account_id)
        .bind(min_id)
        .bind(max_id)
        .fetch_all(&mut *conn)
        .await?;
        let mut shas_by_msg: HashMap<i64, Vec<String>> = HashMap::new();
        for (message_id, sha) in att_rows {
            shas_by_msg.entry(message_id).or_default().push(sha);
        }

        Ok(Some(Self {
            rows,
            group_handles,
            shas_by_msg,
        }))
    }

    /// `(message id, content key)` for every row, hashed in parallel.
    fn hash(&self) -> Vec<(i64, String)> {
        hash_content_keys(&self.rows, &self.group_handles, &self.shas_by_msg)
    }
}

/// Write the keys onto `messages` through the `_content_keys` temp table,
/// which is dropped again afterwards.
async fn apply_content_keys(conn: &mut AnyConnection, keys: &[(i64, String)]) -> Result<()> {
    for stmt in schema::split_ddl(
        r"
        CREATE TEMP TABLE IF NOT EXISTS _content_keys (
            id BIGINT PRIMARY KEY,
            content_key TEXT NOT NULL
        );
        DELETE FROM _content_keys;
        ",
    ) {
        sqlx::query(&stmt).execute(&mut *conn).await?;
    }
    insert_content_key_rows(conn, keys).await?;
    sqlx::query(
        r"
        UPDATE messages AS m
        SET content_key = k.content_key
        FROM _content_keys AS k
        WHERE m.id = k.id
        ",
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query("DROP TABLE IF EXISTS _content_keys")
        .execute(&mut *conn)
        .await?;
    Ok(())
}

#[derive(Clone)]
struct Cand {
    id: i64,
    source: String,
    att_count: i64,
}

/// Hide every message that shares a fingerprint with a preferred-source twin. Returns (groups, hidden).
async fn flag_exact_content_key_dupes(
    conn: &mut AnyConnection,
    account_id: i64,
    prio: &HashMap<&str, usize>,
) -> Result<(u64, u64)> {
    // One scan of messages + one aggregated attachment pass, then group in Rust.
    // Avoids N round-trips (one SELECT + several UPDATEs per duplicate key).
    let rows: Vec<(i64, String, String, i64)> = sqlx::query_as(
        r"
        SELECT m.id, m.source, m.content_key, COALESCE(ac.n, 0)
        FROM messages m
        JOIN conversations c ON c.id = m.conversation_id
        LEFT JOIN (
            SELECT a.message_id, COUNT(*) AS n
            FROM attachments a
            JOIN messages m2 ON m2.id = a.message_id
            JOIN conversations c2 ON c2.id = m2.conversation_id
            WHERE c2.account_id = $1
              AND a.sha256 IS NOT NULL AND a.sha256 != ''
            GROUP BY a.message_id
        ) ac ON ac.message_id = m.id
        WHERE c.account_id = $1
          AND m.content_key IS NOT NULL AND m.content_key != ''
        ",
    )
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await?;

    let mut by_key: HashMap<String, Vec<Cand>> = HashMap::new();
    for (id, source, content_key, att_count) in rows {
        by_key.entry(content_key).or_default().push(Cand {
            id,
            source,
            att_count,
        });
    }

    let mut flags: Vec<(i64, i64)> = Vec::new(); // (loser_id, winner_id)
    let mut groups = 0u64;
    for cands in by_key.values() {
        let sources: HashSet<&str> = cands.iter().map(|c| c.source.as_str()).collect();
        if sources.len() < 2 {
            continue;
        }
        groups += 1;
        let winner = pick_winner(cands, prio);
        for c in cands {
            if c.id != winner {
                flags.push((c.id, winner));
            }
        }
    }
    let flagged = flags.len() as u64;

    if flags.is_empty() {
        return Ok((groups, 0));
    }

    apply_duplicate_flags(conn, "_pass_a_flags", &flags).await?;

    Ok((groups, flagged))
}

/// The message to keep from a duplicate group: most attachments, then the earliest-imported source, then the lowest id.
fn pick_winner(cands: &[Cand], prio: &HashMap<&str, usize>) -> i64 {
    cands
        .iter()
        .min_by(|a, b| {
            b.att_count
                .cmp(&a.att_count)
                .then_with(|| {
                    let pa = prio.get(a.source.as_str()).copied().unwrap_or(usize::MAX);
                    let pb = prio.get(b.source.as_str()).copied().unwrap_or(usize::MAX);
                    pa.cmp(&pb)
                })
                .then_with(|| a.id.cmp(&b.id))
        })
        .map_or(cands[0].id, |c| c.id)
}

/// Parse an RFC3339 timestamp into Unix UTC seconds, honoring Z / ±HH:MM offsets.
///
/// Strict RFC3339 is sufficient: `messages.timestamp`
/// are only ever written by `models::format_timestamps` (chrono's
/// `to_rfc3339_opts(SecondsFormat::Secs, true)`), so no lenient spellings reach
/// this path. Unparseable input yields `None`.
fn parse_rfc3339_utc_secs(ts: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(ts.trim())
        .ok()
        .map(|dt| dt.timestamp())
}

#[derive(Clone)]
struct NearRow {
    id: i64,
    source: String,
    is_from_me: i64,
    sender_norm: String,
    secs: i64,
    body_norm: String,
    att_fp: String,
    att_count: i64,
}

impl NearRow {
    /// Whether `other`, a later row of the same conversation, is a near-time
    /// twin of this one: same direction and sender, a different source, and
    /// the same body or the same attachments.
    fn is_twin_of(&self, other: &NearRow) -> bool {
        let same_body = !self.body_norm.is_empty() && other.body_norm == self.body_norm;
        let same_attachments = !self.att_fp.is_empty() && other.att_fp == self.att_fp;
        other.is_from_me == self.is_from_me
            && other.sender_norm == self.sender_norm
            && other.source != self.source
            && (same_body || same_attachments)
    }

    /// The row as a winner candidate.
    fn candidate(&self) -> Cand {
        Cand {
            id: self.id,
            source: self.source.clone(),
            att_count: self.att_count,
        }
    }
}

/// Flag messages that match another within `window_secs` on chat, direction, and body but
/// not on the exact second. Returns how many were flagged.
async fn flag_near_time_dupes(
    conn: &mut AnyConnection,
    account_id: i64,
    prio: &HashMap<&str, usize>,
    window_secs: i64,
) -> Result<u64> {
    let by_conversation = load_near_rows(conn, account_id).await?;
    let flags = cluster_near_dupes(by_conversation, prio, window_secs);
    if flags.is_empty() {
        return Ok(0);
    }
    apply_duplicate_flags(conn, "_pass_b_flags", &flags).await?;
    Ok(flags.len() as u64)
}

/// Every unflagged message of the account with its attachment fingerprint,
/// grouped by conversation. Two queries and the grouping happen here so the
/// clustering does no per-message lookups.
async fn load_near_rows(
    conn: &mut AnyConnection,
    account_id: i64,
) -> Result<HashMap<i64, Vec<NearRow>>> {
    type NearDedupeRow = (i64, i64, String, i64, String, Option<String>, String);
    let msg_rows: Vec<NearDedupeRow> = sqlx::query_as(
        r"
        SELECT m.id, m.conversation_id, m.source, m.is_from_me, m.timestamp, m.body,
               COALESCE(hs.normalized, '')
        FROM messages m
        JOIN conversations c ON c.id = m.conversation_id
        LEFT JOIN handles hs ON hs.id = m.sender_handle_id
        WHERE c.account_id = $1
          AND m.duplicate_of IS NULL
        ",
    )
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await?;

    let att_rows: Vec<(i64, String)> = sqlx::query_as(
        r"
        SELECT a.message_id, a.sha256
        FROM attachments a
        JOIN messages m ON m.id = a.message_id
        JOIN conversations c ON c.id = m.conversation_id
        WHERE c.account_id = $1
          AND a.sha256 IS NOT NULL AND a.sha256 != ''
        ORDER BY a.message_id, a.sha256
        ",
    )
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await?;
    let mut shas_by_msg: HashMap<i64, Vec<String>> = HashMap::new();
    for (message_id, sha) in att_rows {
        shas_by_msg.entry(message_id).or_default().push(sha);
    }

    let mut by_conversation: HashMap<i64, Vec<NearRow>> = HashMap::new();
    for (id, conversation_id, source, is_from_me, ts, body, sender_norm) in msg_rows {
        let Some(secs) = parse_rfc3339_utc_secs(ts.trim()) else {
            continue;
        };
        let shas = shas_by_msg.remove(&id).unwrap_or_default();
        by_conversation
            .entry(conversation_id)
            .or_default()
            .push(NearRow {
                id,
                source,
                is_from_me,
                // Outgoing rows have no sender of their own.
                sender_norm: if is_from_me != 0 {
                    String::new()
                } else {
                    sender_norm
                },
                secs,
                body_norm: normalize_body(body.as_deref()),
                att_count: shas.len() as i64,
                att_fp: shas.join(","),
            });
    }
    Ok(by_conversation)
}

/// Walk each conversation in time order, gather each message's twins within
/// the window, and pick one winner per cluster. A cluster counts only when
/// it spans two sources: same-source near-duplicates are left alone.
/// Returns `(loser, winner)` pairs.
fn cluster_near_dupes(
    by_conversation: HashMap<i64, Vec<NearRow>>,
    prio: &HashMap<&str, usize>,
    window_secs: i64,
) -> Vec<(i64, i64)> {
    let mut flagged_ids: HashSet<i64> = HashSet::new();
    let mut flags: Vec<(i64, i64)> = Vec::new();
    for mut rows in by_conversation.into_values() {
        rows.sort_by(|a, b| a.secs.cmp(&b.secs).then(a.id.cmp(&b.id)));
        for i in 0..rows.len() {
            let first = &rows[i];
            if flagged_ids.contains(&first.id) {
                continue;
            }
            let cluster: Vec<Cand> = std::iter::once(first)
                .chain(
                    rows[i + 1..]
                        .iter()
                        .take_while(|row| row.secs - first.secs <= window_secs)
                        .filter(|row| first.is_twin_of(row) && !flagged_ids.contains(&row.id)),
                )
                .map(NearRow::candidate)
                .collect();
            let sources: HashSet<&str> = cluster.iter().map(|c| c.source.as_str()).collect();
            if sources.len() < 2 {
                continue;
            }
            let winner = pick_winner(&cluster, prio);
            for cand in &cluster {
                if cand.id != winner {
                    flagged_ids.insert(cand.id);
                    flags.push((cand.id, winner));
                }
            }
        }
    }
    flags
}

/// Apply (message id, duplicate-of id) pairs through a temp table so one UPDATE covers them all.
async fn apply_duplicate_flags(
    conn: &mut AnyConnection,
    table: &str,
    flags: &[(i64, i64)],
) -> Result<()> {
    for stmt in schema::split_ddl(&format!(
        "CREATE TEMP TABLE IF NOT EXISTS {table} (
            id BIGINT PRIMARY KEY,
            winner BIGINT NOT NULL
        );
        DELETE FROM {table};"
    )) {
        sqlx::query(&stmt).execute(&mut *conn).await?;
    }
    {
        let insert_sql = format!("INSERT INTO {table} (id, winner) VALUES ($1, $2)");
        for (id, winner) in flags {
            sqlx::query(&insert_sql)
                .bind(id)
                .bind(winner)
                .execute(&mut *conn)
                .await?;
        }
    }
    sqlx::query(&format!(
        "UPDATE messages AS m
         SET duplicate_of = f.winner
         FROM {table} AS f
         WHERE m.id = f.id"
    ))
    .execute(&mut *conn)
    .await?;
    for stmt in schema::split_ddl(&format!("DROP TABLE IF EXISTS {table};")) {
        sqlx::query(&stmt).execute(&mut *conn).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
