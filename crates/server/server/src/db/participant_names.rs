//! The one query that decides the name shown for a participant.
//!
//! ADR-0006: the Contact's name, else what that backup called them in that
//! conversation, else the handle.
//!
//! Every participant has a handle: a person the source named with no address
//! has a handle of type `other` holding the name. So the name is never empty.
//!
//! Every route that names a participant calls
//! [`load_for_conversations`], so one person cannot show two names on one
//! screen. A participant's Contact is the one its handle is on in
//! `contact_handles`, which is what makes renaming a Contact, or moving the
//! handle to another, reach every conversation at once.
//!
//! One conversation shape has no participants rows to read at all: a backup
//! that recorded the thread's address and nothing about who was in it.
//! [`load_for_conversations`] answers for that shape too, from the
//! conversation's own chat handle, so a caller never has to know the shape
//! exists or carry a second naming path of its own. Only a one-to-one
//! conversation's chat handle is a person's address; a group's chat handle is
//! the group's own id, such as `chat1000000005`, so a group with no
//! participants rows has no participants. Reading one conversation's
//! messages and listing conversations therefore name the same person the same
//! way; while the fallback lived beside the list, a message page showed no one
//! at all for such a thread.

use std::collections::HashMap;

use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

pub use message_crate_api_types::Participant;

use crate::db::sql::group_rows_by_id;

/// Join condition that leaves a trashed contact `c` out. A trashed contact is
/// not one of a conversation's contacts (CONTEXT.md, "Trash"), and
/// `GET /v1/contacts/{id}` answers 404 for it, so its participant is named and
/// linked as if no contact held the identity. Search does the same unless the
/// query carries `trashed:`.
const NOT_TRASHED: &str = "NOT EXISTS (SELECT 1 FROM trashed_contacts t
                         WHERE t.account_id = c.account_id AND t.contact_id = c.id)";

/// Participants of each conversation in `conversation_ids`, ordered by
/// participant id within a conversation.
///
/// # Errors
///
/// Returns a database error when the query fails.
pub async fn load_for_conversations(
    conn: &mut SqliteConnection,
    conversation_ids: &[i64],
) -> Result<HashMap<i64, Vec<Participant>>, sqlx::Error> {
    let mut loaded = load_participant_rows(conn, conversation_ids).await?;
    let without_rows: Vec<i64> = conversation_ids
        .iter()
        .copied()
        .filter(|id| loaded.get(id).is_none_or(Vec::is_empty))
        .collect();
    if !without_rows.is_empty() {
        for (id, participants) in load_from_chat_handle(conn, &without_rows).await? {
            loaded.insert(id, participants);
        }
    }
    Ok(loaded)
}

/// The `participants` rows themselves, with no fallback. Private: every
/// caller wants the fallback, and one that did not would be a second naming
/// path.
async fn load_participant_rows(
    conn: &mut SqliteConnection,
    conversation_ids: &[i64],
) -> Result<HashMap<i64, Vec<Participant>>, sqlx::Error> {
    group_rows_by_id(
        conn,
        conversation_ids,
        |placeholders| {
            format!(
                "SELECT p.conversation_id,
                        COALESCE(NULLIF(trim(c.preferred_name), ''),
                                 NULLIF(trim(p.name_alias), ''),
                                 h.raw) AS name,
                        h.raw AS handle,
                        COALESCE(NULLIF(trim(h.service), ''), h.handle_type) AS service,
                        -- The id is the joined contact's, so a trashed one
                        -- leaves no link behind.
                        c.id AS contact_id
                 FROM participants p
                 JOIN handles h ON h.id = p.handle_id
                 JOIN conversations conv ON conv.id = p.conversation_id
                 LEFT JOIN contact_handles ch
                   ON ch.handle_id = p.handle_id AND ch.account_id = conv.account_id
                 LEFT JOIN contacts c
                   ON c.id = ch.contact_id
                  AND c.account_id = conv.account_id
                  AND {NOT_TRASHED}
                 WHERE p.conversation_id IN ({placeholders})
                 ORDER BY p.conversation_id, p.id"
            )
        },
        participant_row,
    )
    .await
}

/// The chat handle of each conversation in `conversation_ids` as its sole
/// participant, for conversations that have no participants rows at all.
///
/// Only a one-to-one (`individual`) conversation's chat handle is a person
/// (`docs/architecture/contacts-identities-and-messages.md`, "A group
/// conversation is not a person"). Any other conversation type is left out,
/// so it has no participants. The type is compared without case, as the import
/// compares it when it decides whether the chat handle gets a contact.
///
/// Same rule, one clause shorter: with no participants row there is no
/// per-conversation backup name, so it is the Contact's name, else the handle.
/// The Contact is reached through `contact_handles` exactly as above, so a
/// person the database has a name for is named here too and their row opens the
/// contact drawer instead of showing a bare phone number.
///
/// A conversation with no chat handle row is simply absent from the result.
async fn load_from_chat_handle(
    conn: &mut SqliteConnection,
    conversation_ids: &[i64],
) -> Result<HashMap<i64, Vec<Participant>>, sqlx::Error> {
    // `conv.chat_handle_id` is `NOT NULL`, so this join always matches and
    // `handle`/`service` are never absent here.
    group_rows_by_id(
        conn,
        conversation_ids,
        |placeholders| {
            format!(
                "SELECT conv.id,
                        COALESCE(NULLIF(trim(c.preferred_name), ''), h.raw) AS name,
                        h.raw AS handle,
                        COALESCE(NULLIF(trim(h.service), ''), h.handle_type) AS service,
                        c.id AS contact_id
                 FROM conversations conv
                 JOIN handles h ON h.id = conv.chat_handle_id
                 LEFT JOIN contact_handles ch
                   ON ch.handle_id = h.id AND ch.account_id = conv.account_id
                 LEFT JOIN contacts c
                   ON c.id = ch.contact_id AND c.account_id = conv.account_id
                  AND {NOT_TRASHED}
                 WHERE conv.id IN ({placeholders})
                   AND conv.conversation_type = 'individual' COLLATE NOCASE"
            )
        },
        participant_row,
    )
    .await
}

/// One row of either query above: the conversation id, then the participant
/// as (name, handle, service, contact id).
fn participant_row(row: &SqliteRow) -> Result<(i64, Participant), sqlx::Error> {
    Ok((
        row.try_get::<i64, _>(0)?,
        Participant {
            name: row.try_get(1)?,
            identity: row.try_get(2)?,
            service: row.try_get(3)?,
            contact_id: row.try_get(4)?,
        },
    ))
}

#[cfg(test)]
mod tests;
