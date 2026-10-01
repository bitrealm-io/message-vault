//! Holds the first, last, and middle name lists and picks from them.

use anyhow::{Result, bail};
use rand::Rng;
use rand::seq::IndexedRandom;

/// First, last, and middle names from the lists in `data/names/`.
pub struct NameBank {
    pub first: Vec<String>,
    pub last: Vec<String>,
    pub middle: Vec<String>,
}

impl NameBank {
    /// The name lists from `data/names/` in this crate, compiled into the
    /// program so generating needs no files beside it.
    ///
    /// # Errors
    ///
    /// Returns an error if a list is empty.
    pub fn load_default() -> Result<Self> {
        Ok(Self {
            first: usable_lines(
                "first_names.txt",
                include_str!("../data/names/first_names.txt"),
            )?,
            last: usable_lines(
                "last_names.txt",
                include_str!("../data/names/last_names.txt"),
            )?,
            middle: usable_lines(
                "middle_names.txt",
                include_str!("../data/names/middle_names.txt"),
            )?,
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

/// The non-empty lines of `text`, skipping comments that start with `#`.
/// `name` is the list's file name, for the error.
///
/// # Errors
///
/// Returns an error if the list has no usable lines.
fn usable_lines(name: &str, text: &str) -> Result<Vec<String>> {
    let mut lines = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        lines.push(line.to_string());
    }
    if lines.is_empty() {
        bail!("{name} is empty");
    }
    Ok(lines)
}
