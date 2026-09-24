//! VCF 3.0 parser (names, phones, email, categories) for contact books and vault ingest.

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// One parsed vCard.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VcfCard {
    /// Unescaped `FN` value.
    pub fn_raw: String,
    /// `N` family (last) name component.
    pub n_family: String,
    /// `N` given (first) name component.
    pub n_given: String,
    /// `N` middle name component.
    pub n_middle: String,
    /// Deduplicated raw `TEL` values.
    pub phones: Vec<String>,
    /// First `EMAIL` value, if any.
    pub email: Option<String>,
    /// Values from `CATEGORIES` (and repeated CATEGORIES lines), already unescaped.
    pub categories: Vec<String>,
}

/// Parse a VCF file into cards (unfolded lines).
///
/// # Errors
///
/// Returns an error when the file cannot be read.
pub fn parse_vcf(path: &Path) -> Result<Vec<VcfCard>> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read VCF {}", path.display()))?;
    Ok(parse_vcf_str(&text))
}

/// Parse VCF text into cards (unfolded lines).
fn parse_vcf_str(text: &str) -> Vec<VcfCard> {
    let lines = unfold_lines(text);
    let mut cards = Vec::new();
    let mut current: Option<VcfCard> = None;

    for line in lines {
        if line.eq_ignore_ascii_case("BEGIN:VCARD") {
            current = Some(VcfCard::default());
            continue;
        }
        if line.eq_ignore_ascii_case("END:VCARD") {
            if let Some(card) = current.take() {
                cards.push(card);
            }
            continue;
        }
        let Some(card) = current.as_mut() else {
            continue;
        };
        apply_line(card, &line);
    }

    cards
}

/// Join vCard continuation lines (starting with a space or tab) onto the line before.
fn unfold_lines(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(last) = out.last_mut() {
                last.push_str(&line[1..]);
            }
            continue;
        }
        out.push(line.to_string());
    }
    out
}

/// Apply one `PROP;params:value` line to the card being built.
fn apply_line(card: &mut VcfCard, line: &str) {
    let Some((name, value)) = line.split_once(':') else {
        return;
    };
    let prop = name.split(';').next().unwrap_or(name);
    let prop_upper = prop.to_ascii_uppercase();
    let base = prop_upper
        .rsplit_once('.')
        .map(|(_, rest)| rest.to_string())
        .unwrap_or(prop_upper);

    match base.as_str() {
        "FN" => card.fn_raw = unescape(value),
        "N" => {
            let parts: Vec<&str> = value.split(';').collect();
            card.n_family = unescape(parts.first().copied().unwrap_or(""));
            card.n_given = unescape(parts.get(1).copied().unwrap_or(""));
            card.n_middle = unescape(parts.get(2).copied().unwrap_or(""));
        }
        "TEL" => {
            let phone = value.trim();
            if !phone.is_empty() && !card.phones.iter().any(|p| p == phone) {
                card.phones.push(phone.to_string());
            }
        }
        "EMAIL" => {
            if card.email.is_none() {
                let email = value.trim();
                if !email.is_empty() {
                    card.email = Some(email.to_string());
                }
            }
        }
        "CATEGORIES" => {
            for category in split_categories(value) {
                if !card
                    .categories
                    .iter()
                    .any(|c| c.eq_ignore_ascii_case(&category))
                {
                    card.categories.push(category);
                }
            }
        }
        _ => {}
    }
}

/// Undo vCard escaping of newlines, commas, semicolons, and backslashes.
fn unescape(s: &str) -> String {
    s.replace("\\n", "\n")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
        .trim()
        .to_string()
}

/// Split a CATEGORIES value on unescaped commas, then unescape each token.
fn split_categories(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                cur.push('\\');
                cur.push(next);
            } else {
                cur.push('\\');
            }
            continue;
        }
        if ch == ',' {
            let token = unescape(&cur);
            if !token.is_empty() {
                out.push(token);
            }
            cur.clear();
            continue;
        }
        cur.push(ch);
    }
    let token = unescape(&cur);
    if !token.is_empty() {
        out.push(token);
    }
    out
}

/// Extract `[Tag]` values from a string and return (`stripped_text`, tags).
pub fn extract_tags(raw: &str) -> (String, Vec<String>) {
    let mut tags = Vec::new();
    let mut out = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '[' {
            let mut tag = String::new();
            for c in chars.by_ref() {
                if c == ']' {
                    break;
                }
                tag.push(c);
            }
            let tag = tag.trim();
            if !tag.is_empty() {
                tags.push(tag.to_string());
            }
        } else {
            out.push(ch);
        }
    }
    let stripped = out.split_whitespace().collect::<Vec<_>>().join(" ");
    (stripped, tags)
}

/// Strip `[Tag]` markers from a VCF name field.
pub fn strip_tags(raw: &str) -> String {
    extract_tags(raw).0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_categories_handles_escaped_commas() {
        assert_eq!(
            split_categories(r"Family,Work\,Inc,Friends"),
            vec!["Family", "Work,Inc", "Friends"]
        );
        assert_eq!(split_categories("  Family  ,  "), vec!["Family"]);
        assert!(split_categories("").is_empty());
    }

    #[test]
    fn parse_vcf_collects_repeated_categories_email_middle() {
        let text = "BEGIN:VCARD\nVERSION:3.0\nFN:Ada Lovelace\nN:Lovelace;Ada;Augusta;;\n\
             TEL:+15551234567\nEMAIL:ada@example.com\nCATEGORIES:Family,Friends\nCATEGORIES:Work\n\
             CATEGORIES:family\nEND:VCARD\n";
        let cards = parse_vcf_str(text);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].n_middle, "Augusta");
        assert_eq!(cards[0].email.as_deref(), Some("ada@example.com"));
        assert_eq!(cards[0].categories, vec!["Family", "Friends", "Work"]);
    }

    #[test]
    fn extract_tags_strips_brackets() {
        let (stripped, tags) = extract_tags("Ada [Family] Lovelace [Work]");
        assert_eq!(stripped, "Ada Lovelace");
        assert_eq!(tags, vec!["Family", "Work"]);
        assert_eq!(strip_tags("Ada [Family]"), "Ada");
    }

    #[test]
    fn parse_vcf_unfolds_lines_continued_with_a_space_or_a_tab() {
        // A continuation line drops its one leading space or tab and joins
        // the line before it.
        let text = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Ada Augusta King, Coun\r\n \
             tess of Lovelace\r\nTEL:+1555\r\n\t1234567\r\nEND:VCARD\r\n";
        let cards = parse_vcf_str(text);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].fn_raw, "Ada Augusta King, Countess of Lovelace");
        assert_eq!(cards[0].phones, vec!["+15551234567"]);
    }

    #[test]
    fn parse_vcf_keeps_one_phone_for_a_repeated_or_empty_tel() {
        let text = "BEGIN:VCARD\nFN:Ada\nTEL;TYPE=CELL:+15551234567\n\
             TEL;TYPE=HOME:+15551234567\nTEL:\nEND:VCARD\n";
        let cards = parse_vcf_str(text);
        assert_eq!(cards[0].phones, vec!["+15551234567"]);
    }
}
