//! The response shapes the server's HTTP API sends, defined once for the
//! server that writes them and the client crates that read them.
//!
//! Two crates sit on either side of these shapes: `message-crate-server`
//! serializes them, and `message-crate-pull` deserializes them on its way to
//! `message-ir`. While each kept its own struct, the two could disagree
//! silently and did, three times: `message-crate-pull` declared a
//! participant's address a `String` after the server started sending `null`
//! for a participant a backup named without an address, kept
//! `#[serde(default)]` on a field the server had removed, and read a
//! `service` off the conversation the server has never sent there. Each of
//! those was a pull that failed at runtime, or quietly produced worse data,
//! with nothing in either crate's tests to catch it — `message-crate-pull`'s
//! "real export page" was a JSON literal it wrote itself, so it agreed with
//! whatever the mirror said.
//!
//! One definition makes the compiler the check. A field the server renames
//! stops compiling in the client, which is the whole point of putting the
//! shape here.
//!
//! `#[serde(skip_serializing_if = ...)]` and `#[serde(default)]` come as a
//! pair here, and only as a pair: a field the server may leave out is a field
//! a reader has to be able to do without, and a field the server always sends
//! stays required in the OpenAPI document rather than turning optional in
//! every generated client.
//!
//! The `schema` feature adds `utoipa::ToSchema`, so the same structs describe
//! themselves in the OpenAPI document. The server turns it on; a client crate
//! leaves it off and never builds utoipa.

use serde::{Deserialize, Serialize};

/// Request header naming the app a request comes from: `desktop` or
/// `website`, the two values of [`AppKind`].
pub const APP_HEADER: &str = "x-message-crate-app";

/// Request header carrying that app's Build, such as `0.9.0+343fe0d8`.
pub const APP_VERSION_HEADER: &str = "x-message-crate-version";

/// Which app a session's requests come from. The server records it beside the
/// app's Build and shows both to the owner; it never refuses a request
/// on account of either.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum AppKind {
    /// The desktop app, from its SPA or from its import and export code.
    Desktop,
    /// The SPA the server serves to a browser.
    Website,
}

impl AppKind {
    /// The value sent in [`APP_HEADER`] and stored by the server.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Website => "website",
        }
    }

    /// Read a sent or stored value; anything else names no app.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "desktop" => Some(Self::Desktop),
            "website" => Some(Self::Website),
            _ => None,
        }
    }
}

/// What happens to a source's messages that were imported before: `replace`
/// wipes them first, `append` keeps them and adds only new ones.
// Every path that carries a mode, the `POST /v1/imports` body, the `import` CLI flag, `message-crate-push`'s settings and the
// desktop push command, uses this type, so a misspelling cannot compile as
// "append".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum ImportMode {
    /// Wipe the source's existing messages before importing.
    Replace,
    /// Keep existing messages and add only new ones. The default, and what
    /// the HTTP API assumes when a request names no mode: it never removes
    /// anything.
    #[default]
    Append,
}

impl ImportMode {
    /// The wire form: `replace` or `append`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Replace => "replace",
            Self::Append => "append",
        }
    }
}

impl std::fmt::Display for ImportMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A mode string that is neither `replace` nor `append`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidImportMode(String);

impl std::fmt::Display for InvalidImportMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid import mode '{}' (expected replace or append)",
            self.0
        )
    }
}

impl std::error::Error for InvalidImportMode {}

impl std::str::FromStr for ImportMode {
    type Err = InvalidImportMode;

    /// Case-insensitive, so a CLI flag typed as `Replace` still parses.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "replace" => Ok(Self::Replace),
            "append" => Ok(Self::Append),
            _ => Err(InvalidImportMode(s.to_string())),
        }
    }
}

#[cfg(test)]
mod import_mode_tests {
    use super::*;

    #[test]
    fn parses_both_modes_case_insensitively() {
        assert_eq!("replace".parse::<ImportMode>(), Ok(ImportMode::Replace));
        assert_eq!("Append".parse::<ImportMode>(), Ok(ImportMode::Append));
        assert_eq!(
            "merge".parse::<ImportMode>().unwrap_err().to_string(),
            "invalid import mode 'merge' (expected replace or append)"
        );
    }

    #[test]
    fn serde_uses_the_lowercase_name() {
        assert_eq!(
            serde_json::to_string(&ImportMode::Replace).unwrap(),
            "\"replace\""
        );
        let parsed: ImportMode = serde_json::from_str("\"append\"").unwrap();
        assert_eq!(parsed, ImportMode::Append);
        assert_eq!(ImportMode::Append.to_string(), "append");
    }
}

/// Derive `ToSchema` only when the `schema` feature is on.
macro_rules! api_shape {
    ($(#[$meta:meta])* pub struct $name:ident { $($body:tt)* }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Serialize, Deserialize)]
        #[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
        pub struct $name { $($body)* }
    };
}

/// Which list an Export Run's query is for (`docs/architecture/http-api.md`,
/// "Runs"). The list decides which search words the query may use and what
/// the run hands over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum ExportQueryList {
    /// The Conversations list: every message of each conversation the query
    /// shows there.
    Conversations,
    /// The Messages list: the messages the query matches, and no others.
    Messages,
}

impl ExportQueryList {
    /// The value as the wire and the database spell it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Conversations => "conversations",
            Self::Messages => "messages",
        }
    }
}

/// What an Export Run asked for, stored as given (`docs/architecture/http-api.md`,
/// "Runs"). One of three forms: everything the account holds, a query in the
/// search language, or conversations and messages picked by hand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ExportScope {
    /// Every non-trashed message the account holds.
    Everything,
    /// What a query in the search language finds on one of two lists.
    Query {
        /// The list the query is for. `messages` hands over the messages the
        /// query matches; `conversations` hands over every message of each
        /// conversation the query shows on the Conversations list.
        list: ExportQueryList,
        /// The query, as typed. Never blank: an empty query is the
        /// `everything` form.
        q: String,
    },
    /// Conversations and messages picked by hand. A message is selected when
    /// its conversation is listed or it is listed itself; either list may be
    /// empty, but not both.
    Selection {
        /// Conversation ids whose every message is exported.
        #[serde(default)]
        conversation_ids: Vec<i64>,
        /// Message ids exported on their own.
        #[serde(default)]
        message_ids: Vec<i64>,
    },
}

// Every list route answers this shape (`docs/architecture/http-api.md`), and
// `GET /v1/exports/{id}/messages` answers a `Page<Message>`, which the OpenAPI
// document names `Page_Message`. The doc comment is the schema description,
// so it stays one line.
/// One page of a list.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct Page<T> {
    /// The rows on this page.
    pub items: Vec<T>,
    /// Rows matching the query across every page.
    pub total: u64,
    /// Page size used.
    pub limit: usize,
    /// Page offset used.
    pub offset: usize,
}

/// How an Export Run stands: the values `exports.status` holds, the values
/// `GET /v1/exports?status=` accepts, and the word every Export Run carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum ExportStatus {
    /// Still open: its messages can be read.
    Running,
    /// Closed by the client once it had everything.
    Completed,
    /// Ended without finishing.
    Failed,
    /// Closed by the person before it finished.
    Cancelled,
}

impl ExportStatus {
    /// Every status, in the order the documentation lists them.
    pub const ALL: [Self; 4] = [
        Self::Running,
        Self::Completed,
        Self::Failed,
        Self::Cancelled,
    ];

    /// The value as the wire and the database spell it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// The status `value` spells, or `None` for any other word.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == value)
    }
}

impl std::fmt::Display for ExportStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

api_shape! {
    /// One Export Run: what was asked for and how much matched, never what
    /// the messages said. `POST /v1/exports` creates one, every route under
    /// `/v1/exports/{id}` answers it.
    pub struct ExportRun {
        /// Export Run id.
        pub id: i64,
        /// What the run asked for, as given.
        pub scope: ExportScope,
        /// Exporting tool, e.g. `message-crate-pull`, when the client named one.
        pub tool: Option<String>,
        /// Lifecycle status.
        pub status: ExportStatus,
        /// UTC time the run started.
        pub started_at: String,
        /// UTC time the run finished, when it has.
        pub finished_at: Option<String>,
        /// Messages the scope matched when the run was created: the places in
        /// the list `GET /v1/exports/{id}/messages` pages, and every page's
        /// `total`.
        pub message_count: i64,
        /// Distinct conversations with at least one matching message.
        pub conversation_count: i64,
        /// Distinct attachment fingerprints among the matching messages.
        pub attachment_count: i64,
        /// Sum of the known sizes of those distinct attachments, in bytes.
        pub total_bytes: i64,
        /// How far `GET /v1/exports/{id}/messages` has read into the run's
        /// list, in places, so an abandoned run shows how far it got.
        pub messages_delivered: i64,
    }
}

api_shape! {
    /// One participant of a conversation, carrying the name to show for them:
    /// the Contact's name, else what that backup called them in that
    /// conversation, else the identity.
    pub struct Participant {
        /// What to show for this person. Never empty — the server falls back to
        /// the identity when nothing else names them, and to the name alone for
        /// someone a backup named without recording any address.
        pub name: String,
        /// The identity as the source wrote it: a phone number, email address
        /// or username. `None` when the source named this person without
        /// recording any address for them.
        pub identity: Option<String>,
        /// Platform service, e.g. `imessage`. `None` for the same reason as
        /// `identity`: with no address there is nothing to carry a service on.
        pub service: Option<String>,
        /// Linked contact id: when the identity is on a Contact, or — for a
        /// participant with no identity — the contact the server bound the name to
        /// directly, since that is the only place the link is recorded for
        /// them. Matches the `id` every other contact shape uses, so a caller
        /// can compare the two without converting either.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub contact_id: Option<i64>,
    }
}

api_shape! {
    /// One exported message.
    pub struct Message {
        /// Message row id.
        pub id: i64,
        /// Import source id.
        pub source: String,
        /// Platform service, e.g. `imessage`, when known. It rides on the
        /// message, never on the conversation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub service: Option<String>,
        /// Export GUID for replies and grouping.
        pub guid: Option<String>,
        /// The instant the message was sent: RFC 3339 in UTC with a `Z`
        /// suffix. A caller shows it in the account's time zone
        /// (`Account.time_zone`); the database stores nothing
        /// about where the phone was.
        pub timestamp: String,
        /// Ordering key within the conversation.
        pub sort_order: i64,
        /// True for messages sent by the account owner.
        pub is_from_me: bool,
        /// The sender's identity, for incoming messages.
        pub sender: Option<String>,
        /// The account holder's own address on this message: the one it was
        /// sent from, or the one it was received at. `None` when the backup
        /// named no owner.
        pub owner: Option<String>,
        /// Subject line, when set.
        pub subject: Option<String>,
        /// Body text, when present.
        pub text: Option<String>,
        /// True for group announcements.
        pub is_announcement: bool,
        /// True when part of a reply thread.
        pub is_reply: bool,
        /// GUID of the message this replies to.
        pub thread_originator_guid: Option<String>,
        /// Part index of the originator (for tapbacks).
        pub thread_originator_part: Option<i64>,
        /// Replies in this thread.
        pub num_replies: i64,
        /// The conversation this message belongs to.
        pub conversation: MessageConversation,
        /// Attachments on this message.
        pub attachments: Vec<Attachment>,
        /// Reactions on this message.
        pub tapbacks: Vec<Tapback>,
    }
}

api_shape! {
    /// The conversation a message belongs to.
    pub struct MessageConversation {
        /// Conversation row id.
        pub id: i64,
        /// Original chat id from the export.
        pub chat_identifier: String,
        /// `individual` or `group`.
        pub conversation_type: String,
        /// Group label, when set.
        pub group_title: Option<String>,
        /// Participants of the conversation.
        pub participants: Vec<Participant>,
    }
}

api_shape! {
    /// One attachment of an exported message.
    pub struct Attachment {
        /// Path inside the export.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub path: Option<String>,
        /// File name from the export.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub original_name: Option<String>,
        /// MIME type, when known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub mime_type: Option<String>,
        /// Content fingerprint of the stored bytes.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub sha256: Option<String>,
        /// True for sticker files.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        pub is_sticker: bool,
        /// OCR/ASR transcription, when processed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub transcription: Option<String>,
        /// Why the file is missing, when it is.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub missing_reason: Option<String>,
        /// MIME type of the attachment's preview, when it has one. The
        /// preview's bytes are at `/v1/assets/{sha256}/preview`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub preview_mime_type: Option<String>,
    }
}

api_shape! {
    /// One tapback reaction on an exported message.
    pub struct Tapback {
        /// Attachment part the reaction applies to.
        pub part_index: i64,
        /// Reaction type, e.g. `love`.
        pub kind: String,
        /// Emoji form of the reaction, when one exists.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub emoji: Option<String>,
        /// True when the account owner reacted.
        pub is_from_me: bool,
        /// The identity that reacted, for incoming reactions.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub sender: Option<String>,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The database column and the wire carry one spelling of each status:
    /// the server writes the row with `as_str` and the response with serde.
    #[test]
    fn every_export_status_serializes_as_the_word_the_database_holds() {
        for status in ExportStatus::ALL {
            assert_eq!(
                serde_json::to_value(status).unwrap(),
                serde_json::Value::String(status.as_str().to_string())
            );
            assert_eq!(ExportStatus::parse(status.as_str()), Some(status));
        }
        assert_eq!(ExportStatus::Cancelled.as_str(), "cancelled");
        assert_eq!(ExportStatus::parse("canceled"), None);
    }

    /// The pairing rule this module states, checked rather than trusted: a
    /// field the server may leave out has to read back without it. Serializing
    /// a value whose every skippable field is absent and reading it again is
    /// the shortest way to say that, and it fails the moment a
    /// `skip_serializing_if` arrives without its `default`.
    #[test]
    fn a_value_with_every_skippable_field_absent_reads_back() {
        let message = Message {
            id: 1,
            source: "imessage".into(),
            service: None,
            guid: None,
            timestamp: "2024-01-01T00:00:00Z".into(),
            sort_order: 0,
            is_from_me: false,
            sender: None,
            owner: None,
            subject: None,
            text: None,
            is_announcement: false,
            is_reply: false,
            thread_originator_guid: None,
            thread_originator_part: None,
            num_replies: 0,
            conversation: MessageConversation {
                id: 9,
                chat_identifier: "+15555550100".into(),
                conversation_type: "individual".into(),
                group_title: None,
                participants: vec![Participant {
                    name: "Sarah Vale".into(),
                    identity: None,
                    service: None,
                    contact_id: None,
                }],
            },
            attachments: vec![Attachment {
                path: None,
                original_name: None,
                mime_type: None,
                sha256: None,
                is_sticker: false,
                transcription: None,
                missing_reason: None,
                preview_mime_type: None,
            }],
            tapbacks: vec![Tapback {
                part_index: 0,
                kind: "loved".into(),
                emoji: None,
                is_from_me: true,
                sender: None,
            }],
        };

        let json = serde_json::to_string(&message).expect("serializes");
        let written: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert!(
            written.get("service").is_none(),
            "a message with no service must not carry the key at all: {json}"
        );
        assert_eq!(
            written["attachments"][0],
            serde_json::json!({}),
            "an attachment with nothing known about it writes as an empty object: {json}"
        );

        let read: Message = serde_json::from_str(&json).expect("reads back");
        assert_eq!(read.id, 1);
        assert_eq!(read.conversation.participants[0].name, "Sarah Vale");
        assert_eq!(read.conversation.participants[0].identity, None);
        assert_eq!(read.attachments.len(), 1);
        assert_eq!(read.tapbacks[0].kind, "loved");
    }
}

#[cfg(test)]
mod export_scope_tests {
    use super::*;

    #[test]
    fn the_three_scope_forms_carry_their_kind_and_read_back() {
        let everything = serde_json::to_value(ExportScope::Everything).unwrap();
        assert_eq!(everything, serde_json::json!({ "kind": "everything" }));

        let query = ExportScope::Query {
            list: ExportQueryList::Conversations,
            q: "from:me".into(),
        };
        assert_eq!(
            serde_json::to_value(&query).unwrap(),
            serde_json::json!({ "kind": "query", "list": "conversations", "q": "from:me" })
        );
        // A query names its list: no list is assumed for one that leaves it out.
        assert!(serde_json::from_str::<ExportScope>(r#"{"kind":"query","q":"from:me"}"#).is_err());

        let selection: ExportScope =
            serde_json::from_str(r#"{"kind":"selection","conversation_ids":[3]}"#).unwrap();
        assert_eq!(
            selection,
            ExportScope::Selection {
                conversation_ids: vec![3],
                message_ids: Vec::new(),
            }
        );
        assert!(serde_json::from_str::<ExportScope>(r#"{"kind":"backup"}"#).is_err());
    }
}

/// An RFC 7807 problem document: the body of every failure the server answers,
/// served as `application/problem+json` (`docs/architecture/http-api.md`).
///
/// `type` is the URL of the page describing this kind of failure, one page per
/// type, or `about:blank` for an internal error. A validation failure carries
/// `errors`, every field that failed, in place of `detail`. Every problem
/// repeats the response's `x-request-id` as `request_id`, so the person
/// reading the failure and the operator reading the log are looking at the
/// same request.
///
/// The extension members belong to one type each: `word` and `did_you_mean`
/// to `search-query-invalid`, `retry_after` to `rate-limited`, `line` to
/// `malformed-body`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct Problem {
    /// URL of the page describing this kind of failure; `about:blank` for a
    /// `500 Internal Server Error`.
    #[serde(rename = "type")]
    pub kind: String,
    /// The type's fixed, human-readable name.
    pub title: String,
    /// The HTTP status, repeated in the body.
    pub status: u16,
    /// One sentence about this occurrence. Absent on a validation failure,
    /// which lists `errors` instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Every rule the request broke, one sentence each. Only on
    /// `validation-failed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<String>>,
    /// The `x-request-id` of the response this came in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// `search-query-invalid`: the `word:` the query used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub word: Option<String>,
    /// `search-query-invalid`: a word the language does have, within a small
    /// edit distance of the one used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub did_you_mean: Option<String>,
    /// `rate-limited`: seconds until an attempt may succeed, the same figure
    /// as the `Retry-After` header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<u64>,
    /// `malformed-body` from an import batch: the line of the request body
    /// the server could not read; `validation-failed` from an import batch:
    /// the first line that broke a rule, such as a message without a guid.
    /// Counted from 1 with blank lines included. The body is a batch the
    /// client packed, so only the client can say which file and line of its
    /// own that line came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
}

impl Problem {
    /// The media type every problem is served as.
    pub const CONTENT_TYPE: &'static str = "application/problem+json";

    /// The last segment of the `type` URL, or `None` for `about:blank`: the
    /// name a client branches on.
    #[must_use]
    pub fn slug(&self) -> Option<&str> {
        if self.kind == "about:blank" {
            return None;
        }
        self.kind.rsplit('/').next().filter(|s| !s.is_empty())
    }

    /// The sentence a person reads: `detail`, else `errors` joined with
    /// semicolons, else the title.
    #[must_use]
    pub fn sentence(&self) -> String {
        if let Some(detail) = self.detail.as_deref().map(str::trim)
            && !detail.is_empty()
        {
            return detail.to_string();
        }
        if let Some(errors) = &self.errors
            && !errors.is_empty()
        {
            return errors.join("; ");
        }
        self.title.clone()
    }
}

#[cfg(test)]
mod problem_tests {
    use super::Problem;

    #[test]
    fn a_problem_round_trips_and_names_its_slug() {
        let text = r#"{"type":"https://messagecrate.app/docs/developer/reference/errors/username-taken","title":"Username taken","status":409,"detail":"The username 'alice' already belongs to an account.","request_id":"3f2b1c0e-8d4a-4b6e-9f21-5c7d8e9a0b1c"}"#;
        let problem: Problem = serde_json::from_str(text).unwrap();
        assert_eq!(problem.slug(), Some("username-taken"));
        assert_eq!(
            problem.sentence(),
            "The username 'alice' already belongs to an account."
        );
        assert_eq!(serde_json::to_string(&problem).unwrap(), text);
    }

    #[test]
    fn a_validation_failure_reads_as_its_errors_and_an_internal_error_has_no_slug() {
        let problem = Problem {
            kind: "about:blank".into(),
            title: "Internal server error".into(),
            status: 500,
            detail: None,
            errors: None,
            request_id: None,
            word: None,
            did_you_mean: None,
            retry_after: None,
            line: None,
        };
        assert_eq!(problem.slug(), None);
        assert_eq!(problem.sentence(), "Internal server error");
        let problem = Problem {
            kind: "https://messagecrate.app/docs/developer/reference/errors/validation-failed"
                .into(),
            title: "Validation failed".into(),
            status: 422,
            errors: Some(vec![
                "limit must be at least 1".into(),
                "offset exceeds maximum of 50000".into(),
            ]),
            ..problem
        };
        assert_eq!(
            problem.sentence(),
            "limit must be at least 1; offset exceeds maximum of 50000"
        );
    }
}
