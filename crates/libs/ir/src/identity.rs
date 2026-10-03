//! One message identity, made in one place.
//!
//! A message's `guid` is what the server dedupes an import on, and its
//! content key is what the server matches one message across sources on.
//! Both come from [`MessageIdentity::key`]: the `guid` at milliseconds
//! ([`MessageGuid::new`]) and the content key at whole seconds, because
//! iMazing, OpenExtract and GO SMS Pro's PDU files record whole seconds only.
//!
//! [`one_copy_per_message`] is the one dedupe step every exporter runs before
//! the ids are made. Two copies the backup cannot tell apart are one message,
//! so the id carries no counter of occurrences: a counter would depend on the
//! order the copies are read in.

use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// How finely a source recorded a message's time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TimePrecision {
    /// Whole seconds: the milliseconds are zero because the source has none.
    Seconds,
    /// Milliseconds, as the phone stored them.
    Milliseconds,
}

/// What a message's identity is made from.
///
/// The chat is the caller's: an exporter passes the conversation's chat id,
/// and the server passes the identity it matches conversations across
/// sources on. The sender counts only for a message someone else sent, since
/// every outgoing message has the same sender.
#[derive(Debug, Clone, Copy)]
pub struct MessageIdentity<'a> {
    /// The conversation's identity, as the caller defines it.
    pub chat: &'a str,
    /// Whether the owner sent the message.
    pub is_from_me: bool,
    /// The sender's handle; ignored for an outgoing message.
    pub sender: Option<&'a str>,
    /// The UTC instant, in milliseconds since 1970.
    pub timestamp_unix_ms: i64,
    /// The body. Runs of whitespace count as one space.
    pub text: &'a str,
    /// SHA-256 digests of the attachments, in any order. Blank ones are left out.
    pub attachment_digests: &'a [String],
    /// The source's own id for the message, where it has one (WhatsApp's `key_id`).
    pub vendor_key: Option<&'a str>,
}

impl MessageIdentity<'_> {
    /// SHA-256 over the identity, in lowercase hex, with the time taken at `precision`.
    pub fn key(&self, precision: TimePrecision) -> String {
        let time = match precision {
            TimePrecision::Seconds => self.timestamp_unix_ms.div_euclid(1000),
            TimePrecision::Milliseconds => self.timestamp_unix_ms,
        };
        let mut digests: Vec<&str> = self
            .attachment_digests
            .iter()
            .map(|d| d.trim())
            .filter(|d| !d.is_empty())
            .collect();
        digests.sort_unstable();
        digests.dedup();

        let mut hasher = Sha256::new();
        hasher.update(self.chat.as_bytes());
        hasher.update(b"|");
        hasher.update(if self.is_from_me { b"1" } else { b"0" });
        hasher.update(b"|");
        hasher.update(sender_key(self.is_from_me, self.sender).as_bytes());
        hasher.update(b"|");
        hasher.update(time.to_string().as_bytes());
        hasher.update(b"|");
        hasher.update(collapse_whitespace(self.text).as_bytes());
        for digest in digests {
            hasher.update(b"|");
            hasher.update(digest.as_bytes());
        }
        if let Some(vendor) = self.vendor_key.map(str::trim).filter(|v| !v.is_empty()) {
            hasher.update(b"|vendor:");
            hasher.update(vendor.as_bytes());
        }
        hex::encode(hasher.finalize())
    }
}

/// A message's `guid`: its [`MessageIdentity`] at milliseconds.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MessageGuid(String);

impl MessageGuid {
    /// The `guid` of the message `identity` describes.
    pub fn new(identity: &MessageIdentity<'_>) -> Self {
        Self(identity.key(TimePrecision::Milliseconds))
    }

    /// The `guid` as 64 lowercase hex digits.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The `guid` as an owned string, for [`crate::IrMessage::guid`].
    pub fn into_string(self) -> String {
        self.0
    }
}

impl std::fmt::Display for MessageGuid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// `text` with every run of whitespace turned into one space and the ends trimmed,
/// so two copies that differ only in line breaks or spacing have one identity.
pub fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The sender as the identity hashes it: empty for an outgoing message.
fn sender_key(is_from_me: bool, sender: Option<&str>) -> &str {
    if is_from_me {
        ""
    } else {
        sender.map_or("", str::trim)
    }
}

/// One copy of a message as a source recorded it, for [`one_copy_per_message`].
///
/// Every copy passed in one call belongs to one conversation.
#[derive(Debug, Clone, Copy)]
pub struct MessageCopy<'a> {
    /// Whether the owner sent the message.
    pub is_from_me: bool,
    /// The sender's handle; ignored for an outgoing message.
    pub sender: Option<&'a str>,
    /// The UTC instant, in milliseconds since 1970.
    pub timestamp_unix_ms: i64,
    /// How finely the source recorded [`Self::timestamp_unix_ms`].
    pub precision: TimePrecision,
    /// The body.
    pub text: &'a str,
    /// What tells the attachments apart, normally their SHA-256 digests.
    pub attachment_digests: &'a [String],
    /// The source's own id for the message, where it has one.
    pub vendor_key: Option<&'a str>,
}

/// Which copies of one conversation to keep: one per message.
///
/// Two copies are one message when the direction, the sender, the text, the
/// vendor key and the attachments agree and the times are compatible. Times
/// are compatible when they are equal to the millisecond, or when one of them
/// has whole seconds only and falls in the same second as the other.
/// Attachments agree when the digests are the same, or when one copy has
/// none: GO SMS Pro writes an MMS once to its XML backup without the
/// attachments and once as a PDU file with them.
///
/// Of the copies of one message, the one with attachments is kept, then the
/// one with milliseconds. A kept copy with whole seconds takes the time of a
/// millisecond copy of the same message, so the result does not depend on
/// which copy was read first. Two millisecond copies with different times
/// are two messages.
///
/// Returns one entry per copy, in order: `Some(time)` for a copy that is
/// kept, with the time in milliseconds it keeps, and `None` for a copy that
/// repeats a kept one.
pub fn one_copy_per_message(copies: &[MessageCopy<'_>]) -> Vec<Option<i64>> {
    struct Kept {
        index: usize,
        timestamp_unix_ms: i64,
        exact: bool,
        digests: Vec<String>,
    }

    let digests: Vec<Vec<String>> = copies
        .iter()
        .map(|c| {
            let mut d: Vec<String> = c
                .attachment_digests
                .iter()
                .map(|d| d.trim().to_string())
                .filter(|d| !d.is_empty())
                .collect();
            d.sort_unstable();
            d.dedup();
            d
        })
        .collect();

    // Copies that can be one message share a direction, a sender, a text, a
    // vendor key and a second.
    let mut buckets: HashMap<(bool, &str, String, &str, i64), Vec<usize>> = HashMap::new();
    for (i, c) in copies.iter().enumerate() {
        let key = (
            c.is_from_me,
            sender_key(c.is_from_me, c.sender),
            collapse_whitespace(c.text),
            c.vendor_key.map_or("", str::trim),
            c.timestamp_unix_ms.div_euclid(1000),
        );
        buckets.entry(key).or_default().push(i);
    }

    let mut fate = vec![None; copies.len()];
    for mut members in buckets.into_values() {
        // Richest copy first: attachments, then milliseconds, then the
        // earliest time. Ties fall back to the digests and the input order,
        // so the pick is the same however the copies were read.
        members.sort_by(|&a, &b| {
            let (ca, cb) = (&copies[a], &copies[b]);
            digests[a]
                .is_empty()
                .cmp(&digests[b].is_empty())
                .then(cb.precision.cmp(&ca.precision))
                .then(ca.timestamp_unix_ms.cmp(&cb.timestamp_unix_ms))
                .then(digests[a].cmp(&digests[b]))
                .then(a.cmp(&b))
        });
        let mut kept: Vec<Kept> = Vec::new();
        for i in members {
            let c = &copies[i];
            let exact = c.precision == TimePrecision::Milliseconds;
            let same = kept.iter_mut().find(|k| {
                let times = !(k.exact && exact) || k.timestamp_unix_ms == c.timestamp_unix_ms;
                let attachments = digests[i].is_empty() || k.digests == digests[i];
                times && attachments
            });
            match same {
                Some(k) => {
                    if !k.exact && exact {
                        k.timestamp_unix_ms = c.timestamp_unix_ms;
                        k.exact = true;
                    }
                }
                None => kept.push(Kept {
                    index: i,
                    timestamp_unix_ms: c.timestamp_unix_ms,
                    exact,
                    digests: digests[i].clone(),
                }),
            }
        }
        for k in kept {
            fate[k.index] = Some(k.timestamp_unix_ms);
        }
    }
    fate
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity<'a>(text: &'a str, ms: i64, digests: &'a [String]) -> MessageIdentity<'a> {
        MessageIdentity {
            chat: "+15555550101",
            is_from_me: false,
            sender: Some("+15555550101"),
            timestamp_unix_ms: ms,
            text,
            attachment_digests: digests,
            vendor_key: None,
        }
    }

    /// Pinned to values computed outside Rust (Python's `hashlib`), because a
    /// change to the recipe gives every message already imported a new id.
    #[test]
    fn guid_known_answers() {
        assert_eq!(
            MessageGuid::new(&identity("hello", 1_609_459_200_123, &[])).as_str(),
            "17e5020f5b4f3581787af162b88292f9267f0d9b384cfa3e9a976373d0c2ae06"
        );
        let digests = ["b".repeat(64), "a".repeat(64), String::new()];
        let outgoing = MessageIdentity {
            is_from_me: true,
            ..identity(" hello \n world ", 1_609_459_200_000, &digests)
        };
        assert_eq!(
            MessageGuid::new(&outgoing).as_str(),
            "9f32e7ffc83bea1d600d9d9c236d0492d07cde0cb40ab331399933d20f8c7bf5",
            "no sender when outgoing, collapsed text, sorted digests, blanks left out"
        );
        let whatsapp = MessageIdentity {
            vendor_key: Some("3EB0ABC"),
            ..identity("hi", 1_609_459_200_123, &[])
        };
        assert_eq!(
            MessageGuid::new(&whatsapp).as_str(),
            "d3dc7b0db0c35f6e30abd75f5d18b505a78eec3c9fcbc61f0ced9771acad5876"
        );
    }

    #[test]
    fn the_guid_keeps_the_milliseconds_and_the_content_key_drops_them() {
        let first = identity("?", 1_609_459_200_100, &[]);
        let second = identity("?", 1_609_459_200_400, &[]);
        assert_ne!(MessageGuid::new(&first), MessageGuid::new(&second));
        assert_eq!(
            first.key(TimePrecision::Seconds),
            second.key(TimePrecision::Seconds)
        );
    }

    #[test]
    fn a_time_before_1970_falls_in_the_second_it_belongs_to() {
        let a = identity("hi", -1, &[]);
        let b = identity("hi", -1000, &[]);
        assert_eq!(a.key(TimePrecision::Seconds), b.key(TimePrecision::Seconds));
    }

    #[test]
    fn the_sender_of_an_outgoing_message_does_not_count() {
        let mine = |sender| MessageIdentity {
            is_from_me: true,
            sender,
            ..identity("hi", 0, &[])
        };
        assert_eq!(
            MessageGuid::new(&mine(Some("+15555550100"))),
            MessageGuid::new(&mine(None))
        );
    }

    fn copy<'a>(
        sender: &'a str,
        ms: i64,
        precision: TimePrecision,
        digests: &'a [String],
    ) -> MessageCopy<'a> {
        MessageCopy {
            is_from_me: false,
            sender: Some(sender),
            timestamp_unix_ms: ms,
            precision,
            text: "lol",
            attachment_digests: digests,
            vendor_key: None,
        }
    }

    const MS: TimePrecision = TimePrecision::Milliseconds;
    const SECS: TimePrecision = TimePrecision::Seconds;

    #[test]
    fn two_senders_in_one_second_are_two_messages() {
        let copies = [
            copy("+15555550122", 1_609_459_200_000, SECS, &[]),
            copy("+15555550133", 1_609_459_200_000, SECS, &[]),
        ];
        assert_eq!(
            one_copy_per_message(&copies),
            [Some(1_609_459_200_000), Some(1_609_459_200_000)]
        );
    }

    #[test]
    fn copies_the_backup_cannot_tell_apart_are_one_message() {
        let copies = [
            copy("+15555550122", 1_609_459_200_300, MS, &[]),
            copy("+15555550122", 1_609_459_200_300, MS, &[]),
        ];
        assert_eq!(
            one_copy_per_message(&copies),
            [Some(1_609_459_200_300), None]
        );
    }

    #[test]
    fn two_millisecond_copies_at_different_times_are_two_messages() {
        let copies = [
            copy("+15555550122", 1_609_459_200_100, MS, &[]),
            copy("+15555550122", 1_609_459_200_400, MS, &[]),
        ];
        assert_eq!(
            one_copy_per_message(&copies),
            [Some(1_609_459_200_100), Some(1_609_459_200_400)]
        );
    }

    #[test]
    fn a_whole_second_copy_yields_to_a_millisecond_copy_whichever_comes_first() {
        let whole = copy("+15555550122", 1_609_459_200_000, SECS, &[]);
        let exact = copy("+15555550122", 1_609_459_200_876, MS, &[]);
        assert_eq!(
            one_copy_per_message(&[whole, exact]),
            [None, Some(1_609_459_200_876)]
        );
        assert_eq!(
            one_copy_per_message(&[exact, whole]),
            [Some(1_609_459_200_876), None]
        );
    }

    #[test]
    fn the_copy_with_attachments_is_kept_and_takes_the_milliseconds() {
        let photo = ["a".repeat(64)];
        let pdu = copy("+15555550122", 1_609_459_200_000, SECS, &photo);
        let xml = copy("+15555550122", 1_609_459_200_250, MS, &[]);
        assert_eq!(
            one_copy_per_message(&[xml, pdu]),
            [None, Some(1_609_459_200_250)]
        );
        assert_eq!(
            one_copy_per_message(&[pdu, xml]),
            [Some(1_609_459_200_250), None]
        );
    }

    #[test]
    fn copies_with_different_attachments_are_two_messages() {
        let a = ["a".repeat(64)];
        let b = ["b".repeat(64)];
        let copies = [
            copy("+15555550122", 1_609_459_200_000, SECS, &a),
            copy("+15555550122", 1_609_459_200_000, SECS, &b),
        ];
        assert_eq!(
            one_copy_per_message(&copies),
            [Some(1_609_459_200_000), Some(1_609_459_200_000)]
        );
    }

    #[test]
    fn copies_with_different_vendor_keys_are_two_messages() {
        let mut a = copy("+15555550122", 1_609_459_200_000, MS, &[]);
        let mut b = a;
        a.vendor_key = Some("A1");
        b.vendor_key = Some("B2");
        assert_eq!(
            one_copy_per_message(&[a, b]),
            [Some(1_609_459_200_000), Some(1_609_459_200_000)]
        );
    }

    #[test]
    fn copies_in_different_seconds_are_two_messages() {
        let copies = [
            copy("+15555550122", 1_609_459_200_000, SECS, &[]),
            copy("+15555550122", 1_609_459_201_000, SECS, &[]),
        ];
        assert_eq!(
            one_copy_per_message(&copies),
            [Some(1_609_459_200_000), Some(1_609_459_201_000)]
        );
    }
}
