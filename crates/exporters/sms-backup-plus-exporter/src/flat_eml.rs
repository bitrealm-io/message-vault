//! Parse SMS Backup+ EMLs: one text message per `.eml` file.

use crate::assets::extract_body;
use crate::types::ParsedMessage;
use mailparse::{MailHeaderMap, ParsedMail};
use message_ir::IrConversationType;
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

/// The domain SMS Backup+ puts after a number or name it has no email
/// address for: `+14075550108@unknown.email`.
pub(crate) const UNKNOWN_EMAIL_DOMAIN: &str = "unknown.email";

/// Two or more other participants make a group conversation.
const GROUP_MIN_PARTICIPANTS: usize = 2;

/// The fewest addresses in `To` that can name a group. A sent MMS names only
/// the recipients there, and a received one the owner too, but never its
/// sender, so either way two addresses is the least a group gives.
const GROUP_MIN_TO_ADDRESSES: usize = 2;

/// The person whose phone the archive came from: their numbers, and the
/// email addresses SMS Backup+ writes for them in `From` and `To`.
pub(crate) struct Owner {
    handles: OwnerHandleSet,
    /// Trimmed and lowercased; none empty.
    emails: Vec<String>,
}

impl Owner {
    /// The owner from their numbers and email addresses as the person gave
    /// them. Email addresses are trimmed and lowercased once here.
    pub(crate) fn new(handles: OwnerHandleSet, emails: &[String]) -> Self {
        let emails = emails
            .iter()
            .map(|e| e.trim().to_ascii_lowercase())
            .filter(|e| !e.is_empty())
            .collect();
        Self { handles, emails }
    }

    /// The owner's first number, as a handle key.
    pub(crate) fn primary_handle(&self) -> Option<String> {
        self.handles.primary_owner_handle()
    }

    /// True when `handle` is one of the owner's numbers.
    fn is_owner_handle(&self, handle: &Handle) -> bool {
        self.handles.is_owner(handle)
    }

    /// How many email addresses the owner has.
    pub(crate) fn email_count(&self) -> usize {
        self.emails.len()
    }

    /// True when `addr_spec` (no display name) is one of the owner's email
    /// addresses, compared whole: `ce@example.com` is not `alice@example.com`.
    fn is_owner_email(&self, addr_spec: &str) -> bool {
        let addr_spec = addr_spec.trim();
        self.emails
            .iter()
            .any(|e| addr_spec.eq_ignore_ascii_case(e))
    }
}

/// The address inside `<…>` of one mail address, or the whole value when it
/// has no `<`. A display name is never part of it.
fn addr_spec(address: &str) -> &str {
    address
        .split_once('<')
        .map_or(address, |(_, rest)| rest.split('>').next().unwrap_or(rest))
        .trim()
}

/// The handle in one mail address (`addr-spec`, no display name). SMS Backup+
/// writes a contact with an email address as that address, and anyone else as
/// `<number or name>@unknown.email`, whose part before the `@` is the handle.
fn mail_address_handle(addr_spec: &str) -> Option<Handle> {
    let addr_spec = addr_spec.trim();
    match addr_spec.rsplit_once('@') {
        Some((local, domain)) if domain.eq_ignore_ascii_case(UNKNOWN_EMAIL_DOMAIN) => {
            Handle::parse(local)
        }
        _ => Handle::parse(addr_spec),
    }
}

/// The address in a `From` header, which SMS Backup+ writes as
/// `"Bob" <+14075550108@unknown.email>`. Only the part inside `<…>` is read:
/// a digit in the display name is not part of the number.
fn from_address(from: &str) -> Option<Handle> {
    mail_address_handle(addr_spec(from))
}

/// Every address a `To` header names, as `addr-spec`s. Empty when the header
/// cannot be read.
fn to_addresses(to: &str) -> Vec<String> {
    let Ok(list) = mailparse::addrparse(to) else {
        return Vec::new();
    };
    list.iter()
        .flat_map(|addr| match addr {
            mailparse::MailAddr::Single(info) => std::slice::from_ref(info),
            mailparse::MailAddr::Group(group) => group.addrs.as_slice(),
        })
        .map(|single| single.addr.trim().to_string())
        .collect()
}

/// What the `From` and `To` of a mail say about a group.
enum MailParticipants {
    /// Not a group: fewer than two other participants.
    NotAGroup,
    /// A group of these other participants, once each by key.
    Group(Vec<Handle>),
    /// A received mail whose `To` names two or more addresses, none of them
    /// a number or email address the owner gave. One of them is the owner
    /// under an address the run does not know, so the set cannot be trusted.
    OwnerNotNamed,
}

/// The other participants an MMS names in `From` and `To`, leaving out the
/// owner's numbers and email addresses.
///
/// SMS Backup+ writes only the first address of an MMS in
/// `X-smssync-address`. A sent MMS names every recipient in `To`; a received
/// one names its sender in `From` and every other recipient, the owner
/// included, in `To` (`MessageGenerator.messageFromMapMms` and
/// `MmsSupport.getDetails` in jberkel/sms-backup-plus at `fd33c32`). So a
/// one-to-one MMS names one address in `To` either way: the recipient, or
/// the owner.
fn mail_participants(headers: &MailHeaders, sent: bool, owner: &Owner) -> MailParticipants {
    let to = to_addresses(&headers.to);
    if to.len() < GROUP_MIN_TO_ADDRESSES {
        return MailParticipants::NotAGroup;
    }
    let sender = (!sent).then(|| addr_spec(&headers.from).to_string());
    let mut participants = Vec::new();
    let mut seen = HashSet::new();
    let mut owner_named = false;
    for address in sender.into_iter().chain(to) {
        if owner.is_owner_email(&address) {
            owner_named = true;
            continue;
        }
        let Some(handle) = mail_address_handle(&address) else {
            continue;
        };
        if owner.is_owner_handle(&handle) {
            owner_named = true;
        } else if seen.insert(handle.key().to_string()) {
            participants.push(handle);
        }
    }
    if !sent && !owner_named {
        MailParticipants::OwnerNotNamed
    } else if participants.len() < GROUP_MIN_PARTICIPANTS {
        MailParticipants::NotAGroup
    } else {
        MailParticipants::Group(participants)
    }
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

/// Unix seconds from the SMS Backup+ date header (milliseconds or seconds), else the `Date` header,
/// and whether the time carries milliseconds.
fn timestamp_seconds(headers: &MailHeaders) -> Option<(f64, bool)> {
    let raw = &headers.smssync_date;
    if !raw.is_empty() && raw.chars().all(|c| c.is_ascii_digit()) {
        let value: i64 = raw.parse().ok()?;
        // Android uses epoch ms (~1e12 today). Seconds stay ~1e9 until year 5138.
        // Threshold 1e11 catches pre-2001 ms timestamps that the old 1e12 cutoff missed.
        return Some(if value >= 100_000_000_000 {
            (value as f64 / 1000.0, true)
        } else {
            (value as f64, false)
        });
    }
    if headers.date.is_empty() {
        return None;
    }
    // mailparse does not parse Date headers; try chrono RFC2822.
    chrono::DateTime::parse_from_rfc2822(&headers.date)
        .ok()
        .map(|d| (d.timestamp() as f64, false))
}

/// True for a message the owner sent: by `X-smssync-type`, else when `From`
/// is one of the owner's email addresses.
fn is_sent(headers: &MailHeaders, owner: &Owner) -> bool {
    let typ = headers.smssync_type.as_str();
    if SENT_TYPES.contains(&typ) {
        return true;
    }
    if RECEIVED_TYPES.contains(&typ) {
        return false;
    }
    owner.is_owner_email(addr_spec(&headers.from))
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
    owner: &Owner,
) -> Option<ParsedMessage> {
    if !is_single_sms_eml(headers) {
        return None;
    }
    let (timestamp_secs, has_milliseconds) = timestamp_seconds(headers)?;
    let name_alias = contact_name_from_subject(&headers.subject);
    let sent = is_sent(headers, owner);
    let addresses = FlatAddresses::from_headers(headers, owner, sent);
    let conversation = addresses.conversation(headers, sent, name_alias.as_deref())?;
    let owner_not_named = addresses.owner_not_named;
    // The subject names the `X-smssync-address` contact, who is not known to
    // have written a mail that does not name the owner. It keys only a
    // conversation nothing else identifies.
    let name_alias = name_alias.filter(|_| !owner_not_named || conversation.chat_key.is_empty());

    let file_key = hex::encode(Sha256::digest(path.to_string_lossy().as_bytes()));
    let body = extract_body(
        mail,
        timestamp_secs * 1000.0,
        Some(&file_key[..12.min(file_key.len())]),
    );
    Some(ParsedMessage {
        chat_key: conversation.chat_key,
        conversation_type: conversation.conversation_type,
        group_title: conversation.group_title,
        participants: conversation.participants,
        timestamp_secs,
        has_milliseconds,
        is_from_me: sent,
        sender: conversation.sender,
        text: body.text,
        attachments: body.attachments,
        unreadable_parts: body.unreadable_parts,
        name_alias,
        smssync_id: (!headers.smssync_id.is_empty()).then(|| headers.smssync_id.clone()),
        android_type: headers.smssync_type.clone(),
        eml_path: String::new(),
        owner_not_named,
    })
}

/// The addresses on a flat EML: the first of them, and those that are not
/// the owner's. They come from `From` and `To` when an MMS names two or more
/// other participants there, from `From` alone when a received one does not
/// name the owner, else from the SMS Backup+ address header.
struct FlatAddresses {
    /// The first address in the header.
    first: Option<Handle>,
    non_owner: Vec<Handle>,
    /// A received mail whose `To` names a group without naming the owner by
    /// any number or email address they gave (`MailParticipants::OwnerNotNamed`).
    owner_not_named: bool,
}

/// Where a flat EML lands and who sent it.
struct FlatConversation {
    chat_key: String,
    conversation_type: IrConversationType,
    group_title: Option<String>,
    participants: Vec<Handle>,
    sender: Option<Handle>,
}

impl FlatAddresses {
    /// An MMS that names two or more other participants in `From` and `To`
    /// is a group of them. A received one whose `To` does not name the owner
    /// is not trusted, since the owner would count as a participant: it goes
    /// to the one-to-one conversation of the address in `From`, who wrote it,
    /// or, when `From` gives none, to `X-smssync-address`.
    /// Otherwise the addresses come from
    /// `X-smssync-address`: `To` decides nothing for one participant, because
    /// it may give that person's email address where `X-smssync-address`
    /// gives the number, and the number is what keys the one-to-one
    /// conversation.
    fn from_headers(headers: &MailHeaders, owner: &Owner, sent: bool) -> Self {
        let owner_not_named = match mail_participants(headers, sent, owner) {
            MailParticipants::Group(participants) => {
                return Self {
                    first: participants.first().cloned(),
                    non_owner: participants,
                    owner_not_named: false,
                };
            }
            MailParticipants::OwnerNotNamed => {
                if let Some(from) = from_address(&headers.from) {
                    return Self {
                        first: Some(from.clone()),
                        non_owner: vec![from],
                        owner_not_named: true,
                    };
                }
                true
            }
            MailParticipants::NotAGroup => false,
        };
        let addresses = smssync_addresses(&headers.smssync_address);
        let first = addresses.first().cloned();
        let non_owner = addresses
            .into_iter()
            .filter(|a| !owner.is_owner_handle(a))
            .collect();
        Self {
            first,
            non_owner,
            owner_not_named,
        }
    }

    /// A group when two or more other participants are named, else the one-to-one chat
    /// with the peer. `None` when nothing identifies the other party and no
    /// display name exists to key the conversation on (`name_only_key`).
    fn conversation(
        &self,
        headers: &MailHeaders,
        sent: bool,
        name_alias: Option<&str>,
    ) -> Option<FlatConversation> {
        if self.non_owner.len() >= GROUP_MIN_PARTICIPANTS {
            let keys: Vec<String> = self.non_owner.iter().map(|a| a.key().to_string()).collect();
            let (chat_key, title) = phone::group_chat_id("group-", &keys);
            return Some(FlatConversation {
                chat_key,
                conversation_type: IrConversationType::Group,
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
            conversation_type: IrConversationType::Individual,
            group_title: None,
            participants: peer.iter().cloned().collect(),
            sender: peer.filter(|_| !sent && self.peer_is_sender(headers)),
        })
    }

    /// True when the one-to-one peer is who wrote an incoming message. For a
    /// mail that does not name the owner, only the address in `From` says
    /// who wrote it, and the peer is that address or nobody.
    fn peer_is_sender(&self, headers: &MailHeaders) -> bool {
        !self.owner_not_named || from_address(&headers.from).is_some()
    }

    /// The sender of an incoming group message: the `From` header's address
    /// when it is one of the peers, else nobody. Where the sender sits in the
    /// address list is arbitrary, so the first peer would be a guess, and a
    /// contact with an email address is named in `From` by that address
    /// alone.
    fn group_sender(&self, headers: &MailHeaders) -> Option<Handle> {
        let from = from_address(&headers.from)?;
        self.non_owner
            .iter()
            .find(|peer| peer.key() == from.key())
            .cloned()
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
X-smssync-address: 4075550107\r\n\
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
        let msg = parse_flat_eml_mail(&path, &mail, &headers, &Owner::new(owners, &[])).unwrap();
        assert!(!msg.is_from_me);
        assert_eq!(msg.text.trim(), "Hello from Alice");
        assert_eq!(msg.chat_key, "+14075550107");
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
X-smssync-address: 5555550100~4075550107\r\n\
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
        let msg = parse_flat_eml_mail(
            &path,
            &mail,
            &headers,
            &Owner::new(owners, &["me@example.com".into()]),
        )
        .unwrap();
        assert_eq!(msg.chat_key, "+14075550107");
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
X-smssync-address: 4075550107\r\n\
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
        let msg = parse_flat_eml_mail(&path, &mail, &headers, &Owner::new(owners, &[])).unwrap();
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
        let owner = Owner::new(
            OwnerHandleSet::from_phones(&["5555550100".to_string()]).unwrap(),
            &["ce@example.com".into()],
        );
        assert!(!is_sent(&headers_with_from("alice@example.com"), &owner));
        // Exact addr-spec matches still detect sent mail, including the
        // `Name <addr>` form.
        let owner = Owner::new(
            OwnerHandleSet::from_phones(&["5555550100".to_string()]).unwrap(),
            &["Alice@Example.com ".into()],
        );
        assert!(is_sent(&headers_with_from("alice@example.com"), &owner));
        assert!(is_sent(
            &headers_with_from("Alice <alice@example.com>"),
            &owner
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
            smssync_address: "4075550107".into(),
            smssync_date: String::new(),
            smssync_id: String::new(),
            subject: "SMS with Alice".into(),
            from: String::new(),
            to: String::new(),
            date: "Fri, 01 Jan 2021 00:00:00 +0000".into(),
        };
        assert_eq!(timestamp_seconds(&headers), Some((1_609_459_200.0, false)));
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
        let owner = Owner::new(owners, &["me@example.com".to_string()]);
        parse_flat_eml_mail(&path, &mail, &headers, &owner)
    }

    /// An incoming MMS from 4075550107 whose MIME parts are `body`.
    fn mms_mail(body: &str) -> ParsedMessage {
        parse(
            &format!(
                "From: x@unknown.email\nTo: me@example.com\nSubject: SMS with X\nX-smssync-type: 1\nX-smssync-address: 4075550107\nX-smssync-date: 1609459200000\nMIME-Version: 1.0\nContent-Type: multipart/mixed; boundary=\"b\"\n\n{body}--b--\n"
            ),
            &["5555550100"],
        )
        .unwrap()
    }

    /// Every `text/plain` part is the message's text, in the order of the
    /// parts, and nothing is removed for repeating another.
    #[test]
    fn every_text_part_is_kept_in_order() {
        let msg = mms_mail(
            "--b\nContent-Type: text/plain; charset=utf-8\n\nTickets attached\n--b\nContent-Type: text/plain; charset=utf-8\n\nAll three are for Friday\n--b\nContent-Type: text/plain; charset=utf-8\n\nTickets attached\n",
        );
        assert_eq!(
            msg.text,
            "Tickets attached\nAll three are for Friday\nTickets attached"
        );
    }

    /// A contact card is a text type, and an attachment all the same.
    #[test]
    fn a_contact_card_is_an_attachment() {
        let msg = mms_mail(
            "--b\nContent-Type: text/plain; charset=utf-8\n\ncard\n--b\nContent-Type: text/x-vcard; name=\"sam.vcf\"\n\nBEGIN:VCARD\nFN:Sam\nEND:VCARD\n",
        );
        assert_eq!(msg.text, "card");
        assert_eq!(msg.attachments.len(), 1);
        assert_eq!(msg.attachments[0].original_name.as_deref(), Some("sam.vcf"));
        assert!(msg.attachments[0].data.starts_with(b"BEGIN:VCARD"));
    }

    /// A part whose bytes cannot be decoded is neither text nor an
    /// attachment, and it is counted.
    #[test]
    fn a_part_that_cannot_be_decoded_is_counted() {
        let msg = mms_mail(
            "--b\nContent-Type: text/plain; charset=utf-8\n\nhi\n--b\nContent-Type: image/jpeg\nContent-Transfer-Encoding: base64\n\n@@@@\n",
        );
        assert_eq!(msg.text, "hi");
        assert!(msg.attachments.is_empty());
        assert_eq!(msg.unreadable_parts, 1);
    }

    /// SMS Backup+ writes `X-smssync-type` on a call-log mail too, holding
    /// the call's type, so only `X-smssync-datatype` tells a call from a text.
    #[test]
    fn a_call_log_mail_is_not_a_text_message() {
        let msg = parse(
            "From: x@unknown.email\nTo: me@example.com\nSubject: Call with Alice\nX-smssync-datatype: CALLLOG\nX-smssync-type: 1\nX-smssync-address: 4075550107\nX-smssync-date: 1609459200000\nContent-Type: text/plain; charset=utf-8\n\n123s (00:02:03)\n4075550107 (incoming call)\n",
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
        assert_eq!(msg.conversation_type, IrConversationType::Individual);
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
        let msg = received_from("+6595550100");
        assert_eq!(crate::identity::chat_id_for(&msg), "+6595550100");
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

    /// A digit in the display name of `From` is not part of the number:
    /// `"Mom 2" <+14075550108@unknown.email>` is from 4075550108.
    #[test]
    fn a_name_with_a_digit_does_not_move_the_sender() {
        let msg = parse(
            "From: \"Mom 2\" <+14075550108@unknown.email>\nTo: me@example.com\nSubject: SMS with group\nX-smssync-type: 132\nX-smssync-address: 4075550150~4075550108~5555550100\nX-smssync-date: 1609459200000\nContent-Type: text/plain; charset=utf-8\n\nhi\n",
            &["5555550100"],
        )
        .unwrap();
        assert_eq!(msg.conversation_type, IrConversationType::Group);
        assert_eq!(msg.sender.unwrap().key(), "+14075550108");
    }

    /// A received group message whose `From` names nobody in the group has
    /// no sender: the first address of the list would be a guess.
    #[test]
    fn a_group_message_from_nobody_in_the_group_has_no_sender() {
        for from in [
            "Bob <bob@example.com>",
            "\"Carol\" <+14075550152@unknown.email>",
        ] {
            let msg = parse(
                &format!("From: {from}\nTo: me@example.com\nSubject: SMS with group\nX-smssync-type: 132\nX-smssync-address: 4075550150~4075550108\nX-smssync-date: 1609459200000\nContent-Type: text/plain; charset=utf-8\n\nhi\n"),
                &["5555550100"],
            )
            .unwrap();
            assert_eq!(msg.conversation_type, IrConversationType::Group, "{from}");
            assert!(msg.sender.is_none(), "{from}: {:?}", msg.sender);
        }
    }

    /// SMS Backup+ names one address in `X-smssync-address` for a sent MMS
    /// and every recipient in `To`, so `To` is what makes it a group.
    #[test]
    fn a_sent_mms_to_three_recipients_is_a_group_of_three() {
        let msg = parse(
            "From: me@example.com\nTo: \"Alice\" <+14075550150@unknown.email>, Bob <4075550151@unknown.email>,\n \"Carol\" <carol@example.org>\nSubject: SMS with Alice\nX-smssync-type: 128\nX-smssync-address: 4075550150\nX-smssync-date: 1609459200000\nContent-Type: text/plain; charset=utf-8\n\nhi all\n",
            &["5555550100"],
        )
        .unwrap();
        assert!(msg.is_from_me);
        assert_eq!(msg.conversation_type, IrConversationType::Group);
        let mut keys: Vec<&str> = msg.participants.iter().map(Handle::key).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["+14075550150", "+14075550151", "carol@example.org"]);
        assert!(msg.sender.is_none());
    }

    /// SMS Backup+ names the sender of a received MMS in `From` and every
    /// other recipient, the owner included, in `To`. A reply in a group lands
    /// in the conversation the owner's sent MMS to that group made, with its
    /// sender read from `From`.
    #[test]
    fn a_received_group_mms_joins_the_group_the_owner_sent_to() {
        let sent = parse(
            "From: me@example.com\nTo: \"Alice\" <+14075550150@unknown.email>, \"Carol\" <carol@example.org>\nSubject: SMS with Alice\nX-smssync-type: 128\nX-smssync-address: 4075550150\nX-smssync-date: 1609459200000\nContent-Type: text/plain; charset=utf-8\n\nhi all\n",
            &["5555550100"],
        )
        .unwrap();
        let received = parse(
            "From: \"Carol\" <carol@example.org>\nTo: <+15555550100@unknown.email>, \"Alice\" <+14075550150@unknown.email>\nSubject: SMS with Carol\nX-smssync-type: 132\nX-smssync-address: 4075550150\nX-smssync-date: 1609459260000\nContent-Type: text/plain; charset=utf-8\n\nhello\n",
            &["5555550100"],
        )
        .unwrap();
        assert_eq!(received.conversation_type, IrConversationType::Group);
        assert_eq!(received.chat_key, sent.chat_key);
        assert_eq!(received.sender.unwrap().key(), "carol@example.org");
    }

    /// A received group MMS names the owner in `To` under the address on
    /// their own contact card. When that is no number or email address the
    /// owner gave, the owner would count as a participant, so the message is
    /// not filed as a group. `From` names who wrote it, so it goes to that
    /// person's one-to-one conversation, and neither the `X-smssync-address`
    /// peer nor the subject's name is taken for its sender.
    #[test]
    fn a_received_group_mms_that_does_not_name_the_owner_is_filed_under_its_from() {
        let msg = parse(
            "From: carol@example.org\nTo: <me@icloud.example>, <+14075550150@unknown.email>\nSubject: SMS with Alice\nX-smssync-type: 132\nX-smssync-address: 4075550150\nX-smssync-date: 1609459260000\nContent-Type: text/plain; charset=utf-8\n\nhello\n",
            &["5555550100"],
        )
        .unwrap();
        assert_eq!(msg.conversation_type, IrConversationType::Individual);
        assert_eq!(msg.chat_key, "carol@example.org");
        assert_eq!(
            msg.sender.map(Handle::into_key).as_deref(),
            Some("carol@example.org")
        );
        assert_eq!(msg.name_alias, None);
        assert!(msg.owner_not_named);
    }

    /// When a received group MMS names neither the owner nor, in `From`, its
    /// sender, it stays in the `X-smssync-address` conversation with no
    /// sender: the peer there is not known to have written it.
    #[test]
    fn a_received_group_mms_naming_neither_the_owner_nor_a_sender_has_no_sender() {
        let msg = parse(
            "From: \nTo: <me@icloud.example>, <+14075550150@unknown.email>\nSubject: SMS with Alice\nX-smssync-type: 132\nX-smssync-address: 4075550150\nX-smssync-date: 1609459260000\nContent-Type: text/plain; charset=utf-8\n\nhello\n",
            &["5555550100"],
        )
        .unwrap();
        assert_eq!(msg.conversation_type, IrConversationType::Individual);
        assert_eq!(msg.chat_key, "+14075550150");
        assert!(msg.sender.is_none(), "{:?}", msg.sender);
        assert_eq!(msg.name_alias, None);
        assert!(msg.owner_not_named);
    }

    /// A received MMS from one person names only the owner in `To`, so it
    /// stays one-to-one with the number in `X-smssync-address`.
    #[test]
    fn a_received_mms_from_one_person_stays_one_to_one() {
        let msg = parse(
            "From: \"Alice\" <alice@example.org>\nTo: <+15555550100@unknown.email>\nSubject: SMS with Alice\nX-smssync-type: 132\nX-smssync-address: 4075550150\nX-smssync-date: 1609459200000\nContent-Type: text/plain; charset=utf-8\n\nhi\n",
            &["5555550100"],
        )
        .unwrap();
        assert_eq!(msg.conversation_type, IrConversationType::Individual);
        assert_eq!(msg.chat_key, "+14075550150");
    }

    /// A sent message with one recipient in `To` stays one-to-one with the
    /// number in `X-smssync-address`, even when `To` gives that person's
    /// email address instead of the number.
    #[test]
    fn a_sent_message_to_one_recipient_stays_one_to_one() {
        let msg = parse(
            "From: me@example.com\nTo: \"Alice\" <alice@example.org>\nSubject: SMS with Alice\nX-smssync-type: 2\nX-smssync-address: 4075550150\nX-smssync-date: 1609459200000\nContent-Type: text/plain; charset=utf-8\n\nhi\n",
            &["5555550100"],
        )
        .unwrap();
        assert_eq!(msg.conversation_type, IrConversationType::Individual);
        assert_eq!(msg.chat_key, "+14075550150");
    }
}
