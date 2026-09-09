//! Parse flat EMLs: one text message per `.eml` file (not a multi-message archive).

use crate::archive_html::transcript_from_html;
use crate::assets::extract_attachments;
use crate::types::ParsedMessage;
use mailparse::{MailHeaderMap, ParsedMail};
use phone::sanitize_number;
use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

/// `SMS with <name>` subject matcher.
static SUBJECT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^SMS with (.+)$").expect("subject"));
/// Separator matcher for multi-address headers.
static ADDRESS_SPLIT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[~;,|]+").expect("split"));
/// `SMS archive ` subject prefix matcher.
static ARCHIVE_SUBJECT_PREFIX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^SMS archive ").expect("archive subject"));

/// Android SMS/MMS type codes SMS Backup+ puts in `X-smssync-type` for sent messages
/// (Telephony `MESSAGE_TYPE_SENT`/`OUTBOX`/… and common MMS PDU sent codes).
const SENT_TYPES: &[&str] = &["2", "128", "4", "135", "6", "5"];
/// Android SMS/MMS type codes for inbox / received messages.
const RECEIVED_TYPES: &[&str] = &["1", "132", "130"];

/// Cached headers read once per EML (avoids repeated `get_first_value` + alloc).
#[derive(Debug, Clone)]
pub(crate) struct MailHeaders {
    pub smssync_type: String,
    pub smssync_address: String,
    pub smssync_date: String,
    pub smssync_id: String,
    pub subject: String,
    pub from: String,
    pub to: String,
    pub date: String,
}

impl MailHeaders {
    /// Read the headers this exporter uses, once per EML.
    pub(crate) fn from_mail(mail: &ParsedMail<'_>) -> Self {
        /// The first value of a header, trimmed.
        fn one(mail: &ParsedMail<'_>, name: &str) -> String {
            mail.headers
                .get_first_value(name)
                .unwrap_or_default()
                .trim()
                .to_string()
        }
        Self {
            smssync_type: one(mail, "X-smssync-type"),
            smssync_address: one(mail, "X-smssync-address"),
            smssync_date: one(mail, "X-smssync-date"),
            smssync_id: one(mail, "X-smssync-id"),
            subject: one(mail, "Subject"),
            from: one(mail, "From"),
            to: one(mail, "To"),
            date: one(mail, "Date"),
        }
    }
}

/// Distinct phone numbers from an `X-smssync-address` header.
fn smssync_participant_numbers(raw_address: &str) -> Vec<String> {
    if raw_address.trim().is_empty() {
        return Vec::new();
    }
    let mut numbers = Vec::new();
    let mut seen = HashSet::new();
    for part in ADDRESS_SPLIT_RE.split(raw_address) {
        let token = part.trim();
        if token.is_empty() {
            continue;
        }
        let Some(num) = sanitize_number(token) else {
            continue;
        };
        if !seen.insert(num.clone()) {
            continue;
        }
        numbers.push(num);
    }
    numbers
}

/// The contact name from an `SMS with <name>` subject, unless it is a number.
fn contact_name_from_subject(subject: &str) -> Option<String> {
    let caps = SUBJECT_RE.captures(subject.trim())?;
    let name = caps[1].trim();
    if name.starts_with('+') || name.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(name.to_string())
}

/// Unix seconds from the SMS Backup+ date header (milliseconds or seconds), else the `Date` header.
fn timestamp_seconds(headers: &MailHeaders) -> Option<f64> {
    let raw = &headers.smssync_date;
    if !raw.is_empty() && raw.chars().all(|c| c.is_ascii_digit()) {
        let value: i64 = raw.parse().ok()?;
        // Android uses epoch ms (~1e12 today). Seconds stay ~1e9 until year 5138.
        // Threshold 1e11 catches pre-2001 ms timestamps that the old 1e12 cutoff missed.
        return Some(if value >= 100_000_000_000 {
            value as f64 / 1000.0
        } else {
            value as f64
        });
    }
    if headers.date.is_empty() {
        return None;
    }
    // mailparse does not parse Date headers; try chrono RFC2822.
    chrono::DateTime::parse_from_rfc2822(&headers.date)
        .ok()
        .map(|d| d.timestamp() as f64)
}

/// `owner_emails` must already be trimmed + lowercased.
fn is_sent(headers: &MailHeaders, owner_emails: &[String]) -> bool {
    let typ = headers.smssync_type.as_str();
    if SENT_TYPES.contains(&typ) {
        return true;
    }
    if RECEIVED_TYPES.contains(&typ) {
        return false;
    }
    let from = headers.from.to_ascii_lowercase();
    // Compare against the bare addr-spec, not a substring: owner
    // `ce@example.com` would otherwise match `alice@example.com`.
    let from_addr = if let Some(start) = from.find('<') {
        if let Some(end) = from[start..].find('>') {
            &from[start + 1..start + end]
        } else {
            &from[start + 1..]
        }
    } else {
        &from
    };
    let from_addr = from_addr.trim();
    owner_emails
        .iter()
        .any(|e| !e.is_empty() && from_addr == e.as_str())
}

/// The first body of one MIME type in the tree, newlines normalized to `\n`.
///
/// `mail.parts()` is a depth-first walk that starts with the mail itself, so a
/// single-part message is covered without a special case.
fn first_body_of_type(mail: &ParsedMail<'_>, want: &str) -> Option<String> {
    mail.parts()
        .filter(|part| part.ctype.mimetype.eq_ignore_ascii_case(want))
        .find_map(|part| part.get_body().ok())
        .map(|body| body.replace("\r\n", "\n").replace('\r', "\n"))
}

/// The text of a mail: its HTML rendering when it has one, else its plain text.
///
/// HTML wins because of what archive mail looks like. Many archives carry no
/// `text/plain` part at all and would otherwise read as empty, and in the ones
/// that carry both, the plain-text copy has been hard-wrapped by the sending
/// mail client, so a sentence arrives broken across lines while the HTML keeps
/// it whole. Flat SMS Backup+ mail carries no HTML part, so the preference
/// only ever decides an archive.
pub(crate) fn extract_body_text(mail: &ParsedMail<'_>) -> String {
    if let Some(html) = first_body_of_type(mail, "text/html") {
        return transcript_from_html(&html);
    }
    first_body_of_type(mail, "text/plain").unwrap_or_default()
}

/// True when the EML is one SMS Backup+ message rather than an archive or unrelated mail.
fn is_single_sms_eml(headers: &MailHeaders) -> bool {
    if !headers.smssync_type.is_empty() {
        return true;
    }
    let headers_blob = format!("{} {}", headers.from, headers.to);
    SUBJECT_RE.is_match(&headers.subject) && headers_blob.contains("@sms-backup-plus.local")
}

/// True when this looks like a flat single-message SMS Backup+ EML.
pub(crate) fn is_flat_sms_eml(headers: &MailHeaders) -> bool {
    is_single_sms_eml(headers)
}

/// One SMS Backup+ "flat" EML (one text per file) as a message, or `None`
/// when the file is not one, has no readable date, or names nobody.
pub(crate) fn parse_flat_eml_mail(
    path: &Path,
    mail: &ParsedMail<'_>,
    headers: &MailHeaders,
    owner_digits: &HashSet<String>,
    owner_emails: &[String],
) -> Option<ParsedMessage> {
    if !is_single_sms_eml(headers) {
        return None;
    }
    let timestamp_secs = timestamp_seconds(headers)?;
    let name_alias = contact_name_from_subject(&headers.subject);
    let addresses = FlatAddresses::from_headers(headers, name_alias.as_deref(), owner_digits);
    if addresses.is_blank() {
        return None;
    }
    let sent = is_sent(headers, owner_emails);
    let conversation = addresses.conversation(headers, sent, name_alias.as_deref())?;

    let file_key = hex::encode(Sha256::digest(path.to_string_lossy().as_bytes()));
    let attachments = extract_attachments(
        mail,
        timestamp_secs * 1000.0,
        Some(&file_key[..12.min(file_key.len())]),
    );
    Some(ParsedMessage {
        chat_key: conversation.chat_key,
        conversation_type: conversation.conversation_type.into(),
        group_title: conversation.group_title,
        participant_digits: conversation.participant_digits,
        timestamp_secs,
        is_from_me: sent,
        sender_digits: conversation.sender_digits,
        text: extract_body_text(mail),
        attachments,
        name_alias,
        smssync_id: (!headers.smssync_id.is_empty()).then(|| headers.smssync_id.clone()),
        source_kind: "flat".into(),
        android_type: headers.smssync_type.clone(),
        eml_path: String::new(),
    })
}

/// The numbers on a flat EML: everyone in the SMS Backup+ address header (or
/// the subject's name when that header is blank), the first of them as the
/// address, and those that are not the owner's.
struct FlatAddresses {
    /// The header text the numbers were read from.
    raw: String,
    /// The first participant number, or the raw text sanitized, or blank.
    first: String,
    non_owner: Vec<String>,
}

/// Where a flat EML lands and who sent it.
struct FlatConversation {
    chat_key: String,
    conversation_type: &'static str,
    group_title: Option<String>,
    participant_digits: Vec<(String, Option<String>)>,
    sender_digits: Option<String>,
}

impl FlatAddresses {
    fn from_headers(
        headers: &MailHeaders,
        subject_name: Option<&str>,
        owner_digits: &HashSet<String>,
    ) -> Self {
        let raw = if headers.smssync_address.is_empty() {
            subject_name.unwrap_or_default().to_string()
        } else {
            headers.smssync_address.clone()
        };
        let numbers = smssync_participant_numbers(&raw);
        let first = numbers
            .first()
            .cloned()
            .or_else(|| sanitize_number(&raw))
            .unwrap_or_default();
        let non_owner = numbers
            .into_iter()
            .filter(|n| !owner_digits.contains(n))
            .collect();
        Self {
            raw,
            first,
            non_owner,
        }
    }

    /// Nothing at all to key a conversation on.
    fn is_blank(&self) -> bool {
        self.first.is_empty() && self.raw.is_empty()
    }

    /// A group when two or more peers are named, else the one-to-one chat
    /// with the peer. `None` when nothing identifies the other party and no
    /// display name exists for the contacts lookup to fill it in.
    fn conversation(
        &self,
        headers: &MailHeaders,
        sent: bool,
        name_alias: Option<&str>,
    ) -> Option<FlatConversation> {
        if self.non_owner.len() >= 2 {
            let (chat_key, title) = phone::group_chat_id("group-", &self.non_owner);
            return Some(FlatConversation {
                chat_key,
                conversation_type: "group",
                group_title: Some(title),
                participant_digits: self.non_owner.iter().map(|d| (d.clone(), None)).collect(),
                sender_digits: if sent {
                    None
                } else {
                    self.group_sender(headers)
                },
            });
        }
        // Prefer the first non-owner address (groups already use this rule). An
        // owner-first `owner~peer` list must not key the CSV to the owner's number.
        let peer = self
            .non_owner
            .first()
            .cloned()
            .unwrap_or_else(|| self.first.clone());
        // Keep an empty chat_key when a display name exists so contacts reverse-lookup can fill it.
        if peer.is_empty() && name_alias.map(str::trim).unwrap_or_default().is_empty() {
            return None;
        }
        Some(FlatConversation {
            chat_key: peer.clone(),
            conversation_type: "individual",
            group_title: None,
            participant_digits: if peer.is_empty() {
                vec![]
            } else {
                vec![(peer.clone(), name_alias.map(str::to_string))]
            },
            sender_digits: (!sent && !peer.is_empty()).then_some(peer),
        })
    }

    /// The sender of an incoming group message: the `From` header's number
    /// when it is one of the peers, else the first peer.
    fn group_sender(&self, headers: &MailHeaders) -> Option<String> {
        smssync_participant_numbers(&headers.from)
            .into_iter()
            .find(|n| self.non_owner.contains(n))
            .or_else(|| self.non_owner.first().cloned())
    }
}

/// Classify whether this EML looks like a consolidated archive thread.
pub(crate) fn is_archive_eml(headers: &MailHeaders) -> bool {
    ARCHIVE_SUBJECT_PREFIX_RE.is_match(headers.subject.trim()) && headers.smssync_type.is_empty()
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flat_received() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("msg.eml");
        std::fs::write(
            &path,
            b"From: alice@unknown.email\r\n\
To: me@example.com\r\n\
Subject: SMS with Alice\r\n\
X-smssync-type: 1\r\n\
X-smssync-address: 4075551234\r\n\
X-smssync-date: 1609459200000\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Hello from Alice\r\n",
        )
        .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let mail = mailparse::parse_mail(&bytes).unwrap();
        let headers = MailHeaders::from_mail(&mail);
        let owners = HashSet::from(["5555550100".to_string()]);
        let msg = parse_flat_eml_mail(&path, &mail, &headers, &owners, &[]).unwrap();
        assert!(!msg.is_from_me);
        assert_eq!(msg.text.trim(), "Hello from Alice");
        assert_eq!(msg.chat_key, "4075551234");
        assert!((msg.timestamp_secs - 1_609_459_200.0).abs() < 0.001);
    }

    #[test]
    fn individual_chat_uses_first_non_owner_address() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("msg.eml");
        std::fs::write(
            &path,
            b"From: me@example.com\r\n\
To: alice@unknown.email\r\n\
Subject: SMS with Alice\r\n\
X-smssync-type: 2\r\n\
X-smssync-address: 5555550100~4075551234\r\n\
X-smssync-date: 1609459200000\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Hello\r\n",
        )
        .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let mail = mailparse::parse_mail(&bytes).unwrap();
        let headers = MailHeaders::from_mail(&mail);
        let owners = HashSet::from(["5555550100".to_string()]);
        let msg = parse_flat_eml_mail(&path, &mail, &headers, &owners, &["me@example.com".into()])
            .unwrap();
        assert_eq!(msg.chat_key, "4075551234");
        assert!(msg.is_from_me);
    }

    #[test]
    fn early_2000s_ms_dates_are_not_treated_as_seconds() {
        // 2001-01-01T00:00:00Z as epoch ms is < 1e12; the old >1e12 cutoff
        // would have treated this as seconds (~year 32995).
        let ms = 978_307_200_000_i64;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("msg.eml");
        std::fs::write(
            &path,
            format!(
                "From: alice@unknown.email\r\n\
To: me@example.com\r\n\
Subject: SMS with Alice\r\n\
X-smssync-type: 1\r\n\
X-smssync-address: 4075551234\r\n\
X-smssync-date: {ms}\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
old message\r\n"
            ),
        )
        .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let mail = mailparse::parse_mail(&bytes).unwrap();
        let headers = MailHeaders::from_mail(&mail);
        let owners = HashSet::from(["5555550100".to_string()]);
        let msg = parse_flat_eml_mail(&path, &mail, &headers, &owners, &[]).unwrap();
        assert!((msg.timestamp_secs - 978_307_200.0).abs() < 0.001);
    }

    #[test]
    fn sent_detection_uses_exact_owner_email() {
        fn headers_with_from(from: &str) -> MailHeaders {
            MailHeaders {
                smssync_type: String::new(),
                smssync_address: String::new(),
                smssync_date: String::new(),
                smssync_id: String::new(),
                subject: String::new(),
                from: from.into(),
                to: String::new(),
                date: String::new(),
            }
        }
        // `ce@example.com` is a substring of `alice@example.com`; substring
        // matching would misclassify this received message as sent.
        assert!(!is_sent(
            &headers_with_from("alice@example.com"),
            &["ce@example.com".into()]
        ));
        // Exact addr-spec matches still detect sent mail, including the
        // `Name <addr>` form.
        assert!(is_sent(
            &headers_with_from("alice@example.com"),
            &["alice@example.com".into()]
        ));
        assert!(is_sent(
            &headers_with_from("Alice <alice@example.com>"),
            &["alice@example.com".into()]
        ));
    }

    #[test]
    fn group_chat_id_does_not_collide_on_digit_split() {
        let (k1, _) = phone::group_chat_id("group-", &["12".to_string(), "34".to_string()]);
        let (k2, _) = phone::group_chat_id("group-", &["123".to_string(), "4".to_string()]);
        assert_ne!(k1, k2);
        assert_eq!(k1, "group-2:12_2:34");
        assert_eq!(k2, "group-3:123_1:4");
    }
}
