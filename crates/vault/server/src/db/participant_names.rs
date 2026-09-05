//! The one query that decides the name shown for a participant.
//!
//! ADR-0006: the Contact's name, else what that backup called them in that
//! conversation, else the handle.
//!
//! The query's `COALESCE` does not actually end at the handle — it ends at
//! `''`, because a participant with no address has no handle to end at. What
//! keeps `name` non-empty is an import invariant rather than this query:
//! import never creates a participant with neither an address nor a name, so
//! every row has already matched one of the three earlier clauses. Read that
//! guarantee here and enforce it in `import::contact_name`, whose
//! `resolve_name_only_participant` is the only writer of a handle-less
//! participant row.
//!
//! Every route that names a participant calls
//! [`load_for_conversations`], so one person cannot show two names on one
//! screen. `participants.contact_id` is not consulted for naming a
//! participant who has a handle — that always routes through
//! `contact_handles`, which is what makes renaming a Contact reach every
//! conversation at once. A handle-less participant has no handle for
//! `contact_handles` to key on, so for that one case `participants.contact_id`
//! is used instead, because it is the only link to the Contact that exists.
//!
//! One conversation shape has no participants rows to read at all: a backup
//! that recorded the thread's address and nothing about who was in it.
//! [`load_for_conversations`] answers for that shape too, from the
//! conversation's own chat handle, so a caller never has to know the shape
//! exists or carry a second naming path of its own. Reading one conversation's
//! messages and listing conversations therefore name the same person the same
//! way; while the fallback lived beside the list, a message page showed no one
//! at all for such a thread.

use std::collections::HashMap;

use sqlx::any::AnyRow;
use sqlx::{AnyConnection, Row};

pub use vault_api_types::Participant;

use crate::db::sql::group_rows_by_id;

/// Participants of each conversation in `conversation_ids`, ordered by
/// participant id within a conversation.
///
/// # Errors
///
/// Returns a database error when the query fails.
pub async fn load_for_conversations(
    conn: &mut AnyConnection,
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
    conn: &mut AnyConnection,
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
                                 h.raw, '') AS name,
                        h.raw AS handle,
                        COALESCE(NULLIF(trim(h.service), ''), h.handle_type) AS service,
                        -- A handle-less participant's Contact link lives on
                        -- p.contact_id (contact_handles has no handle to key
                        -- on for them); a handle-bearing one's link is always
                        -- ch.contact_id, never p.contact_id. Same rule below
                        -- for joining contacts, so a renamed Contact reaches
                        -- a handle-less participant's name too.
                        CASE WHEN p.handle_id IS NULL THEN p.contact_id ELSE ch.contact_id END
                          AS contact_id
                 FROM participants p
                 LEFT JOIN handles h ON h.id = p.handle_id
                 JOIN conversations conv ON conv.id = p.conversation_id
                 LEFT JOIN contact_handles ch
                   ON ch.handle_id = p.handle_id AND ch.account_id = conv.account_id
                 LEFT JOIN contacts c
                   ON c.id = CASE WHEN p.handle_id IS NULL THEN p.contact_id ELSE ch.contact_id END
                  AND c.account_id = conv.account_id
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
/// Same rule, one clause shorter: with no participants row there is no
/// per-conversation backup name, so it is the Contact's name, else the handle.
/// The Contact is reached through `contact_handles` exactly as above, so a
/// person the vault has a name for is named here too and their row opens the
/// contact drawer instead of showing a bare phone number.
///
/// A conversation with no chat handle row is simply absent from the result.
async fn load_from_chat_handle(
    conn: &mut AnyConnection,
    conversation_ids: &[i64],
) -> Result<HashMap<i64, Vec<Participant>>, sqlx::Error> {
    // `conv.chat_handle_id` is `NOT NULL`, so this join always matches and
    // `handle`/`service` are never actually absent here — the column types
    // just have to match `Participant`'s, which carry the address-less case
    // that only a `participants` row can produce.
    group_rows_by_id(
        conn,
        conversation_ids,
        |placeholders| {
            format!(
                "SELECT conv.id,
                        COALESCE(NULLIF(trim(c.preferred_name), ''), h.raw) AS name,
                        h.raw AS handle,
                        COALESCE(NULLIF(trim(h.service), ''), h.handle_type) AS service,
                        ch.contact_id
                 FROM conversations conv
                 JOIN handles h ON h.id = conv.chat_handle_id
                 LEFT JOIN contact_handles ch
                   ON ch.handle_id = h.id AND ch.account_id = conv.account_id
                 LEFT JOIN contacts c
                   ON c.id = ch.contact_id AND c.account_id = conv.account_id
                 WHERE conv.id IN ({placeholders})"
            )
        },
        participant_row,
    )
    .await
}

/// One row of either query above: the conversation id, then the participant
/// as (name, handle, service, contact id).
fn participant_row(row: &AnyRow) -> Result<(i64, Participant), sqlx::Error> {
    Ok((
        row.try_get::<i64, _>(0)?,
        Participant {
            name: row.try_get(1)?,
            handle: row.try_get(2)?,
            service: row.try_get(3)?,
            contact_id: row.try_get(4)?,
        },
    ))
}

#[cfg(test)]
mod tests;
