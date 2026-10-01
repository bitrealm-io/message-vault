//! Tokens to an expression tree. Every `word:` is resolved against the
//! registry for the requested list here, so the emitters never see a word
//! they do not own.

use std::fmt::Write as _;
use std::ops::Range;

use chrono::NaiveDate;

use super::ListKind;
use super::error::{QueryError, QueryErrorKind};
use super::fields::{self, FieldSpec, ValueType};
use super::lex::{Token, TokenKind};
use super::value::{self, Value};

/// A free-text leaf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TextTerm {
    Term { text: String, prefix: bool },
    Phrase(String),
}

/// One `word:values`.
#[derive(Debug)]
pub(crate) struct FieldTerm {
    pub spec: &'static FieldSpec,
    /// Comma-separated values, meaning OR.
    pub values: Vec<Value>,
    pub span: Range<usize>,
}

/// The parsed query.
#[derive(Debug)]
pub(crate) enum Expr {
    And(Vec<Expr>),
    Or(Vec<Expr>),
    Not(Box<Expr>),
    Field(FieldTerm),
    Text(TextTerm),
}

/// At most this many free-text terms in one query.
const MAX_TEXT_TERMS: usize = 32;
/// At most this many expression nodes in one query.
const MAX_NODES: usize = 64;
/// At most this much parenthesis/negation nesting.
const MAX_DEPTH: usize = 32;

impl Expr {
    /// True when any field term in the tree is `word`.
    pub(crate) fn uses(&self, word: &str) -> bool {
        match self {
            Self::And(v) | Self::Or(v) => v.iter().any(|e| e.uses(word)),
            Self::Not(e) => e.uses(word),
            Self::Field(t) => t.spec.word == word,
            Self::Text(_) => false,
        }
    }

    /// Total nodes in the tree, for the complexity limit.
    fn nodes(&self) -> usize {
        match self {
            Self::And(v) | Self::Or(v) => 1 + v.iter().map(Expr::nodes).sum::<usize>(),
            Self::Not(e) => 1 + e.nodes(),
            Self::Field(_) | Self::Text(_) => 1,
        }
    }

    /// Free-text terms in the tree, for the complexity limit.
    fn text_terms(&self) -> usize {
        match self {
            Self::And(v) | Self::Or(v) => v.iter().map(Expr::text_terms).sum(),
            Self::Not(e) => e.text_terms(),
            Self::Field(_) => 0,
            Self::Text(_) => 1,
        }
    }
}

/// What a valid value for this field looks like, for the error message.
fn value_hint(spec: &FieldSpec) -> String {
    let choices = || spec.values.join(", ");
    match spec.value_type {
        ValueType::Date => "Write a year, a month like 2024-05, a day, or a span like 7d, with >, >=, <, <=, or a..b.".into(),
        ValueType::Count => "Write a number, with >, >=, <, <=, or a..b.".into(),
        ValueType::Size => "Write a size like 500k, 1M, or 2G, with >, >=, <, <=, or a..b.".into(),
        ValueType::Choice | ValueType::Flag => format!("Write one of: {}.", choices()),
        ValueType::Name if spec.word == "import" => "Write #id or last.".into(),
        ValueType::Name if spec.values.is_empty() => "Write a name, pre* for a prefix, or #id.".into(),
        ValueType::Name => format!("Write a name, pre* for a prefix, #id, or one of: {}.", choices()),
        ValueType::Person if spec.values.is_empty() => "Write a name, a handle, pre* for a prefix, or #id.".into(),
        ValueType::Person => format!("Write a name, a handle, pre* for a prefix, #id, or one of: {}.", choices()),
        ValueType::Text if spec.values.is_empty() => "Write some text, or pre* for a prefix.".into(),
        ValueType::Text => format!("Write some text, pre* for a prefix, or one of: {}.", choices()),
    }
}

/// One value for `spec`, restricted to the shapes that word's meaning
/// allows: `import:` (the only Name word without a name fallback) takes only
/// `#id` or its `last` keyword. Every `Text`, `Name`, and `Person` word takes
/// a trailing `*` as a prefix, unquoted and non-empty before the star.
fn parse_one_value(spec: &FieldSpec, raw: &str, quoted: bool, today: NaiveDate) -> Option<Value> {
    let lower = raw.trim().to_ascii_lowercase();
    if let Some(kw) = spec.values.iter().find(|v| **v == lower) {
        return Some(match spec.value_type {
            ValueType::Choice | ValueType::Flag => Value::Choice(kw),
            _ => Value::Keyword(kw),
        });
    }
    let text_or_prefix = || {
        if !quoted
            && let Some(p) = raw.strip_suffix('*')
            && !p.is_empty()
        {
            Value::Prefix(p.to_string())
        } else {
            Value::Text(raw.trim().to_string())
        }
    };
    match spec.value_type {
        ValueType::Choice | ValueType::Flag => None,
        ValueType::Text => Some(text_or_prefix()),
        ValueType::Name if spec.word == "import" => value::parse_id(raw).map(Value::Id),
        ValueType::Name | ValueType::Person => {
            if !quoted && let Some(id) = value::parse_id(raw) {
                Some(Value::Id(id))
            } else {
                Some(text_or_prefix())
            }
        }
        ValueType::Date => value::parse_date(raw, today).map(Value::Date),
        ValueType::Count => value::parse_cmp(raw, value::parse_count).map(Value::Count),
        ValueType::Size => value::parse_cmp(raw, value::parse_size_bytes).map(Value::Size),
    }
}

/// `word:` with nothing usable after the colon: a missing value, or a value
/// list that was empty once commas were split out (`tag:,`). Names a second,
/// different example from `spec.values` when one exists, so a word whose
/// `example` already spells its only keyword (`import:last`) is not told to
/// try `import:last` twice.
fn empty_value_error(spec: &FieldSpec, word: &str, span: Range<usize>) -> QueryError {
    let also = spec
        .values
        .iter()
        .find(|v| format!("{word}:{v}") != spec.example)
        .map(|v| format!(" or {word}:{v}"))
        .unwrap_or_default();
    let mut err = QueryError::new(
        QueryErrorKind::EmptyValue,
        span,
        format!("{word}: needs a value, for example {}{also}.", spec.example),
    );
    err.field = Some(spec.word);
    err
}

/// Parse one `field:value` pair into a typed term, or explain why the value does not fit the field.
fn field_term(
    list: ListKind,
    word: &str,
    raw: &str,
    quoted: bool,
    span: Range<usize>,
    today: NaiveDate,
) -> Result<FieldTerm, QueryError> {
    let Some(spec) = fields::lookup(word) else {
        let mut err = QueryError::new(
            QueryErrorKind::UnknownWord,
            span,
            format!("{word}: is not a search word."),
        );
        if let Some(near) = fields::nearest(word, list) {
            let _ = write!(err.message, " Did you mean {near}:?");
            err.did_you_mean = Some(near);
        }
        return Err(err);
    };
    if !spec.lists.contains(&list) {
        let works_on: Vec<&str> = spec.lists.iter().map(|l| l.label()).collect();
        let mut err = QueryError::new(
            QueryErrorKind::WrongList,
            span,
            format!(
                "{word}: is not a {} word. It works on {}.",
                list.label(),
                works_on.join(" and ")
            ),
        );
        err.field = Some(spec.word);
        if let Some(near) = fields::nearest(word, list).filter(|n| *n != spec.word) {
            let _ = write!(err.message, " Did you mean {near}:?");
            err.did_you_mean = Some(near);
        }
        return Err(err);
    }
    if raw.trim().is_empty() {
        return Err(empty_value_error(spec, word, span));
    }
    let pieces: Vec<&str> = if quoted {
        vec![raw]
    } else {
        raw.split(',').filter(|p| !p.trim().is_empty()).collect()
    };
    if pieces.is_empty() {
        return Err(empty_value_error(spec, word, span));
    }
    let mut values = Vec::with_capacity(pieces.len());
    for piece in pieces {
        let Some(v) = parse_one_value(spec, piece, quoted, today) else {
            let mut err = if spec.word == "import" {
                QueryError::new(
                    QueryErrorKind::BadValue,
                    span,
                    "import: needs #id or last.".to_string(),
                )
            } else {
                QueryError::new(
                    QueryErrorKind::BadValue,
                    span,
                    format!(
                        "{word}: does not understand {}. {}",
                        piece.trim(),
                        value_hint(spec)
                    ),
                )
            };
            err.field = Some(spec.word);
            return Err(err);
        };
        values.push(v);
    }
    Ok(FieldTerm { spec, values, span })
}

struct Parser<'t> {
    list: ListKind,
    tokens: &'t [Token],
    i: usize,
    today: NaiveDate,
    end: usize,
}

impl Parser<'_> {
    /// The next token without consuming it.
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.i)
    }

    /// An unbalanced-syntax error at `span`.
    fn unbalanced(span: Range<usize>, msg: &str) -> QueryError {
        QueryError::new(QueryErrorKind::Unbalanced, span, msg)
    }

    /// True when the next token cannot start an operand.
    fn at_operand_end(&self) -> bool {
        matches!(
            self.peek().map(|t| &t.kind),
            None | Some(TokenKind::RParen | TokenKind::Or | TokenKind::And)
        )
    }

    /// `a or b or c`: the lowest-precedence level.
    fn parse_or(&mut self, depth: usize) -> Result<Expr, QueryError> {
        let mut parts = vec![self.parse_and(depth)?];
        while matches!(self.peek().map(|t| &t.kind), Some(TokenKind::Or)) {
            let or_span = self.peek().unwrap().span.clone();
            self.i += 1;
            if self.at_operand_end() {
                return Err(Self::unbalanced(
                    or_span,
                    "or needs something on both sides.",
                ));
            }
            parts.push(self.parse_and(depth)?);
        }
        Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            Expr::Or(parts)
        })
    }

    /// `a b c` or `a and b`: adjacent terms are an implicit and.
    fn parse_and(&mut self, depth: usize) -> Result<Expr, QueryError> {
        let mut parts = vec![self.parse_unary(depth)?];
        loop {
            match self.peek().map(|t| &t.kind) {
                None | Some(TokenKind::Or | TokenKind::RParen) => break,
                Some(TokenKind::And) => {
                    let and_span = self.peek().unwrap().span.clone();
                    self.i += 1;
                    if self.at_operand_end() {
                        return Err(Self::unbalanced(
                            and_span,
                            "and needs something on both sides.",
                        ));
                    }
                    parts.push(self.parse_unary(depth)?);
                }
                Some(_) => parts.push(self.parse_unary(depth)?),
            }
        }
        Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            Expr::And(parts)
        })
    }

    /// `not x` and `-x`, with the nesting-depth check.
    fn parse_unary(&mut self, depth: usize) -> Result<Expr, QueryError> {
        if depth > MAX_DEPTH {
            return Err(QueryError::new(
                QueryErrorKind::TooComplex,
                0..self.end,
                "The search nests too deeply.",
            ));
        }
        let Some(tok) = self.peek() else {
            return Err(Self::unbalanced(
                self.end..self.end,
                "The search ends where a word was expected.",
            ));
        };
        if matches!(tok.kind, TokenKind::Not) {
            let span = tok.span.clone();
            self.i += 1;
            if self.at_operand_end() {
                return Err(Self::unbalanced(span, "not needs something after it."));
            }
            return Ok(Expr::Not(Box::new(self.parse_unary(depth + 1)?)));
        }
        let negated = tok.negated;
        let inner = self.parse_primary(depth)?;
        Ok(if negated {
            Expr::Not(Box::new(inner))
        } else {
            inner
        })
    }

    /// A parenthesised group, a `field:value` term, or a free-text term.
    fn parse_primary(&mut self, depth: usize) -> Result<Expr, QueryError> {
        let tok = self
            .peek()
            .expect("parse_unary checked for a token")
            .clone();
        match tok.kind {
            TokenKind::LParen => {
                self.i += 1;
                if matches!(self.peek().map(|t| &t.kind), None | Some(TokenKind::RParen)) {
                    return Err(Self::unbalanced(
                        tok.span,
                        "A parenthesis has nothing inside it.",
                    ));
                }
                let inner = self.parse_or(depth + 1)?;
                match self.peek().map(|t| &t.kind) {
                    Some(TokenKind::RParen) => {
                        self.i += 1;
                        Ok(inner)
                    }
                    _ => Err(Self::unbalanced(tok.span, "A parenthesis never closes.")),
                }
            }
            TokenKind::RParen => Err(Self::unbalanced(
                tok.span,
                "A closing parenthesis has no opening one.",
            )),
            TokenKind::Or | TokenKind::And => Err(Self::unbalanced(
                tok.span,
                "or and and need something on both sides.",
            )),
            TokenKind::Not => Err(Self::unbalanced(tok.span, "not needs something after it.")),
            TokenKind::Field {
                word,
                value,
                quoted,
            } => {
                self.i += 1;
                Ok(Expr::Field(field_term(
                    self.list, &word, &value, quoted, tok.span, self.today,
                )?))
            }
            TokenKind::Word { text, prefix } => {
                self.i += 1;
                Ok(Expr::Text(TextTerm::Term { text, prefix }))
            }
            TokenKind::Phrase(text) => {
                self.i += 1;
                Ok(Expr::Text(TextTerm::Phrase(text)))
            }
        }
    }
}

/// Parse tokens for `list`. `Ok(None)` is an empty query.
pub(crate) fn parse(
    list: ListKind,
    tokens: &[Token],
    today: NaiveDate,
) -> Result<Option<Expr>, QueryError> {
    if tokens.is_empty() {
        return Ok(None);
    }
    let end = tokens.last().map_or(0, |t| t.span.end);
    let mut p = Parser {
        list,
        tokens,
        i: 0,
        today,
        end,
    };
    let expr = p.parse_or(0)?;
    if let Some(extra) = p.peek() {
        return Err(Parser::unbalanced(
            extra.span.clone(),
            "A closing parenthesis has no opening one.",
        ));
    }
    if expr.text_terms() > MAX_TEXT_TERMS {
        return Err(QueryError::new(
            QueryErrorKind::TooComplex,
            0..end,
            format!("The search has more than {MAX_TEXT_TERMS} words."),
        ));
    }
    if expr.nodes() > MAX_NODES {
        return Err(QueryError::new(
            QueryErrorKind::TooComplex,
            0..end,
            "The search has too many parts.",
        ));
    }
    Ok(Some(expr))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::lex::tokenize;
    use crate::search::value::{Cmp, Value};
    use chrono::NaiveDate;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 2).unwrap()
    }

    fn parse_ok(list: ListKind, q: &str) -> Expr {
        parse(list, &tokenize(q).unwrap(), today())
            .unwrap()
            .unwrap()
    }

    fn parse_err(list: ListKind, q: &str) -> QueryError {
        parse(list, &tokenize(q).unwrap(), today()).unwrap_err()
    }

    #[test]
    fn space_is_and_and_or_binds_loosest() {
        let e = parse_ok(ListKind::Messages, "a b or c");
        let Expr::Or(parts) = e else {
            panic!("expected or")
        };
        assert_eq!(parts.len(), 2);
        assert!(matches!(parts[0], Expr::And(_)));
        assert!(matches!(parts[1], Expr::Text(_)));
    }

    #[test]
    fn minus_and_not_negate_and_parentheses_group() {
        let e = parse_ok(ListKind::Conversations, "-tag:Work not (a or b)");
        let Expr::And(parts) = e else {
            panic!("expected and")
        };
        assert!(matches!(parts[0], Expr::Not(_)));
        assert!(matches!(parts[1], Expr::Not(_)));
    }

    #[test]
    fn a_field_resolves_against_the_registry_for_its_list() {
        let e = parse_ok(ListKind::Contacts, "groups:>5");
        let Expr::Field(term) = e else {
            panic!("expected field")
        };
        assert_eq!(term.spec.word, "groups");
        assert_eq!(term.values, vec![Value::Count(Cmp::Gt(5))]);
    }

    #[test]
    fn commas_mean_or_inside_a_field_unless_quoted() {
        let e = parse_ok(ListKind::Messages, "service:imessage,sms");
        let Expr::Field(term) = e else {
            panic!("expected field")
        };
        assert_eq!(
            term.values,
            vec![Value::Choice("imessage"), Value::Choice("sms")]
        );
        let e = parse_ok(ListKind::Contacts, r#"group:"A, B""#);
        let Expr::Field(term) = e else {
            panic!("expected field")
        };
        assert_eq!(term.values, vec![Value::Text("A, B".into())]);
    }

    #[test]
    fn keywords_and_ids_are_typed() {
        let e = parse_ok(ListKind::Contacts, "group:none");
        let Expr::Field(term) = e else { panic!() };
        assert_eq!(term.values, vec![Value::Keyword("none")]);
        let e = parse_ok(ListKind::Messages, "with:#42");
        let Expr::Field(term) = e else { panic!() };
        assert_eq!(term.values, vec![Value::Id(42)]);
        let e = parse_ok(ListKind::Messages, "from:me");
        let Expr::Field(term) = e else { panic!() };
        assert_eq!(term.values, vec![Value::Keyword("me")]);
        let e = parse_ok(ListKind::Messages, "filename:IMG_*");
        let Expr::Field(term) = e else { panic!() };
        assert_eq!(term.values, vec![Value::Prefix("IMG_".into())]);
    }

    #[test]
    fn any_text_word_understands_an_unquoted_trailing_star_as_a_prefix() {
        let e = parse_ok(ListKind::Messages, "body:avo*");
        let Expr::Field(term) = e else { panic!() };
        assert_eq!(term.values, vec![Value::Prefix("avo".into())]);
        // So do the Name and Person words, `import:` aside.
        for (q, p) in [
            ("tag:Arch*", "Arch"),
            ("group:Fam*", "Fam"),
            ("with:Ja*", "Ja"),
        ] {
            let Expr::Field(term) = parse_ok(ListKind::Messages, q) else {
                panic!()
            };
            assert_eq!(term.values, vec![Value::Prefix(p.into())], "{q}");
        }
        assert_eq!(
            parse_err(ListKind::Messages, "import:la*").kind,
            QueryErrorKind::BadValue
        );
        // Quoting keeps the star literal.
        let e = parse_ok(ListKind::Messages, r#"body:"avo*""#);
        let Expr::Field(term) = e else { panic!() };
        assert_eq!(term.values, vec![Value::Text("avo*".into())]);
    }

    #[test]
    fn unknown_word_is_refused_without_naming_anything_else() {
        let err = parse_err(ListKind::Messages, "wombat:Family");
        assert_eq!(err.kind, QueryErrorKind::UnknownWord);
        assert_eq!(err.message, "wombat: is not a search word.");
        assert_eq!(err.span, 0..13);
        assert_eq!(err.did_you_mean, None);
    }

    #[test]
    fn a_near_miss_gets_a_suggestion() {
        let err = parse_err(ListKind::Conversations, "paticipants:>2");
        assert_eq!(err.kind, QueryErrorKind::UnknownWord);
        assert_eq!(err.did_you_mean, Some("participants"));
        assert_eq!(
            err.message,
            "paticipants: is not a search word. Did you mean participants:?"
        );
    }

    #[test]
    fn a_word_from_another_list_says_where_it_works() {
        let err = parse_err(ListKind::Contacts, "from:me");
        assert_eq!(err.kind, QueryErrorKind::WrongList);
        assert_eq!(err.field, Some("from"));
        assert_eq!(
            err.message,
            "from: is not a Contacts word. It works on Messages."
        );
        assert_eq!(err.span, 0..7);
        assert_eq!(err.did_you_mean, None);
    }

    #[test]
    fn empty_and_bad_values() {
        let err = parse_err(ListKind::Messages, "tag:");
        assert_eq!(err.kind, QueryErrorKind::EmptyValue);
        assert_eq!(
            err.message,
            "tag: needs a value, for example tag:Holiday or tag:none."
        );
        let err = parse_err(ListKind::Messages, "date:2019-13");
        assert_eq!(err.kind, QueryErrorKind::BadValue);
        assert_eq!(
            err.message,
            "date: does not understand 2019-13. Write a year, a month like 2024-05, a day, or a span like 7d, with >, >=, <, <=, or a..b."
        );
        let err = parse_err(ListKind::Messages, "kind:big");
        assert_eq!(
            err.message,
            "kind: does not understand big. Write one of: direct, group."
        );
    }

    #[test]
    fn an_empty_value_names_a_second_example_when_the_word_has_one() {
        // kind:'s own example already spells "direct"; the "or" clause must
        // name a different keyword ("group"), not repeat "direct".
        let err = parse_err(ListKind::Messages, "kind:");
        assert_eq!(err.kind, QueryErrorKind::EmptyValue);
        assert_eq!(
            err.message,
            "kind: needs a value, for example kind:direct or kind:group."
        );
        // import:'s only keyword ("last") is already its example, so there
        // is no second example to offer.
        let err = parse_err(ListKind::Messages, "import:");
        assert_eq!(err.kind, QueryErrorKind::EmptyValue);
        assert_eq!(
            err.message,
            "import: needs a value, for example import:last."
        );
    }

    #[test]
    fn import_only_accepts_id_or_last() {
        let err = parse_err(ListKind::Messages, "import:foo");
        assert_eq!(err.kind, QueryErrorKind::BadValue);
        assert_eq!(err.message, "import: needs #id or last.");
        assert_eq!(err.field, Some("import"));
        let e = parse_ok(ListKind::Messages, "import:last");
        let Expr::Field(term) = e else { panic!() };
        assert_eq!(term.values, vec![Value::Keyword("last")]);
        let e = parse_ok(ListKind::Messages, "import:#7");
        let Expr::Field(term) = e else { panic!() };
        assert_eq!(term.values, vec![Value::Id(7)]);
    }

    #[test]
    fn an_empty_value_list_after_commas_is_still_empty_value() {
        let err = parse_err(ListKind::Messages, "tag:,");
        assert_eq!(err.kind, QueryErrorKind::EmptyValue);
        assert_eq!(
            err.message,
            "tag: needs a value, for example tag:Holiday or tag:none."
        );
        let err = parse_err(ListKind::Messages, "tag:,,");
        assert_eq!(err.kind, QueryErrorKind::EmptyValue);
        assert_eq!(
            err.message,
            "tag: needs a value, for example tag:Holiday or tag:none."
        );
    }

    #[test]
    fn unbalanced_shapes() {
        assert_eq!(
            parse_err(ListKind::Messages, "(a or b").kind,
            QueryErrorKind::Unbalanced
        );
        assert_eq!(
            parse_err(ListKind::Messages, "a or").kind,
            QueryErrorKind::Unbalanced
        );
        assert_eq!(
            parse_err(ListKind::Messages, "a)").kind,
            QueryErrorKind::Unbalanced
        );
    }

    #[test]
    fn empty_query_is_none() {
        assert!(
            parse(ListKind::Messages, &tokenize("   ").unwrap(), today())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn describe_lists_only_that_lists_words() {
        let docs = crate::search::describe(ListKind::Contacts);
        assert!(docs.iter().any(|d| d.word == "groups"));
        assert!(!docs.iter().any(|d| d.word == "from"));
        assert_eq!(
            crate::search::describe(ListKind::Messages)
                .iter()
                .filter(|d| d.word == "date")
                .count(),
            1
        );
    }
}
