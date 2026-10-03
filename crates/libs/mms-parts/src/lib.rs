//! The parts of an MMS as the message text and its attachments.
//!
//! An MMS is a list of parts: text, pictures, a contact card, and a SMIL
//! part that lays them out. SMS Backup & Restore, SMS Backup+ and GO SMS Pro
//! each store those parts their own way, and each reader maps its parts into
//! [`Part`] and calls [`body_of`], so one rule decides what becomes text and
//! what becomes an attachment. The rules:
//!
//! 1. **The text is every `text/plain` part**, joined with a newline. Parts
//!    the SMIL names come first, in the SMIL's order, and the rest follow in
//!    the order they are written. A text part the SMIL does not name is still
//!    the sender's words, so it is kept. Nothing is sorted and nothing is
//!    removed for repeating another part: "ha" sent twice is "ha\nha".
//! 2. **Every other part with content is an attachment**, a text type such
//!    as `text/x-vcard` or `text/calendar` included. A contact card is a file
//!    the sender attached, not words of the message.
//! 3. **Attachments are told apart by position, not by name.** Two pictures
//!    both called `image.jpg`, or the same picture sent twice, are two
//!    attachments. They are ordered the way text is: SMIL order, then the
//!    order they are written.
//! 4. **A SMIL part is layout.** It orders the other parts and is neither
//!    text nor an attachment.
//! 5. **The SMIL names a part by any key it has**: its name, its
//!    Content-Location, its Content-ID (`cid:text_0` names the part with
//!    Content-ID `<text_0>`), or its file name, each also by the last segment
//!    of a path.
//! 6. **A part whose content cannot be read is dropped and reported** in
//!    [`Body::unreadable`], so the reader can count it.

use regex::Regex;
use std::sync::LazyLock;

/// What a reader could read from one part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content<'a> {
    /// The part holds nothing.
    None,
    /// Text the reader has already decoded.
    Text(String),
    /// The part's bytes as stored.
    Bytes(&'a [u8]),
    /// The part holds content the reader could not decode.
    Unreadable,
}

/// One MMS part as the reader found it.
#[derive(Debug, Clone)]
pub struct Part<'a> {
    /// The content type as written, parameters and all.
    pub content_type: &'a str,
    /// Every key the part goes by, as written: name, Content-Location,
    /// Content-ID and file name. Blank keys, `null` and `none` are ignored.
    pub keys: Vec<&'a str>,
    /// The part's content.
    pub content: Content<'a>,
}

/// The message text and attachments an MMS's parts make.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Body {
    /// The text parts joined with a newline (rule 1).
    pub text: String,
    /// The index in the parts of each attachment, in order (rules 2 and 3).
    pub attachments: Vec<usize>,
    /// The index in the parts of each part that was dropped because its
    /// content could not be read (rule 6).
    pub unreadable: Vec<usize>,
}

/// The media type, lower case, without parameters: `Text/Plain; charset=x`
/// gives `text/plain`.
fn media_type(content_type: &str) -> String {
    content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

/// True for a part whose content is message text (`text/plain`).
pub fn is_text(content_type: &str) -> bool {
    media_type(content_type) == "text/plain"
}

/// True for the SMIL part that lays the others out.
pub fn is_smil(content_type: &str) -> bool {
    media_type(content_type) == "application/smil"
}

/// The text and attachments the parts make. See the crate documentation for
/// the rules.
pub fn body_of(parts: &[Part<'_>]) -> Body {
    let rank = smil_rank(parts);
    let mut order: Vec<usize> = (0..parts.len()).collect();
    order.sort_by_key(|&i| rank[i]);
    let mut texts = Vec::new();
    let mut body = Body::default();
    for index in order {
        let part = &parts[index];
        if is_smil(part.content_type) {
            continue;
        }
        if is_text(part.content_type) {
            let text = match &part.content {
                Content::Text(text) => text.clone(),
                Content::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
                Content::Unreadable => {
                    body.unreadable.push(index);
                    continue;
                }
                Content::None => continue,
            };
            if !text.is_empty() {
                texts.push(text);
            }
            continue;
        }
        match &part.content {
            Content::Text(text) if !text.is_empty() => body.attachments.push(index),
            Content::Bytes(_) => body.attachments.push(index),
            Content::Unreadable => body.unreadable.push(index),
            Content::Text(_) | Content::None => {}
        }
    }
    body.unreadable.sort_unstable();
    body.text = texts.join("\n");
    body
}

/// Each `src` in a SMIL document that names a part to show.
static SMIL_SRC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)<(?:text|img|image|video|audio|ref)\b[^>]*?\bsrc\s*=\s*["']([^"']+)["']"#)
        .expect("valid regex")
});

/// Where each part falls in the message: the parts the SMIL names by the
/// position of their first mention, then the rest by their position in
/// `parts`. Each part is named once, by the first `src` that matches it.
fn smil_rank(parts: &[Part<'_>]) -> Vec<usize> {
    let mut rank: Vec<usize> = (0..parts.len()).map(|i| usize::MAX / 2 + i).collect();
    let Some(smil) = parts.iter().find(|p| is_smil(p.content_type)) else {
        return rank;
    };
    let document = match &smil.content {
        Content::Text(text) => text.clone(),
        Content::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        Content::None | Content::Unreadable => return rank,
    };
    let names: Vec<Vec<String>> = parts.iter().map(part_names).collect();
    let mut named = vec![false; parts.len()];
    for (position, src) in SMIL_SRC
        .captures_iter(&document)
        .filter_map(|c| c.get(1))
        .enumerate()
    {
        let src = src_key(src.as_str());
        let found = (0..parts.len())
            .find(|&i| !named[i] && !is_smil(parts[i].content_type) && names[i].contains(&src));
        if let Some(i) = found {
            named[i] = true;
            rank[i] = position;
        }
    }
    rank
}

/// The part a SMIL `src` names: the value less a `cid:` prefix.
fn src_key(src: &str) -> String {
    let src = src.trim();
    match src.get(..4) {
        Some(prefix) if prefix.eq_ignore_ascii_case("cid:") => src[4..].to_string(),
        _ => src.to_string(),
    }
}

/// Every name a SMIL `src` may use for the part: each key less the angle
/// brackets a Content-ID is written with, and the last segment of a path.
fn part_names(part: &Part<'_>) -> Vec<String> {
    let mut names = Vec::new();
    for key in &part.keys {
        let key = key.trim();
        let key = key
            .strip_prefix('<')
            .and_then(|k| k.strip_suffix('>'))
            .unwrap_or(key)
            .trim();
        if key.is_empty() || key.eq_ignore_ascii_case("null") || key.eq_ignore_ascii_case("none") {
            continue;
        }
        names.push(key.to_string());
        if let Some(base) = key
            .rsplit('/')
            .next()
            .filter(|b| !b.is_empty() && *b != key)
        {
            names.push(base.to_string());
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text<'a>(keys: &[&'a str], value: &str) -> Part<'a> {
        Part {
            content_type: "text/plain",
            keys: keys.to_vec(),
            content: Content::Text(value.into()),
        }
    }

    fn smil(document: &str) -> Part<'static> {
        Part {
            content_type: "application/smil",
            keys: vec![],
            content: Content::Text(document.into()),
        }
    }

    fn bytes<'a>(content_type: &'a str, keys: &[&'a str], data: &'a [u8]) -> Part<'a> {
        Part {
            content_type,
            keys: keys.to_vec(),
            content: Content::Bytes(data),
        }
    }

    #[test]
    fn without_smil_text_keeps_its_order_and_its_repeats() {
        let parts = [
            text(&[], "Tickets attached"),
            text(&[], "All three are for Friday"),
            text(&[], "ha"),
            text(&[], "ha"),
        ];
        assert_eq!(
            body_of(&parts).text,
            "Tickets attached\nAll three are for Friday\nha\nha"
        );
    }

    #[test]
    fn the_smil_orders_text_and_attachments_and_unnamed_parts_follow() {
        let parts = [
            smil(
                r#"<smil><body><par><img src="b.jpg"/><text src="cid:t1"/></par><par><text src="parts/a.txt"/></par></body></smil>"#,
            ),
            text(&["a.txt"], "unnamed"),
            text(&["parts/a.txt"], "second"),
            bytes("image/jpeg", &["a.jpg"], b"a"),
            text(&["<t1>"], "first"),
            bytes("image/jpeg", &["b.jpg"], b"b"),
        ];
        let body = body_of(&parts);
        assert_eq!(body.text, "first\nsecond\nunnamed");
        assert_eq!(body.attachments, vec![5, 3]);
    }

    #[test]
    fn a_part_is_named_once_so_two_parts_with_one_name_both_follow_the_smil() {
        let parts = [
            smil(r#"<smil><body><img src="image.jpg"/><img src="image.jpg"/></body></smil>"#),
            bytes("image/jpeg", &["image.jpg"], b"1"),
            bytes("image/jpeg", &["image.jpg"], b"2"),
        ];
        assert_eq!(body_of(&parts).attachments, vec![1, 2]);
    }

    #[test]
    fn a_text_type_with_content_that_is_not_plain_text_is_an_attachment() {
        let parts = [
            text(&[], "card"),
            bytes("text/x-vcard", &["sam.vcf"], b"BEGIN:VCARD"),
            Part {
                content_type: "text/calendar",
                keys: vec![],
                content: Content::Text("BEGIN:VCALENDAR".into()),
            },
            Part {
                content_type: "text/x-vcard",
                keys: vec![],
                content: Content::Text(String::new()),
            },
        ];
        let body = body_of(&parts);
        assert_eq!(body.text, "card");
        assert_eq!(body.attachments, vec![1, 2]);
    }

    #[test]
    fn plain_text_in_bytes_is_text_and_a_parameter_does_not_change_the_type() {
        let parts = [bytes("Text/Plain; charset=utf-8", &[], b"hi")];
        assert_eq!(body_of(&parts).text, "hi");
    }

    #[test]
    fn unreadable_parts_are_reported_and_the_smil_is_never_content() {
        let parts = [
            smil("<smil/>"),
            Part {
                content_type: "image/jpeg",
                keys: vec![],
                content: Content::Unreadable,
            },
            Part {
                content_type: "text/plain",
                keys: vec![],
                content: Content::Unreadable,
            },
        ];
        let body = body_of(&parts);
        assert_eq!(body.unreadable, vec![1, 2]);
        assert!(body.attachments.is_empty());
        assert_eq!(body.text, "");
    }
}
