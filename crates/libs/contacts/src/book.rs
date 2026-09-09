//! Bidirectional contacts index (name↔handle), keyed by handle type.

use crate::name::{collapse_inner_whitespace, is_blank_or_unknown_name, normalize_name_key};
use crate::vcard_csv::read_vcard_csv_rows;
use crate::vcf::{self, strip_tags};
use anyhow::{Result, bail};
use message_ir::HandleType;
use phone::sanitize_number;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Bidirectional contacts index (name↔handle), keyed by handle type.
#[derive(Debug, Default, Clone)]
pub struct ContactsBook {
    /// Normalized name key → (normalized handle, handle type).
    by_name: HashMap<String, (String, HandleType)>,
    /// (normalized handle, handle type) → display name.
    by_handle: HashMap<(String, HandleType), String>,
}

impl ContactsBook {
    /// Construct an empty index.
    pub fn empty() -> Self {
        Self {
            by_name: HashMap::new(),
            by_handle: HashMap::new(),
        }
    }

    /// Load a contacts file, choosing the loader from its detected format.
    ///
    /// # Errors
    ///
    /// Returns an error when the format cannot be detected or the file cannot
    /// be read or parsed.
    pub fn load_contacts_file(path: &Path) -> Result<Self> {
        use crate::format::{ContactsFormat, detect_contacts_format};
        let format = detect_contacts_format(path)?;
        match format {
            ContactsFormat::Vcf => Self::load_vcf(path),
            ContactsFormat::VcardCsv => Self::load_vcard_csv(path),
        }
    }

    /// Load contacts from a VCF file (FN/N + TEL).
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read or parsed.
    pub fn load_vcf(path: &Path) -> Result<Self> {
        let cards = vcf::parse_vcf(path)?;
        let mut book = Self::empty();
        for card in cards {
            let phones: Vec<String> = card
                .phones
                .iter()
                .filter_map(|p| sanitize_number(p))
                .collect();
            if phones.is_empty() {
                continue;
            }
            let first = strip_tags(&card.n_given);
            let last = strip_tags(&card.n_family);
            let fn_stripped = strip_tags(&card.fn_raw);
            let display = if !first.is_empty() || !last.is_empty() {
                if last.is_empty() {
                    first
                } else if first.is_empty() {
                    last
                } else {
                    format!("{first} {last}")
                }
            } else if !fn_stripped.is_empty() {
                fn_stripped
            } else {
                continue;
            };
            book.insert_entry(&display, &phones);
        }
        Ok(book)
    }

    /// Load a vCard CSV export (wide address-book columns).
    ///
    /// Phones come from phone/fax columns, plus `+E.164` tokens scraped from
    /// `Notes` (including `PROP-ID: +…`).
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read or parsed.
    pub fn load_vcard_csv(path: &Path) -> Result<Self> {
        let rows = read_vcard_csv_rows(path)?;
        let mut book = Self::empty();
        for row in rows {
            let Some(display) = row.display_name() else {
                continue;
            };

            let mut phones = Vec::new();
            for p in &row.phones {
                push_phones_from_raw(p, &mut phones);
            }
            if let Some(notes) = &row.notes {
                push_phones_from_raw(notes, &mut phones);
            }
            if phones.is_empty() {
                continue;
            }
            book.insert_entry(&display, &phones);
        }
        Ok(book)
    }

    /// Add one name with its phones, merging into an existing entry with the same normalized name.
    fn insert_entry(&mut self, display: &str, phones: &[String]) {
        let display = collapse_inner_whitespace(display);
        if display.is_empty() || phones.is_empty() {
            return;
        }
        let key = normalize_name_key(&display);
        // All entries from VCF/vCard CSV are phone type
        let handle_type = HandleType::Phone;
        for phone in phones {
            // Keep digits as-is when the value is ambiguous for the US-centric
            // book. A trunk-zero `020 7946 0000` must never become the invalid
            // `+02079460000`. The vault server records a review note on the
            // handles table for those cases.
            let Some(normalized) = phone::normalize_digits_us(phone) else {
                continue;
            };
            if !key.is_empty() {
                self.by_name
                    .entry(key.clone())
                    .or_insert_with(|| (normalized.clone(), handle_type));
            }
            self.by_handle
                .entry((normalized.clone(), handle_type))
                .or_insert_with(|| display.clone());
        }
    }

    /// Look up (normalized handle, type) for a display / export name.
    pub fn lookup_handle_by_name(&self, name: &str) -> Option<(String, HandleType)> {
        let key = normalize_name_key(name);
        if key.is_empty() {
            return None;
        }
        self.by_name.get(&key).cloned()
    }

    /// Look up display name for a (normalized handle, type).
    pub fn lookup_name_by_handle(&self, normalized: &str, handle_type: HandleType) -> Option<&str> {
        self.by_handle
            .get(&(normalized.to_string(), handle_type))
            .map(String::as_str)
    }

    /// If `name` is blank/unknown and `handle` is in the book, return the display name.
    pub fn enrich_display_name(
        &self,
        handle: &str,
        handle_type: HandleType,
        name: &str,
    ) -> Option<String> {
        if !is_blank_or_unknown_name(name) {
            return None;
        }
        // Normalize handle based on type before lookup
        let normalized = normalize_handle(handle, handle_type);
        self.lookup_name_by_handle(&normalized, handle_type)
            .map(str::to_string)
    }

    /// Number of (handle, type) entries indexed.
    pub fn len(&self) -> usize {
        self.by_handle.len()
    }

    /// Whether the book has no entries.
    pub fn is_empty(&self) -> bool {
        self.by_handle.is_empty() && self.by_name.is_empty()
    }
}

/// The stored form of a handle for its type.
fn normalize_handle(raw: &str, handle_type: HandleType) -> String {
    phone::normalize_typed_handle(raw, handle_type).0
}

/// Load contacts from at most one of `--contacts` or `--vcf`.
///
/// `--contacts` accepts either shape (VCF or vCard
/// CSV). `--vcf` is a VCF-only alias.
///
/// When neither is passed, returns an empty book and writes a warning via `log`
/// (or stderr when `log` is `None`).
///
/// # Errors
///
/// Returns an error when both flags are passed, or when the contacts file
/// cannot be loaded.
pub fn resolve_contacts_cli(
    contacts: Option<PathBuf>,
    vcf: Option<PathBuf>,
    log: Option<&dyn Fn(&str)>,
) -> Result<(ContactsBook, Option<PathBuf>)> {
    match (contacts, vcf) {
        (Some(path), None) | (None, Some(path)) => {
            let book = ContactsBook::load_contacts_file(&path)?;
            Ok((book, Some(path)))
        }
        (Some(_), Some(_)) => {
            bail!("pass only one of --contacts PATH or --vcf PATH")
        }
        (None, None) => {
            let msg = "warning: no contacts file provided (--contacts or --vcf); \
                 phone numbers will not be resolved to names";
            match log {
                Some(emit) => emit(msg),
                None => eprintln!("{msg}"),
            }
            Ok((ContactsBook::empty(), None))
        }
    }
}

/// Collect sanitized digit strings from semicolon-separated fields and `+E.164` tokens in free text.
fn push_phones_from_raw(raw: &str, out: &mut Vec<String>) {
    for part in raw.split([';', ',', '|']) {
        if let Some(digits) = sanitize_number(part.trim())
            && !out.contains(&digits)
        {
            out.push(digits);
        }
    }
    // Scrape bare +digits runs (PROP-ID notes, trailing phones in Notes blobs).
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'+' {
            let start = i;
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i > start + 1
                && let Some(digits) = sanitize_number(&raw[start..i])
                && !out.contains(&digits)
            {
                out.push(digits);
            }
        } else {
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    fn write_file(dir: &tempfile::TempDir, name: &str, body: &str) -> PathBuf {
        let path = dir.path().join(name);
        let mut f = File::create(&path).unwrap();
        write!(f, "{body}").unwrap();
        path
    }

    #[test]
    fn loads_vcard_csv_both_directions() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(
            &dir,
            "contacts.csv",
            "First Name,Last Name,Mobile Phone,Home Phone\n\
Sam,Example,15555550122,\n\
Pat,Contact,+15555550133,+15555550144\n",
        );
        let book = ContactsBook::load_vcard_csv(&path).unwrap();
        assert_eq!(
            book.lookup_handle_by_name("Sam Example"),
            Some(("+15555550122".to_string(), HandleType::Phone))
        );
        assert_eq!(
            book.lookup_name_by_handle("+15555550122", HandleType::Phone),
            Some("Sam Example")
        );
        assert_eq!(
            book.lookup_name_by_handle("+15555550133", HandleType::Phone),
            Some("Pat Contact")
        );
        assert_eq!(
            book.lookup_name_by_handle("+15555550144", HandleType::Phone),
            Some("Pat Contact")
        );
    }

    #[test]
    fn loads_vcf() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(
            &dir,
            "contacts.vcf",
            "BEGIN:VCARD\nVERSION:3.0\nN:Lovelace;Ada;;;\nFN:Ada Lovelace\n\
TEL;TYPE=CELL:+1-555-555-0100\nEND:VCARD\n",
        );
        let book = ContactsBook::load_vcf(&path).unwrap();
        assert_eq!(
            book.lookup_handle_by_name("Ada Lovelace"),
            Some(("+15555550100".to_string(), HandleType::Phone))
        );
        assert_eq!(
            book.lookup_name_by_handle("+15555550100", HandleType::Phone),
            Some("Ada Lovelace")
        );
    }

    #[test]
    fn resolve_cli_allows_none_and_rejects_both() {
        let (book, path) = resolve_contacts_cli(None, None, None).unwrap();
        assert!(book.is_empty());
        assert!(path.is_none());
        let dir = tempfile::tempdir().unwrap();
        let csv = write_file(
            &dir,
            "c.csv",
            "First Name,Last Name,Mobile Phone\nA,B,+15555550100\n",
        );
        let vcf = write_file(
            &dir,
            "c.vcf",
            "BEGIN:VCARD\nN:B;A;;;\nTEL:+15555550100\nEND:VCARD\n",
        );
        assert!(resolve_contacts_cli(Some(csv.clone()), Some(vcf), None).is_err());
        let (book, path) = resolve_contacts_cli(Some(csv), None, None).unwrap();
        assert!(!book.is_empty());
        assert!(path.is_some());
    }

    #[test]
    fn resolve_cli_loads_vcard_csv_via_contacts() {
        let dir = tempfile::tempdir().unwrap();
        let csv = write_file(
            &dir,
            "Contacts.csv",
            "First Name,Last Name,Mobile Phone\n\
Ada,Lovelace,+15555550100\n",
        );
        let (book, path) = resolve_contacts_cli(Some(csv), None, None).unwrap();
        assert!(path.is_some());
        assert_eq!(
            book.lookup_name_by_handle("+15555550100", HandleType::Phone),
            Some("Ada Lovelace")
        );
    }

    #[test]
    fn enrich_only_when_blank() {
        let mut book = ContactsBook::empty();
        book.insert_entry("Sam Example", &["5555550122".into()]);
        assert_eq!(
            book.enrich_display_name("5555550122", HandleType::Phone, "")
                .as_deref(),
            Some("Sam Example")
        );
        assert_eq!(
            book.enrich_display_name("5555550122", HandleType::Phone, "Already Set"),
            None
        );
    }

    #[test]
    fn loads_vcard_csv_phone_cols_and_notes() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(
            &dir,
            "Contacts.csv",
            "First Name,Middle Name,Last Name,Mobile Phone,Home Phone,Notes\n\
Bob,,McRoy,+13212462167,,mcroyr@gmail.com\n\
Kyle,,,,,PROP-ID: +17276875182; \n\
NoPhone,,Person,,,,\n",
        );
        let book = ContactsBook::load_vcard_csv(&path).unwrap();
        assert_eq!(
            book.lookup_handle_by_name("Bob McRoy"),
            Some(("+13212462167".to_string(), HandleType::Phone))
        );
        assert_eq!(
            book.lookup_name_by_handle("+13212462167", HandleType::Phone),
            Some("Bob McRoy")
        );
        assert_eq!(
            book.lookup_handle_by_name("Kyle"),
            Some(("+17276875182".to_string(), HandleType::Phone))
        );
        assert!(book.lookup_handle_by_name("NoPhone Person").is_none());
    }

    /// Phone scraping, which is how a number reaches a contact when it is not
    /// in a phone column at all.
    ///
    /// The loop that scrapes bare `+digits` runs out of free text carried nine
    /// surviving mutants — the index arithmetic, the `i > start + 1` guard that
    /// rejects a lone `+`, and the duplicate check. Nothing exercised it: every
    /// test used a well-formed phone column.
    #[test]
    fn phones_are_scraped_from_separators_and_from_free_text() {
        let mut out = Vec::new();
        push_phones_from_raw("+15551234567; +15557654321, +15550000000", &mut out);
        assert_eq!(
            out,
            // A leading US country digit is dropped by `sanitize_number`,
            // so the stored form is the ten-digit number.
            ["5551234567", "5557654321", "5550000000"],
            "each separator splits a field"
        );

        // The same number twice in one field must be stored once.
        let mut out = Vec::new();
        push_phones_from_raw("+15551234567; +15551234567", &mut out);
        assert_eq!(out, ["5551234567"], "a repeat must not be stored twice");

        // Two numbers in one field with no separator between them are run
        // together into one nonsense handle, because the separator pass keeps
        // every digit in the part it is given. Pinned as it is rather than as
        // it should be; issue #526 is the defect.
        let mut out = Vec::new();
        push_phones_from_raw("+15551234567 (see also +15557654321)", &mut out);
        assert_eq!(
            out[0], "1555123456715557654321",
            "today the digits run together"
        );

        // A bare run inside prose, with no separator around it.
        let mut out = Vec::new();
        push_phones_from_raw("ring me on +442071838750 after six", &mut out);
        assert_eq!(out, ["442071838750"]);

        // A lone `+` is not a number, and neither is `+` followed by one digit:
        // the guard is `i > start + 1`, and dropping it produces junk handles.
        let mut out = Vec::new();
        push_phones_from_raw("a + b +1 c", &mut out);
        assert!(out.is_empty(), "got {out:?}");

        // Same cause, plainer: a note with a year and a house number becomes a
        // "phone number", because every digit in the field is collected into
        // one string and six digits is enough to pass. Pinned as it is; issue
        // #526 is the defect.
        let mut out = Vec::new();
        push_phones_from_raw("met in 2019 at 42 Acacia Avenue", &mut out);
        assert_eq!(out, ["201942"], "today a note becomes a handle");
    }

    /// `len` and `is_empty` are what a caller checks before deciding a
    /// contacts file was worth loading, and both could be replaced with a
    /// constant without failing a test.
    #[test]
    fn the_book_reports_how_much_it_holds() {
        let empty = ContactsBook::empty();
        assert_eq!(empty.len(), 0);
        assert!(empty.is_empty());

        let dir = tempfile::tempdir().unwrap();
        let path = write_file(
            &dir,
            "contacts.csv",
            "First Name,Last Name,Mobile Phone,Home Phone\n\
             Ada,Lovelace,+15551234567,+15557654321\n\
             Alan,Turing,+15550000000,\n",
        );
        let book = ContactsBook::load_vcard_csv(&path).unwrap();

        // Three numbers across two people: the count is of handles, not of
        // people, which is what the doc comment says and what callers use it
        // for.
        assert_eq!(book.len(), 3, "one entry per handle");
        assert!(!book.is_empty());
    }
}
