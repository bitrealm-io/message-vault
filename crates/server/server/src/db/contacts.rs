//! Contacts and their links to identities and Contact Groups. Loading and
//! writing the address book is in `db::address_book`.

use anyhow::Result;
use chrono::Utc;
use sqlx::SqliteConnection;

use crate::search::emit::NOT_TRASHED_CONTACT;

pub mod read;

/// Bump `contacts.last_modified` after the contact's name, identities or
/// Contact Groups change.
pub async fn touch_contact(
    conn: &mut SqliteConnection,
    account_id: i64,
    contact_id: i64,
) -> Result<()> {
    let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    sqlx::query("UPDATE contacts SET last_modified = $1 WHERE id = $2 AND account_id = $3")
        .bind(now)
        .bind(contact_id)
        .bind(account_id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Where a contact, identity, or link came from: what made the row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Made by loading an address book.
    AddressBook,
    /// Discovered while importing messages.
    Import,
    /// Created or edited by the person.
    User,
}

impl Origin {
    /// Storage id (`address_book` / `import` / `user`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AddressBook => "address_book",
            Self::Import => "import",
            Self::User => "user",
        }
    }
}

/// Offer `name` as the contact's preferred name, on behalf of `by`. Returns
/// true when the row changed.
///
/// This is the one place that decides who may name a Contact, the rule
/// ADR-0006 sets. Three things name people: an import reading a backup, an
/// address book the person loaded, and the person typing in the contact
/// drawer.
///
/// - The person outranks everything. Typing a name is the most deliberate
///   naming act in the product, so the row becomes theirs (`origin = 'user'`)
///   and no import renames it.
/// - An address book is the person typing in a spreadsheet, so its name
///   replaces whatever the contact carried, an imported name and a typed one
///   alike. A name equal to the one the contact has changes nothing, so a
///   file loaded straight back leaves every contact as it was.
/// - An import names only a contact that has no name. The same number
///   arrives spelled differently across backups and the first spelling is as
///   good as the second.
///
/// An empty `name` says nothing about who someone is, so it never overwrites
/// anything; only [`create_contact`] stores an empty name, for a contact
/// nothing has named yet.
///
/// # Errors
///
/// Returns an error when a statement fails.
pub async fn propose_name(
    conn: &mut SqliteConnection,
    account_id: i64,
    contact_id: i64,
    name: &str,
    by: Origin,
) -> Result<bool> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(false);
    }
    // Each arm is a fixed literal chosen by matching on `by`; the name and the
    // ids are bound. `origin` is only rewritten when the person types the
    // name; an import never renames a contact that has one, whatever its
    // origin.
    let sql = match by {
        Origin::User => {
            "UPDATE contacts SET preferred_name = $1, origin = 'user'
             WHERE account_id = $2 AND id = $3"
        }
        Origin::AddressBook => {
            "UPDATE contacts SET preferred_name = $1
             WHERE account_id = $2 AND id = $3 AND trim(preferred_name) <> $1"
        }
        Origin::Import => {
            "UPDATE contacts SET preferred_name = $1
             WHERE account_id = $2 AND id = $3
               AND origin = 'import' AND trim(preferred_name) = ''"
        }
    };
    let changed = sqlx::query(sql)
        .bind(name)
        .bind(account_id)
        .bind(contact_id)
        .execute(&mut *conn)
        .await?
        .rows_affected()
        > 0;
    if changed {
        touch_contact(conn, account_id, contact_id).await?;
    }
    Ok(changed)
}

/// Create a contact carrying `preferred_name`, which may be empty.
///
/// # Errors
///
/// Returns an error when the insert fails.
pub async fn create_contact(
    conn: &mut SqliteConnection,
    account_id: i64,
    preferred_name: &str,
    origin: Origin,
) -> Result<i64> {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO contacts (account_id, preferred_name, origin) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(account_id)
    .bind(preferred_name)
    .bind(origin.as_str())
    .fetch_one(&mut *conn)
    .await?;
    Ok(id)
}

/// Link `handle_id` to `contact_id`, doing nothing when the link exists.
/// Returns true when the link was made here, false when it already existed —
/// the difference a caller needs to decide whether the contact changed.
///
/// # Errors
///
/// Returns an error when the insert fails.
pub async fn link_handle_to_contact(
    conn: &mut SqliteConnection,
    account_id: i64,
    handle_id: i64,
    contact_id: i64,
    origin: Origin,
) -> Result<bool> {
    let inserted = sqlx::query(
        "INSERT INTO contact_handles (account_id, handle_id, contact_id, origin)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT DO NOTHING",
    )
    .bind(account_id)
    .bind(handle_id)
    .bind(contact_id)
    .bind(origin.as_str())
    .execute(&mut *conn)
    .await?
    .rows_affected();
    Ok(inserted > 0)
}

/// Link every sibling of `handle_id` that belongs to no contact — the same
/// normalized value and handle type on another platform service — to
/// `contact_id`. Returns how many links were made.
///
/// A handle is only ever unowned beside a linked sibling after an import
/// discarded a trashed contact (ADR-0013). One number is one person on every
/// service, the rule [`contact_id_of_sibling_handle`] applies in the other
/// direction, so the fresh contact takes the number on every service the
/// server has met it on rather than leaving half of it Unknown.
///
/// # Errors
///
/// Returns an error when the insert fails.
pub async fn link_sibling_handles_to_contact(
    conn: &mut SqliteConnection,
    account_id: i64,
    handle_id: i64,
    contact_id: i64,
) -> Result<u64> {
    let inserted = sqlx::query(
        "INSERT INTO contact_handles (account_id, handle_id, contact_id, origin)
         SELECT h2.account_id, h2.id, $3, $4
         FROM handles h
         JOIN handles h2
           ON h2.account_id = h.account_id
          AND h2.normalized = h.normalized
          AND h2.handle_type = h.handle_type
          AND h2.id != h.id
         WHERE h.id = $2 AND h.account_id = $1
           AND NOT EXISTS (
               SELECT 1 FROM contact_handles ch
               WHERE ch.account_id = h2.account_id AND ch.handle_id = h2.id
           )",
    )
    .bind(account_id)
    .bind(handle_id)
    .bind(contact_id)
    .bind(Origin::Import.as_str())
    .execute(&mut *conn)
    .await?
    .rows_affected();
    Ok(inserted)
}

/// The contact a sibling of `handle_id` is on, if any: the same normalized
/// value and handle type on a different platform service, already linked.
///
/// One person's phone number arrives once as a text-message address
/// (iMessage and SMS share the service `phone`) and again as a WhatsApp one;
/// they are two `handles` rows and one person, so a link made for either is
/// the answer for both.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn contact_id_of_sibling_handle(
    conn: &mut SqliteConnection,
    account_id: i64,
    handle_id: i64,
) -> Result<Option<i64>> {
    let contact_id: Option<i64> = sqlx::query_scalar(
        "SELECT ch.contact_id
         FROM handles h
         JOIN handles h2
           ON h2.account_id = h.account_id
          AND h2.normalized = h.normalized
          AND h2.handle_type = h.handle_type
          AND h2.id != h.id
         JOIN contact_handles ch
           ON ch.account_id = h.account_id AND ch.handle_id = h2.id
         WHERE h.id = $1 AND h.account_id = $2
         LIMIT 1",
    )
    .bind(handle_id)
    .bind(account_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(contact_id)
}

/// The contact this account already has under exactly `name`, if any.
///
/// Used to resolve a participant the source named without recording any
/// address: a unique match binds to that contact instead of creating a second
/// row for the same person.
///
/// A contact in the Trash is left out, as it is from the rest of an import
/// (ADR 0013). The same run may discard it on meeting one of its identities,
/// and a participant bound to it would then point at a deleted id. A trashed
/// contact that shares a live contact's name would also make the name look
/// ambiguous.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn contact_id_by_preferred_name(
    conn: &mut SqliteConnection,
    account_id: i64,
    name: &str,
) -> Result<Option<i64>> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(None);
    }
    // Two contacts sharing a name is ambiguous, and choosing between them
    // would silently merge different people. Leave that for the person.
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT c.id FROM contacts c
         WHERE c.account_id = $1 AND lower(trim(c.preferred_name)) = lower($2)
           AND NOT EXISTS (SELECT 1 FROM trashed_contacts t
                           WHERE t.account_id = c.account_id AND t.contact_id = c.id)
         LIMIT 2",
    )
    .bind(account_id)
    .bind(name)
    .fetch_all(&mut *conn)
    .await?;
    match ids.as_slice() {
        [id] => Ok(Some(*id)),
        _ => Ok(None),
    }
}

/// Create an empty Contact Group for one Import Run and answer its id and
/// name. The name is `base`, or `base` with " 2", " 3", … when the account
/// already has a group under that name in any case, the same rule the run's
/// Saved Search follows. The group is always a new row: an import never adds
/// to, or re-marks, a group an earlier run or a person made.
///
/// The insert itself claims the name, through
/// [`crate::db::free_name::insert_under_free_name`], which the run's Saved
/// Search goes through too.
///
/// # Errors
///
/// Returns an error when an insert fails, or when all 999 names are taken.
pub async fn create_import_group(
    conn: &mut SqliteConnection,
    account_id: i64,
    base: &str,
) -> Result<(i64, String)> {
    let claimed = crate::db::free_name::insert_under_free_name(
        conn,
        "contact_groups",
        account_id,
        base,
        &[("kind", "import")],
    )
    .await?;
    match claimed {
        Some(claimed) => Ok(claimed),
        None => anyhow::bail!("too many Contact Groups named like {base:?}"),
    }
}

/// SQL predicate selecting the Unknown contacts of alias `ct`.
///
/// Unknown is a contact missing either half of what makes a contact useful:
/// one with no identity at all, or one with identities but no preferred name.
/// Membership is computed rather than stored, because a contact stops being
/// Unknown the moment someone names it or links an identity to it.
pub const UNKNOWN_CONTACT_SQL: &str = "(
    trim(ct.preferred_name) = ''
    OR NOT EXISTS (
        SELECT 1 FROM contact_handles ch2
        WHERE ch2.account_id = ct.account_id AND ch2.contact_id = ct.id
    )
)";

/// Contact linked to a handle via `contact_handles`, if any.
pub async fn contact_id_for_handle(
    conn: &mut SqliteConnection,
    account_id: i64,
    handle_id: i64,
) -> Result<Option<i64>> {
    let found: Option<i64> = sqlx::query_scalar(
        "SELECT contact_id FROM contact_handles WHERE account_id = $1 AND handle_id = $2",
    )
    .bind(account_id)
    .bind(handle_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(found)
}

/// True when the contact belongs to this account and is not in the trash.
///
/// # Errors
///
/// Returns an error when the statement fails.
pub async fn live_contact_exists(
    conn: &mut SqliteConnection,
    account_id: i64,
    contact_id: i64,
) -> Result<bool> {
    let found: Option<i64> = sqlx::query_scalar(&format!(
        "SELECT ct.id
         FROM contacts ct
         WHERE ct.id = $1 AND ct.account_id = $2
           AND {NOT_TRASHED_CONTACT}",
    ))
    .bind(contact_id)
    .bind(account_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(found.is_some())
}

/// Id and service of the handle row for `raw` that is linked to this
/// contact, if any. With a service, only that service's row; without one,
/// the phone row first, then WhatsApp, then anything else.
///
/// # Errors
///
/// Returns an error when the statement fails.
pub async fn linked_handle_id(
    conn: &mut SqliteConnection,
    account_id: i64,
    contact_id: i64,
    raw: &str,
    service: Option<&str>,
) -> Result<Option<(i64, message_ir::HandleService)>> {
    let needle = raw.trim();
    if needle.is_empty() {
        return Ok(None);
    }
    let mut sql = String::from(
        "SELECT ch.handle_id, h.service
         FROM contact_handles ch
         JOIN handles h ON h.id = ch.handle_id
         WHERE ch.account_id = $1 AND ch.contact_id = $2
           AND (h.raw = $3 OR h.normalized = $3)",
    );
    let row = if let Some(svc) = service.and_then(message_ir::trimmed) {
        sql.push_str(" AND h.service = $4 LIMIT 1");
        let platform = message_ir::HandleService::parse(svc);
        sqlx::query_as::<_, (i64, String)>(&sql)
            .bind(account_id)
            .bind(contact_id)
            .bind(needle)
            .bind(platform.as_str())
            .fetch_optional(&mut *conn)
            .await?
    } else {
        sql.push_str(
            " ORDER BY CASE h.service WHEN 'phone' THEN 0 WHEN 'whatsapp' THEN 1 ELSE 2 END
             LIMIT 1",
        );
        sqlx::query_as::<_, (i64, String)>(&sql)
            .bind(account_id)
            .bind(contact_id)
            .bind(needle)
            .fetch_optional(&mut *conn)
            .await?
    };
    Ok(row.map(|(id, service)| (id, message_ir::HandleService::parse(&service))))
}

/// Point the contact's link at `new_handle_id` in place of `old_handle_id`.
///
/// # Errors
///
/// Returns an error when the statement fails.
pub async fn relink_handle(
    conn: &mut SqliteConnection,
    account_id: i64,
    contact_id: i64,
    old_handle_id: i64,
    new_handle_id: i64,
) -> Result<()> {
    sqlx::query(
        "UPDATE contact_handles SET handle_id = $1
         WHERE account_id = $2 AND contact_id = $3 AND handle_id = $4",
    )
    .bind(new_handle_id)
    .bind(account_id)
    .bind(contact_id)
    .bind(old_handle_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Drop the link between the contact and the handle. The handle row itself
/// stays: messages still cite it.
///
/// # Errors
///
/// Returns an error when the statement fails.
pub async fn unlink_handle(
    conn: &mut SqliteConnection,
    account_id: i64,
    contact_id: i64,
    handle_id: i64,
) -> Result<()> {
    sqlx::query(
        "DELETE FROM contact_handles
         WHERE account_id = $1 AND contact_id = $2 AND handle_id = $3",
    )
    .bind(account_id)
    .bind(contact_id)
    .bind(handle_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests;
