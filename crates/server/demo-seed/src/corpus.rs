//! Loads public-domain sentences used as message bodies.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use rand::Rng;
use rand::RngExt;
use rand::seq::IndexedRandom;

/// Sentences taken from a public-domain book and used as message bodies.
pub struct Corpus {
    sentences: Vec<String>,
}

impl Corpus {
    /// Load Pride and Prejudice from `data/corpus/` in this crate.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or it yields fewer than 100 sentences.
    pub fn load_pride_and_prejudice() -> Result<Self> {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("data/corpus/pride-and-prejudice.txt");
        Self::load(&path)
    }

    /// Split a book text into sentences long enough to use as message bodies.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or it yields fewer than 100 sentences.
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            fs::read_to_string(path).with_context(|| format!("read corpus {}", path.display()))?;
        let sentences = extract_sentences(&text);
        if sentences.len() < 100 {
            bail!(
                "corpus {} yielded only {} sentences (need ≥100)",
                path.display(),
                sentences.len()
            );
        }
        Ok(Self { sentences })
    }

    /// Pick one sentence at random. Returns `"Okay."` if the list is empty.
    pub fn pick(&self, rng: &mut impl Rng) -> &str {
        match self.sentences.choose(rng) {
            Some(sentence) => sentence.as_str(),
            None => "Okay.",
        }
    }

    /// Pick one sentence, and sometimes a second, for a slightly longer message.
    pub fn pick_message(&self, rng: &mut impl Rng) -> String {
        let first = self.pick(rng).to_string();
        if rng.random_bool(0.18) {
            let second = self.pick(rng);
            if second != first {
                return format!("{first} {second}");
            }
        }
        first
    }

    /// Number of sentences kept after splitting the book.
    pub fn len(&self) -> usize {
        self.sentences.len()
    }
}

/// Split book text on `.`, `!`, and `?`, skipping chapter headings and empty lines.
fn extract_sentences(text: &str) -> Vec<String> {
    let mut flat = String::with_capacity(text.len());
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() {
            flat.push(' ');
            continue;
        }
        if t.starts_with('[') || is_heading(t) {
            flat.push(' ');
            continue;
        }
        flat.push_str(t);
        flat.push(' ');
    }

    // Replace curly quotes and dashes with plain ASCII so sentence splitting
    // sees ordinary punctuation.
    let flat = flat
        .replace(['“', '”', '„'], "\"")
        .replace(['‘', '’'], "'")
        .replace(['—', '–'], "-");

    let mut out = Vec::new();
    let mut start = 0usize;
    for (i, &b) in flat.as_bytes().iter().enumerate() {
        let c = b as char;
        if matches!(c, '.' | '!' | '?') {
            let end = i + 1;
            if let Some(s) = normalize_sentence(&flat[start..end]) {
                out.push(s);
            }
            start = end;
        }
    }
    out
}

/// True for chapter or volume headings that should not become messages.
fn is_heading(t: &str) -> bool {
    let u = t.to_ascii_uppercase();
    u.starts_with("CHAPTER ")
        || u.starts_with("VOLUME ")
        || u.starts_with("CHAPTER ")
        || u == "PRIDE AND PREJUDICE"
}

/// Trim quotes and skip sentences that are too short, too long, or mostly digits.
fn normalize_sentence(raw: &str) -> Option<String> {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed
        .trim()
        .trim_matches(|c: char| matches!(c, '"' | '\'' | '*' | '_' | '-' | '[' | ']'));
    if trimmed.len() < 20 || trimmed.len() > 180 {
        return None;
    }
    if !trimmed.chars().any(|c| c.is_ascii_lowercase()) {
        return None;
    }
    // Skip leftover footnote fragments that are mostly digits.
    let digit_count = trimmed.chars().filter(|c| c.is_ascii_digit()).count();
    if digit_count > trimmed.len() / 3 {
        return None;
    }
    Some(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_bundled_corpus() {
        let c = Corpus::load_pride_and_prejudice().unwrap();
        assert!(c.len() >= 1000, "got {}", c.len());
    }
}
