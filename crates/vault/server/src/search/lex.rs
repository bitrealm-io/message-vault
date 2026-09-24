//! Query string to tokens. Every token carries the byte range it came from,
//! so an error can point at the exact text.

use std::ops::Range;

use super::error::{QueryError, QueryErrorKind};

/// Reject huge query strings before doing anything else.
pub(crate) const MAX_QUERY_BYTES: usize = 2_048;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TokenKind {
    LParen,
    RParen,
    Or,
    And,
    Not,
    /// `word:value`. `quoted` says the value came in quotes, so a comma in it
    /// is text rather than a list separator.
    Field {
        word: String,
        value: String,
        quoted: bool,
    },
    /// A bare word; `prefix` when it ended in `*`.
    Word {
        text: String,
        prefix: bool,
    },
    /// A quoted phrase.
    Phrase(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Token {
    pub kind: TokenKind,
    pub span: Range<usize>,
    /// A leading `-` was attached to this token.
    pub negated: bool,
}

/// Read a quoted value starting just after the opening quote. `""` inside is
/// one literal quote. Returns the text and the index just past the closing
/// quote, or `None` when the quote never closes.
fn read_quoted(bytes: &[u8], mut i: usize) -> Option<(String, usize)> {
    let mut out = Vec::new();
    while i < bytes.len() {
        if bytes[i] == b'"' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                out.push(b'"');
                i += 2;
                continue;
            }
            return Some((String::from_utf8_lossy(&out).into_owned(), i + 1));
        }
        out.push(bytes[i]);
        i += 1;
    }
    None
}

/// True for a word that could be a field name: letters and hyphens, starting with a letter.
fn is_field_word(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphabetic() || c == '-')
}

/// True for a byte that ends a bare (unquoted) word.
fn is_bare_end(b: u8) -> bool {
    b.is_ascii_whitespace() || b == b'(' || b == b')'
}

/// Tokenize `input`.
///
/// # Errors
///
/// `TooLong` past [`MAX_QUERY_BYTES`]; `Unbalanced` for a quote that never closes.
pub(crate) fn tokenize(input: &str) -> Result<Vec<Token>, QueryError> {
    if input.len() > MAX_QUERY_BYTES {
        return Err(QueryError::new(
            QueryErrorKind::TooLong,
            0..input.len(),
            format!("The search is longer than {MAX_QUERY_BYTES} bytes."),
        ));
    }
    // A NUL byte separates words like a space. Postgres refuses NUL in any
    // text and SQLite's FTS5 reads it as the end of the query, so none may
    // reach a bound value. One byte for one byte, so every span still points
    // at the text the person typed.
    let without_nul;
    let input = if input.contains('\0') {
        without_nul = input.replace('\0', " ");
        without_nul.as_str()
    } else {
        input
    };
    let mut lexer = Lexer {
        input,
        bytes: input.as_bytes(),
        pos: 0,
        tokens: Vec::new(),
    };
    while lexer.skip_whitespace() {
        lexer.next_token()?;
    }
    Ok(lexer.tokens)
}

/// A cursor over the query bytes and the tokens read so far.
struct Lexer<'a> {
    input: &'a str,
    bytes: &'a [u8],
    pos: usize,
    tokens: Vec<Token>,
}

impl Lexer<'_> {
    /// Move past whitespace. False at the end of the input.
    fn skip_whitespace(&mut self) -> bool {
        while self
            .bytes
            .get(self.pos)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.pos += 1;
        }
        self.pos < self.bytes.len()
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    /// The index of the first byte at or after the cursor that `keep` rejects.
    fn scan(&self, keep: impl Fn(u8) -> bool) -> usize {
        let mut end = self.pos;
        while self.bytes.get(end).is_some_and(|&b| keep(b)) {
            end += 1;
        }
        end
    }

    /// Record a token spanning `start..end` and move the cursor past it.
    fn push(&mut self, kind: TokenKind, start: usize, end: usize, negated: bool) {
        self.tokens.push(Token {
            kind,
            span: start..end,
            negated,
        });
        self.pos = end;
    }

    /// Read one token; the cursor is on a non-whitespace byte.
    fn next_token(&mut self) -> Result<(), QueryError> {
        let start = self.pos;
        match self.peek() {
            Some(b'(') => {
                self.push(TokenKind::LParen, start, start + 1, false);
                return Ok(());
            }
            Some(b')') => {
                self.push(TokenKind::RParen, start, start + 1, false);
                return Ok(());
            }
            _ => {}
        }
        // A leading `-` negates the token after it: a word, a phrase, or a
        // group. A `-` with nothing, a space, or `)` after it is a word.
        let negated = self.peek() == Some(b'-')
            && self
                .bytes
                .get(self.pos + 1)
                .is_some_and(|&b| !b.is_ascii_whitespace() && b != b')');
        if negated {
            self.pos += 1;
        }
        match self.peek() {
            // `-(a or b)`: the minus applies to the group.
            Some(b'(') => {
                self.push(TokenKind::LParen, start, self.pos + 1, negated);
                Ok(())
            }
            Some(b'"') => {
                let (text, next) = self.quoted_after(self.pos)?;
                self.push(TokenKind::Phrase(text), start, next, negated);
                Ok(())
            }
            _ => self.read_bare(start, negated),
        }
    }

    /// The quoted value whose opening quote is at `quote`, and the index just
    /// past its closing quote.
    ///
    /// # Errors
    ///
    /// `Unbalanced` from the quote to the end when it never closes.
    fn quoted_after(&self, quote: usize) -> Result<(String, usize), QueryError> {
        read_quoted(self.bytes, quote + 1).ok_or_else(|| {
            QueryError::new(
                QueryErrorKind::Unbalanced,
                quote..self.bytes.len(),
                "A quote never closes.",
            )
        })
    }

    /// A bare run up to whitespace or a parenthesis: a `word:value` field when
    /// the run before a colon is a field word, else an operator or a word.
    fn read_bare(&mut self, start: usize, negated: bool) -> Result<(), QueryError> {
        let input = self.input;
        let head_end = self.scan(|b| !is_bare_end(b) && b != b':');
        let head = &input[self.pos..head_end];
        // `word:` is a field unless the value starts with `/`, so a pasted
        // URL such as `http://x` stays a word.
        let value_starts_with_slash = self.bytes.get(head_end + 1) == Some(&b'/');
        if self.bytes.get(head_end) == Some(&b':')
            && is_field_word(head)
            && !value_starts_with_slash
        {
            return self.read_field(start, negated, head.to_ascii_lowercase(), head_end + 1);
        }
        // Not a field: take the whole bare run, colons included.
        let end = self.scan(|b| !is_bare_end(b));
        let kind = word_or_operator(&input[self.pos..end], negated);
        self.push(kind, start, end, negated);
        Ok(())
    }

    /// `word:value`, where the value is quoted or runs to whitespace.
    fn read_field(
        &mut self,
        start: usize,
        negated: bool,
        word: String,
        value_start: usize,
    ) -> Result<(), QueryError> {
        if self.bytes.get(value_start) == Some(&b'"') {
            let (value, next) = self.quoted_after(value_start)?;
            self.push(
                TokenKind::Field {
                    word,
                    value,
                    quoted: true,
                },
                start,
                next,
                negated,
            );
            return Ok(());
        }
        let mut end = value_start;
        while self.bytes.get(end).is_some_and(|&b| !is_bare_end(b)) {
            end += 1;
        }
        let value = self.input[value_start..end].to_string();
        self.push(
            TokenKind::Field {
                word,
                value,
                quoted: false,
            },
            start,
            end,
            negated,
        );
        Ok(())
    }
}

/// `or`, `and`, and `not` are operators unless negated (`-or` is a word);
/// anything else is a word, with a trailing `*` marking a prefix.
fn word_or_operator(text: &str, negated: bool) -> TokenKind {
    match text.to_ascii_lowercase().as_str() {
        "or" if !negated => TokenKind::Or,
        "and" if !negated => TokenKind::And,
        "not" if !negated => TokenKind::Not,
        _ => match text.strip_suffix('*') {
            Some(stem) if !stem.is_empty() => TokenKind::Word {
                text: stem.to_string(),
                prefix: true,
            },
            _ => TokenKind::Word {
                text: text.to_string(),
                prefix: false,
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(input: &str) -> Vec<TokenKind> {
        tokenize(input)
            .unwrap()
            .into_iter()
            .map(|t| t.kind)
            .collect()
    }

    #[test]
    fn words_phrases_and_prefixes() {
        assert_eq!(
            kinds(r#"hello "two words" avoc*"#),
            vec![
                TokenKind::Word {
                    text: "hello".into(),
                    prefix: false
                },
                TokenKind::Phrase("two words".into()),
                TokenKind::Word {
                    text: "avoc".into(),
                    prefix: true
                },
            ]
        );
    }

    #[test]
    fn fields_take_bare_and_quoted_values() {
        assert_eq!(
            kinds(r#"tag:Work group:"Book Club" date:2019..2021"#),
            vec![
                TokenKind::Field {
                    word: "tag".into(),
                    value: "Work".into(),
                    quoted: false
                },
                TokenKind::Field {
                    word: "group".into(),
                    value: "Book Club".into(),
                    quoted: true
                },
                TokenKind::Field {
                    word: "date".into(),
                    value: "2019..2021".into(),
                    quoted: false
                },
            ]
        );
    }

    #[test]
    fn a_doubled_quote_is_a_literal_quote() {
        assert_eq!(
            kinds(r#"title:"say ""hi"" now""#),
            vec![TokenKind::Field {
                word: "title".into(),
                value: r#"say "hi" now"#.into(),
                quoted: true
            }]
        );
    }

    #[test]
    fn operators_and_negation() {
        let toks = tokenize("-tag:Work or (a and not b)").unwrap();
        assert!(toks[0].negated);
        assert!(matches!(toks[0].kind, TokenKind::Field { .. }));
        assert_eq!(toks[1].kind, TokenKind::Or);
        assert_eq!(toks[2].kind, TokenKind::LParen);
        assert_eq!(toks[4].kind, TokenKind::And);
        assert_eq!(toks[5].kind, TokenKind::Not);
        assert_eq!(toks[7].kind, TokenKind::RParen);
        // Operator words are case-insensitive.
        assert_eq!(kinds("OR")[0], TokenKind::Or);
    }

    #[test]
    fn a_minus_before_a_group_negates_the_group() {
        let toks = tokenize("-(a or b)").unwrap();
        assert_eq!(toks[0].kind, TokenKind::LParen);
        assert!(toks[0].negated);
        assert_eq!(toks[0].span, 0..2);
        assert_eq!(toks.len(), 5);
        // A `-` before a space or a closing parenthesis is a word.
        assert_eq!(
            kinds("- a")[0],
            TokenKind::Word {
                text: "-".into(),
                prefix: false
            }
        );
        assert!(!tokenize("(a -)").unwrap()[2].negated);
    }

    #[test]
    fn spans_are_byte_ranges_into_the_input() {
        let toks = tokenize("hello tag:Work").unwrap();
        assert_eq!(toks[0].span, 0..5);
        assert_eq!(toks[1].span, 6..14);
        // A negated token's span starts at the minus.
        let toks = tokenize("a -b").unwrap();
        assert_eq!(toks[1].span, 2..4);
    }

    #[test]
    fn a_field_word_is_lowercase_letters_and_hyphens() {
        // `Re:` inside a phrase is text, and a token like `http://x` is a word,
        // not a field, because the part before the colon is not a field shape.
        assert_eq!(
            kinds("http://x")[0],
            TokenKind::Word {
                text: "http://x".into(),
                prefix: false
            }
        );
        assert_eq!(
            kinds("First-Message:2019")[0],
            TokenKind::Field {
                word: "first-message".into(),
                value: "2019".into(),
                quoted: false
            }
        );
    }

    #[test]
    fn an_empty_field_value_is_kept_for_the_parser_to_reject() {
        assert_eq!(
            kinds("tag:")[0],
            TokenKind::Field {
                word: "tag".into(),
                value: String::new(),
                quoted: false
            }
        );
    }

    #[test]
    fn unterminated_quote_is_unbalanced() {
        let err = tokenize(r#"tag:"Book"#).unwrap_err();
        assert_eq!(err.kind, QueryErrorKind::Unbalanced);
        assert_eq!(err.span, 4..9);
    }

    #[test]
    fn too_long_is_refused_before_anything_else() {
        let long = "a".repeat(MAX_QUERY_BYTES + 1);
        assert_eq!(tokenize(&long).unwrap_err().kind, QueryErrorKind::TooLong);
    }
}
