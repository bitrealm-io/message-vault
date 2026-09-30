//! The trash marker tables: `trashed_conversations` and `trashed_contacts`.
//!
//! A row in either table is a soft-delete flag; the conversation or contact
//! it points at is untouched until [`delete_trashed`] or [`empty_trash`]
//! removes it for good (or [`purge_account`] and account deletion clear the
//! markers along with everything else). Both marker operations first check
//! that the target row belongs to `account_id` — explicitly, through
//! [`crate::db::ownership`], rather than inferring ownership from whether an
//! insert or delete affected a row. That is what lets [`restore`] tell "not
//! this account's id" (`false`) apart from "this account's id, and it was not
//! trashed" (`true`, a no-op): a `DELETE` that matches zero rows looks
//! identical in both cases.
//!
//! Trashing something already trashed, and restoring something not trashed,
//! both return `true` — these operations are idempotent, per the HTTP routes
//! built on top of them.
//!
//! A conversation and a contact are trashed the same way, so one pair of
//! functions covers both and [`Trashable`] carries which is meant. The marker
//! table and its id column come from that enum and nowhere else, so no part
//! of a request reaches the SQL text.
//!
//! # Permanent deletion
//!
//! Trash is the only door to permanent deletion (CONTEXT.md, "Trash").
//! [`delete_trashed`] refuses a row that is not in the trash, so nothing can
//! be destroyed without having been set aside first, and both it and
//! [`empty_trash`] run inside one transaction so a failure part-way leaves
//! the vault as it was.
//!
//! A conversation is deleted outright: the row goes and the schema's cascades
//! take its messages, attachments, tapbacks, participants and tag memberships
//! with it. A contact is not deleted: deleting a trashed contact does what a
//! phone's Delete Contact does — the name and the person's edits go, the row
//! stays as an Unknown contact holding the same handles, and every
//! conversation it was in is untouched and now shows the handle. The two are
//! still one operation to the caller ("Delete" on a trash row), so they share
//! [`delete_trashed`].
//!
//! An import is the other way out of the trash. When it meets a handle that
//! belongs to a trashed contact, [`discard_contact_if_trashed`] deletes that
//! contact outright — row, handle links, Contact Group memberships and
//! marker — and the import makes a fresh contact from the backup, as a first
//! import would (ADR-0013). A backup that still holds the person is the
//! person saying they still talk to them.

use sqlx::{AnyConnection, Connection};

use crate::db::ownership::{owns_contact, owns_conversation};
use crate::db::sql::{SQLITE_IN_CHUNK, in_placeholders};

/// A thing that can be put in the trash, named by id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trashable {
    /// A row of `conversations`, marked in `trashed_conversations`.
    Conversation(i64),
    /// A row of `contacts`, marked in `trashed_contacts`.
    Contact(i64),
}

impl Trashable {
    /// The row id, whichever kind this is.
    fn id(self) -> i64 {
        match self {
            Self::Conversation(id) | Self::Contact(id) => id,
        }
    }

    /// The marker table and the column in it that carries the row id.
    fn marker(self) -> (&'static str, &'static str) {
        match self {
            Self::Conversation(_) => ("trashed_conversations", "conversation_id"),
            Self::Contact(_) => ("trashed_contacts", "contact_id"),
        }
    }

    /// True when `account_id` owns the row this names.
    async fn is_owned(
        self,
        conn: &mut AnyConnection,
        account_id: i64,
    ) -> Result<bool, sqlx::Error> {
        match self {
            Self::Conversation(id) => owns_conversation(conn, account_id, id).await,
            Self::Contact(id) => owns_contact(conn, account_id, id).await,
        }
    }

    /// True when this row carries a trash marker for `account_id`.
    async fn is_trashed(
        self,
        conn: &mut AnyConnection,
        account_id: i64,
    ) -> Result<bool, sqlx::Error> {
        let (table, id_column) = self.marker();
        let found: Option<i64> = sqlx::query_scalar(&format!(
            "SELECT 1 FROM {table} WHERE account_id = $1 AND {id_column} = $2"
        ))
        .bind(account_id)
        .bind(self.id())
        .fetch_optional(&mut *conn)
        .await?;
        Ok(found.is_some())
    }
}

/// Mark a conversation or contact trashed. Returns `false` when the id is not
/// `account_id`'s; `true` (a no-op) when it is already trashed.
///
/// # Errors
///
/// Returns a database error when a statement fails.
pub async fn move_to_trash(
    conn: &mut AnyConnection,
    account_id: i64,
    target: Trashable,
) -> Result<bool, sqlx::Error> {
    if !target.is_owned(conn, account_id).await? {
        return Ok(false);
    }
    let (table, id_column) = target.marker();
    sqlx::query(&format!(
        "INSERT INTO {table} (account_id, {id_column}) VALUES ($1, $2)
         ON CONFLICT DO NOTHING"
    ))
    .bind(account_id)
    .bind(target.id())
    .execute(&mut *conn)
    .await?;
    Ok(true)
}

/// Remove a conversation's or contact's trash marker, if any. Returns `false`
/// when the id is not `account_id`'s; `true` (a no-op) when it was not
/// trashed.
///
/// # Errors
///
/// Returns a database error when a statement fails.
pub async fn restore(
    conn: &mut AnyConnection,
    account_id: i64,
    target: Trashable,
) -> Result<bool, sqlx::Error> {
    if !target.is_owned(conn, account_id).await? {
        return Ok(false);
    }
    let (table, id_column) = target.marker();
    sqlx::query(&format!(
        "DELETE FROM {table} WHERE account_id = $1 AND {id_column} = $2"
    ))
    .bind(account_id)
    .bind(target.id())
    .execute(&mut *conn)
    .await?;
    Ok(true)
}

/// Delete `contact_id` for good when it is `account_id`'s trashed contact,
/// and do nothing when it is not trashed. Returns true when the contact was
/// trashed and is now gone.
///
/// Unlike [`delete_trashed`], which forgets a contact and keeps its row, this
/// deletes the row: the schema's cascades take its handle links and Contact
/// Group memberships with it, so every handle it had belongs to no contact
/// afterwards, and a participant the source named without an address loses
/// its contact. The marker goes with the row. This is what an import does
/// when the backup still holds someone the person set aside (ADR-0013); the
/// import then makes a fresh contact, which is why nothing here creates one.
///
/// # Errors
///
/// Returns a database error when a statement fails.
pub async fn discard_contact_if_trashed(
    conn: &mut AnyConnection,
    account_id: i64,
    contact_id: i64,
) -> Result<bool, sqlx::Error> {
    let target = Trashable::Contact(contact_id);
    if !target.is_trashed(conn, account_id).await? {
        return Ok(false);
    }
    sqlx::query("DELETE FROM contacts WHERE account_id = $1 AND id = $2")
        .bind(account_id)
        .bind(contact_id)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM trashed_contacts WHERE account_id = $1 AND contact_id = $2")
        .bind(account_id)
        .bind(contact_id)
        .execute(&mut *conn)
        .await?;
    Ok(true)
}

/// Remove every trash marker `account_id` holds. Called when an account's
/// conversations (and, by extension, whatever they trashed) are purged, and
/// at the end of [`empty_trash`].
///
/// # Errors
///
/// Returns a database error when a statement fails.
pub async fn purge_account(conn: &mut AnyConnection, account_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM trashed_conversations WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM trashed_contacts WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// What [`delete_trashed`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteOutcome {
    /// The row was deleted (a conversation) or made Unknown (a contact). The
    /// files are those no remaining message references, for the caller to
    /// remove from disk once the transaction has committed.
    Deleted(Vec<OrphanedFile>),
    /// The id is not `account_id`'s, or does not exist: a 404, the same
    /// answer [`move_to_trash`] gives.
    NotOwned,
    /// The row is this account's but is not in the trash: a 409. Only a
    /// trashed row can be deleted.
    NotTrashed,
}

/// A stored attachment file that no remaining message references.
///
/// Attachments are stored by content hash under
/// `data_dir/<account>/<source>/<assets_dir>/`, and several messages — in one
/// conversation or across many — can point at the same file. A file is
/// therefore reported here only after the delete has run and a lookup for the
/// same `(source, sha256)` finds no attachment left, in the promoted tables or
/// in staging. Paths are relative to the per-source directory that `source`
/// names; the caller joins them.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum OrphanedFile {
    /// The original bytes: `assets_path` under the source's assets directory,
    /// plus the `.<sha256>.mime` sidecar beside it when one was written.
    Original {
        source: String,
        sha256: String,
        assets_path: String,
    },
    /// A browser derivative: `assets_path` under the source's converted
    /// directory.
    Derived { source: String, assets_path: String },
}

/// Permanently delete a trashed conversation, or make a trashed contact
/// Unknown again. See the module notes for what each means.
///
/// # Errors
///
/// Returns a database error when a statement fails; the transaction is then
/// rolled back.
pub async fn delete_trashed(
    conn: &mut AnyConnection,
    account_id: i64,
    target: Trashable,
) -> Result<DeleteOutcome, sqlx::Error> {
    if !target.is_owned(conn, account_id).await? {
        return Ok(DeleteOutcome::NotOwned);
    }
    if !target.is_trashed(conn, account_id).await? {
        return Ok(DeleteOutcome::NotTrashed);
    }
    let mut tx = conn.begin().await?;
    let orphaned = match target {
        Trashable::Conversation(id) => delete_conversations(&mut tx, account_id, &[id]).await?,
        Trashable::Contact(id) => {
            forget_contacts(&mut tx, account_id, &[id]).await?;
            Vec::new()
        }
    };
    tx.commit().await?;
    Ok(DeleteOutcome::Deleted(orphaned))
}

/// Empty the trash: delete every trashed conversation permanently and make
/// every trashed contact Unknown, then clear both marker tables. One
/// transaction, so the trash is either emptied or untouched.
///
/// # Errors
///
/// Returns a database error when a statement fails; the transaction is then
/// rolled back.
pub async fn empty_trash(
    conn: &mut AnyConnection,
    account_id: i64,
) -> Result<Vec<OrphanedFile>, sqlx::Error> {
    let mut tx = conn.begin().await?;
    let conversation_ids: Vec<i64> = sqlx::query_scalar(
        "SELECT t.conversation_id
         FROM trashed_conversations t
         JOIN conversations c ON c.id = t.conversation_id AND c.account_id = t.account_id
         WHERE t.account_id = $1
         ORDER BY t.conversation_id",
    )
    .bind(account_id)
    .fetch_all(&mut *tx)
    .await?;
    let contact_ids: Vec<i64> = sqlx::query_scalar(
        "SELECT t.contact_id
         FROM trashed_contacts t
         JOIN contacts ct ON ct.id = t.contact_id AND ct.account_id = t.account_id
         WHERE t.account_id = $1
         ORDER BY t.contact_id",
    )
    .bind(account_id)
    .fetch_all(&mut *tx)
    .await?;
    let orphaned = delete_conversations(&mut tx, account_id, &conversation_ids).await?;
    forget_contacts(&mut tx, account_id, &contact_ids).await?;
    // The two calls above cleared the markers for the rows they found; this
    // also drops any marker whose row is already gone, which the marker
    // tables allow because neither carries a foreign key to its target.
    purge_account(&mut tx, account_id).await?;
    tx.commit().await?;
    Ok(orphaned)
}

/// One attachment's stored files, read before its message is deleted so the
/// reference check afterwards knows what to look for: source, sha256,
/// assets_path, derived_sha256, derived_assets_path.
type AttachmentFilesRow = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Delete `ids`, which the caller has already established are `account_id`'s
/// trashed conversations, and report the attachment files nothing references
/// any more. The schema's cascades remove messages, attachments, tapbacks,
/// participants and tag memberships; `duplicate_of` on a message elsewhere
/// that pointed at one of these is set NULL, so the surviving copy becomes
/// the one that shows.
async fn delete_conversations(
    conn: &mut AnyConnection,
    account_id: i64,
    ids: &[i64],
) -> Result<Vec<OrphanedFile>, sqlx::Error> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut candidates: Vec<AttachmentFilesRow> = Vec::new();
    for chunk in ids.chunks(SQLITE_IN_CHUNK) {
        let placeholders = in_placeholders(1, chunk.len());
        let sql = format!(
            "SELECT DISTINCT m.source, a.sha256, a.assets_path,
                    a.derived_sha256, a.derived_assets_path
             FROM attachments a
             JOIN messages m ON m.id = a.message_id
             WHERE m.conversation_id IN ({placeholders})
               AND (a.sha256 IS NOT NULL OR a.derived_sha256 IS NOT NULL)"
        );
        let mut q = sqlx::query_as::<_, AttachmentFilesRow>(&sql);
        for id in chunk {
            q = q.bind(*id);
        }
        candidates.extend(q.fetch_all(&mut *conn).await?);
    }
    for chunk in ids.chunks(SQLITE_IN_CHUNK) {
        let placeholders = in_placeholders(2, chunk.len());
        for (table, id_column) in [
            ("conversations", "id"),
            ("trashed_conversations", "conversation_id"),
        ] {
            let sql = format!(
                "DELETE FROM {table} WHERE account_id = $1 AND {id_column} IN ({placeholders})"
            );
            let mut q = sqlx::query(&sql).bind(account_id);
            for id in chunk {
                q = q.bind(*id);
            }
            q.execute(&mut *conn).await?;
        }
    }
    orphaned_files(conn, account_id, candidates).await
}

/// The files among `candidates` that no attachment of `account_id` points at
/// any more, checked one `(source, sha256)` at a time against the promoted
/// and the staging attachment tables. Sorted and de-duplicated, so two
/// deleted messages sharing one file report it once.
async fn orphaned_files(
    conn: &mut AnyConnection,
    account_id: i64,
    candidates: Vec<AttachmentFilesRow>,
) -> Result<Vec<OrphanedFile>, sqlx::Error> {
    let mut out = Vec::new();
    for (source, sha256, assets_path, derived_sha256, derived_assets_path) in candidates {
        if let (Some(sha256), Some(assets_path)) = (sha256, assets_path)
            && !asset_is_referenced(conn, account_id, &source, "sha256", &sha256).await?
        {
            out.push(OrphanedFile::Original {
                source: source.clone(),
                sha256,
                assets_path,
            });
        }
        if let (Some(derived_sha256), Some(assets_path)) = (derived_sha256, derived_assets_path)
            && !asset_is_referenced(conn, account_id, &source, "derived_sha256", &derived_sha256)
                .await?
        {
            out.push(OrphanedFile::Derived {
                source,
                assets_path,
            });
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// True when any attachment of `account_id` from `source`, promoted or in
/// staging, still carries `sha256` in `column` — `sha256` or
/// `derived_sha256`, a literal chosen by the caller. Staging is included so
/// an import that has already uploaded a file it is about to promote does
/// not lose it.
async fn asset_is_referenced(
    conn: &mut AnyConnection,
    account_id: i64,
    source: &str,
    column: &'static str,
    sha256: &str,
) -> Result<bool, sqlx::Error> {
    let sql = format!(
        "SELECT 1 FROM attachments a
         JOIN messages m ON m.id = a.message_id
         WHERE m.account_id = $1 AND m.source = $2 AND a.{column} = $3
         UNION ALL
         SELECT 1 FROM staging_attachments sa
         JOIN staging_messages sm ON sm.id = sa.message_id
         WHERE sm.account_id = $1 AND sm.source = $2 AND sa.{column} = $3
         LIMIT 1"
    );
    let found: Option<i64> = sqlx::query_scalar(&sql)
        .bind(account_id)
        .bind(source)
        .bind(sha256)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(found.is_some())
}

/// Make `ids`, which the caller has already established are `account_id`'s
/// trashed contacts, Unknown again: the name goes, the row returns to an
/// import's ownership so the next import that meets one of its handles may
/// name it, its Contact Group memberships go, and its trash marker goes. The
/// handles stay linked, so the conversations the person was in keep showing
/// them as one participant — by handle now, since the name is blank.
async fn forget_contacts(
    conn: &mut AnyConnection,
    account_id: i64,
    ids: &[i64],
) -> Result<(), sqlx::Error> {
    if ids.is_empty() {
        return Ok(());
    }
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    for chunk in ids.chunks(SQLITE_IN_CHUNK) {
        let placeholders = in_placeholders(3, chunk.len());
        let sql = format!(
            "UPDATE contacts SET preferred_name = '', origin = 'import', last_modified = $2
             WHERE account_id = $1 AND id IN ({placeholders})"
        );
        let mut q = sqlx::query(&sql).bind(account_id).bind(&now);
        for id in chunk {
            q = q.bind(*id);
        }
        q.execute(&mut *conn).await?;

        let placeholders = in_placeholders(2, chunk.len());
        let sql = format!(
            "DELETE FROM contact_group_members
             WHERE contact_id IN (
                 SELECT id FROM contacts WHERE account_id = $1 AND id IN ({placeholders})
             )"
        );
        let mut q = sqlx::query(&sql).bind(account_id);
        for id in chunk {
            q = q.bind(*id);
        }
        q.execute(&mut *conn).await?;

        let sql = format!(
            "DELETE FROM trashed_contacts WHERE account_id = $1 AND contact_id IN ({placeholders})"
        );
        let mut q = sqlx::query(&sql).bind(account_id);
        for id in chunk {
            q = q.bind(*id);
        }
        q.execute(&mut *conn).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
