//! Contacts and their links to identities and Contact Groups. Loading and
//! writing the address book is in `db::address_book`.

use std::collections::HashMap;

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

/// `handle_id`'s siblings that `contact_id` holds: the same normalized value
/// and handle type on another platform service.
///
/// An import that replaces a trashed contact (ADR-0013) moves these to the
/// fresh contact with the identity it met. One number is one person on every
/// service, the rule [`contact_id_of_sibling_handle`] applies in the other
/// direction, so the fresh contact takes the number on every service rather
/// than leaving half of it Unknown.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn siblings_on_contact(
    conn: &mut SqliteConnection,
    account_id: i64,
    handle_id: i64,
    contact_id: i64,
) -> Result<Vec<i64>> {
    Ok(sqlx::query_scalar(
        "SELECT h2.id
         FROM handles h
         JOIN handles h2
           ON h2.account_id = h.account_id
          AND h2.normalized = h.normalized
          AND h2.handle_type = h.handle_type
          AND h2.id != h.id
         JOIN contact_handles ch
           ON ch.account_id = h2.account_id AND ch.handle_id = h2.id
         WHERE h.id = $2 AND h.account_id = $1 AND ch.contact_id = $3
         ORDER BY h2.id",
    )
    .bind(account_id)
    .bind(handle_id)
    .bind(contact_id)
    .fetch_all(&mut *conn)
    .await?)
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
/// one with no address, or one with addresses but no preferred name. An
/// identity of type `other` is no address: it holds a name the backup gave
/// with no address, so a contact whose only identities are of that type is
/// Unknown however it is named. Membership is computed rather than stored,
/// because a contact stops being Unknown the moment someone names it or
/// links an address to it.
pub const UNKNOWN_CONTACT_SQL: &str = "(
    trim(ct.preferred_name) = ''
    OR NOT EXISTS (
        SELECT 1 FROM contact_handles ch2
        JOIN handles h2 ON h2.id = ch2.handle_id
        WHERE ch2.account_id = ct.account_id AND ch2.contact_id = ct.id
          AND h2.handle_type <> 'other'
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

/// Where an identity goes when it leaves its contact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityGoes {
    /// Onto this contact, the link marked as made by the `Origin`.
    To(i64, Origin),
    /// Off its contact, to wherever the rule sends it: see [`move_identity`].
    Off,
}

/// What [`move_identity`] did with an identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityMoved {
    /// It is on the contact it was given.
    To(i64),
    /// It is in a conversation, so it is on this new contact with no name:
    /// the person is Unknown again.
    NewContact(i64),
    /// Only the account holder's side uses it (a message's owner identity,
    /// one of the account's identities, a group's id), so it is on no
    /// contact, as the holder's identities always are (ADR-0015).
    Unlinked,
    /// Nothing used it, so it is gone.
    Deleted,
}

/// The one way an identity leaves a contact: moved to the contact it is
/// given, or taken off.
///
/// An identity in a conversation is a person, and every person is a contact
/// (`docs/architecture/contacts-identities-and-messages.md`), so one that is
/// taken off goes to a new contact with no name and the person is Unknown
/// again. "In a conversation" means a participant, the sender of a message or
/// a reaction, or a one-to-one conversation's chat handle, in the promoted
/// tables or in staging. An identity nothing refers to is deleted, because on
/// no contact it would appear in no list. One that only the holder's side
/// uses comes off and stays, because the holder's identities are on no
/// contact.
///
/// The identity's sibling on another service (the same number on WhatsApp)
/// is not touched; [`take_identities_off`] keeps siblings it takes off
/// together.
///
/// # Errors
///
/// Returns an error when a statement fails.
pub async fn move_identity(
    conn: &mut SqliteConnection,
    account_id: i64,
    handle_id: i64,
    goes: IdentityGoes,
) -> Result<IdentityMoved> {
    match goes {
        IdentityGoes::To(contact_id, origin) => {
            put_on_contact(conn, account_id, handle_id, contact_id, origin).await?;
            Ok(IdentityMoved::To(contact_id))
        }
        IdentityGoes::Off => match identity_use(conn, account_id, handle_id).await? {
            IdentityUse::Person => {
                let contact_id = create_contact(conn, account_id, "", Origin::Import).await?;
                put_on_contact(conn, account_id, handle_id, contact_id, Origin::Import).await?;
                Ok(IdentityMoved::NewContact(contact_id))
            }
            IdentityUse::Holder => {
                sqlx::query("DELETE FROM contact_handles WHERE account_id = $1 AND handle_id = $2")
                    .bind(account_id)
                    .bind(handle_id)
                    .execute(&mut *conn)
                    .await?;
                Ok(IdentityMoved::Unlinked)
            }
            IdentityUse::Nothing => {
                // The cascade takes the `contact_handles` link with it.
                sqlx::query("DELETE FROM handles WHERE account_id = $1 AND id = $2")
                    .bind(account_id)
                    .bind(handle_id)
                    .execute(&mut *conn)
                    .await?;
                Ok(IdentityMoved::Deleted)
            }
        },
    }
}

/// Take each of `handle_ids` off its contact with [`move_identity`], in
/// order, and answer what happened to each. Siblings among them (one number
/// on two services) that go to a new contact go to the same one, because one
/// number is one person on every service.
///
/// # Errors
///
/// Returns an error when a statement fails.
pub async fn take_identities_off(
    conn: &mut SqliteConnection,
    account_id: i64,
    handle_ids: &[i64],
) -> Result<Vec<IdentityMoved>> {
    let mut new_contact_of: HashMap<(String, String), i64> = HashMap::new();
    let mut out = Vec::with_capacity(handle_ids.len());
    for &handle_id in handle_ids {
        let key: Option<(String, String)> = sqlx::query_as(
            "SELECT normalized, handle_type FROM handles WHERE account_id = $1 AND id = $2",
        )
        .bind(account_id)
        .bind(handle_id)
        .fetch_optional(&mut *conn)
        .await?;
        let Some(key) = key else {
            continue;
        };
        if let Some(&contact_id) = new_contact_of.get(&key) {
            let goes = IdentityGoes::To(contact_id, Origin::Import);
            out.push(move_identity(conn, account_id, handle_id, goes).await?);
            continue;
        }
        let moved = move_identity(conn, account_id, handle_id, IdentityGoes::Off).await?;
        if let IdentityMoved::NewContact(contact_id) = moved {
            new_contact_of.insert(key, contact_id);
        }
        out.push(moved);
    }
    Ok(out)
}

/// True when `contact_id` has no preferred name.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn is_nameless(
    conn: &mut SqliteConnection,
    account_id: i64,
    contact_id: i64,
) -> Result<bool> {
    let nameless: Option<bool> = sqlx::query_scalar(
        "SELECT trim(preferred_name) = '' FROM contacts WHERE account_id = $1 AND id = $2",
    )
    .bind(account_id)
    .bind(contact_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(nameless.unwrap_or(false))
}

/// Delete `contact_id` when it has neither a name nor an identity, which
/// nothing could ever reach, with its trash marker. Returns true when it
/// went.
///
/// # Errors
///
/// Returns an error when a statement fails.
pub async fn delete_if_empty(
    conn: &mut SqliteConnection,
    account_id: i64,
    contact_id: i64,
) -> Result<bool> {
    let deleted = sqlx::query(
        "DELETE FROM contacts
         WHERE account_id = $1 AND id = $2 AND trim(preferred_name) = ''
           AND NOT EXISTS (SELECT 1 FROM contact_handles ch
                           WHERE ch.account_id = $1 AND ch.contact_id = $2)",
    )
    .bind(account_id)
    .bind(contact_id)
    .execute(&mut *conn)
    .await?
    .rows_affected()
        > 0;
    if deleted {
        sqlx::query("DELETE FROM trashed_contacts WHERE account_id = $1 AND contact_id = $2")
            .bind(account_id)
            .bind(contact_id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(deleted)
}

/// The identities `contact_id` holds, in the order they were linked.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn identities_of_contact(
    conn: &mut SqliteConnection,
    account_id: i64,
    contact_id: i64,
) -> Result<Vec<i64>> {
    Ok(sqlx::query_scalar(
        "SELECT handle_id FROM contact_handles
         WHERE account_id = $1 AND contact_id = $2
         ORDER BY rowid",
    )
    .bind(account_id)
    .bind(contact_id)
    .fetch_all(&mut *conn)
    .await?)
}

/// Link `handle_id` to `contact_id`, moving it there when another contact
/// holds it.
async fn put_on_contact(
    conn: &mut SqliteConnection,
    account_id: i64,
    handle_id: i64,
    contact_id: i64,
    origin: Origin,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO contact_handles (account_id, handle_id, contact_id, origin)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (account_id, handle_id)
         DO UPDATE SET contact_id = excluded.contact_id, origin = excluded.origin",
    )
    .bind(account_id)
    .bind(handle_id)
    .bind(contact_id)
    .bind(origin.as_str())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// What refers to an identity, for [`move_identity`] to decide where it goes.
enum IdentityUse {
    /// A person in a conversation: a participant, a sender, or a one-to-one
    /// conversation's chat handle.
    Person,
    /// Only the account holder's side: a message's owner identity, one of
    /// the account's identities, or a group's id.
    Holder,
    /// Nothing.
    Nothing,
}

/// Which of [`IdentityUse`] `handle_id` is. Staging counts as well as the
/// promoted tables, because an import takes identities off a trashed contact
/// while its rows are staged.
async fn identity_use(
    conn: &mut SqliteConnection,
    account_id: i64,
    handle_id: i64,
) -> Result<IdentityUse> {
    let (person, holder): (bool, bool) = sqlx::query_as(
        "SELECT
           EXISTS (SELECT 1 FROM participants WHERE handle_id = $2)
           OR EXISTS (SELECT 1 FROM staging_participants WHERE handle_id = $2)
           OR EXISTS (SELECT 1 FROM messages WHERE account_id = $1 AND sender_handle_id = $2)
           OR EXISTS (SELECT 1 FROM staging_messages
                      WHERE account_id = $1 AND sender_handle_id = $2)
           OR EXISTS (SELECT 1 FROM tapbacks WHERE sender_handle_id = $2)
           OR EXISTS (SELECT 1 FROM staging_tapbacks WHERE sender_handle_id = $2)
           OR EXISTS (SELECT 1 FROM conversations
                      WHERE account_id = $1 AND chat_handle_id = $2
                        AND conversation_type = 'individual' COLLATE NOCASE)
           OR EXISTS (SELECT 1 FROM staging_conversations
                      WHERE account_id = $1 AND chat_handle_id = $2
                        AND conversation_type = 'individual' COLLATE NOCASE),
           EXISTS (SELECT 1 FROM messages WHERE account_id = $1 AND owner_handle_id = $2)
           OR EXISTS (SELECT 1 FROM staging_messages
                      WHERE account_id = $1 AND owner_handle_id = $2)
           OR EXISTS (SELECT 1 FROM account_handles WHERE account_id = $1 AND handle_id = $2)
           OR EXISTS (SELECT 1 FROM conversations WHERE account_id = $1 AND chat_handle_id = $2)
           OR EXISTS (SELECT 1 FROM staging_conversations
                      WHERE account_id = $1 AND chat_handle_id = $2)",
    )
    .bind(account_id)
    .bind(handle_id)
    .fetch_one(&mut *conn)
    .await?;
    Ok(if person {
        IdentityUse::Person
    } else if holder {
        IdentityUse::Holder
    } else {
        IdentityUse::Nothing
    })
}

#[cfg(test)]
mod tests;
