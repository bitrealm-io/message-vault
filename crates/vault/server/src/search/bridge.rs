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

/// Which list a fragment is for, and what it needs from the request.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ListCtx {
    pub list: ListKind,
    pub engine: DbEngine,
    pub account_id: i64,
    /// The account's time zone, for the date words' boundaries.
    pub zone: chrono_tz::Tz,
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
                    "EXISTS (SELECT 1 FROM conversations c WHERE c.account_id = ct.account_id AND {} AND (",
                    conversation_involves("c", "ct.id")
                ));
                inner(out);
                out.push("))");
            }
        }
    }

    /// Wrap `inner`, written against message alias `m` (a non-duplicate
    /// message; conversation alias `c` is also in scope), so it is true of the
    /// base row.
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
                    "EXISTS (SELECT 1 FROM conversations c JOIN messages m ON m.conversation_id = c.id AND m.duplicate_of IS NULL \
                       WHERE c.account_id = ct.account_id AND {} AND (",
                    conversation_involves("c", "ct.id")
                ));
                inner(out);
                out.push("))");
            }
        }
    }

    /// Wrap `inner`, written against contact alias `ct`, so it is true of the
    /// base row: the contact itself, or some contact linked to a participant.
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
                    "EXISTS (SELECT 1 FROM contacts ct WHERE ct.account_id = c.account_id AND {} AND (",
                    conversation_involves("c", "ct.id")
                ));
                inner(out);
                out.push("))");
            }
            ListKind::Messages => {
                out.push(&format!(
                    "EXISTS (SELECT 1 FROM conversations c JOIN contacts ct ON ct.account_id = c.account_id \
                       WHERE c.id = m.conversation_id AND {} AND (",
                    conversation_involves("c", "ct.id")
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
    /// through the conversation index.
    ///
    /// A contact's messages are those of every conversation the contact is
    /// in, trashed conversations left out, the same answer the contact
    /// drawer gives, so `messages:>0` and the number in the drawer cannot
    /// disagree once something is trashed. They are counted per
    /// conversation in a subquery that mentions no outer alias, so SQLite
    /// computes it once for the whole list and each contact sums the rows
    /// of its conversations. The earlier shape counted every message of the
    /// account again for each contact, with an `EXISTS` per message, and
    /// `messages:0` took minutes on the demo vault (#413).
    pub fn message_aggregate(&self, agg: MessageAgg) -> String {
        let (per_conversation, over_conversations) = match agg {
            MessageAgg::Count => ("COUNT(*)", "COALESCE(SUM(mc.v), 0)"),
            MessageAgg::First => ("MIN(m2.timestamp)", "MIN(mc.v)"),
            MessageAgg::Last => ("MAX(m2.timestamp)", "MAX(mc.v)"),
        };
        match self.list {
            ListKind::Messages => format!(
                "(SELECT {per_conversation} FROM messages m2 WHERE m2.conversation_id = m.conversation_id AND m2.duplicate_of IS NULL)"
            ),
            ListKind::Conversations => format!(
                "(SELECT {per_conversation} FROM messages m2 WHERE m2.conversation_id = c.id AND m2.duplicate_of IS NULL)"
            ),
            ListKind::Contacts => format!(
                "(SELECT {over_conversations} FROM conversations c2 \
                   JOIN (SELECT m2.conversation_id, {per_conversation} AS v FROM messages m2 WHERE m2.duplicate_of IS NULL GROUP BY m2.conversation_id) mc \
                     ON mc.conversation_id = c2.id \
                   WHERE {} AND {})",
                contact_conversations_link("c2"),
                super::emit::not_trashed_conversation("c2")
            ),
        }
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

/// A WHERE fragment tying conversations alias `c2` to the base contact `ct`.
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

/// Which end of a contact's sent messages [`contact_heard`] reads.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Heard {
    /// The earliest message the contact sent.
    First,
    /// The latest message the contact sent.
    Last,
}

/// The timestamp of the first or last message contact `ct` sent: an
/// incoming message whose sender is one of the contact's handles, in any
/// conversation, direct or group, that is not in the trash, duplicates left
/// out. NULL when the contact never sent one, which is how `first-heard:`
/// matches no such contact and `-first-heard:` matches every one.
///
/// Your own messages and other people's messages in a shared group chat are
/// not hearing from this contact, which is why this is not
/// [`ListCtx::message_aggregate`]: that reads every message of every
/// conversation the contact is in. `ix_messages_sender_timestamp` answers
/// it per handle.
///
/// A free function for the same reason as [`contact_conversations_link`]:
/// it is only meaningful when the base row is a contact.
pub(crate) fn contact_heard(end: Heard) -> String {
    let agg = match end {
        Heard::First => "MIN",
        Heard::Last => "MAX",
    };
    format!(
        "(SELECT {agg}(mh.timestamp) FROM contact_handles chh \
           JOIN messages mh ON mh.sender_handle_id = chh.handle_id AND mh.account_id = chh.account_id \
           JOIN conversations ch ON ch.id = mh.conversation_id \
           WHERE chh.account_id = ct.account_id AND chh.contact_id = ct.id \
             AND mh.is_from_me = 0 AND mh.duplicate_of IS NULL AND {})",
        super::emit::not_trashed_conversation("ch")
    )
}
