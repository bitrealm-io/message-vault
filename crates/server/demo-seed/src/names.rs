//! Loads first, last, and middle name lists and picks from them.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use rand::Rng;
use rand::seq::IndexedRandom;

/// First, last, and middle names loaded from text files.
pub struct NameBank {
    pub first: Vec<String>,
    pub last: Vec<String>,
    pub middle: Vec<String>,
}

impl NameBank {
    /// Load first, last, and middle name lists from `data/names/` in this crate.
    ///
    /// # Errors
    ///
    /// Returns an error if a list file cannot be read or is empty.
    pub fn load_default() -> Result<Self> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/names");
        Ok(Self {
            first: load_lines(&root.join("first_names.txt"))?,
            last: load_lines(&root.join("last_names.txt"))?,
            middle: load_lines(&root.join("middle_names.txt"))?,
        })
    }

    /// Pick a first name at random. Returns `"Alex"` if the list is empty.
    pub fn pick_first(&self, rng: &mut impl Rng) -> &str {
        match self.first.choose(rng) {
            Some(name) => name.as_str(),
            None => "Alex",
        }
    }

    /// Pick a last name at random. Returns `"Lee"` if the list is empty.
    pub fn pick_last(&self, rng: &mut impl Rng) -> &str {
        match self.last.choose(rng) {
            Some(name) => name.as_str(),
            None => "Lee",
        }
    }

    /// Pick a middle name at random. Returns `"Lee"` if the list is empty.
    pub fn pick_middle(&self, rng: &mut impl Rng) -> &str {
        match self.middle.choose(rng) {
            Some(name) => name.as_str(),
            None => "Lee",
        }
    }
}

/// Read non-empty lines, skipping comments that start with `#`.
///
/// # Errors
///
/// Returns an error if the file cannot be read or has no usable lines.
fn load_lines(path: &Path) -> Result<Vec<String>> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut lines = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        lines.push(line.to_string());
    }
    if lines.is_empty() {
        bail!("{} is empty", path.display());
    }
    Ok(lines)
}
