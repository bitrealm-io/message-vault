//! Parse SMS Backup+ EMLs: one text message per `.eml` file.

use crate::assets::extract_attachments;
use crate::types::ParsedMessage;
use mailparse::{MailHeaderMap, ParsedMail};
use phone::{Handle, OwnerHandleSet};
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

/// Android SMS/MMS type codes SMS Backup+ puts in `X-smssync-type` for sent messages
/// (Telephony `MESSAGE_TYPE_SENT`/`OUTBOX`/… and common MMS PDU sent codes).
const SENT_TYPES: &[&str] = &["2", "128", "4", "135", "6", "5"];
/// Android SMS/MMS type codes for inbox / received messages.
const RECEIVED_TYPES: &[&str] = &["1", "132", "130"];

/// Cached headers read once per EML (avoids repeated `get_first_value` + alloc).
#[derive(Debug, Clone)]
pub(crate) struct MailHeaders {
    /// `SMS`, `MMS` or `CALLLOG`; SMS Backup+ writes it on every mail.
    pub smssync_datatype: String,
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
            smssync_datatype: one(mail, "X-smssync-datatype"),
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

    /// True for a mail SMS Backup+ wrote from the phone's call log.
    ///
    /// Such a mail carries `X-smssync-type` too, holding the call's type, so
    /// only `X-smssync-datatype` tells a call from a text message.
    pub(crate) fn is_call_log(&self) -> bool {
        self.smssync_datatype.eq_ignore_ascii_case("CALLLOG")
    }
}

/// The addresses in an `X-smssync-address` header, once each by key.
fn smssync_addresses(raw_address: &str) -> Vec<Handle> {
    let mut addresses = Vec::new();
    let mut seen = HashSet::new();
    for handle in ADDRESS_SPLIT_RE
        .split(raw_address)
        .filter_map(Handle::parse)
    {
        if seen.insert(handle.key().to_string()) {
            addresses.push(handle);
        }
    }
    addresses
}

/// The address in a `From` header, which SMS Backup+ writes as
/// `"Bob" <+14075555678@unknown.email>`: the part before the `@`.
fn from_address(from: &str) -> Option<Handle> {
    let addr_spec = from
        .split_once('<')
        .map_or(from, |(_, rest)| rest.split('>').next().unwrap_or(rest));
    let local = addr_spec.split('@').next().unwrap_or(addr_spec);
    Handle::parse(local)
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

/// The message text: the first `text/plain` part of the mail.
///
/// SMS Backup+ writes the message body as `text/plain` on every mail it
/// produces — zero of 20,000 sampled carry a `text/html` part — so plain text
/// is the only part worth reading.
pub(crate) fn extract_body_text(mail: &ParsedMail<'_>) -> String {
    first_body_of_type(mail, "text/plain").unwrap_or_default()
}

/// True when the EML is one SMS Backup+ message rather than unrelated mail
/// or a call from the call log.
fn is_single_sms_eml(headers: &MailHeaders) -> bool {
    if headers.is_call_log() {
        return false;
    }
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
    owners: &OwnerHandleSet,
    owner_emails: &[String],
) -> Option<ParsedMessage> {
    if !is_single_sms_eml(headers) {
        return None;
    }
    let timestamp_secs = timestamp_seconds(headers)?;
    let name_alias = contact_name_from_subject(&headers.subject);
    let addresses = FlatAddresses::from_headers(headers, owners);
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
        participants: conversation.participants,
        timestamp_secs,
        is_from_me: sent,
        sender: conversation.sender,
        text: extract_body_text(mail),
        attachments,
        name_alias,
        smssync_id: (!headers.smssync_id.is_empty()).then(|| headers.smssync_id.clone()),
        android_type: headers.smssync_type.clone(),
        eml_path: String::new(),
    })
}

/// The addresses on a flat EML: everyone in the SMS Backup+ address header,
/// the first of them, and those that are not the owner's.
struct FlatAddresses {
    /// The first address in the header.
    first: Option<Handle>,
    non_owner: Vec<Handle>,
}

/// Where a flat EML lands and who sent it.
struct FlatConversation {
    chat_key: String,
    conversation_type: &'static str,
    group_title: Option<String>,
    participants: Vec<Handle>,
    sender: Option<Handle>,
}

impl FlatAddresses {
    fn from_headers(headers: &MailHeaders, owners: &OwnerHandleSet) -> Self {
        let addresses = smssync_addresses(&headers.smssync_address);
        let first = addresses.first().cloned();
        let non_owner = addresses
            .into_iter()
            .filter(|a| !owners.is_owner(a))
            .collect();
        Self { first, non_owner }
    }

    /// A group when two or more peers are named, else the one-to-one chat
    /// with the peer. `None` when nothing identifies the other party and no
    /// display name exists to key the conversation on (`name_only_key`).
    fn conversation(
        &self,
        headers: &MailHeaders,
        sent: bool,
        name_alias: Option<&str>,
    ) -> Option<FlatConversation> {
        if self.non_owner.len() >= 2 {
            let keys: Vec<String> = self.non_owner.iter().map(|a| a.key().to_string()).collect();
            let (chat_key, title) = phone::group_chat_id("group-", &keys);
            return Some(FlatConversation {
                chat_key,
                conversation_type: "group",
                group_title: Some(title),
                participants: self.non_owner.clone(),
                sender: if sent {
                    None
                } else {
                    self.group_sender(headers)
                },
            });
        }
        // Prefer the first non-owner address (groups already use this rule). An
        // owner-first `owner~peer` list must not key the CSV to the owner's number.
        let peer = self.non_owner.first().or(self.first.as_ref()).cloned();
        // Keep an empty chat_key when a display name exists so `name_only_key` can key on it.
        if peer.is_none() && name_alias.map(str::trim).unwrap_or_default().is_empty() {
            return None;
        }
        Some(FlatConversation {
            chat_key: peer
                .as_ref()
                .map(|p| p.key().to_string())
                .unwrap_or_default(),
            conversation_type: "individual",
            group_title: None,
            participants: peer.iter().cloned().collect(),
            sender: peer.filter(|_| !sent),
        })
    }

    /// The sender of an incoming group message: the `From` header's address
    /// when it is one of the peers, else the first peer.
    fn group_sender(&self, headers: &MailHeaders) -> Option<Handle> {
        from_address(&headers.from)
            .and_then(|from| {
                self.non_owner
                    .iter()
                    .find(|peer| peer.key() == from.key())
                    .cloned()
            })
            .or_else(|| self.non_owner.first().cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use message_ir::HandleType;

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
        let owners = OwnerHandleSet::from_phones(&["5555550100".to_string()]).unwrap();
        let msg = parse_flat_eml_mail(&path, &mail, &headers, &owners, &[]).unwrap();
        assert!(!msg.is_from_me);
        assert_eq!(msg.text.trim(), "Hello from Alice");
        assert_eq!(msg.chat_key, "+14075551234");
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
        let owners = OwnerHandleSet::from_phones(&["5555550100".to_string()]).unwrap();
        let msg = parse_flat_eml_mail(&path, &mail, &headers, &owners, &["me@example.com".into()])
            .unwrap();
        assert_eq!(msg.chat_key, "+14075551234");
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
        let owners = OwnerHandleSet::from_phones(&["5555550100".to_string()]).unwrap();
        let msg = parse_flat_eml_mail(&path, &mail, &headers, &owners, &[]).unwrap();
        assert!((msg.timestamp_secs - 978_307_200.0).abs() < 0.001);
    }

    #[test]
    fn sent_detection_uses_exact_owner_email() {
        fn headers_with_from(from: &str) -> MailHeaders {
            MailHeaders {
                smssync_datatype: String::new(),
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

    /// SMS Backup+ titles a thread "SMS with <who>". For a saved contact
    /// that is their name; for an unsaved one it is the number, which is an
    /// address and never a display name.
    #[test]
    fn a_subject_naming_a_number_gives_no_contact_name() {
        assert_eq!(contact_name_from_subject("SMS with +15555550101"), None);
        assert_eq!(contact_name_from_subject("SMS with 5550101"), None);
        assert_eq!(
            contact_name_from_subject("SMS with Sam").as_deref(),
            Some("Sam")
        );
    }

    /// A mail with no `X-smssync-date` header takes its time from `Date`.
    #[test]
    fn a_mail_without_the_smssync_date_is_timed_by_its_date_header() {
        let headers = MailHeaders {
            smssync_datatype: String::new(),
            smssync_type: "1".into(),
            smssync_address: "4075551234".into(),
            smssync_date: String::new(),
            smssync_id: String::new(),
            subject: "SMS with Alice".into(),
            from: String::new(),
            to: String::new(),
            date: "Fri, 01 Jan 2021 00:00:00 +0000".into(),
        };
        assert_eq!(timestamp_seconds(&headers), Some(1_609_459_200.0));
    }

    #[test]
    fn group_chat_id_does_not_collide_on_digit_split() {
        let (k1, _) = phone::group_chat_id("group-", &["12".to_string(), "34".to_string()]);
        let (k2, _) = phone::group_chat_id("group-", &["123".to_string(), "4".to_string()]);
        assert_ne!(k1, k2);
        assert_eq!(k1, "group-2:12_2:34");
        assert_eq!(k2, "group-3:123_1:4");
    }

    /// Parse `eml` (written with `\n` line ends) as one flat EML.
    fn parse(eml: &str, owners: &[&str]) -> Option<ParsedMessage> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("msg.eml");
        std::fs::write(&path, eml.replace('\n', "\r\n")).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let mail = mailparse::parse_mail(&bytes).unwrap();
        let headers = MailHeaders::from_mail(&mail);
        let owners: Vec<String> = owners.iter().map(|s| s.to_string()).collect();
        let owners = OwnerHandleSet::from_phones(&owners).unwrap();
        parse_flat_eml_mail(&path, &mail, &headers, &owners, &[])
    }

    /// SMS Backup+ writes `X-smssync-type` on a call-log mail too, holding
    /// the call's type, so only `X-smssync-datatype` tells a call from a text.
    #[test]
    fn a_call_log_mail_is_not_a_text_message() {
        let msg = parse(
            "From: x@unknown.email\nTo: me@example.com\nSubject: Call with Alice\nX-smssync-datatype: CALLLOG\nX-smssync-type: 1\nX-smssync-address: 4075551234\nX-smssync-date: 1609459200000\nContent-Type: text/plain; charset=utf-8\n\n123s (00:02:03)\n4075551234 (incoming call)\n",
            &["5555550100"],
        );
        assert!(msg.is_none(), "{:?}", msg.map(|m| m.text));
    }

    /// An MMS whose address list names the owner in national form
    /// (`07700900123` for `+447700900123`) is one-to-one with the other number.
    #[test]
    fn the_owner_in_national_form_is_not_a_peer() {
        let msg = parse(
            "From: x@unknown.email\nTo: me@example.com\nSubject: MMS with X\nX-smssync-type: 132\nX-smssync-address: 07700900123~+447911123456\nX-smssync-date: 1609459200000\nContent-Type: text/plain; charset=utf-8\n\nhi\n",
            &["+447700900123"],
        )
        .unwrap();
        assert_eq!(msg.conversation_type, "individual");
    }

    fn received_from(address: &str) -> ParsedMessage {
        parse(
            &format!("From: x@unknown.email\nTo: me@example.com\nSubject: SMS with X\nX-smssync-type: 1\nX-smssync-address: {address}\nX-smssync-date: 1609459200000\nContent-Type: text/plain; charset=utf-8\n\nhi\n"),
            &["5555550100"],
        )
        .unwrap()
    }

    #[test]
    fn an_international_number_keeps_its_country() {
        let msg = received_from("+6591234567");
        assert_eq!(crate::identity::chat_id_for(&msg), "+6591234567");
    }

    #[test]
    fn an_email_address_and_a_sender_name_are_not_numbers() {
        let msg = received_from("john1985@example.com");
        assert_eq!(crate::identity::chat_id_for(&msg), "john1985@example.com");
        let sender = msg.sender.unwrap();
        assert_eq!(sender.kind(), HandleType::Email);
        let msg = received_from("AMAZON");
        assert_eq!(crate::identity::chat_id_for(&msg), "AMAZON");
        assert_eq!(msg.sender.unwrap().kind(), HandleType::Other);
    }

    /// The `From` of a group message names its sender inside the address,
    /// `"Bob" <+447911123456@unknown.email>`.
    #[test]
    fn a_group_sender_is_read_from_the_from_address() {
        let msg = parse(
            "From: \"Bob\" <+447911123456@unknown.email>\nTo: me@example.com\nSubject: MMS with X\nX-smssync-type: 132\nX-smssync-address: +447700900456~+447911123456\nX-smssync-date: 1609459200000\nContent-Type: text/plain; charset=utf-8\n\nhi\n",
            &["+447700900123"],
        )
        .unwrap();
        assert_eq!(msg.sender.unwrap().key(), "+447911123456");
    }
}
