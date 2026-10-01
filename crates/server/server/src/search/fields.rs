//! The registry: every word the language has, once. `describe` and the
//! parser read this table, so a word cannot exist for one and not the other.

use super::ListKind;
use super::ListKind::{Contacts as C, Conversations as V, Messages as M};

/// What shape of value a word takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ValueType {
    /// Free text, matched as "contains"; `pre*` matches a prefix.
    Text,
    /// A named thing: a name, or `#id`.
    Name,
    /// A person: a name, a handle, `#id`, or `me` where allowed.
    Person,
    /// One of a fixed set of words.
    Choice,
    /// A day, month, year, or relative span, with comparisons and ranges.
    Date,
    /// A whole number, with comparisons and ranges.
    Count,
    /// A byte size, with comparisons and ranges.
    Size,
    /// `yes`, `no`, or `any`.
    Flag,
}

/// One word.
#[derive(Debug)]
pub(crate) struct FieldSpec {
    pub word: &'static str,
    pub value_type: ValueType,
    pub lists: &'static [ListKind],
    /// Keyword values this word accepts besides its value type: `none`,
    /// `any`, `me`, `unknown`, `last`, or the fixed choices of a Choice/Flag.
    pub values: &'static [&'static str],
    pub help: &'static str,
    pub example: &'static str,
}

const NONE_ANY: &[&str] = &["none", "any"];

/// The twenty-seven words, in the spec's order.
pub(crate) static FIELDS: &[FieldSpec] = &[
    FieldSpec {
        word: "body",
        value_type: ValueType::Text,
        lists: &[V, M],
        values: NONE_ANY,
        help: "message body only",
        example: "body:avocado",
    },
    FieldSpec {
        word: "subject",
        value_type: ValueType::Text,
        lists: &[V, M],
        values: NONE_ANY,
        help: "subject line only",
        example: "subject:dinner",
    },
    FieldSpec {
        word: "name",
        value_type: ValueType::Text,
        lists: &[C, V, M],
        values: NONE_ANY,
        help: "a person's name: this contact, or a participant",
        example: "name:jane",
    },
    FieldSpec {
        word: "title",
        value_type: ValueType::Text,
        lists: &[V, M],
        values: NONE_ANY,
        help: "the conversation's title",
        example: "title:\"book club\"",
    },
    FieldSpec {
        word: "handle",
        value_type: ValueType::Text,
        lists: &[C, V, M],
        values: NONE_ANY,
        help: "a phone number, email, or username",
        example: "handle:@gmail.com",
    },
    FieldSpec {
        word: "with",
        value_type: ValueType::Person,
        lists: &[V, M],
        values: &[],
        help: "this person is a participant",
        example: "with:jane",
    },
    FieldSpec {
        word: "from",
        value_type: ValueType::Person,
        lists: &[M],
        values: &["me"],
        help: "this person sent it",
        example: "from:me",
    },
    FieldSpec {
        word: "to",
        value_type: ValueType::Person,
        lists: &[M],
        values: &["me"],
        help: "it was sent to this person",
        example: "to:jane",
    },
    FieldSpec {
        word: "in",
        value_type: ValueType::Name,
        lists: &[M],
        values: &[],
        help: "this one conversation",
        example: "in:#19",
    },
    FieldSpec {
        word: "group",
        value_type: ValueType::Name,
        lists: &[C, V, M],
        values: &["none", "unknown"],
        help: "in this Contact Group: the contact itself, or a participant",
        example: "group:Family",
    },
    FieldSpec {
        word: "tag",
        value_type: ValueType::Name,
        lists: &[C, V, M],
        values: &["none"],
        help: "the conversation carries this Message Tag",
        example: "tag:Holiday",
    },
    FieldSpec {
        word: "kind",
        value_type: ValueType::Choice,
        lists: &[C, V, M],
        values: &["direct", "group"],
        help: "the conversation's shape",
        example: "kind:direct",
    },
    FieldSpec {
        word: "service",
        value_type: ValueType::Choice,
        lists: &[C, V, M],
        values: &["imessage", "sms", "mms", "rcs", "whatsapp"],
        help: "the transport that carried the message",
        example: "service:imessage",
    },
    FieldSpec {
        word: "source",
        value_type: ValueType::Choice,
        lists: &[V, M],
        values: &["imessage", "whatsapp", "sms"],
        help: "the backup family it was imported from",
        example: "source:whatsapp",
    },
    FieldSpec {
        word: "import",
        value_type: ValueType::Name,
        lists: &[V, M],
        values: &["last"],
        help: "brought in by this Import Run",
        example: "import:last",
    },
    FieldSpec {
        word: "date",
        value_type: ValueType::Date,
        lists: &[C, V, M],
        values: &[],
        help: "when a message was sent; on Contacts, one the contact sent; on Conversations, one in it",
        example: "date:2019..2021",
    },
    FieldSpec {
        word: "first-message",
        value_type: ValueType::Date,
        lists: &[C, V, M],
        values: &[],
        help: "the date of the first message: the contact's own, or the conversation's",
        example: "first-message:<2020",
    },
    FieldSpec {
        word: "last-message",
        value_type: ValueType::Date,
        lists: &[C, V, M],
        values: &[],
        help: "the date of the last message: the contact's own, or the conversation's",
        example: "last-message:<2022",
    },
    FieldSpec {
        word: "attachment",
        value_type: ValueType::Choice,
        lists: &[V, M],
        values: &[
            "image", "video", "audio", "document", "pdf", "contact", "other", "any", "none",
        ],
        help: "what is attached",
        example: "attachment:image",
    },
    FieldSpec {
        word: "filename",
        value_type: ValueType::Text,
        lists: &[V, M],
        values: &[],
        help: "an attachment's file name",
        example: "filename:IMG_*",
    },
    FieldSpec {
        word: "size",
        value_type: ValueType::Size,
        lists: &[V, M],
        values: &[],
        help: "an attachment's size",
        example: "size:>1M",
    },
    FieldSpec {
        word: "messages",
        value_type: ValueType::Count,
        lists: &[C, V],
        values: &[],
        help: "how many messages: the contact sent, or the conversation holds",
        example: "messages:>100",
    },
    FieldSpec {
        word: "conversations",
        value_type: ValueType::Count,
        lists: &[C],
        values: &[],
        help: "how many conversations",
        example: "conversations:0",
    },
    FieldSpec {
        word: "groups",
        value_type: ValueType::Count,
        lists: &[C],
        values: &[],
        help: "how many Contact Groups",
        example: "groups:>5",
    },
    FieldSpec {
        word: "participants",
        value_type: ValueType::Count,
        lists: &[V, M],
        values: &[],
        help: "how many people in the conversation",
        example: "participants:>2",
    },
    FieldSpec {
        word: "attachments",
        value_type: ValueType::Count,
        lists: &[M],
        values: &[],
        help: "how many attachments on the message",
        example: "attachments:>0",
    },
    FieldSpec {
        word: "trashed",
        value_type: ValueType::Flag,
        lists: &[C, V, M],
        values: &["yes", "no", "any"],
        help: "in the trash",
        example: "trashed:yes",
    },
];

/// The spec for a word, on any list.
pub(crate) fn lookup(word: &str) -> Option<&'static FieldSpec> {
    FIELDS.iter().find(|f| f.word == word)
}

/// The words one list accepts, in registry order.
pub(crate) fn for_list(list: ListKind) -> impl Iterator<Item = &'static FieldSpec> {
    FIELDS.iter().filter(move |f| f.lists.contains(&list))
}

/// Levenshtein distance between two strings, for "did you mean" suggestions.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// A word on `list` within two edits of `word`, for "did you mean". The
/// language keeps no memory of spellings that came before it: this only ever
/// searches the current word list for the requested list.
pub(crate) fn nearest(word: &str, list: ListKind) -> Option<&'static str> {
    for_list(list)
        .map(|f| (edit_distance(word, f.word), f.word))
        .filter(|(d, _)| *d <= 2)
        .min_by_key(|(d, _)| *d)
        .map(|(_, w)| w)
}

/// One word as the web and the docs see it.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct FieldDoc {
    /// The spelling, without the colon.
    pub word: String,
    /// What shape of value it takes.
    pub value_type: ValueType,
    /// Keyword or fixed values the word accepts.
    pub values: Vec<String>,
    /// One line of help.
    pub help: String,
    /// One example, ready to type.
    pub example: String,
}

/// The words for one list.
pub fn describe(list: ListKind) -> Vec<FieldDoc> {
    for_list(list)
        .map(|f| FieldDoc {
            word: f.word.to_string(),
            value_type: f.value_type,
            values: f.values.iter().map(|v| v.to_string()).collect(),
            help: f.help.to_string(),
            example: f.example.to_string(),
        })
        .collect()
}
