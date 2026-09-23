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
            let phones: Vec<String> = card.phones.iter().filter_map(|p| phone_key(p)).collect();
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
                push_phones_from_field(p, &mut phones);
            }
            if let Some(notes) = &row.notes {
                push_plus_runs(notes, &mut phones);
            }
            if phones.is_empty() {
                continue;
            }
            book.insert_entry(&display, &phones);
        }
        Ok(book)
    }

    /// Add one name with its phones, merging into an existing entry with the same normalized name.
    ///
    /// Each phone is filed under [`phone_key`], the key the vault gives the
    /// same number as a handle.
    fn insert_entry(&mut self, display: &str, phones: &[String]) {
        let display = collapse_inner_whitespace(display);
        if display.is_empty() || phones.is_empty() {
            return;
        }
        let key = normalize_name_key(&display);
        // All entries from VCF/vCard CSV are phone type
        let handle_type = HandleType::Phone;
        for phone in phones {
            let Some(normalized) = phone_key(phone) else {
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

/// The vault's handle key for a written phone number, or `None` when it has
/// too few digits to be one.
///
/// The `+` is what says a number is international, so the key is taken from
/// the number as written, never from digits it was stripped to. Stripping it
/// first turned `+65 9123 4567` into ten digits that US rules read as
/// `+16591234567`, a different person.
fn phone_key(written: &str) -> Option<String> {
    sanitize_number(written)?;
    Some(normalize_handle(written, HandleType::Phone))
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

/// Collect handle keys from a field that is known to hold phone numbers.
///
/// One field may hold several, separated by `;`, `,`, `|` or `/`. Each part has to
/// be written as a number: [`phone::sanitize_phone_shaped`] rejects a part
/// carrying prose, so `+15551234567 (see also +15557654321)` no longer collapses
/// into one 22-digit handle. The `+E.164` scrape that follows recovers both
/// numbers from it instead.
fn push_phones_from_field(raw: &str, out: &mut Vec<String>) {
    for part in raw.split([';', ',', '|', '/']) {
        if phone::sanitize_phone_shaped(part).is_some()
            && let Some(key) = phone_key(part)
            && !out.contains(&key)
        {
            out.push(key);
        }
    }
    push_plus_runs(raw, out);
}

/// Scrape bare `+digits` runs: `PROP-ID` notes, and trailing phones in Notes.
///
/// This is the whole of what free text yields. Notes routinely hold addresses,
/// dates and account numbers, and splitting free text on separators and keeping
/// any part with four digits in it turns every one of them into a handle — and
/// a handle is what an imported message is matched against. The `+` prefix is
/// what marks a number as a number in prose, so it is what is required.
fn push_plus_runs(raw: &str, out: &mut Vec<String>) {
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
                && let Some(key) = phone_key(&raw[start..i])
                && !out.contains(&key)
            {
                out.push(key);
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

    /// One number, written any common way, has one key: the key the vault
    /// gives the handle, the key the book files the card under, and the key
    /// an owner phone is matched on must be the same string. The book once
    /// dropped the `+` and applied US rules, so `+65 9123 4567` was filed as
    /// the US number `+16591234567`, and an owner `+44 7700 900123` became
    /// `447700900123`.
    #[test]
    fn one_number_has_one_key_in_the_book_the_owner_set_and_the_vault() {
        let rows: [(&str, &str); 11] = [
            ("+44 20 7946 0000", "+442079460000"),
            ("+442079460000", "+442079460000"),
            ("+65 9123 4567", "+6591234567"),
            ("+47 912 34 567", "+4791234567"),
            ("+45 12 34 56 78", "+4512345678"),
            ("(555) 555-0122", "+15555550122"),
            ("+1 555 555 0122", "+15555550122"),
            ("5555550122", "+15555550122"),
            // No country is written, so nothing can make it E.164 with
            // certainty. It stays digits, never the invented `+02079460000`.
            ("020 7946 0000", "02079460000"),
            // A short code has no country either, and stays as written.
            ("72345", "72345"),
            ("+1 (555) 555-0199", "+15555550199"),
        ];
        let dir = tempfile::tempdir().unwrap();
        let mut wrong = Vec::new();
        for (written, expected) in rows {
            let (handle_key, _) = phone::normalize_typed_handle(written, HandleType::Phone);

            let owners = phone::OwnerHandleSet::from_phones(&[written.to_string()]).unwrap();
            let owner_key = owners.primary_owner_handle().unwrap_or_default();
            let owner_matches_handle = owners.is_owner(&handle_key, HandleType::Phone);

            let vcf = write_file(
                &dir,
                "row.vcf",
                &format!("BEGIN:VCARD\nVERSION:3.0\nFN:Row Person\nTEL:{written}\nEND:VCARD\n"),
            );
            let vcf_key = ContactsBook::load_vcf(&vcf)
                .unwrap()
                .lookup_handle_by_name("Row Person")
                .map(|(key, _)| key)
                .unwrap_or_default();

            let csv = write_file(
                &dir,
                "row.csv",
                &format!("First Name,Last Name,Mobile Phone\nRow,Person,{written}\n"),
            );
            let csv_key = ContactsBook::load_vcard_csv(&csv)
                .unwrap()
                .lookup_handle_by_name("Row Person")
                .map(|(key, _)| key)
                .unwrap_or_default();

            if [&handle_key, &owner_key, &vcf_key, &csv_key]
                .iter()
                .any(|key| key.as_str() != expected)
                || !owner_matches_handle
            {
                wrong.push(format!(
                    "{written}: expected {expected}, handle {handle_key}, owner {owner_key} \
                     (is_owner {owner_matches_handle}), vcf {vcf_key}, csv {csv_key}"
                ));
            }
        }
        assert!(wrong.is_empty(), "keys disagree:\n{}", wrong.join("\n"));

        // Agreement is not enough if two numbers share one key: the
        // Singapore owner is not the US subscriber with the same digits.
        let owners = phone::OwnerHandleSet::from_phones(&["+65 9123 4567".into()]).unwrap();
        assert!(!owners.is_owner("+16591234567", HandleType::Phone));
    }

    /// A card with a Singapore and a UK number names those two handles and
    /// no US number that shares their digits.
    #[test]
    fn a_card_with_international_numbers_names_only_those_handles() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(
            &dir,
            "contacts.vcf",
            "BEGIN:VCARD\nVERSION:3.0\nN:Tan;Mei;;;\nFN:Mei Tan\n\
TEL;TYPE=CELL:+65 9123 4567\nTEL;TYPE=WORK:+44 20 7946 0000\nEND:VCARD\n",
        );
        let book = ContactsBook::load_vcf(&path).unwrap();
        assert_eq!(
            book.lookup_name_by_handle("+6591234567", HandleType::Phone),
            Some("Mei Tan")
        );
        assert_eq!(
            book.lookup_name_by_handle("+442079460000", HandleType::Phone),
            Some("Mei Tan")
        );
        assert_eq!(
            book.lookup_name_by_handle("+16591234567", HandleType::Phone),
            None,
            "+16591234567 is a different person, in the US"
        );
        assert_eq!(
            book.enrich_display_name("+16591234567", HandleType::Phone, ""),
            None
        );
        assert_eq!(book.len(), 2);
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

    /// A phone field: separated values, each of which has to be written as a
    /// number.
    #[test]
    fn a_phone_field_splits_on_separators_and_rejects_prose() {
        let mut out = Vec::new();
        push_phones_from_field("+15551234567; +15557654321, +15550000000", &mut out);
        assert_eq!(
            out,
            // Each is stored under the vault's handle key, `+` included.
            ["+15551234567", "+15557654321", "+15550000000"],
            "each separator splits a field"
        );

        // The same number twice in one field must be stored once.
        let mut out = Vec::new();
        push_phones_from_field("+15551234567; +15551234567", &mut out);
        assert_eq!(out, ["+15551234567"], "a repeat must not be stored twice");

        // Written formatting is not prose, and `/` separates two numbers.
        let mut out = Vec::new();
        push_phones_from_field("(555) 123-4567 / 555.765.4321", &mut out);
        assert_eq!(
            out,
            ["+15551234567", "+15557654321"],
            "punctuation is part of how a number is written; `/` is not"
        );

        // Issue #526: two numbers in one field with no separator between them.
        // The part carries prose, so it is dropped whole rather than run
        // together into `1555123456715557654321`; the `+` scrape then recovers
        // both numbers from it.
        let mut out = Vec::new();
        push_phones_from_field("+15551234567 (see also +15557654321)", &mut out);
        assert_eq!(out, ["+15551234567", "+15557654321"]);

        // Same defect without any prose to catch it: the digits are inside
        // permitted punctuation, so the E.164 ceiling is what rejects them.
        let mut out = Vec::new();
        push_phones_from_field("+15551234567 +15557654321", &mut out);
        assert_eq!(
            out,
            ["+15551234567", "+15557654321"],
            "20 digits is not a phone number"
        );

        // A field that holds no number at all yields nothing.
        let mut out = Vec::new();
        push_phones_from_field("met in 2019 at 42 Acacia Avenue", &mut out);
        assert!(out.is_empty(), "got {out:?}");
    }

    /// Free text, which yields `+E.164` tokens and nothing else.
    ///
    /// The loop that scrapes bare `+digits` runs carried nine surviving
    /// mutants — the index arithmetic, the `i > start + 1` guard that rejects a
    /// lone `+`, and the duplicate check. Nothing exercised it: every test used
    /// a well-formed phone column.
    #[test]
    fn free_text_yields_only_plus_prefixed_numbers() {
        // A bare run inside prose, with no separator around it.
        let mut out = Vec::new();
        push_plus_runs("ring me on +442071838750 after six", &mut out);
        assert_eq!(out, ["+442071838750"]);

        // Two runs in one blob, and a repeat stored once.
        let mut out = Vec::new();
        push_plus_runs(
            "PROP-ID: +15551234567 / alt +15557654321 (+15551234567)",
            &mut out,
        );
        assert_eq!(out, ["+15551234567", "+15557654321"]);

        // A lone `+` is not a number, and neither is `+` followed by one digit:
        // the guard is `i > start + 1`, and dropping it produces junk handles.
        let mut out = Vec::new();
        push_plus_runs("a + b +1 c", &mut out);
        assert!(out.is_empty(), "got {out:?}");

        // Issue #526: a note with a year and a house number yielded `201942`,
        // a plausible short code, because every digit in the field was
        // collected into one string and six digits was enough to pass. Notes
        // are free text, so nothing without a `+` is taken from them.
        let mut out = Vec::new();
        push_plus_runs("met in 2019 at 42 Acacia Avenue", &mut out);
        assert!(out.is_empty(), "a note is not a phone number, got {out:?}");
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
