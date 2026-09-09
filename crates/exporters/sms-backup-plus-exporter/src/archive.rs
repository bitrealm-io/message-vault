//! Parse archive EMLs: one email file that holds many texts in its body.
//!
//! Example subject: `SMS archive with Alice`.
//! Example body lines:
//! ```text
//! 2012-05-24 14:20:31 - Alice
//! Hello from Alice
//!
//! 2012-05-24 14:21:05 - Me
//! See you later
//! ```
//!
//! Each dated block becomes one [`ParsedMessage`]. Attachments are paired by
//! guesswork — see [`assign_archive_attachments`].

use crate::assets::extract_attachments;
use crate::flat_eml::{MailHeaders, extract_body_text, is_archive_eml};
use crate::types::{AttachmentBlob, ParsedMessage};
use anyhow::{Context, Result};
use phone::sanitize_number;
use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::path::Path;
use std::sync::LazyLock;

/// `SMS archive <name>` subject matcher.
static ARCHIVE_SUBJECT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^SMS archive (.+)$").expect("arch subj"));
/// `YYYY-MM-DD HH:MM:SS - <sender>` line matcher for archive bodies.
static MESSAGE_HEADER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}) - (.+)$").expect("hdr"));
/// `YYYY-MM-DD` matcher.
static DATE_ONLY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d{4}-\d{2}-\d{2}$").expect("date"));
/// A trailing `(YYYY - YYYY)` year range on an archive contact name.
static YEAR_RANGE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*\(\d{4}\s*[-–—]\s*\d{4}\)\s*$").expect("year range"));

/// Clean contact name from an archive subject capture.
///
/// Subjects are often `SMS archive with Alice` or `SMS archive with Alice (2011-2013)`.
/// Strip a leading `with` and a trailing `(year-year)` so contacts lookup can match.
fn clean_archive_contact_name(raw: &str) -> String {
    let mut name = raw.trim();
    // `name[..5]` would panic when byte 5 lands mid-character in a multi-byte
    // UTF-8 name (e.g. `SMS archive 中文名`). "with " is 5 ASCII bytes, so a
    // match at a char boundary is always a real prefix.
    if name.len() >= 5 && name.is_char_boundary(5) && name[..5].eq_ignore_ascii_case("with ") {
        name = name[5..].trim();
    }
    YEAR_RANGE_RE.replace(name, "").trim().to_string()
}

/// The address inside a `From:` header's angle brackets, else the header text.
fn phone_from_from_header(from_hdr: &str) -> String {
    // parseaddr-ish: extract email local-part or display
    if let Some(start) = from_hdr.find('<')
        && let Some(end) = from_hdr[start..].find('>')
    {
        let addr = &from_hdr[start + 1..start + end];
        let local = addr.split('@').next().unwrap_or(addr);
        if let Some(digits) = sanitize_number(local) {
            return digits;
        }
    }
    sanitize_number(from_hdr).unwrap_or_default()
}

/// Unix seconds from an archive line's timestamp, trying each date format the app has used.
fn parse_archive_timestamp(date_str: &str) -> Option<f64> {
    use chrono::{Local, LocalResult, TimeZone};
    for fmt in [
        "%Y-%m-%d %H:%M:%S",
        "%Y/%m/%d %H:%M:%S",
        "%m/%d/%Y %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S",
    ] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(date_str, fmt) {
            // Archive body times are local wall-clock, not UTC. DST transitions
            // make some wall-clock times ambiguous (fall-back) or nonexistent
            // (spring-forward); keep the earliest interpretation instead of
            // silently dropping the message.
            return match Local.from_local_datetime(&naive) {
                LocalResult::Single(dt) => Some(dt.timestamp() as f64),
                LocalResult::Ambiguous(earliest, _) => Some(earliest.timestamp() as f64),
                LocalResult::None => None,
            };
        }
    }
    None
}

/// Guess which MIME attachments belong to which archive body lines.
///
/// The archive file lists attachments in order, but does not say which JPEG
/// belongs to which timestamped message. Perfect matching is impossible, so
/// the pairing is a guess:
///
/// 1. **Empty-body first** — if a message has no text (common for photo-only
///    MMS), give it the next unused attachment.
/// 2. **First come, first served** — walk remaining messages in order; each
///    that still has no attachment gets the next unused one.
/// 3. **Leftovers on the last message** — if attachments remain, pile them on
///    the final message rather than drop the files.
///
/// Tiny example: three messages (text, empty, text) and two images → the empty
/// one gets image 1; the first text still without an attachment gets image 2.
fn assign_archive_attachments(messages: &mut [ParsedMessage], att_queue: Vec<AttachmentBlob>) {
    if messages.is_empty() || att_queue.is_empty() {
        return;
    }
    let mut att_queue: VecDeque<AttachmentBlob> = att_queue.into();

    // Pass 1: empty-body messages first.
    for msg in messages.iter_mut() {
        if att_queue.is_empty() {
            break;
        }
        if msg.text.trim().is_empty()
            && let Some(att) = att_queue.pop_front()
        {
            msg.attachments.push(att);
        }
    }

    // Pass 2: first come, first served for messages that still lack an attachment.
    for msg in messages.iter_mut() {
        if att_queue.is_empty() {
            break;
        }
        if msg.attachments.is_empty()
            && let Some(att) = att_queue.pop_front()
        {
            msg.attachments.push(att);
        }
    }

    // Pass 3: leftovers → last message.
    if !att_queue.is_empty()
        && let Some(last) = messages.last_mut()
    {
        last.attachments.extend(att_queue);
    }
}

/// Parse a consolidated `SMS archive …` EML into multiple messages.
///
/// Returns the messages and how many dated blocks were dropped for an
/// unreadable timestamp.
pub(crate) fn parse_archive_eml_mail(
    path: &Path,
    mail: &mailparse::ParsedMail<'_>,
    headers: &MailHeaders,
) -> Result<(Vec<ParsedMessage>, u64)> {
    if !is_archive_eml(headers) {
        return Ok((Vec::new(), 0));
    }
    let caps = ARCHIVE_SUBJECT_RE
        .captures(headers.subject.trim())
        .context("archive subject")?;
    let peer = ArchivePeer::new(
        clean_archive_contact_name(&caps[1]),
        phone_from_from_header(&headers.from),
    );

    let file_key = hex::encode(Sha256::digest(path.to_string_lossy().as_bytes()));
    let attachments = extract_attachments(mail, 0.0, Some(&file_key[..12.min(file_key.len())]));

    let mut reader = ArchiveReader::new(peer);
    for line in extract_body_text(mail).lines() {
        reader.feed(line);
    }
    let (mut messages, skipped_invalid_date) = reader.finish();
    assign_archive_attachments(&mut messages, attachments);
    // Drop messages that ended up with neither text nor attachments.
    messages.retain(|m| !m.text.trim().is_empty() || !m.attachments.is_empty());
    Ok((messages, skipped_invalid_date))
}

/// The other party of an archive: the name from the subject and the number
/// from the `From:` header, or from the name when the name is itself a number.
struct ArchivePeer {
    name: String,
    /// Empty when no number is known; such messages are written under the `unknown` chat stem.
    number: String,
}

impl ArchivePeer {
    fn new(name: String, phone: String) -> Self {
        let number = if !phone.is_empty() {
            phone
        } else if name.starts_with('+') || name.chars().all(|c| c.is_ascii_digit()) {
            sanitize_number(&name).unwrap_or_default()
        } else {
            String::new()
        };
        Self { name, number }
    }

    /// A message in this peer's conversation holding one dated block's text.
    fn message(&self, timestamp_secs: f64, is_from_me: bool, text: String) -> ParsedMessage {
        let number = self.number.clone();
        let name = Some(self.name.clone());
        ParsedMessage {
            chat_key: number.clone(),
            conversation_type: "individual".into(),
            group_title: None,
            participant_digits: if number.is_empty() {
                vec![]
            } else {
                vec![(number.clone(), name.clone())]
            },
            timestamp_secs,
            is_from_me,
            sender_digits: (!is_from_me && !number.is_empty()).then(|| number.clone()),
            text,
            attachments: Vec::new(),
            name_alias: name,
            smssync_id: None,
            source_kind: "archive".into(),
            android_type: String::new(),
            eml_path: String::new(),
        }
    }
}

/// The dated block being read: its header line's date and sender, then its body lines.
struct OpenMessage {
    date: String,
    sender: String,
    lines: Vec<String>,
}

/// Line-by-line reader for an archive body. A `date - sender` line opens a
/// message and the lines after it are its text until the next header. The
/// export's own name and a bare date may precede the first header; both are
/// skipped.
struct ArchiveReader {
    peer: ArchivePeer,
    messages: Vec<ParsedMessage>,
    skipped_invalid_date: u64,
    open: Option<OpenMessage>,
    /// Whether the preamble (the contact name, a bare date) has been passed.
    past_preamble: bool,
}

impl ArchiveReader {
    fn new(peer: ArchivePeer) -> Self {
        Self {
            peer,
            messages: Vec::new(),
            skipped_invalid_date: 0,
            open: None,
            past_preamble: false,
        }
    }

    /// Read one line of the body.
    fn feed(&mut self, line: &str) {
        let stripped = line.trim();
        if stripped.is_empty() {
            if let Some(open) = &mut self.open {
                open.lines.push(String::new());
            }
            return;
        }
        if !self.past_preamble {
            if stripped.eq_ignore_ascii_case(&self.peer.name) {
                self.past_preamble = true;
                return;
            }
            if DATE_ONLY_RE.is_match(stripped) && self.open.is_none() {
                return;
            }
            self.past_preamble = true;
        }
        if let Some(caps) = MESSAGE_HEADER_RE.captures(stripped) {
            self.flush();
            self.open = Some(OpenMessage {
                date: caps[1].to_string(),
                sender: caps[2].trim().to_string(),
                lines: Vec::new(),
            });
            return;
        }
        // Anything before the first header is preamble. Once a message body
        // is open every line is content, a bare date included: a text that is
        // just a date must not lose its text.
        if let Some(open) = &mut self.open {
            open.lines.push(line.trim_end().to_string());
        }
    }

    /// Close the open block as a message. A block whose timestamp does not
    /// parse is counted and dropped. An empty text is kept, because
    /// [`assign_archive_attachments`] may still give the message media.
    fn flush(&mut self) {
        let Some(open) = self.open.take() else {
            return;
        };
        let Some(timestamp_secs) = parse_archive_timestamp(&open.date) else {
            self.skipped_invalid_date += 1;
            return;
        };
        let text = open
            .lines
            .iter()
            .map(|l| l.trim_end())
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();
        let is_from_me = open.sender.trim().eq_ignore_ascii_case("me");
        self.messages
            .push(self.peer.message(timestamp_secs, is_from_me, text));
    }

    /// Close the last block and return the messages with the skipped count.
    fn finish(mut self) -> (Vec<ParsedMessage>, u64) {
        self.flush();
        (self.messages, self.skipped_invalid_date)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_archive_thread() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("archive.eml");
        std::fs::write(
            &path,
            b"From: <4075551234@sms-backup-plus.local>\r\n\
To: me@example.com\r\n\
Subject: SMS archive with Alice (2011-2013)\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Alice\r\n\
2020-01-01 12:00:00 - Me\r\n\
Check this\r\n\
2020-01-01 12:01:00 - Alice\r\n\
Thanks\r\n",
        )
        .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let mail = mailparse::parse_mail(&bytes).unwrap();
        let headers = MailHeaders::from_mail(&mail);
        let (msgs, _) = parse_archive_eml_mail(&path, &mail, &headers).unwrap();
        assert_eq!(msgs.len(), 2);
        assert!(msgs[0].is_from_me);
        assert_eq!(msgs[0].text, "Check this");
        assert!(!msgs[1].is_from_me);
        assert_eq!(msgs[1].text, "Thanks");
        assert_eq!(msgs[0].name_alias.as_deref(), Some("Alice"));
    }

    #[test]
    fn clean_archive_contact_name_strips_with_and_years() {
        assert_eq!(clean_archive_contact_name("with Alice"), "Alice");
        assert_eq!(
            clean_archive_contact_name("with Alice (2011-2013)"),
            "Alice"
        );
        assert_eq!(clean_archive_contact_name("Alice"), "Alice");
        // Non-ASCII names: byte 5 can land mid-character (H5). Must not panic
        // and must not strip a `with` that is not actually there.
        assert_eq!(clean_archive_contact_name("with 中文名"), "中文名");
        assert_eq!(clean_archive_contact_name("SMS 中文名"), "SMS 中文名");
    }

    #[test]
    fn date_only_line_inside_message_body_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("archive.eml");
        std::fs::write(
            &path,
            b"From: <4075551234@sms-backup-plus.local>\r\n\
To: me@example.com\r\n\
Subject: SMS archive Alice\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Alice\r\n\
2020-01-01 12:00:00 - Me\r\n\
2020-01-02\r\n\
2020-01-01 12:01:00 - Alice\r\n\
Thanks\r\n",
        )
        .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let mail = mailparse::parse_mail(&bytes).unwrap();
        let headers = MailHeaders::from_mail(&mail);
        let (msgs, _) = parse_archive_eml_mail(&path, &mail, &headers).unwrap();
        assert_eq!(msgs.len(), 2);
        // A text that is just a date is content, not a separator.
        assert_eq!(msgs[0].text, "2020-01-02");
        assert_eq!(msgs[1].text, "Thanks");
    }

    #[test]
    fn archive_named_peer_without_a_phone_keys_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("archive.eml");
        std::fs::write(
            &path,
            b"From: someone@example.com\r\n\
To: me@example.com\r\n\
Subject: SMS archive Mystery\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Mystery\r\n\
2020-01-01 12:00:00 - Me\r\n\
Hi there\r\n",
        )
        .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let mail = mailparse::parse_mail(&bytes).unwrap();
        let headers = MailHeaders::from_mail(&mail);
        let (msgs, _) = parse_archive_eml_mail(&path, &mail, &headers).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].chat_key, "");
        // The archive names the peer and records no address, so the chat is
        // keyed by the name rather than merged into a shared `unknown` chat.
        assert_eq!(crate::identity::chat_id_for(&msgs[0]), "Mystery");
        assert_eq!(msgs[0].text, "Hi there");
    }

    #[test]
    fn skips_unparseable_archive_dates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("archive.eml");
        std::fs::write(
            &path,
            b"From: <4075551234@sms-backup-plus.local>\r\n\
To: me@example.com\r\n\
Subject: SMS archive Alice\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Alice\r\n\
2020-13-01 12:00:00 - Me\r\n\
Bad stamp\r\n\
2020-01-01 12:00:00 - Alice\r\n\
Thanks\r\n",
        )
        .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let mail = mailparse::parse_mail(&bytes).unwrap();
        let headers = MailHeaders::from_mail(&mail);
        let (msgs, skipped) = parse_archive_eml_mail(&path, &mail, &headers).unwrap();
        assert_eq!(skipped, 1);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].text, "Thanks");
        assert!(msgs[0].timestamp_secs > 1.0);
    }

    #[test]
    fn empty_body_message_prefers_attachment() {
        let mut messages = vec![
            ParsedMessage {
                chat_key: "4075551234".into(),
                conversation_type: "individual".into(),
                timestamp_secs: 1.0,
                text: "hello".into(),
                ..Default::default()
            },
            ParsedMessage {
                chat_key: "4075551234".into(),
                conversation_type: "individual".into(),
                timestamp_secs: 2.0,
                text: "".into(),
                ..Default::default()
            },
        ];
        let att = AttachmentBlob {
            filename: "a.jpg".into(),
            original_name: Some("a.jpg".into()),
            mime_type: Some("image/jpeg".into()),
            digest_hex: "abc".into(),
            data: vec![1, 2, 3],
        };
        assign_archive_attachments(&mut messages, vec![att]);
        assert!(messages[0].attachments.is_empty());
        assert_eq!(messages[1].attachments.len(), 1);
    }

    /// Parse one committed fixture and hand back its messages.
    fn parse_fixture(name: &str) -> Vec<ParsedMessage> {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        let bytes = std::fs::read(&path).unwrap();
        let mail = mailparse::parse_mail(&bytes).unwrap();
        let headers = MailHeaders::from_mail(&mail);
        parse_archive_eml_mail(&path, &mail, &headers).unwrap().0
    }

    #[test]
    fn an_archive_with_only_an_html_part_is_read() {
        // Before the HTML part was read, this whole conversation was dropped
        // and nothing said so.
        let msgs = parse_fixture("archive_html_only.eml");
        assert_eq!(msgs.len(), 2);
        assert!(msgs[0].is_from_me);
        // A stray `<` is message text, not the start of a tag.
        assert_eq!(msgs[0].text, "i love it <3 and it was under <$120 too");
        assert!(!msgs[1].is_from_me);
        assert_eq!(msgs[1].text, "Tom & Jerry were \"on\"");
        assert_eq!(msgs[0].name_alias.as_deref(), Some("Bea"));
    }

    #[test]
    fn the_html_part_wins_over_a_hard_wrapped_plain_text_part() {
        // Both parts carry the same message. The plain-text copy has been
        // wrapped by the sending mail client mid-sentence; the HTML has not,
        // and it is the one worth keeping.
        let msgs = parse_fixture("archive_both_parts.eml");
        assert_eq!(msgs.len(), 1);
        assert_eq!(
            msgs[0].text,
            "Not bad, kinda drained. I'll be more worried when he gets out. \
             Feel kinda guilty it got to this, but there's only so much a friend can do"
        );
    }
}
