//! Defaults plus one emitter per word. Every emitter writes SQL against the
//! innermost alias it needs and lets `ListCtx` wrap it for the base row.

use crate::db::contacts::UNKNOWN_CONTACT_SQL;
use crate::db::dialect::name_eq_ci;
use crate::db::engine::DbEngine;

use super::bridge::{ListCtx, Sql, contact_conversations_link};
use super::error::{QueryError, QueryErrorKind};
use super::fts;
use super::parse::{Expr, FieldTerm, TextTerm};
use super::value::{Cmp, DateCmp, Value, utc_instant};
use super::{Filter, ListKind};

/// Contact `ct` is not in the trash.
pub(crate) const NOT_TRASHED_CONTACT: &str = "NOT EXISTS (SELECT 1 FROM trashed_contacts tct WHERE tct.account_id = ct.account_id AND tct.contact_id = ct.id)";
/// Conversation `c` is not in the trash.
pub(crate) const NOT_TRASHED_CONVERSATION: &str = "NOT EXISTS (SELECT 1 FROM trashed_conversations tc WHERE tc.account_id = c.account_id AND tc.conversation_id = c.id)";

/// Conversation `conv` is not in the trash: [`NOT_TRASHED_CONVERSATION`]
/// for a subquery whose conversations alias is not `c`. The one clause,
/// written once, so the lists and the contact counts cannot drift apart.
pub(crate) fn not_trashed_conversation(conv: &str) -> String {
    format!(
        "NOT EXISTS (SELECT 1 FROM trashed_conversations tc WHERE tc.account_id = {conv}.account_id AND tc.conversation_id = {conv}.id)"
    )
}

/// Compile a parsed query into one parenthesised WHERE fragment.
pub(crate) fn compile(
    list: ListKind,
    expr: Option<&Expr>,
    account_id: i64,
    engine: DbEngine,
    zone: chrono_tz::Tz,
) -> Result<Filter, QueryError> {
    let ctx = ListCtx {
        list,
        engine,
        account_id,
        zone,
    };
    let mut out = Sql::default();
    out.push("(");
    out.push(ctx.account_col());
    out.push(" = ");
    out.bind_int(ctx.account_id);
    let uses = |word: &str| expr.is_some_and(|e| e.uses(word));
    match list {
        ListKind::Contacts => {
            if !uses("trashed") {
                out.push(" AND ");
                out.push(NOT_TRASHED_CONTACT);
            }
        }
        ListKind::Conversations => {
            if !uses("trashed") {
                out.push(" AND ");
                out.push(NOT_TRASHED_CONVERSATION);
            }
            // A thread with only duplicate messages is hidden, unless the
            // query is about an Import Run, whose threads may be exactly that.
            if !uses("import") {
                out.push(
                    " AND EXISTS (SELECT 1 FROM messages m0 WHERE m0.conversation_id = c.id AND m0.duplicate_of IS NULL)",
                );
            }
        }
        ListKind::Messages => {
            // A query about one source or one Import Run wants that backup's
            // or that run's copies, duplicates included: a re-imported run is
            // often nothing but duplicates.
            if !uses("source") && !uses("import") {
                out.push(" AND m.duplicate_of IS NULL");
            }
            if !uses("trashed") {
                out.push(
                    " AND EXISTS (SELECT 1 FROM conversations c WHERE c.id = m.conversation_id AND ",
                );
                out.push(NOT_TRASHED_CONVERSATION);
                out.push(")");
            }
        }
    }
    if let Some(expr) = expr {
        out.push(" AND ");
        emit_expr(&ctx, &mut out, expr)?;
    }
    out.push(")");
    Ok(Filter {
        where_sql: out.text,
        params: out.params,
    })
}

/// Write the SQL for one expression node, recursing into and, or, and not.
fn emit_expr(ctx: &ListCtx, out: &mut Sql, expr: &Expr) -> Result<(), QueryError> {
    match expr {
        Expr::And(parts) | Expr::Or(parts) => {
            let joiner = if matches!(expr, Expr::And(_)) {
                " AND "
            } else {
                " OR "
            };
            out.push("(");
            for (i, part) in parts.iter().enumerate() {
                if i > 0 {
                    out.push(joiner);
                }
                emit_expr(ctx, out, part)?;
            }
            out.push(")");
        }
        Expr::Not(inner) => {
            out.push("NOT (");
            emit_expr(ctx, out, inner)?;
            out.push(")");
        }
        Expr::Text(term) => emit_text(ctx, out, term),
        Expr::Field(term) => emit_field(ctx, out, term)?,
    }
    Ok(())
}

/// A contains-or-prefix pattern against `column`, case-insensitive on both
/// engines. A prefix means "a word starts with this", so it matches at the
/// start of the column or just after a space — never only at the very
/// start, which would make `avoc*` find less than `avoc`. Anything else, a
/// phrase included, is an ordinary substring.
///
/// The one place that turns text or a prefix into a LIKE pattern: free text
/// (`free_text_match`) and the text words (`text_match`) both go through it.
fn like_contains(out: &mut Sql, engine: DbEngine, column: &str, text: &str, prefix: bool) {
    if prefix {
        out.push("(");
        out.like(engine, column, &format!("{text}%"));
        out.push(" OR ");
        out.like(engine, column, &format!("% {text}%"));
        out.push(")");
    } else {
        out.like(engine, column, &format!("%{text}%"));
    }
}

/// One free-text test on `column`, for the lists matched with LIKE.
fn free_text_match(out: &mut Sql, engine: DbEngine, column: &str, term: &TextTerm) {
    match term {
        TextTerm::Term { text, prefix } => like_contains(out, engine, column, text, *prefix),
        TextTerm::Phrase(text) => like_contains(out, engine, column, text, false),
    }
}

/// A participant's display name: the linked contact's name, else the
/// per-conversation alias. Alias `p` is a participants row, `pct` its
/// contact. Shared by free text and `name:` so there is one copy of what
/// "this participant's name" means.
const PARTICIPANT_NAME: &str =
    "coalesce(NULLIF(trim(pct.preferred_name), ''), NULLIF(trim(p.name_alias), ''), '')";

/// FROM clause binding `p` to a participants row and `pct` to the Contact its
/// handle is on, scoped to conversation `c`'s account.
///
/// The route is `participants → contact_handles → contacts`, the same one
/// `db::participant_names` takes, because ADR-0006 says a handle counts as a
/// Contact's the moment it is on the Contact.
/// `participants.contact_id` is written once at import and never updated,
/// while the link in `contact_handles` changes whenever a handle is linked,
/// two contacts are merged, or an address book adopts someone. Joining on
/// `participants.contact_id` therefore showed one name in the conversation
/// list and found a different one with `name:`.
const PARTICIPANTS_WITH_CONTACT: &str = "participants p \
     LEFT JOIN contact_handles pch ON pch.handle_id = p.handle_id AND pch.account_id = c.account_id \
     LEFT JOIN contacts pct ON pct.id = pch.contact_id AND pct.account_id = c.account_id";

/// Free text: the row's own text, one meaning applied per row type.
fn emit_text(ctx: &ListCtx, out: &mut Sql, term: &TextTerm) {
    let e = ctx.engine;
    match ctx.list {
        ListKind::Contacts => {
            out.push("(");
            free_text_match(
                out,
                e,
                "COALESCE(NULLIF(trim(ct.preferred_name), ''), '(unknown)')",
                term,
            );
            out.push(
                " OR EXISTS (SELECT 1 FROM contact_handles ch JOIN handles h ON h.id = ch.handle_id WHERE ch.account_id = ct.account_id AND ch.contact_id = ct.id AND (",
            );
            free_text_match(out, e, "h.raw", term);
            out.push(" OR ");
            free_text_match(out, e, "coalesce(h.normalized, '')", term);
            out.push(")))");
        }
        ListKind::Conversations => {
            out.push("(");
            free_text_match(out, e, "coalesce(c.group_title, '')", term);
            out.push(" OR EXISTS (SELECT 1 FROM handles hc WHERE hc.id = c.chat_handle_id AND ");
            free_text_match(out, e, "hc.raw", term);
            // The handle join is a LEFT join: a source may name a participant
            // and record no address for them, and that person is searchable by
            // the name the source gave.
            out.push(&format!(
                ") OR EXISTS (SELECT 1 FROM {PARTICIPANTS_WITH_CONTACT} LEFT JOIN handles ph ON ph.id = p.handle_id WHERE p.conversation_id = c.id AND ("
            ));
            free_text_match(out, e, "coalesce(ph.raw, '')", term);
            out.push(" OR ");
            free_text_match(out, e, PARTICIPANT_NAME, term);
            out.push(")))");
        }
        // The index, or an attachment's file name. Both are needed: a file
        // name is one token to Postgres's text parser, so "IMG_0001" never
        // reaches "IMG_0001.jpg" through the index there, while SQLite's
        // tokenizer does split it. The file-name match makes the two engines
        // agree and makes part of a file name findable on either.
        ListKind::Messages => {
            out.push("(");
            fts::leaf(out, e, term);
            out.push(" OR EXISTS (SELECT 1 FROM attachments a WHERE a.message_id = m.id AND ");
            free_text_match(out, e, "coalesce(a.original_name, '')", term);
            out.push("))");
        }
    }
}

/// One `word:values`. The values are OR-ed, so `body:avocado,guac` is
/// either. Every word in the registry has an arm in `emit_one`; a word that
/// somehow reaches it without one is refused by name rather than quietly
/// matching.
fn emit_field(ctx: &ListCtx, out: &mut Sql, term: &FieldTerm) -> Result<(), QueryError> {
    out.push("(");
    for (i, value) in term.values.iter().enumerate() {
        if i > 0 {
            out.push(" OR ");
        }
        emit_one(ctx, out, term, value)?;
    }
    out.push(")");
    Ok(())
}

/// One value of one word, written against the innermost alias it needs.
fn emit_one(ctx: &ListCtx, out: &mut Sql, term: &FieldTerm, v: &Value) -> Result<(), QueryError> {
    match term.spec.word {
        "body" | "subject" | "name" | "title" | "handle" | "filename" => {
            emit_text_word(ctx, out, term, v)
        }
        "with" | "from" | "to" | "in" | "group" | "tag" | "import" => {
            emit_people_word(ctx, out, term, v)
        }
        "kind" | "service" | "source" | "attachment" | "size" | "trashed" => {
            emit_kind_word(ctx, out, term, v)
        }
        "date" | "first-message" | "last-message" | "messages" | "conversations" | "groups"
        | "participants" | "attachments" => emit_measure_word(ctx, out, term, v),
        other => Err(QueryError::new(
            QueryErrorKind::BadValue,
            term.span.clone(),
            format!("{other}: has no emitter; add one in emit.rs"),
        )),
    }
}

/// A refusal naming `term`'s word, for a value shape the word does not
/// accept. `what` is the tail of the sentence: `"needs text, a prefix, or
/// none/any."`, `"needs a name or #id."`, and so on.
fn bad_value(term: &FieldTerm, what: &str) -> QueryError {
    QueryError::new(
        QueryErrorKind::BadValue,
        term.span.clone(),
        format!("{}: {what}", term.spec.word),
    )
}

/// `column` contains `v` (a text value), starts with it (a prefix value),
/// is empty (`none`), or is not empty (`any`). The LIKE pattern itself is
/// `like_contains`, shared with free text.
///
/// The other `Value` shapes never reach a text word: the parser only ever
/// hands a `Text`-typed word a `Text`, a `Prefix`, or one of its own
/// `values` keywords (`none`/`any`, or nothing for a word like `filename`
/// that declares neither). That arm exists only so the match is exhaustive;
/// it refuses by name rather than emitting a fallback that quietly matches
/// everything or nothing.
fn text_match(
    out: &mut Sql,
    engine: DbEngine,
    column: &str,
    term: &FieldTerm,
    v: &Value,
) -> Result<(), QueryError> {
    match v {
        Value::Text(t) => {
            like_contains(out, engine, column, t, false);
            Ok(())
        }
        Value::Prefix(p) => {
            like_contains(out, engine, column, p, true);
            Ok(())
        }
        Value::Keyword("none") => {
            out.push(&format!("NULLIF(trim({column}), '') IS NULL"));
            Ok(())
        }
        Value::Keyword("any") => {
            out.push(&format!("NULLIF(trim({column}), '') IS NOT NULL"));
            Ok(())
        }
        _ => Err(bad_value(term, "needs text, a prefix, or none/any.")),
    }
}

/// The six text words. On Contacts, `name:` and `handle:` look at the
/// contact itself; everywhere else they look at the conversation's
/// participants. `body:`, `subject:`, and `filename:` always look at
/// messages (and their attachments); `title:` always looks at the
/// conversation.
fn emit_text_word(
    ctx: &ListCtx,
    out: &mut Sql,
    term: &FieldTerm,
    v: &Value,
) -> Result<(), QueryError> {
    let e = ctx.engine;
    let mut result: Result<(), QueryError> = Ok(());
    match (term.spec.word, ctx.list) {
        ("body", _) => ctx.message(out, |o| {
            result = text_match(o, e, "coalesce(m.body, '')", term, v);
        }),
        ("subject", _) => ctx.message(out, |o| {
            result = text_match(o, e, "coalesce(m.subject, '')", term, v);
        }),
        ("title", _) => ctx.conversation(out, |o| {
            result = text_match(o, e, "coalesce(c.group_title, '')", term, v);
        }),
        ("name", ListKind::Contacts) => {
            result = text_match(out, e, "ct.preferred_name", term, v);
        }
        ("name", _) => ctx.conversation(out, |o| {
            o.push(&format!(
                "EXISTS (SELECT 1 FROM {PARTICIPANTS_WITH_CONTACT} WHERE p.conversation_id = c.id AND "
            ));
            result = text_match(o, e, PARTICIPANT_NAME, term, v);
            o.push(")");
        }),
        ("handle", ListKind::Contacts) => match v {
            Value::Keyword("none") => out.push(
                "NOT EXISTS (SELECT 1 FROM contact_handles ch WHERE ch.account_id = ct.account_id AND ch.contact_id = ct.id)",
            ),
            Value::Keyword("any") => out.push(
                "EXISTS (SELECT 1 FROM contact_handles ch WHERE ch.account_id = ct.account_id AND ch.contact_id = ct.id)",
            ),
            Value::Text(_) | Value::Prefix(_) => {
                out.push(
                    "EXISTS (SELECT 1 FROM contact_handles ch JOIN handles h ON h.id = ch.handle_id WHERE ch.account_id = ct.account_id AND ch.contact_id = ct.id AND (",
                );
                result = text_match(out, e, "h.raw", term, v);
                out.push(" OR ");
                if result.is_ok() {
                    result = text_match(out, e, "coalesce(h.normalized, '')", term, v);
                }
                out.push("))");
            }
            _ => {
                result = Err(bad_value(term, "needs text, a prefix, or none/any."));
            }
        },
        ("handle", _) => ctx.conversation(out, |o| match v {
            Value::Keyword("none") => o.push(
                "NOT EXISTS (SELECT 1 FROM participants p WHERE p.conversation_id = c.id AND p.handle_id IS NOT NULL)",
            ),
            // The true complement of `none`: some participant does have a
            // handle. Never `1=1` — a conversation can be all name-only
            // participants (see `named_participant` in the fixture), and
            // `any` must not match those.
            Value::Keyword("any") => o.push(
                "EXISTS (SELECT 1 FROM participants p WHERE p.conversation_id = c.id AND p.handle_id IS NOT NULL)",
            ),
            Value::Text(_) | Value::Prefix(_) => {
                o.push(
                    "EXISTS (SELECT 1 FROM handles h WHERE (h.id = c.chat_handle_id OR EXISTS (SELECT 1 FROM participants p WHERE p.conversation_id = c.id AND p.handle_id = h.id)) AND (",
                );
                result = text_match(o, e, "h.raw", term, v);
                o.push(" OR ");
                if result.is_ok() {
                    result = text_match(o, e, "coalesce(h.normalized, '')", term, v);
                }
                o.push("))");
            }
            _ => {
                result = Err(bad_value(term, "needs text, a prefix, or none/any."));
            }
        }),
        ("filename", _) => ctx.message(out, |o| {
            o.push("EXISTS (SELECT 1 FROM attachments a WHERE a.message_id = m.id AND ");
            result = text_match(o, e, "coalesce(a.original_name, '')", term, v);
            o.push(")");
        }),
        _ => {
            result = Err(QueryError::new(
                QueryErrorKind::BadValue,
                term.span.clone(),
                format!("{}: is not built yet.", term.spec.word),
            ));
        }
    }
    result
}

/// The handle with id `handle_id_expr` belongs to the person `v`: by contact
/// id, or by a contains-or-prefix match on the handle or the contact's name.
fn person_matches(
    out: &mut Sql,
    engine: DbEngine,
    handle_id_expr: &str,
    term: &FieldTerm,
    v: &Value,
) -> Result<(), QueryError> {
    match v {
        Value::Id(id) => {
            out.push(&format!(
                "EXISTS (SELECT 1 FROM contact_handles chp WHERE chp.handle_id = {handle_id_expr} AND chp.contact_id = "
            ));
            out.bind_int(*id);
            out.push(")");
            Ok(())
        }
        Value::Text(t) | Value::Prefix(t) => {
            let prefix = matches!(v, Value::Prefix(_));
            out.push(&format!(
                "EXISTS (SELECT 1 FROM handles hp LEFT JOIN contact_handles chp ON chp.handle_id = hp.id AND chp.account_id = hp.account_id LEFT JOIN contacts ctp ON ctp.id = chp.contact_id WHERE hp.id = {handle_id_expr} AND ("
            ));
            like_contains(out, engine, "hp.raw", t, prefix);
            out.push(" OR ");
            like_contains(out, engine, "coalesce(hp.normalized, '')", t, prefix);
            out.push(" OR ");
            like_contains(out, engine, "coalesce(ctp.preferred_name, '')", t, prefix);
            out.push("))");
            Ok(())
        }
        _ => Err(bad_value(term, "needs a name, a handle, or #id.")),
    }
}

/// The participant row `p` (with `pct` the Contact its handle is on, when
/// any, in scope via [`PARTICIPANTS_WITH_CONTACT`]) is itself the person `v`:
/// by contact id, or by a contains-or-prefix match on their display name.
/// This is how `with:` reaches a participant the source only named —
/// `handle_id` NULL, so `person_matches` on it never sees them, and neither
/// does the `contact_handles` join, which leaves `pct` NULL and the name
/// coming from `p.name_alias`.
fn participant_matches(
    out: &mut Sql,
    engine: DbEngine,
    term: &FieldTerm,
    v: &Value,
) -> Result<(), QueryError> {
    match v {
        Value::Id(id) => {
            out.push("p.contact_id = ");
            out.bind_int(*id);
            Ok(())
        }
        Value::Text(t) | Value::Prefix(t) => {
            let prefix = matches!(v, Value::Prefix(_));
            like_contains(out, engine, PARTICIPANT_NAME, t, prefix);
            Ok(())
        }
        _ => Err(bad_value(term, "needs a name, a handle, or #id.")),
    }
}

/// Some party to conversation `c` is `v`: its chat handle, a participant's
/// handle, or a participant the source only named (see `participant_matches`).
fn with_person(
    ctx: &ListCtx,
    out: &mut Sql,
    term: &FieldTerm,
    v: &Value,
) -> Result<(), QueryError> {
    let e = ctx.engine;
    let mut result = Ok(());
    ctx.conversation(out, |o| {
        o.push("(");
        result = person_matches(o, e, "c.chat_handle_id", term, v);
        o.push(&format!(
            " OR EXISTS (SELECT 1 FROM {PARTICIPANTS_WITH_CONTACT} WHERE p.conversation_id = c.id AND ("
        ));
        if result.is_ok() {
            result = person_matches(o, e, "p.handle_id", term, v);
        }
        o.push(" OR ");
        if result.is_ok() {
            result = participant_matches(o, e, term, v);
        }
        o.push(")))");
    });
    result
}

/// A named set a row can belong to: Contact Groups hold contacts, Message
/// Tags hold conversations. `group:` and `tag:` differ only in these names
/// and in which list the members live on, so both words share one emitter.
#[derive(Debug, Clone, Copy)]
struct NamedSet {
    /// The list whose base row is a member: Contacts or Conversations.
    home: ListKind,
    /// Table of the sets themselves (`ns` in the SQL).
    table: &'static str,
    /// Membership table (`nm`).
    members: &'static str,
    /// `nm` column naming the set.
    set_col: &'static str,
    /// `nm` column naming the member row.
    member_col: &'static str,
    /// The member row's id, written against the home list's alias.
    row_expr: &'static str,
    /// The member row's account column, written against the home alias.
    account_expr: &'static str,
}

const CONTACT_GROUPS: NamedSet = NamedSet {
    home: ListKind::Contacts,
    table: "contact_groups",
    members: "contact_group_members",
    set_col: "group_id",
    member_col: "contact_id",
    row_expr: "ct.id",
    account_expr: "ct.account_id",
};

const MESSAGE_TAGS: NamedSet = NamedSet {
    home: ListKind::Conversations,
    table: "message_tags",
    members: "message_tag_members",
    set_col: "tag_id",
    member_col: "conversation_id",
    row_expr: "c.id",
    account_expr: "c.account_id",
};

impl NamedSet {
    /// `SELECT 1 FROM members JOIN sets ... WHERE <row is a member> AND ns.account_id = ...`,
    /// left open for the caller to add its own condition or close.
    fn membership_from(&self) -> String {
        let Self {
            table,
            members,
            set_col,
            member_col,
            row_expr,
            account_expr,
            ..
        } = self;
        format!(
            "SELECT 1 FROM {members} nm JOIN {table} ns ON ns.id = nm.{set_col} WHERE nm.{member_col} = {row_expr} AND ns.account_id = {account_expr}"
        )
    }

    /// The home row is a member of the set `v` names. Handles `#id` and a
    /// case-insensitive name; a prefix means "a name that starts with this".
    fn contains(
        &self,
        out: &mut Sql,
        engine: DbEngine,
        term: &FieldTerm,
        v: &Value,
    ) -> Result<(), QueryError> {
        out.push(&format!("EXISTS ({} AND ", self.membership_from()));
        let result = match v {
            Value::Id(id) => {
                out.push("ns.id = ");
                out.bind_int(*id);
                Ok(())
            }
            Value::Text(t) => {
                out.push(&name_eq_ci(engine, "ns.name", "?"));
                out.param_text(t.clone());
                Ok(())
            }
            Value::Prefix(t) => {
                like_contains(out, engine, "ns.name", t, true);
                Ok(())
            }
            _ => Err(bad_value(term, "needs a name or #id.")),
        };
        out.push(")");
        result
    }

    /// The home row is in no set at all.
    fn none(&self, out: &mut Sql) {
        out.push(&format!("NOT EXISTS ({})", self.membership_from()));
    }
}

/// The seven people-and-places words: `with`, `from`, `to`, `in`, `group`,
/// `tag`, `import`. `from:` and `to:` are Messages-only in the registry, so
/// they read `m.` directly; `with:`, `group:`, `tag:`, and `import:` go
/// through the bridges so they work on every list the registry allows.
fn emit_people_word(
    ctx: &ListCtx,
    out: &mut Sql,
    term: &FieldTerm,
    v: &Value,
) -> Result<(), QueryError> {
    match term.spec.word {
        "with" => with_person(ctx, out, term, v),
        "from" => emit_from(ctx, out, term, v),
        "to" => emit_to(ctx, out, term, v),
        "in" => emit_in(ctx, out, term, v),
        "group" => emit_set_word(ctx, out, term, v, CONTACT_GROUPS),
        "tag" => emit_set_word(ctx, out, term, v, MESSAGE_TAGS),
        "import" => emit_import(ctx, out, term, v),
        _ => Err(bad_value(term, "is not built yet.")),
    }
}

/// `from:me` is the outgoing flag; `from:<person>` is an incoming message
/// whose sender handle is that person.
fn emit_from(ctx: &ListCtx, out: &mut Sql, term: &FieldTerm, v: &Value) -> Result<(), QueryError> {
    if matches!(v, Value::Keyword("me")) {
        out.push("m.is_from_me = 1");
        return Ok(());
    }
    out.push("(m.is_from_me = 0 AND m.sender_handle_id IS NOT NULL AND ");
    let result = person_matches(out, ctx.engine, "m.sender_handle_id", term, v);
    out.push(")");
    result
}

/// `to:me` is any incoming message; `to:<person>` is a message in a
/// conversation with that person that they did not send themselves.
fn emit_to(ctx: &ListCtx, out: &mut Sql, term: &FieldTerm, v: &Value) -> Result<(), QueryError> {
    if matches!(v, Value::Keyword("me")) {
        out.push("m.is_from_me = 0");
        return Ok(());
    }
    out.push("(");
    let mut result = with_person(ctx, out, term, v);
    out.push(" AND (m.is_from_me = 1 OR m.sender_handle_id IS NULL OR NOT ");
    if result.is_ok() {
        result = person_matches(out, ctx.engine, "m.sender_handle_id", term, v);
    }
    out.push("))");
    result
}

/// `in:#id` names a conversation; `in:<text>` matches its group title or its
/// chat handle.
fn emit_in(ctx: &ListCtx, out: &mut Sql, term: &FieldTerm, v: &Value) -> Result<(), QueryError> {
    match v {
        Value::Id(id) => {
            out.push("m.conversation_id = ");
            out.bind_int(*id);
            Ok(())
        }
        Value::Text(t) | Value::Prefix(t) => {
            let prefix = matches!(v, Value::Prefix(_));
            let e = ctx.engine;
            ctx.conversation(out, |o| {
                o.push("(");
                like_contains(o, e, "coalesce(c.group_title, '')", t, prefix);
                o.push(" OR EXISTS (SELECT 1 FROM handles hc WHERE hc.id = c.chat_handle_id AND ");
                like_contains(o, e, "hc.raw", t, prefix);
                o.push("))");
            });
            Ok(())
        }
        _ => Err(bad_value(term, "needs a name or #id.")),
    }
}

/// `group:` and `tag:`. On the set's home list the base row is the member;
/// on every other list the bridge reaches the member rows. `none` on a
/// bridged list is a double negation around the bridge's EXISTS, so it reads
/// "no row they reach is in any set": a contact with one tagged conversation
/// is out of `tag:none` even when their other conversations carry no tag.
fn emit_set_word(
    ctx: &ListCtx,
    out: &mut Sql,
    term: &FieldTerm,
    v: &Value,
    set: NamedSet,
) -> Result<(), QueryError> {
    match v {
        Value::Keyword("none") if ctx.list == set.home => {
            set.none(out);
            Ok(())
        }
        Value::Keyword("none") => {
            out.push("NOT ");
            ctx.reach(set.home, out, |o| {
                o.push("NOT ");
                set.none(o);
            });
            Ok(())
        }
        Value::Keyword("unknown") if set.home == ListKind::Contacts => {
            ctx.contact(out, |o| o.push(UNKNOWN_CONTACT_SQL));
            Ok(())
        }
        _ => {
            let mut result = Ok(());
            ctx.reach(set.home, out, |o| {
                result = set.contains(o, ctx.engine, term, v);
            });
            result
        }
    }
}

/// `import:#id` and `import:last`. Never through `ctx.message`: that bridge's
/// Conversations shape requires a non-duplicate message, and an Import Run's
/// whole point is to find rows a later run marked as duplicates. So this
/// writes its own EXISTS on Conversations, and compares `m.import_id`
/// directly on Messages (whose own duplicate default is already skipped for
/// `import:` in `compile`).
fn emit_import(
    ctx: &ListCtx,
    out: &mut Sql,
    term: &FieldTerm,
    v: &Value,
) -> Result<(), QueryError> {
    let run = match v {
        Value::Keyword("last") => None,
        Value::Id(id) => Some(*id),
        _ => return Err(bad_value(term, "needs #id or last.")),
    };
    let on_messages = ctx.list == ListKind::Messages;
    let alias = if on_messages { "m" } else { "mi" };
    if !on_messages {
        out.push("EXISTS (SELECT 1 FROM messages mi WHERE mi.conversation_id = c.id AND ");
    }
    out.push(&format!("{alias}.import_id = "));
    if let Some(id) = run {
        out.bind_int(id)
    } else {
        out.push("(SELECT MAX(vi.id) FROM vault_imports vi WHERE vi.account_id = ");
        out.bind_int(ctx.account_id);
        out.push(")");
    }
    if !on_messages {
        out.push(")");
    }
    Ok(())
}
/// `expr <op> ?` for a size comparison; a range is two bounds, both bound in
/// textual order.
fn cmp_sql(out: &mut Sql, expr: &str, cmp: &Cmp<i64>) {
    let (op, val) = match cmp {
        Cmp::Eq(v) => ("=", *v),
        Cmp::Gt(v) => (">", *v),
        Cmp::Gte(v) => (">=", *v),
        Cmp::Lt(v) => ("<", *v),
        Cmp::Lte(v) => ("<=", *v),
        Cmp::Range(a, b) => {
            out.push(&format!("({expr} >= "));
            out.bind_int(*a);
            out.push(&format!(" AND {expr} <= "));
            out.bind_int(*b);
            out.push(")");
            return;
        }
    };
    out.push(&format!("{expr} {op} "));
    out.bind_int(val);
}

/// Attachment `a` is of this kind, by MIME type.
fn attachment_kind_sql(kind: &str) -> String {
    const MIME: &str = "lower(coalesce(a.mime_type, ''))";
    let image = format!("{MIME} LIKE 'image/%'");
    let video = format!("{MIME} LIKE 'video/%'");
    let audio = format!("{MIME} LIKE 'audio/%'");
    let pdf = format!("{MIME} = 'application/pdf'");
    let contact = format!("{MIME} IN ('text/vcard', 'text/x-vcard')");
    let document = format!(
        "({pdf} OR {MIME} LIKE 'text/%' OR {MIME} LIKE 'application/vnd.%' OR {MIME} IN ('application/msword', 'application/rtf'))"
    );
    match kind {
        "image" => image,
        "video" => video,
        "audio" => audio,
        "pdf" => pdf,
        "contact" => contact,
        "document" => format!("({document} AND NOT {contact})"),
        _ => format!("NOT ({image} OR {video} OR {audio} OR {document} OR {contact})"),
    }
}

/// The `source` word's values, mapped to the id an importer writes onto the
/// message row it wrote.
fn source_id(choice: &str) -> &'static str {
    match choice {
        "imessage" => "imessage",
        "whatsapp" => "whatsapp",
        _ => "sms-backup-restore",
    }
}

/// The six kind-and-attachment words: `kind`, `service`, `source`,
/// `attachment`, `size`, `trashed`. `kind:`, `service:`, and `source:` bind
/// their mapped value with `bind_text` rather than interpolating it, even
/// though the value is one the code chose, not user text, so no value ever
/// reaches the SQL text directly. `trashed:` reuses the same "not trashed"
/// constants the per-list defaults do: on Contacts and Conversations it
/// reads `ct.`/`c.` directly, the base row's own alias; on Messages, where
/// `c` is not in scope, it goes through the `conversation` bridge instead.
fn emit_kind_word(
    ctx: &ListCtx,
    out: &mut Sql,
    term: &FieldTerm,
    v: &Value,
) -> Result<(), QueryError> {
    match (term.spec.word, v) {
        ("kind", Value::Choice(k)) => {
            let ty = if *k == "direct" {
                "individual"
            } else {
                "group"
            };
            ctx.conversation(out, |o| {
                o.push("c.conversation_type = ");
                o.bind_text(ty);
            });
            Ok(())
        }
        ("service", Value::Choice(s)) => {
            let s = s.to_string();
            ctx.message(out, |o| {
                o.push("lower(coalesce(m.service, '')) = ");
                o.bind_text(s);
            });
            Ok(())
        }
        ("source", Value::Choice(s)) => {
            let id = source_id(s);
            ctx.message(out, |o| {
                o.push("m.source = ");
                o.bind_text(id);
            });
            Ok(())
        }
        ("attachment", Value::Choice("any")) => {
            ctx.message(out, |o| {
                o.push("EXISTS (SELECT 1 FROM attachments a WHERE a.message_id = m.id)");
            });
            Ok(())
        }
        ("attachment", Value::Choice("none")) => {
            ctx.message(out, |o| {
                o.push("NOT EXISTS (SELECT 1 FROM attachments a WHERE a.message_id = m.id)");
            });
            Ok(())
        }
        ("attachment", Value::Choice(k)) => {
            let pred = attachment_kind_sql(k);
            ctx.message(out, |o| {
                o.push(&format!(
                    "EXISTS (SELECT 1 FROM attachments a WHERE a.message_id = m.id AND {pred})"
                ));
            });
            Ok(())
        }
        ("size", Value::Size(cmp)) => {
            ctx.message(out, |o| {
                o.push(
                    "EXISTS (SELECT 1 FROM attachments a WHERE a.message_id = m.id AND a.size_bytes IS NOT NULL AND ",
                );
                cmp_sql(o, "a.size_bytes", cmp);
                o.push(")");
            });
            Ok(())
        }
        ("trashed", Value::Choice(flag)) => {
            let not_trashed = match ctx.list {
                ListKind::Contacts => NOT_TRASHED_CONTACT,
                _ => NOT_TRASHED_CONVERSATION,
            };
            let write = |o: &mut Sql| match *flag {
                "no" => o.push(not_trashed),
                "yes" => o.push(&format!("NOT ({not_trashed})")),
                // "any" lifts the default and filters nothing: the one place
                // an always-true predicate is legitimate, since the word
                // itself means "show every row regardless of trash state".
                _ => o.push("1=1"),
            };
            match ctx.list {
                // `c` is not the base alias on Messages, so this needs the
                // same bridge every conversation-scoped word on Messages uses.
                ListKind::Messages => ctx.conversation(out, write),
                ListKind::Contacts | ListKind::Conversations => write(out),
            }
            Ok(())
        }
        _ => Err(bad_value(term, "needs a value this word accepts.")),
    }
}

/// `expr` (a stored UTC instant as RFC 3339 text) falls where `cmp` says.
/// Each day bound becomes the instant that day begins in the account's zone,
/// written in the same UTC text form, so the comparison is plain text on
/// both engines and the zone decides which day a message belongs to.
fn date_sql(out: &mut Sql, expr: &str, cmp: &DateCmp, zone: chrono_tz::Tz) {
    match cmp {
        DateCmp::In(span) => {
            out.push(&format!("({expr} >= "));
            out.bind_text(utc_instant(zone, span.start));
            out.push(&format!(" AND {expr} < "));
            out.bind_text(utc_instant(zone, span.end));
            out.push(")");
        }
        DateCmp::Gte(d) | DateCmp::Gt(d) => {
            out.push(&format!("{expr} >= "));
            out.bind_text(utc_instant(zone, *d));
        }
        DateCmp::Lt(d) | DateCmp::Lte(d) => {
            out.push(&format!("{expr} < "));
            out.bind_text(utc_instant(zone, *d));
        }
    }
}

/// The eight date-and-count words: `date`, `first-message`, `last-message`,
/// `messages`, `conversations`, `groups`, `participants`, `attachments`.
/// `first-message:` and `last-message:` compare through a correlated MIN or
/// MAX rather than building a list of ids first; the five plural words are
/// correlated counts. `groups:` and `conversations:` are registered for
/// Contacts only, so they read `ct.` directly; `attachments:` is registered
/// for Messages only, so it reads `m.` directly.
fn emit_measure_word(
    ctx: &ListCtx,
    out: &mut Sql,
    term: &FieldTerm,
    v: &Value,
) -> Result<(), QueryError> {
    match (term.spec.word, v) {
        ("date", Value::Date(cmp)) => {
            ctx.message(out, |o| date_sql(o, "m.timestamp", cmp, ctx.zone));
            Ok(())
        }
        ("first-message", Value::Date(cmp)) => {
            let expr = format!(
                "(SELECT MIN(m2.timestamp) FROM messages m2 WHERE {})",
                ctx.messages_link("m2")
            );
            date_sql(out, &expr, cmp, ctx.zone);
            Ok(())
        }
        ("last-message", Value::Date(cmp)) => {
            let expr = format!(
                "(SELECT MAX(m2.timestamp) FROM messages m2 WHERE {})",
                ctx.messages_link("m2")
            );
            date_sql(out, &expr, cmp, ctx.zone);
            Ok(())
        }
        ("messages", Value::Count(cmp)) => {
            let expr = format!(
                "(SELECT COUNT(*) FROM messages m2 WHERE {})",
                ctx.messages_link("m2")
            );
            cmp_sql(out, &expr, cmp);
            Ok(())
        }
        ("conversations", Value::Count(cmp)) => {
            let expr = format!(
                "(SELECT COUNT(*) FROM conversations c2 WHERE {})",
                contact_conversations_link("c2")
            );
            cmp_sql(out, &expr, cmp);
            Ok(())
        }
        ("groups", Value::Count(cmp)) => {
            cmp_sql(
                out,
                "(SELECT COUNT(*) FROM contact_group_members cgm JOIN contact_groups cg ON cg.id = cgm.group_id WHERE cgm.contact_id = ct.id AND cg.account_id = ct.account_id)",
                cmp,
            );
            Ok(())
        }
        ("participants", Value::Count(cmp)) => {
            ctx.conversation(out, |o| {
                cmp_sql(
                    o,
                    "(SELECT COUNT(*) FROM participants p WHERE p.conversation_id = c.id)",
                    cmp,
                );
            });
            Ok(())
        }
        ("attachments", Value::Count(cmp)) => {
            cmp_sql(
                out,
                "(SELECT COUNT(*) FROM attachments a WHERE a.message_id = m.id)",
                cmp,
            );
            Ok(())
        }
        _ => Err(bad_value(term, "needs a value this word accepts.")),
    }
}
