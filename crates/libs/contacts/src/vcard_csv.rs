//! Shared vCard CSV column layout and row reader (VCF exported as CSV).

use anyhow::{Context, Result, bail};
use std::fs::File;
use std::path::Path;

/// Phone / fax columns commonly used by vCard CSV exports.
const VCARD_CSV_PHONE_COLUMNS: &[&str] = &[
    "mobile phone",
    "home phone",
    "work phone",
    "other phone",
    "home fax",
    "work fax",
    "other fax",
];

/// One contact row from a vCard CSV (raw phone strings).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContactCsvRow {
    /// First-name cell.
    pub first: String,
    /// Middle-name cell.
    pub middle: String,
    /// Last-name cell.
    pub last: String,
    /// Phone values from phone/fax columns, `;`-split, not normalized.
    pub phones: Vec<String>,
    /// Raw Notes cell when present (exporters may scrape `+E.164` tokens).
    pub notes: Option<String>,
}

impl ContactCsvRow {
    /// The name the cells spell: first, middle, and last joined by a space
    /// with inner whitespace collapsed; `None` when all three are blank.
    pub fn display_name(&self) -> Option<String> {
        let joined = [&self.first, &self.middle, &self.last]
            .into_iter()
            .filter(|part| !part.is_empty())
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(" ");
        let display = crate::name::collapse_inner_whitespace(&joined);
        (!display.is_empty()).then_some(display)
    }
}

/// Normalize a Contacts CSV header the same way book/validate loaders do.
pub(crate) fn normalize_vcard_csv_header(h: &str) -> String {
    h.trim()
        .trim_start_matches('\u{feff}')
        .to_ascii_lowercase()
        .replace('_', " ")
}

/// True for phone columns (`Mobile Phone`, …). Bare `phones` is excluded.
fn is_phone_header(h: &str) -> bool {
    h != "phones" && h.contains("phone")
}

/// Column indexes for a vCard CSV header row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VcardCsvColumns {
    /// Index of the first-name column, if present.
    pub first: Option<usize>,
    /// Index of the middle-name column, if present.
    pub middle: Option<usize>,
    /// Index of the last-name column, if present.
    pub last: Option<usize>,
    /// Index of the notes column, if present.
    pub notes: Option<usize>,
    /// Indexes of phone/fax columns.
    pub phones: Vec<usize>,
}

impl VcardCsvColumns {
    /// Resolve column indexes from raw header names.
    pub(crate) fn from_headers<I, S>(headers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let headers: Vec<String> = headers
            .into_iter()
            .map(|h| normalize_vcard_csv_header(h.as_ref()))
            .collect();
        let mut phones: Vec<usize> = VCARD_CSV_PHONE_COLUMNS
            .iter()
            .filter_map(|name| headers.iter().position(|h| h == *name))
            .collect();
        for (i, h) in headers.iter().enumerate() {
            if is_phone_header(h) && !phones.contains(&i) {
                phones.push(i);
            }
        }
        Self {
            first: headers.iter().position(|h| h == "first name"),
            middle: headers.iter().position(|h| h == "middle name"),
            last: headers.iter().position(|h| h == "last name"),
            notes: headers.iter().position(|h| h == "notes"),
            phones,
        }
    }

    /// True when the header looks like a vCard CSV export.
    fn looks_like_vcard_csv(&self) -> bool {
        self.first.is_some() || !self.phones.is_empty()
    }
}

/// Read all contact rows from a vCard CSV.
///
/// Does not require a display name; skips rows with no phone cells.
/// Phone values are raw (caller normalizes with the `phone` crate).
///
/// # Errors
///
/// Returns an error when the file cannot be opened or parsed, or when the
/// header does not look like a vCard CSV.
pub fn read_vcard_csv_rows(path: &Path) -> Result<Vec<ContactCsvRow>> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .has_headers(true)
        .from_reader(file);
    let headers = rdr
        .headers()
        .with_context(|| format!("headers {}", path.display()))?
        .iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let cols = VcardCsvColumns::from_headers(&headers);
    if !cols.looks_like_vcard_csv() {
        bail!(
            "contacts CSV {} does not look like a vCard CSV \
             (expected First Name and/or phone columns)",
            path.display()
        );
    }

    let mut rows = Vec::new();
    for (idx, rec) in rdr.records().enumerate() {
        let rec = rec.with_context(|| format!("row {} in {}", idx + 2, path.display()))?;
        let cell = |i: Option<usize>| -> String {
            i.and_then(|idx| rec.get(idx))
                .unwrap_or("")
                .trim()
                .to_string()
        };
        let mut phones = Vec::new();
        for &i in &cols.phones {
            push_phone_cell_values(rec.get(i).unwrap_or(""), &mut phones);
        }
        let notes = cols.notes.and_then(|i| {
            let n = rec.get(i).unwrap_or("").trim();
            if n.is_empty() {
                None
            } else {
                Some(n.to_string())
            }
        });
        // Keep notes-only rows so callers can scrape +E.164 from Notes.
        if phones.is_empty() && notes.is_none() {
            continue;
        }
        rows.push(ContactCsvRow {
            first: cell(cols.first),
            middle: cell(cols.middle),
            last: cell(cols.last),
            phones,
            notes,
        });
    }
    Ok(rows)
}

/// Split a phone cell on `;` and add each non-empty value, unparsed.
///
/// Named apart from `book::push_phones_from_field` on purpose: this one only
/// separates a cell into values and keeps them as written. Turning a value into
/// a handle is the book's job, and it applies rules this one does not.
fn push_phone_cell_values(raw: &str, out: &mut Vec<String>) {
    let raw = raw.trim();
    if raw.is_empty() {
        return;
    }
    for part in raw.split(';') {
        let part = part.trim();
        if !part.is_empty() && !out.iter().any(|p| p == part) {
            out.push(part.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn resolves_name_and_phone_columns() {
        let cols = VcardCsvColumns::from_headers([
            "First Name",
            "Middle Name",
            "Last Name",
            "Mobile Phone",
            "Notes",
        ]);
        assert_eq!(cols.first, Some(0));
        assert_eq!(cols.middle, Some(1));
        assert_eq!(cols.last, Some(2));
        assert_eq!(cols.phones, vec![3]);
        assert_eq!(cols.notes, Some(4));
        assert!(cols.looks_like_vcard_csv());
    }

    #[test]
    fn accepts_generic_phone_column() {
        let cols = VcardCsvColumns::from_headers(["First Name", "Last Name", "Phone 1"]);
        assert!(cols.phones.contains(&2));
        assert!(cols.looks_like_vcard_csv());
    }

    #[test]
    fn read_rows_splits_semicolons() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmp,
            "First Name,Last Name,Mobile Phone\nAda,Lovelace,+15551111;+15552222\n"
        )
        .unwrap();
        let rows = read_vcard_csv_rows(tmp.path()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].first, "Ada");
        assert_eq!(rows[0].phones, vec!["+15551111", "+15552222"]);
    }
}
