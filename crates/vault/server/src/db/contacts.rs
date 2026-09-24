//! Address book loading (VCF or vCard CSV) and contact/group/handle links.

use std::path::Path;

use anyhow::{Context, Result};
use chrono::Utc;
use contacts::{ContactsFormat, detect_contacts_format, parse_vcf, read_vcard_csv_rows};
use sqlx::{AnyConnection, Connection};

/// Bump `contacts.last_modified` after an address-book shape change.
pub async fn touch_contact(
    conn: &mut AnyConnection,
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

/// Where a contact, identity, or link came from.
///
/// Loading an address book touches only the rows the address book owns, so
/// identities an import discovered and names a person typed both survive a
/// refresh of the address book.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Loaded from an address book file the person supplied.
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
/// ADR-0006 sets. Three things name people — an import reading a backup, an
/// address book the person loaded, and the person typing in the contact
/// drawer — and each used to carry its own copy of the rule, in three files,
/// with three comments explaining it from three angles.
///
/// The rule, in one place:
///
/// - The person outranks everything. Typing a name is the most deliberate
///   naming act in the product, so the row becomes theirs (`origin = 'user'`)
///   and nothing later renames it.
/// - An address book outranks an import, because the person curated it, but
///   it does not touch a name they typed.
/// - An import names only a contact no earlier import managed to name. The
///   same number arrives spelled differently across backups and the first
///   spelling is as good as the second.
///
/// An empty `name` says nothing about who someone is, so it never overwrites
/// anything; only [`create_contact`] stores an empty name, for a contact
/// nothing has named yet.
///
/// # Errors
///
/// Returns an error when a statement fails.
pub async fn propose_name(
    conn: &mut AnyConnection,
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
    // ids are bound. `origin` is only rewritten when the person names the row,
    // because that is the only claim that outranks a later address book.
    let sql = match by {
        Origin::User => {
            "UPDATE contacts SET preferred_name = $1, origin = 'user'
             WHERE account_id = $2 AND id = $3"
        }
        Origin::AddressBook => {
            "UPDATE contacts SET preferred_name = $1
             WHERE account_id = $2 AND id = $3 AND origin = 'import'"
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
    conn: &mut AnyConnection,
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
    conn: &mut AnyConnection,
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
/// vault has met it on rather than leaving half of it Unknown.
///
/// # Errors
///
/// Returns an error when the insert fails.
pub async fn link_sibling_handles_to_contact(
    conn: &mut AnyConnection,
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
/// One person's phone number arrives once as an iMessage address and again as
/// an SMS one; they are two `handles` rows and one person, so a link made for
/// either is the answer for both.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn contact_id_of_sibling_handle(
    conn: &mut AnyConnection,
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
/// # Errors
///
/// Returns an error when the query fails.
pub async fn contact_id_by_preferred_name(
    conn: &mut AnyConnection,
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
        "SELECT id FROM contacts
         WHERE account_id = $1 AND lower(trim(preferred_name)) = lower($2)
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

/// Record how a Contact Group was born.
///
/// # Errors
///
/// Returns an error when the update fails.
pub async fn set_group_kind(
    conn: &mut AnyConnection,
    account_id: i64,
    name: &str,
    kind: &str,
) -> Result<()> {
    sqlx::query("UPDATE contact_groups SET kind = $1 WHERE account_id = $2 AND name = $3")
        .bind(kind)
        .bind(account_id)
        .bind(name)
        .execute(&mut *conn)
        .await?;
    Ok(())
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
    conn: &mut AnyConnection,
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

/// Counts from loading an address book into the vault.
#[derive(Debug, Default)]
pub struct ContactLoadStats {
    /// Contacts the book supplied: one per card, whether the card joined a
    /// contact the vault already had or created a new one.
    pub contacts: u64,
    /// Phone handles linked to contacts.
    pub phones: u64,
    /// True when loading was skipped (contacts already loaded and not forced).
    pub skipped: bool,
    /// Phone handles written with a review note (ambiguous normalized form).
    pub phones_needing_review: u64,
}

#[derive(Debug)]
struct ContactDraft {
    /// (normalized handle, review note when the value is ambiguous).
    phones: Vec<(String, Option<String>)>,
    preferred_name: Option<String>,
}

/// Whether the address book is VCF or vCard CSV, judged by its content.
fn contacts_file_format(path: &Path) -> Result<ContactsFormat> {
    Ok(detect_contacts_format(path)?)
}

/// iMessage-style: any handle containing `@` is treated as email.
fn is_email_handle(handle: &str) -> bool {
    handle.contains('@')
}

/// Raw phone → (normalized, review note) under the guarded policy: E.164 when
/// the raw is unambiguous (`+`-prefixed, or a US national number), else
/// digits-as-is plus a reason — never a fabricated `+0…` value.
fn normalize_phone_guarded(num: &str) -> Option<(String, Option<String>)> {
    let trimmed = num.trim();
    if trimmed.is_empty() || trimmed.contains('@') {
        return None;
    }
    // No usable digits (e.g. a bare `+`): not a phone at all.
    phone::sanitize_number(trimmed)?;
    let guarded = phone::normalize_guarded(trimmed, phone::PhoneRegion::for_raw(trimmed));
    Some((guarded.normalized, guarded.note))
}

/// Phone handles from an address-book row as (normalized, review note) pairs;
/// emails are dropped.
fn phone_handles_only(handles: &[String]) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    for h in handles {
        if is_email_handle(h) {
            continue;
        }
        let Some((normalized, note)) = normalize_phone_guarded(h) else {
            continue;
        };
        if !out.iter().any(|(p, _)| p == &normalized) {
            out.push((normalized, note));
        }
    }
    out
}

/// Load contacts from an address book when the account table is empty or when
/// `overwrite` is true.
///
/// Accepted files: **VCF**, or **vCard CSV** (First Name, Last Name, Phone
/// columns — a contacts app VCF exported as CSV).
///
/// Pass `None` to skip address-book load (keep existing SQLite contacts).
/// On overwrite, the book's own contacts are updated in place: a card that
/// matches one keeps that row, and only the contacts the file dropped go. A
/// file that does not exist counts as an empty book.
pub async fn load_contacts_if_needed(
    conn: &mut AnyConnection,
    contacts_path: Option<&Path>,
    overwrite: bool,
    account_id: i64,
) -> Result<ContactLoadStats> {
    crate::db::schema::ensure_vault_schema(conn).await?;
    crate::db::account_profile::ensure_account_row(conn, account_id).await?;

    let Some(path) = contacts_path else {
        return Ok(ContactLoadStats {
            skipped: true,
            ..Default::default()
        });
    };

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contacts WHERE account_id = $1")
        .bind(account_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap_or(0);

    if count > 0 && !overwrite {
        return Ok(ContactLoadStats {
            skipped: true,
            ..Default::default()
        });
    }

    let book = if path.exists() {
        read_address_book(path)?
    } else {
        eprintln!(
            "warning: contacts file not found at {}; leaving contacts empty",
            path.display()
        );
        AddressBook(Vec::new())
    };

    replace_address_book(conn, account_id, book).await
}

/// An address book read from a file, not yet written to any account.
pub struct AddressBook(Vec<ContactDraft>);

/// Read a VCF or vCard CSV address book.
///
/// # Errors
///
/// Fails when the file is neither format or a row cannot be read. Reading
/// touches no database, so a failure here is always the file's own fault.
pub fn read_address_book(path: &Path) -> Result<AddressBook> {
    Ok(AddressBook(match contacts_file_format(path)? {
        ContactsFormat::VcardCsv => drafts_from_vcard_csv(path)?,
        ContactsFormat::Vcf => drafts_from_vcf(path)?,
    }))
}

/// Write an address book's contacts over the account's book rows: a card that
/// matches one keeps that row, and only the contacts the book dropped go.
pub async fn replace_address_book(
    conn: &mut AnyConnection,
    account_id: i64,
    book: AddressBook,
) -> Result<ContactLoadStats> {
    apply_address_book(conn, account_id, book.0).await
}

/// Drafts from a vCard CSV export (First Name, Last Name, Phone columns).
fn drafts_from_vcard_csv(csv_path: &Path) -> Result<Vec<ContactDraft>> {
    let rows = read_vcard_csv_rows(csv_path)
        .with_context(|| format!("failed to read contacts CSV {}", csv_path.display()))?;
    let mut drafts = Vec::new();
    for row in rows {
        if row.phones.is_empty() {
            continue;
        }
        let preferred_name = row.display_name();
        let phones = phone_handles_only(&row.phones);
        if phones.is_empty() {
            continue;
        }
        drafts.push(ContactDraft {
            phones,
            preferred_name,
        });
    }
    Ok(drafts)
}

/// Drafts from a VCF file.
fn drafts_from_vcf(vcf_path: &Path) -> Result<Vec<ContactDraft>> {
    let cards = parse_vcf(vcf_path)?;
    let mut drafts = Vec::new();
    for card in cards {
        let phones = phone_handles_only(&card.phones);
        if phones.is_empty() {
            continue;
        }

        // The bracket-tag convention this used to read is gone: a name that
        // contains brackets is stored as written.
        let fn_stripped = card.fn_raw.trim().to_string();
        let first = card.n_given.trim().to_string();
        let middle = card.n_middle.trim().to_string();
        let last = card.n_family.trim().to_string();

        let nickname = if last.is_empty()
            && !fn_stripped.is_empty()
            && !fn_stripped.contains(' ')
            && (first.is_empty() || first == fn_stripped)
        {
            Some(fn_stripped.clone())
        } else {
            None
        };

        let preferred_name = if let Some(nick) = nickname {
            Some(nick)
        } else {
            let mut parts = Vec::new();
            if !first.is_empty() {
                parts.push(first.as_str());
            }
            if !middle.is_empty() {
                parts.push(middle.as_str());
            }
            if !last.is_empty() {
                parts.push(last.as_str());
            }
            let from_n = collapse_inner_whitespace(&parts.join(" "));
            if !from_n.is_empty() {
                Some(from_n)
            } else {
                let from_fn = collapse_inner_whitespace(&fn_stripped);
                if from_fn.is_empty() {
                    None
                } else {
                    Some(from_fn)
                }
            }
        };

        // Contact Groups are the person's own; an address book never
        // creates them.
        drafts.push(ContactDraft {
            phones,
            preferred_name,
        });
    }
    Ok(drafts)
}

/// Collapse runs of whitespace to one space.
fn collapse_inner_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The contact this account already has for one of the card's phones among
/// the contacts of the given origins, with where that contact came from.
/// Origins are tried in the order given, so the caller decides which kind of
/// contact wins a phone.
///
/// A card and an existing contact that share a phone are the same person, so
/// the book joins that contact rather than standing a second one beside it.
/// That holds whether the import discovered the person or the person typed
/// their name: adopting a `user` row keeps one person as one contact, so the
/// card's phones and groups reach the real person instead of a second, unnamed
/// row. Whether the book gets to rename what it adopts is a separate question,
/// which the returned origin answers.
///
/// The identity stays the import's — `origin` is left alone — because the
/// messages are what proved the person exists, and a later book that drops the
/// card must not take them with it.
async fn contact_for_draft(
    conn: &mut AnyConnection,
    account_id: i64,
    phones: &[(String, Option<String>)],
    origins: &[Origin],
) -> Result<Option<(i64, String)>> {
    for origin in origins {
        for (phone, _note) in phones {
            let found: Option<(i64, String)> = sqlx::query_as(
                "SELECT ch.contact_id, c.origin
                 FROM contact_handles ch
                 JOIN handles h ON h.id = ch.handle_id
                 JOIN contacts c ON c.id = ch.contact_id
                 WHERE ch.account_id = $1
                   AND h.normalized = $2
                   AND h.handle_type = 'phone'
                   AND c.origin = $3
                 LIMIT 1",
            )
            .bind(account_id)
            .bind(phone)
            .bind(origin.as_str())
            .fetch_optional(&mut *conn)
            .await?;
            if found.is_some() {
                return Ok(found);
            }
        }
    }
    Ok(None)
}

/// Bring the account's address-book contacts in line with the cards in the
/// file, inside one transaction.
///
/// Each card first looks for a contact an import discovered or the person
/// typed, and joins it. Failing that it looks for the book's own contact on
/// one of its phones and updates that row in place: the name and the phones
/// change, the id does not, so the Contact Groups the person put the contact
/// in, the conversations it takes part in, and the record of the import that
/// met it all stay attached. Only a card that matches nothing makes a new
/// contact, and only a book contact that no card matches is deleted. A card
/// whose number changed between two loads therefore reads as one contact
/// gone and one arrived, because nothing else ties the two together.
///
/// Contact Groups themselves are never touched: a person builds those by
/// hand. Identities an import discovered are not the book's to remove, and
/// the book's own identities stay when a conversation, a message, or the
/// account's profile uses them; only a stale identity nothing uses goes.
async fn apply_address_book(
    conn: &mut AnyConnection,
    account_id: i64,
    drafts: Vec<ContactDraft>,
) -> Result<ContactLoadStats> {
    let mut stats = ContactLoadStats::default();
    let drafts = merge_duplicate_phone_drafts(drafts);
    let mut tx = conn.begin().await?;

    let mut kept: Vec<i64> = Vec::new();
    for draft in drafts {
        // A card with no name leaves the preferred name empty rather than
        // storing the literal word "Unknown" as someone's name; the contact is
        // then Unknown by the computed rule, which is the same thing said once.
        let preferred_name = draft.preferred_name.as_deref().unwrap_or("");
        let contact_id = if let Some((existing, _origin)) = contact_for_draft(
            &mut tx,
            account_id,
            &draft.phones,
            &[Origin::Import, Origin::User],
        )
        .await?
        {
            // A card that lists a number without a name has nothing to
            // say about who that person is, and a name the person typed
            // outranks the book. `propose_name` holds both rules.
            propose_name(
                &mut tx,
                account_id,
                existing,
                preferred_name,
                Origin::AddressBook,
            )
            .await?;
            existing
        } else if let Some((own, _origin)) =
            contact_for_draft(&mut tx, account_id, &draft.phones, &[Origin::AddressBook]).await?
        {
            // The book owns this row, so the card's name is the name, even
            // an empty one.
            let renamed = sqlx::query(
                "UPDATE contacts SET preferred_name = $1
                 WHERE account_id = $2 AND id = $3 AND preferred_name <> $1",
            )
            .bind(preferred_name)
            .bind(account_id)
            .bind(own)
            .execute(&mut *tx)
            .await?
            .rows_affected()
                > 0;
            let unlinked =
                unlink_phones_the_card_dropped(&mut tx, account_id, own, &draft.phones).await?;
            if renamed || unlinked {
                touch_contact(&mut tx, account_id, own).await?;
            }
            kept.push(own);
            own
        } else {
            let created: i64 = sqlx::query_scalar(
                "INSERT INTO contacts (account_id, preferred_name, origin)
                     VALUES ($1, $2, 'address_book') RETURNING id",
            )
            .bind(account_id)
            .bind(preferred_name)
            .fetch_one(&mut *tx)
            .await?;
            kept.push(created);
            created
        };
        stats.contacts += 1;

        for (phone, note) in &draft.phones {
            // Ensure handle exists; the note flags ambiguous values for review.
            sqlx::query(
                "INSERT INTO handles (account_id, raw, normalized, normalized_note, handle_type, service, origin)
                 VALUES ($1, $2, $3, $4, 'phone', 'phone', 'address_book')
                 ON CONFLICT DO NOTHING",
            )
            .bind(account_id)
            .bind(phone)
            .bind(phone)
            .bind(note.as_deref())
            .execute(&mut *tx)
            .await?;
            let handle_id: i64 = sqlx::query_scalar(
                "SELECT id FROM handles
                 WHERE account_id = $1 AND normalized = $2 AND handle_type = 'phone' AND service = 'phone'",
            )
            .bind(account_id)
            .bind(phone)
            .fetch_one(&mut *tx)
            .await?;

            // Link contact to handle. A phone another book contact still
            // holds moves to this one, since the file now says it is this
            // card's; a link an import or the person made is left alone.
            sqlx::query(
                "INSERT INTO contact_handles (account_id, handle_id, contact_id, origin)
                 VALUES ($1, $2, $3, 'address_book')
                 ON CONFLICT (account_id, handle_id) DO UPDATE
                     SET contact_id = excluded.contact_id
                     WHERE contact_handles.origin = 'address_book'",
            )
            .bind(account_id)
            .bind(handle_id)
            .bind(contact_id)
            .execute(&mut *tx)
            .await?;
            stats.phones += 1;
            if note.is_some() {
                stats.phones_needing_review += 1;
            }
        }
    }

    // Book contacts no card matched: the person is out of the file, so the
    // row goes, and with it this contact's memberships, participant links,
    // and import records.
    let own: Vec<i64> =
        sqlx::query_scalar("SELECT id FROM contacts WHERE account_id = $1 AND origin = $2")
            .bind(account_id)
            .bind(Origin::AddressBook.as_str())
            .fetch_all(&mut *tx)
            .await?;
    for id in own.into_iter().filter(|id| !kept.contains(id)) {
        sqlx::query("DELETE FROM contacts WHERE account_id = $1 AND id = $2")
            .bind(account_id)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    remove_unused_book_handles(&mut tx, account_id).await?;

    tx.commit().await?;
    Ok(stats)
}

/// Drop the book's links from `contact_id` to phones the card no longer
/// lists. Returns whether anything changed.
async fn unlink_phones_the_card_dropped(
    conn: &mut AnyConnection,
    account_id: i64,
    contact_id: i64,
    phones: &[(String, Option<String>)],
) -> Result<bool> {
    let linked: Vec<(i64, String)> = sqlx::query_as(
        "SELECT ch.handle_id, h.normalized
         FROM contact_handles ch JOIN handles h ON h.id = ch.handle_id
         WHERE ch.account_id = $1 AND ch.contact_id = $2 AND ch.origin = 'address_book'",
    )
    .bind(account_id)
    .bind(contact_id)
    .fetch_all(&mut *conn)
    .await?;
    let mut changed = false;
    for (handle_id, normalized) in linked {
        if phones.iter().any(|(phone, _)| *phone == normalized) {
            continue;
        }
        sqlx::query("DELETE FROM contact_handles WHERE account_id = $1 AND handle_id = $2")
            .bind(account_id)
            .bind(handle_id)
            .execute(&mut *conn)
            .await?;
        changed = true;
    }
    Ok(changed)
}

/// Remove the book's identities that no contact holds and nothing refers to.
///
/// An identity a conversation, a message, a reaction, or the account's own
/// profile uses stays even when the book dropped it, the same way deleting a
/// contact keeps its conversations: the messages are what proved the identity
/// exists. Deleting it would take the conversation with it.
async fn remove_unused_book_handles(conn: &mut AnyConnection, account_id: i64) -> Result<()> {
    sqlx::query(
        "DELETE FROM handles
         WHERE account_id = $1 AND origin = 'address_book'
           AND NOT EXISTS (SELECT 1 FROM contact_handles ch WHERE ch.handle_id = handles.id)
           AND NOT EXISTS (SELECT 1 FROM participants p WHERE p.handle_id = handles.id)
           AND NOT EXISTS (SELECT 1 FROM conversations c WHERE c.chat_handle_id = handles.id)
           AND NOT EXISTS (SELECT 1 FROM messages m WHERE m.sender_handle_id = handles.id)
           AND NOT EXISTS (SELECT 1 FROM messages m WHERE m.owner_handle_id = handles.id)
           AND NOT EXISTS (SELECT 1 FROM tapbacks t WHERE t.sender_handle_id = handles.id)
           AND NOT EXISTS (SELECT 1 FROM account_handles ah WHERE ah.handle_id = handles.id)",
    )
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Merge address-book rows that share any phone, including transitive overlaps.
fn merge_duplicate_phone_drafts(drafts: Vec<ContactDraft>) -> Vec<ContactDraft> {
    let mut merged: Vec<ContactDraft> = Vec::new();
    for mut draft in drafts {
        let mut matching: Vec<usize> = merged
            .iter()
            .enumerate()
            .filter(|(_, existing)| {
                existing
                    .phones
                    .iter()
                    .any(|phone| draft.phones.contains(phone))
            })
            .map(|(index, _)| index)
            .collect();
        if matching.is_empty() {
            merged.push(draft);
            continue;
        }

        let target = matching.remove(0);
        merge_contact_draft(&mut merged[target], draft);
        for index in matching.into_iter().rev() {
            draft = merged.remove(index);
            let adjusted_target = if index < target { target - 1 } else { target };
            merge_contact_draft(&mut merged[adjusted_target], draft);
        }
    }
    merged
}

/// Fold one draft into another: keep the first name, union the phones.
fn merge_contact_draft(into: &mut ContactDraft, from: ContactDraft) {
    if into.preferred_name.is_none() {
        into.preferred_name = from.preferred_name;
    }
    for phone in from.phones {
        if !into.phones.contains(&phone) {
            into.phones.push(phone);
        }
    }
}

/// Contacts are now resolved through the `handles` table during import (Task 10 of the
/// handle-identity-model plan); backfilling unknown contacts from conversation data and
/// filling empty names from participant hints happen there, not here.
#[cfg(test)]
mod tests;
