//! Turn the HTML half of an archive mail back into transcript lines.
//!
//! An archive mail carries the same conversation twice: as a `text/plain`
//! transcript and as an HTML rendering, one bubble per message. Two things
//! make the HTML the better source. Many archives carry no plain-text part at
//! all, and where both exist the plain-text copy has been hard-wrapped by the
//! sending mail client, so a sentence arrives broken across lines while the
//! HTML keeps it whole.
//!
//! The job here is only to get back to the shape
//! [`crate::archive::ArchiveReader`] already reads: a `YYYY-MM-DD HH:MM:SS -
//! sender` line followed by that message's text. Every tag becomes a line
//! break, which puts each `<div>` on its own line, and that is enough.

/// Transcript text from an archive mail's HTML part.
pub(crate) fn transcript_from_html(html: &str) -> String {
    let stripped = strip_tags(html);
    let decoded = html_escape::decode_html_entities(&stripped);
    collapse_blank_lines(&decoded)
}

/// True when the text after a `<` starts a tag, a comment, or a declaration.
///
/// This is the HTML5 rule, and the whole reason the stripper is hand-written:
/// the archive generator does not escape message text, so a text reading `<3`
/// or `<$120` reaches here as a literal `<`. A pattern that treated every `<`
/// as a tag would swallow it along with everything up to the next `>`, losing
/// real words out of the middle of a message.
fn opens_a_tag(after: &str) -> bool {
    matches!(
        after.as_bytes().first(),
        Some(c) if c.is_ascii_alphabetic() || matches!(c, b'/' | b'!' | b'?')
    )
}

/// Replace every tag with a line break, leaving a stray `<` as text.
fn strip_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(open) = rest.find('<') {
        // `<` is ASCII, so both slices land on character boundaries.
        let after = &rest[open + 1..];
        if !opens_a_tag(after) {
            out.push_str(&rest[..=open]);
            rest = after;
            continue;
        }
        out.push_str(&rest[..open]);
        out.push('\n');
        match after.find('>') {
            Some(close) => rest = &after[close + 1..],
            // A tag left unclosed at the end of the body: there is no text
            // after it to keep.
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Collapse runs of blank lines to a single blank line.
///
/// Every tag became a line break, so the paragraph break that the plain-text
/// part writes as one blank line arrives here as several. Collapsing them is
/// what makes both parts of the same mail produce the same message text.
fn collapse_blank_lines(input: &str) -> String {
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(normalized.len());
    let mut blank_seen = false;
    for line in normalized.split('\n') {
        if line.trim().is_empty() {
            blank_seen = true;
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
            if blank_seen {
                out.push('\n');
            }
        }
        blank_seen = false;
        out.push_str(line);
    }
    out
}

#[cfg(test)]
mod tests;
