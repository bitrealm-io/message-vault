//! `Sql`, a fragment plus the values it binds, and `ListCtx`, which says
//! what "this contact", "this conversation", and "this message" mean from
//! each list's base row. Every emitter is written once against the alias it
//! needs and asks the context to wrap it.

use crate::db::dialect::like_ci;
use crate::db::engine::DbEngine;
use crate::db::sql::SqlParam;

use super::ListKind;

/// A fragment under construction. `params` are in the textual order of `text`.
#[derive(Debug, Default)]
pub(crate) struct Sql {
    pub text: String,
    pub params: Vec<SqlParam>,
}

impl Sql {
    /// Append raw SQL text.
    pub fn push(&mut self, s: &str) {
        self.text.push_str(s);
    }

    /// Write `?` and bind a text value.
    pub fn bind_text(&mut self, v: impl Into<String>) {
        self.text.push('?');
        self.params.push(SqlParam::Text(v.into()));
    }

    /// Write `?` and bind an integer.
    pub fn bind_int(&mut self, v: i64) {
        self.text.push('?');
        self.params.push(SqlParam::Int(v));
    }

    /// Bind a text value for a `?` a dialect helper already wrote into
    /// `text` (for example `db::dialect::name_eq_ci`'s own placeholder).
    /// Unlike `bind_text`, this does not write the `?` itself — the helper
    /// already did — so call it immediately after pushing that helper's SQL.
    pub fn param_text(&mut self, v: impl Into<String>) {
        self.params.push(SqlParam::Text(v.into()));
    }

    /// `column LIKE ?` case-insensitively on both engines
    /// ([`db::dialect::like_ci`](crate::db::dialect::like_ci)), binding
    /// `pattern` as it is. In the pattern `%` and `_` are wildcards and `\`
    /// escapes, so text a person typed reaches here only through
    /// `emit::like_contains`, which escapes it.
    pub fn like(&mut self, column: &str, pattern: &str) {
        self.text.push_str(&like_ci(column));
        self.params.push(SqlParam::Text(pattern.to_string()));
    }
}

/// Conversation `conv` involves the contact `contact_expr`: one of the
/// contact's handles is the chat handle or a participant's handle, or the
/// contact is linked directly to a participant row that has no handle of
/// its own (the source named that person and recorded no address for them).
pub(crate) fn conversation_involves(conv: &str, contact_expr: &str) -> String {
    format!(
        "(EXISTS (SELECT 1 FROM contact_handles chi \
           WHERE chi.account_id = {conv}.account_id AND chi.contact_id = {contact_expr} \
             AND (chi.handle_id = {conv}.chat_handle_id \
                  OR EXISTS (SELECT 1 FROM participants pi WHERE pi.conversation_id = {conv}.id AND pi.handle_id = chi.handle_id))) \
         OR EXISTS (SELECT 1 FROM participants pi2 WHERE pi2.conversation_id = {conv}.id AND pi2.contact_id = {contact_expr}))"
    )
}

/// Whether the rows a search reaches on another list count when they are
/// in the trash. A search leaves the trash out everywhere it looks unless
/// the query carries `trashed:`, and then the trash counts everywhere that
/// search looks: the list's own rows and the rows a word reaches on another
/// list (#724). `compile` sets it once from the query, so no emitter decides
/// for itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrashScope {
    /// A trashed row on the far side is not there.
    LeftOut,
    /// The query carries `trashed:`: a trashed row on the far side counts.
    Counted,
}

impl TrashScope {
    /// ` AND <not trashed>` for conversation alias `conv`, or nothing.
    pub(crate) fn conversation_clause(self, conv: &str) -> String {
        match self {
            Self::LeftOut => format!(" AND {}", super::emit::not_trashed_conversation(conv)),
            Self::Counted => String::new(),
        }
    }

    /// ` AND <not trashed>` for contact alias `ct`, or nothing.
    pub(crate) fn contact_clause(self, ct: &str) -> String {
        match self {
            Self::LeftOut => format!(" AND {}", super::emit::not_trashed_contact(ct)),
            Self::Counted => String::new(),
        }
    }
}

/// Which list a fragment is for, and what it needs from the request.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ListCtx {
    pub list: ListKind,
    pub engine: DbEngine,
    pub account_id: i64,
    /// The account's time zone, for the date words' boundaries.
    pub zone: chrono_tz::Tz,
    /// Whether trashed rows on another list count.
    pub trash: TrashScope,
}

impl ListCtx {
    /// The base row's account column.
    pub fn account_col(&self) -> &'static str {
        match self.list {
            ListKind::Contacts => "ct.account_id",
            ListKind::Conversations => "c.account_id",
            ListKind::Messages => "m.account_id",
        }
    }

    /// Wrap `inner`, written against conversation alias `c`, so it is true of
    /// the base row: the conversation itself, the message's conversation, or
    /// some conversation the contact is in.
    ///
    /// Never nest one wrapper inside another on the same list: two of them
    /// each open their own `FROM conversations c`, and the inner one would
    /// shadow the outer alias.
    pub fn conversation(&self, out: &mut Sql, inner: impl FnOnce(&mut Sql)) {
        match self.list {
            ListKind::Conversations => {
                out.push("(");
                inner(out);
                out.push(")");
            }
            ListKind::Messages => {
                out.push(
                    "EXISTS (SELECT 1 FROM conversations c WHERE c.id = m.conversation_id AND (",
                );
                inner(out);
                out.push("))");
            }
            ListKind::Contacts => {
                out.push(&format!(
                    "EXISTS (SELECT 1 FROM conversations c WHERE {} AND (",
                    self.contact_conversations_link("c")
                ));
                inner(out);
                out.push("))");
            }
        }
    }

    /// Wrap `inner`, written against message alias `m` (a non-duplicate
    /// message; conversation alias `c` is also in scope), so it is true of the
    /// base row. On Contacts that is a message the contact sent
    /// ([`Self::contact_sent_messages`]): a word that dates or counts
    /// messages on Contacts asks about the person, not about every message
    /// of every conversation they are in (#726).
    ///
    /// Never nest one wrapper inside another on the same list: two of them
    /// each open their own `FROM conversations c`, and the inner one would
    /// shadow the outer alias.
    pub fn message(&self, out: &mut Sql, inner: impl FnOnce(&mut Sql)) {
        match self.list {
            ListKind::Messages => {
                out.push(
                    "EXISTS (SELECT 1 FROM conversations c WHERE c.id = m.conversation_id AND (",
                );
                inner(out);
                out.push("))");
            }
            ListKind::Conversations => {
                out.push(
                    "EXISTS (SELECT 1 FROM messages m WHERE m.conversation_id = c.id AND m.duplicate_of IS NULL AND (",
                );
                inner(out);
                out.push("))");
            }
            ListKind::Contacts => {
                out.push(&format!(
                    "EXISTS (SELECT 1 {} AND (",
                    self.contact_sent_messages()
                ));
                inner(out);
                out.push("))");
            }
        }
    }

    /// Wrap `inner`, written against contact alias `ct`, so it is true of the
    /// base row: the contact itself, or some contact linked to a participant.
    /// A contact in the trash is reached only when the query carries
    /// `trashed:` (#724).
    ///
    /// Never nest one wrapper inside another on the same list: two of them
    /// each open their own `FROM conversations c`, and the inner one would
    /// shadow the outer alias.
    pub fn contact(&self, out: &mut Sql, inner: impl FnOnce(&mut Sql)) {
        match self.list {
            ListKind::Contacts => {
                out.push("(");
                inner(out);
                out.push(")");
            }
            ListKind::Conversations => {
                out.push(&format!(
                    "EXISTS (SELECT 1 FROM contacts ct WHERE ct.account_id = c.account_id AND {}{} AND (",
                    conversation_involves("c", "ct.id"),
                    self.trash.contact_clause("ct")
                ));
                inner(out);
                out.push("))");
            }
            ListKind::Messages => {
                out.push(&format!(
                    "EXISTS (SELECT 1 FROM conversations c JOIN contacts ct ON ct.account_id = c.account_id \
                       WHERE c.id = m.conversation_id AND {}{} AND (",
                    conversation_involves("c", "ct.id"),
                    self.trash.contact_clause("ct")
                ));
                inner(out);
                out.push("))");
            }
        }
    }

    /// Reach the rows of `list` from the base row: [`Self::contact`] for
    /// Contacts, [`Self::conversation`] for Conversations, [`Self::message`]
    /// for Messages. For a word whose home list is decided by data.
    pub fn reach(&self, list: ListKind, out: &mut Sql, inner: impl FnOnce(&mut Sql)) {
        match list {
            ListKind::Contacts => self.contact(out, inner),
            ListKind::Conversations => self.conversation(out, inner),
            ListKind::Messages => self.message(out, inner),
        }
    }

    /// A scalar subquery over the base row's non-duplicate messages: how
    /// many, the earliest timestamp, or the latest. On Messages and
    /// Conversations that is the one conversation's messages, reached
    /// through the conversation index. On Contacts it is the messages the
    /// contact sent ([`Self::contact_sent_messages`]), so `messages:0` is a
    /// contact who never wrote and `last-message:` is when they last did,
    /// the same answer the contact list's "Last heard from" column gives.
    pub fn message_aggregate(&self, agg: MessageAgg) -> String {
        let per_conversation = match agg {
            MessageAgg::Count => "COUNT(*)",
            MessageAgg::First => "MIN(m2.timestamp)",
            MessageAgg::Last => "MAX(m2.timestamp)",
        };
        match self.list {
            ListKind::Messages => format!(
                "(SELECT {per_conversation} FROM messages m2 WHERE m2.conversation_id = m.conversation_id AND m2.duplicate_of IS NULL)"
            ),
            ListKind::Conversations => format!(
                "(SELECT {per_conversation} FROM messages m2 WHERE m2.conversation_id = c.id AND m2.duplicate_of IS NULL)"
            ),
            ListKind::Contacts => {
                let sent = match agg {
                    MessageAgg::Count => "COUNT(*)",
                    MessageAgg::First => "MIN(m.timestamp)",
                    MessageAgg::Last => "MAX(m.timestamp)",
                };
                format!("(SELECT {sent} {})", self.contact_sent_messages())
            }
        }
    }

    /// A WHERE fragment tying conversations alias `c2` to the base contact
    /// `ct`, a conversation in the trash left out unless the query carries
    /// `trashed:`. Only meaningful when the base row is a contact: on
    /// another list `ct` is not in scope.
    pub fn contact_conversations_link(&self, c2: &str) -> String {
        format!(
            "{}{}",
            contact_conversations_link(c2),
            self.trash.conversation_clause(c2)
        )
    }

    /// The messages the base contact `ct` sent, as [`contact_sent_messages`]
    /// writes them, with the query's trash scope.
    pub fn contact_sent_messages(&self) -> String {
        contact_sent_messages(self.trash)
    }
}

/// What [`ListCtx::message_aggregate`] asks of the base row's messages.
#[derive(Debug, Clone, Copy)]
pub(crate) enum MessageAgg {
    /// How many there are.
    Count,
    /// The earliest timestamp, NULL when there are none.
    First,
    /// The latest timestamp, NULL when there are none.
    Last,
}

/// A WHERE fragment tying conversations alias `c2` to the base contact `ct`,
/// with no word about the trash: [`ListCtx::contact_conversations_link`]
/// adds the query's trash scope.
///
/// A free function rather than a [`ListCtx`] method on purpose: it is only
/// meaningful when the base row is a contact, and a method would let a call
/// site on another list emit `ct`, an alias that is not in scope there.
pub(crate) fn contact_conversations_link(c2: &str) -> String {
    format!(
        "{c2}.account_id = ct.account_id AND {}",
        conversation_involves(c2, "ct.id")
    )
}

/// `FROM … WHERE …` over the messages contact `ct` sent: a received message
/// (alias `m`) whose sender is one of the contact's handles, in any
/// conversation (alias `c`), direct or group, duplicates left out, and a
/// conversation in the trash left out unless `trash` says it counts. The
/// one definition of "a message the contact sent": `date:`, `messages:`,
/// `first-message:` and `last-message:` on Contacts, the contact list's
/// `last_heard_at`, the contact detail's `total_messages`, the selection
/// summary, and the contact's identity table all read it, so none of them
/// can drift from the others (#725, #726, #913).
///
/// Your own messages and other people's messages in a shared group chat are
/// not the contact's, which is why this is not every message of every
/// conversation the contact is in. `ix_messages_sender_timestamp` answers
/// it per handle.
///
/// A free function for the same reason as [`contact_conversations_link`]:
/// it is only meaningful when the base row is a contact.
pub(crate) fn contact_sent_messages(trash: TrashScope) -> String {
    sent_messages_where(
        "chs.account_id = ct.account_id AND chs.contact_id = ct.id",
        trash,
    )
}

/// [`contact_sent_messages`] narrowed to one of the contact's identities:
/// the messages contact `ct` sent from handle `handle_expr`. The contact
/// drawer's identity table reads it, one row per identity.
pub(crate) fn contact_sent_messages_from(handle_expr: &str, trash: TrashScope) -> String {
    sent_messages_where(
        &format!(
            "chs.account_id = ct.account_id AND chs.contact_id = ct.id \
             AND chs.handle_id = {handle_expr}"
        ),
        trash,
    )
}

/// The body both [`contact_sent_messages`] and
/// [`contact_sent_messages_from`] share; `handles` picks the
/// `contact_handles chs` rows whose messages count. The fragment ends in
/// its WHERE clause, so a caller may append ` AND …` to narrow it further.
fn sent_messages_where(handles: &str, trash: TrashScope) -> String {
    format!(
        "FROM contact_handles chs \
           JOIN messages m ON m.sender_handle_id = chs.handle_id AND m.account_id = chs.account_id \
           JOIN conversations c ON c.id = m.conversation_id \
           WHERE {handles} \
             AND m.is_from_me = 0 AND m.duplicate_of IS NULL{}",
        trash.conversation_clause("c")
    )
}
