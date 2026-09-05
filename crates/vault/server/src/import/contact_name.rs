//! Contact linking and display-name merging during import.

use anyhow::Result;
use message_ir::{HandleType, trimmed};
use sqlx::AnyConnection;

use super::ImportStats;
use crate::db::contacts;
use crate::db::handles::{
    HandleIdCache, infer_handle_type_from_shape as infer_handle_type, upsert_handle_row_cached,
};

/// The contact that owns `handle_id`, creating one when nothing owns it yet.
///
/// Every participant an import meets becomes a contact. ADR-0006: a backup is
/// an address book the person already curated, so the name it supplies goes on
/// the contact — on creation, or later if an earlier backup left the contact
/// nameless. A contact that already has a name is untouched, because the same
/// number arrives spelled differently across backups and the first spelling is
/// as good as the second. A contact the person made or an address book loaded
/// is never renamed by an import.
pub(super) async fn ensure_contact_for_handle(
    tx: &mut AnyConnection,
    account_id: &str,
    handle_id: i64,
    backup_name: Option<&str>,
    stats: &mut ImportStats,
) -> Result<i64> {
    let name = backup_name.and_then(trimmed).unwrap_or("");
    if let Some(existing) = ensure_sibling_contact_link(tx, account_id, handle_id).await? {
        // An import names only a contact an earlier import left nameless;
        // `contacts::propose_name` is where that rule and its two siblings
        // live.
        contacts::propose_name(tx, account_id, existing, name, contacts::Origin::Import).await?;
        return Ok(existing);
    }
    let contact_id =
        contacts::create_contact(tx, account_id, name, contacts::Origin::Import).await?;
    contacts::link_handle_to_contact(
        tx,
        account_id,
        handle_id,
        contact_id,
        contacts::Origin::Import,
    )
    .await?;
    stats.contacts_created += 1;
    Ok(contact_id)
}

/// Bind a participant the source named without recording any address.
///
/// A single existing contact under that name is reused, so the same person
/// named across several conversations does not become several contacts. When
/// no contact matches — or when two do, which is ambiguous — a contact is
/// created carrying the name and no identity. Either way the result is Unknown
/// until the person supplies an address for them.
///
/// Returns the contact and the display name to record on the participant.
pub(super) async fn resolve_name_only_participant(
    tx: &mut AnyConnection,
    account_id: &str,
    name: Option<&str>,
) -> Result<(Option<i64>, Option<String>)> {
    let Some(name) = name.and_then(trimmed) else {
        // A participant with neither an address nor a name says nothing at
        // all; there is nothing to create and nothing to show.
        return Ok((None, None));
    };
    if let Some(existing) = contacts::contact_id_by_preferred_name(tx, account_id, name).await? {
        return Ok((Some(existing), Some(name.to_string())));
    }
    let contact_id =
        contacts::create_contact(tx, account_id, name, contacts::Origin::Import).await?;
    Ok((Some(contact_id), Some(name.to_string())))
}

/// What one message says about who sent it. Its own type because these four
/// facts travel together and come from the message, while the connection,
/// handle cache, account and stats around them belong to the import run.
pub(super) struct IncomingSender<'a> {
    /// True when the account owner sent it, in which case there is no sender
    /// handle to resolve.
    pub is_from_me: bool,
    /// The sender's address as the backup recorded it, when it recorded one.
    pub address: Option<&'a str>,
    /// The address's type when the source stated it; inferred from the
    /// address's shape when it did not.
    pub handle_type: Option<HandleType>,
    /// Platform service the message arrived on, e.g. `imessage`.
    pub platform: &'a str,
}

/// The `handles` row for an incoming message's sender, creating it when this
/// import is the first to meet that address. `None` for a message the account
/// owner sent, and for one whose source recorded no sender address.
pub(super) async fn resolve_incoming_sender_handle(
    tx: &mut AnyConnection,
    cache: &mut HandleIdCache,
    account_id: &str,
    sender: IncomingSender<'_>,
    stats: &mut ImportStats,
) -> Result<Option<i64>> {
    if sender.is_from_me {
        return Ok(None);
    }
    let Some(address) = sender.address.and_then(trimmed) else {
        return Ok(None);
    };
    let handle_type = sender
        .handle_type
        .unwrap_or_else(|| infer_handle_type(address));
    let (handle_id, flagged, cached) = upsert_handle_row_cached(
        tx,
        cache,
        account_id,
        address,
        handle_type,
        Some(sender.platform),
    )
    .await?;
    if flagged {
        stats.phones_needing_review += 1;
    }
    if !cached {
        let _ = ensure_sibling_contact_link(tx, account_id, handle_id).await?;
    }
    Ok(Some(handle_id))
}

/// If this handle has no contact but a sibling handle (same normalized value
/// and type, different platform service) is already linked, attach this handle
/// to that contact.
pub(super) async fn ensure_sibling_contact_link(
    conn: &mut AnyConnection,
    account_id: &str,
    handle_id: i64,
) -> Result<Option<i64>> {
    if let Some(existing) = contacts::contact_id_for_handle(conn, account_id, handle_id).await? {
        return Ok(Some(existing));
    }
    let Some(contact_id) =
        contacts::contact_id_of_sibling_handle(conn, account_id, handle_id).await?
    else {
        return Ok(None);
    };
    if contacts::link_handle_to_contact(
        conn,
        account_id,
        handle_id,
        contact_id,
        contacts::Origin::Import,
    )
    .await?
    {
        contacts::touch_contact(conn, account_id, contact_id).await?;
    }
    Ok(Some(contact_id))
}

#[cfg(test)]
mod tests;
