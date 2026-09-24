//! Copy staged import rows into the production tables.
//!
//! Every statement is in `db::staging`; this stage runs them in order, logs
//! each phase and keeps the counts.

use std::collections::HashMap;
use std::io::{self, Write};
use std::time::Instant;

use anyhow::{Result, bail};
use sqlx::AnyConnection;

use crate::db::dialect;
use crate::db::engine::DbEngine;
use crate::db::schema;
use crate::db::staging;

use super::ImportMode;

#[derive(Debug, Default)]
pub(super) struct PromoteStats {
    pub(super) conversations: u64,
    pub(super) participants: u64,
    pub(super) messages: u64,
    pub(super) attachments: u64,
    pub(super) tapbacks: u64,
    pub(super) messages_deduped: u64,
    pub(super) messages_appended: u64,
}

/// Move the account's staged rows into the production tables, wiping `wipe_sources` first
/// in replace mode. Returns the promoted counts.
///
/// `tx` is the import's one transaction, the one staging wrote into, so what
/// staging did to contacts becomes visible only when promote succeeds too.
/// The caller commits it.
pub(super) async fn promote_append(
    tx: &mut AnyConnection,
    mode: ImportMode,
    account_id: i64,
    fill_content_keys: bool,
    wipe_sources: &[String],
) -> Result<PromoteStats> {
    let engine = dialect::engine_of(tx);
    let mut promote = Promote {
        tx,
        account_id,
        mode,
        engine,
        stats: PromoteStats::default(),
        started: Instant::now(),
    };
    if mode == ImportMode::Replace {
        promote.wipe_sources(wipe_sources).await?;
    }
    promote.promote_conversations().await?;
    promote.promote_participants().await?;
    let messages_before = promote.promote_messages().await?;
    promote.promote_attachments().await?;
    promote.promote_tapbacks().await?;
    promote.index_fts(messages_before).await?;
    if fill_content_keys {
        promote.fill_content_keys().await?;
    }
    Ok(promote.finish())
}

/// One promotion in progress: the transaction it runs in, the account, and
/// the counts so far. Each phase is a method; they run in the order
/// [`promote_append`] calls them because later phases read the temp id maps
/// earlier ones write.
struct Promote<'a> {
    tx: &'a mut AnyConnection,
    account_id: i64,
    mode: ImportMode,
    engine: DbEngine,
    stats: PromoteStats,
    /// When the promotion began, for the total in every phase's log line.
    started: Instant,
}

/// Messages are inserted in staging-id ranges this wide, so one statement never
/// holds the whole batch and progress is visible on large imports.
const PROMOTE_MESSAGE_BATCH: i64 = 50_000;
/// Below this many staged messages the secondary indexes are cheaper to keep than to rebuild.
const PROMOTE_INDEX_DROP_MIN_STAGING: i64 = 5_000;

impl Promote<'_> {
    /// Log the start of a phase and return its clock.
    fn begin(msg: impl std::fmt::Display) -> Instant {
        promote_log(msg);
        Instant::now()
    }

    /// Log the end of a phase with its own and the total elapsed time.
    fn done(&self, phase: Instant, msg: impl std::fmt::Display) {
        promote_phase_done(self.started, phase, msg);
    }

    /// Replace mode: delete the account's existing rows for each source before
    /// anything is promoted, inside the same transaction as the inserts.
    async fn wipe_sources(&mut self, sources: &[String]) -> Result<()> {
        for source in sources {
            println!("  sql:      deleting existing messages for source '{source}'…");
            let _ = io::stdout().flush();
            schema::delete_messages_for_source(self.tx, self.account_id, source).await?;
        }
        if !sources.is_empty() {
            println!("  sql:      wipe complete (inside promote transaction)");
            let _ = io::stdout().flush();
        }
        Ok(())
    }

    /// Upsert the staged conversations and write `_promote_conv_map`, the
    /// staging-to-production id map every later phase joins through.
    async fn promote_conversations(&mut self) -> Result<()> {
        let staged = staging::count_staged_conversations(self.tx, self.account_id).await?;
        let phase = Self::begin(format_args!("{staged} staging conversations → production…"));
        let max_before = staging::max_conversation_id(self.tx).await?;
        staging::upsert_conversations(self.tx, self.account_id).await?;
        staging::write_conversation_map(self.tx, self.account_id).await?;
        let new_conversations =
            staging::count_mapped_conversations_above(self.tx, max_before).await?;
        self.stats.conversations = u64::try_from(new_conversations).unwrap_or(0);
        self.done(
            phase,
            format!("conversations done (new={})", self.stats.conversations),
        );
        Ok(())
    }

    /// Insert the staged participants under their production conversations.
    async fn promote_participants(&mut self) -> Result<()> {
        let staged = staging::count_staged_participants(self.tx, self.account_id).await?;
        let phase = Self::begin(format_args!("{staged} staging participants → production…"));
        self.stats.participants = staging::promote_participants(self.tx).await?;
        self.done(
            phase,
            format!("participants done (new={})", self.stats.participants),
        );
        Ok(())
    }

    /// Insert the staged messages with the FTS triggers paused and, for a
    /// large batch, the secondary indexes dropped and rebuilt; then write
    /// `_promote_msg_map` for the child rows. Returns the highest message id
    /// that existed before the insert: every new row lands above it, which
    /// is how [`Self::index_fts`] tells new rows apart from already indexed
    /// ones that the map also names.
    async fn promote_messages(&mut self) -> Result<i64> {
        let total = staging::count_staged_messages(self.tx, self.account_id).await?;
        promote_log(format_args!(
            "{total} staging messages → production ({})…",
            self.mode.as_str()
        ));
        self.pause_fts_triggers().await?;

        let existing = staging::count_messages(self.tx).await?;
        let rebuild_indexes = should_drop_messages_secondary_indexes(total, existing);
        if rebuild_indexes {
            let phase = Self::begin(format_args!(
                "dropping secondary message indexes (staging={total} existing={existing})…"
            ));
            schema::drop_messages_secondary_indexes(self.tx).await?;
            self.done(phase, "secondary indexes dropped");
        } else {
            promote_log(format_args!(
                "keeping secondary message indexes (staging={total} existing={existing})"
            ));
        }

        let messages_before = staging::max_message_id(self.tx).await?;
        let msg_map = self.insert_messages(total).await?;

        if rebuild_indexes {
            let phase = Self::begin("rebuilding secondary message indexes…");
            schema::create_messages_secondary_indexes(self.tx).await?;
            self.done(
                phase,
                format!(
                    "secondary indexes rebuilt (inserted={} skipped={})",
                    self.stats.messages, self.stats.messages_deduped
                ),
            );
        } else {
            promote_log(format_args!(
                "messages done (inserted={} skipped={})  (total {:.1}s)",
                self.stats.messages,
                self.stats.messages_deduped,
                self.started.elapsed().as_secs_f64()
            ));
        }

        let phase = Self::begin(format_args!(
            "writing message id map ({} pairs)…",
            msg_map.len()
        ));
        staging::write_message_map(self.tx, self.account_id, &msg_map).await?;
        self.done(phase, "message id map written");
        Ok(messages_before)
    }

    /// Skip per-row full-text search trigger work during the bulk inserts;
    /// [`Self::index_fts`] indexes once after. SQLite drops the sync triggers;
    /// Postgres disables every trigger on the message tables (same effect,
    /// and the triggers are simply re-enabled instead of reinstalled).
    async fn pause_fts_triggers(&mut self) -> Result<()> {
        let phase = Self::begin("pausing FTS triggers…");
        if self.engine == DbEngine::Postgres {
            schema::disable_fts_triggers_pg(self.tx).await?;
        } else {
            schema::drop_messages_fts_triggers(self.tx).await?;
        }
        self.done(phase, "FTS triggers paused");
        Ok(())
    }

    /// Insert the staged messages in id-range chunks and return the
    /// staging-to-production id map. Nothing staged means nothing inserted.
    async fn insert_messages(&mut self, total: i64) -> Result<HashMap<i64, i64>> {
        let bounds = staging::staged_message_id_bounds(self.tx, self.account_id).await?;
        let (Some(min_id), Some(max_id)) = bounds else {
            self.stats.messages = 0;
            self.stats.messages_appended = 0;
            self.stats.messages_deduped = 0;
            return Ok(HashMap::new());
        };
        if self.mode == ImportMode::Replace {
            self.insert_messages_replace(min_id, max_id, total).await
        } else {
            self.insert_messages_append(min_id, max_id, total).await
        }
    }

    /// Replace mode: every staged row is new, so each chunk is inserted and
    /// its production ids zipped onto the staged ids straight away.
    async fn insert_messages_replace(
        &mut self,
        min_id: i64,
        max_id: i64,
        total: i64,
    ) -> Result<HashMap<i64, i64>> {
        let mut msg_map = HashMap::new();
        let mut max_before = staging::max_message_id(self.tx).await?;
        let mut inserted_total = 0u64;
        for (chunk, lo, hi) in message_chunks(min_id, max_id) {
            let phase = Self::begin(format_args!(
                "inserting messages chunk {chunk} (staging id {}..{hi}, replace)…",
                lo + 1
            ));
            let inserted =
                staging::promote_messages_in_range(self.tx, self.account_id, lo, hi).await?;
            inserted_total += inserted;
            let staged =
                staging::staged_message_ids_in_range(self.tx, self.account_id, lo, hi).await?;
            max_before = self
                .zip_new_message_ids(&mut msg_map, staged, max_before, |n, p| {
                    format!(
                        "promote replace message id map mismatch: staging={n} new_prod={p} (chunk staging id {}..{hi})",
                        lo + 1
                    )
                })
                .await?;
            self.done(
                phase,
                format!("chunk {chunk} inserted={inserted} running={inserted_total}/{total}"),
            );
        }
        self.stats.messages = inserted_total;
        self.stats.messages_appended = inserted_total;
        Ok(msg_map)
    }

    /// Append mode: rows production already has are skipped by the guid
    /// index (`staging::promote_guid_messages_in_range`). Rows without a
    /// guid are outside that index and are always inserted, then zipped
    /// onto their staged ids; guid rows are mapped by the guid join in
    /// `staging::write_message_map`.
    async fn insert_messages_append(
        &mut self,
        min_id: i64,
        max_id: i64,
        total: i64,
    ) -> Result<HashMap<i64, i64>> {
        let mut msg_map = HashMap::new();
        let mut inserted_total = 0u64;
        for (chunk, lo, hi) in message_chunks(min_id, max_id) {
            let phase = Self::begin(format_args!(
                "inserting messages chunk {chunk} (staging id {}..{hi}, append)…",
                lo + 1
            ));
            let inserted =
                staging::promote_guid_messages_in_range(self.tx, self.account_id, lo, hi).await?;
            inserted_total += inserted;
            self.done(
                phase,
                format!("chunk {chunk} inserted={inserted} running={inserted_total}/{total}"),
            );
        }

        let phase = Self::begin("inserting messages with empty guids…");
        let max_before = staging::max_message_id(self.tx).await?;
        let inserted_empty =
            staging::promote_messages_without_guid(self.tx, self.account_id).await?;
        inserted_total += inserted_empty;
        let staged = staging::staged_message_ids_without_guid(self.tx, self.account_id).await?;
        self.zip_new_message_ids(&mut msg_map, staged, max_before, |n, p| {
            format!("promote append empty-guid id map mismatch: staging={n} new_prod={p}")
        })
        .await?;
        self.done(
            phase,
            format!("empty-guid messages inserted={inserted_empty}"),
        );

        self.stats.messages = inserted_total;
        self.stats.messages_appended = inserted_total;
        self.stats.messages_deduped = (total as u64).saturating_sub(inserted_total);
        Ok(msg_map)
    }

    /// Pair the staged ids just promoted with the production ids that appeared
    /// above `max_before`, in order, and add them to the map. Returns the new
    /// highest message id, the next chunk's watermark.
    ///
    /// # Errors
    ///
    /// Returns the `mismatch` message when the two counts differ, which would
    /// mean rows were inserted out of staging order or by someone else.
    async fn zip_new_message_ids(
        &mut self,
        msg_map: &mut HashMap<i64, i64>,
        staged: Vec<i64>,
        max_before: i64,
        mismatch: impl FnOnce(usize, usize) -> String,
    ) -> Result<i64> {
        let prod_ids = staging::message_ids_above(self.tx, self.account_id, max_before).await?;
        if staged.len() != prod_ids.len() {
            bail!("{}", mismatch(staged.len(), prod_ids.len()));
        }
        msg_map.extend(staged.into_iter().zip(prod_ids));
        staging::max_message_id(self.tx).await
    }

    /// Insert the staged attachments under their production messages.
    async fn promote_attachments(&mut self) -> Result<()> {
        let phase = Self::begin("bulk-inserting attachments…");
        let promoted = staging::promote_attachments(self.tx).await?;
        self.stats.attachments = promoted.inserted;
        self.done(
            phase,
            format!(
                "attachments done (inserted={} found={})",
                promoted.inserted, promoted.filled
            ),
        );
        Ok(())
    }

    /// Insert the staged tapbacks under their production messages.
    async fn promote_tapbacks(&mut self) -> Result<()> {
        let phase = Self::begin("bulk-inserting tapbacks…");
        self.stats.tapbacks = staging::promote_tapbacks(self.tx).await?;
        self.done(
            phase,
            format!("tapbacks done (inserted={})", self.stats.tapbacks),
        );
        Ok(())
    }

    /// Index the new messages (those above `messages_before`) for full-text
    /// search in one pass, then put the per-row triggers back.
    async fn index_fts(&mut self, messages_before: i64) -> Result<()> {
        let phase = Self::begin("bulk-indexing FTS for new messages…");
        let indexed = schema::index_messages_fts_from_promote_map(self.tx, messages_before).await?;
        if self.engine == DbEngine::Postgres {
            schema::enable_fts_triggers_pg(self.tx).await?;
        } else {
            schema::install_messages_fts_triggers(self.tx).await?;
        }
        self.done(phase, format!("FTS indexed={indexed} (triggers restored)"));
        Ok(())
    }

    /// Fill the dedupe content keys the new rows are missing.
    async fn fill_content_keys(&mut self) -> Result<()> {
        let phase = Self::begin("filling content keys…");
        let keys = crate::dedupe::fill_missing_content_keys(self.tx, self.account_id).await?;
        self.done(phase, format!("content keys filled={keys}"));
        Ok(())
    }

    /// Log the counts and return them. The caller commits.
    fn finish(self) -> PromoteStats {
        let Promote { stats, started, .. } = self;
        promote_log(format_args!(
            "promoted  convs={} parts={} msgs={} atts={} taps={}  (total {:.1}s)",
            stats.conversations,
            stats.participants,
            stats.messages,
            stats.attachments,
            stats.tapbacks,
            started.elapsed().as_secs_f64()
        ));
        stats
    }
}

/// The `(chunk number, lo exclusive, hi inclusive)` id ranges that cover
/// `min_id..=max_id` in batches of [`PROMOTE_MESSAGE_BATCH`].
fn message_chunks(min_id: i64, max_id: i64) -> impl Iterator<Item = (u32, i64, i64)> {
    let mut lo = min_id - 1;
    let mut chunk = 0u32;
    std::iter::from_fn(move || {
        if lo >= max_id {
            return None;
        }
        chunk += 1;
        let hi = (lo + PROMOTE_MESSAGE_BATCH).min(max_id);
        let range = (chunk, lo, hi);
        lo = hi;
        Some(range)
    })
}

/// One promote progress line, flushed so it shows while the next statement runs.
fn promote_log(msg: impl std::fmt::Display) {
    println!("  sql:      promote: {msg}");
    let _ = io::stdout().flush();
}

/// Log the end of one promote phase with its own and the total elapsed time.
fn promote_phase_done(total: Instant, phase: Instant, msg: impl std::fmt::Display) {
    promote_log(format_args!(
        "{msg}  (phase {:.1}s, total {:.1}s)",
        phase.elapsed().as_secs_f64(),
        total.elapsed().as_secs_f64()
    ));
}

/// True when the staged batch is large relative to the table, so dropping and rebuilding
/// the secondary indexes beats maintaining them row by row.
fn should_drop_messages_secondary_indexes(staging_count: i64, existing_count: i64) -> bool {
    staging_count >= PROMOTE_INDEX_DROP_MIN_STAGING
        && staging_count.saturating_mul(5) >= existing_count.max(1)
}

#[cfg(test)]
mod tests;
