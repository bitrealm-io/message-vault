//! Decide whether a contacts file is a vCard `.vcf` or a phone-column CSV.
//!
//! The desktop app and the vault server both accept either shape and pick a
//! loader from the answer, so detection has to run before any parsing.

use crate::vcard_csv::{VcardCsvColumns, normalize_vcard_csv_header};
use std::fs::{self, File};
use std::path::Path;

/// Shapes a contacts file can have, as accepted by
/// [`crate::ContactsBook::load_contacts_file`] and by the vault server's
/// contacts import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContactsFormat {
    /// vCard `.vcf`/`.vcard` input.
    Vcf,
    /// First/Last Name plus phone-column CSV input.
    VcardCsv,
}

/// Short red-box message when CSV/VCF content is not a known contacts format.
const UNRECOGNIZED_CONTACTS_FORMAT: &str = "Unrecognized contacts format.";

/// Probe failure for GUI preflight (short `message` + optional log `details`).
#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}{}", detail_suffix(.details))]
pub struct ContactsInputError {
    /// Short human-readable error (e.g. `"Unrecognized contacts format."`).
    pub message: String,
    /// Optional verbose detail lines for logs.
    pub details: Vec<String>,
}

/// ` (a; b)` after the message when there are detail lines, nothing otherwise.
fn detail_suffix(details: &[String]) -> String {
    if details.is_empty() {
        String::new()
    } else {
        format!(" ({})", details.join("; "))
    }
}

impl ContactsInputError {
    /// An error with a message and no detail lines.
    fn simple(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            details: Vec::new(),
        }
    }

    /// The unrecognized-format error, with what was seen as detail lines.
    fn unrecognized(details: Vec<String>) -> Self {
        Self {
            message: UNRECOGNIZED_CONTACTS_FORMAT.into(),
            details,
        }
    }
}

/// Returns a `ContactsInputError` when the path is missing, the extension is
/// unknown, or the content is not a recognized contacts format.
pub fn detect_contacts_format(path: &Path) -> Result<ContactsFormat, ContactsInputError> {
    detect_format(path)
}

/// VCF or vCard CSV, by extension first and then by content.
fn detect_format(path: &Path) -> Result<ContactsFormat, ContactsInputError> {
    if !path.is_file() {
        return Err(ContactsInputError::simple("Contacts file not found"));
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext != "csv" && ext != "vcf" && ext != "vcard" {
        return Err(ContactsInputError::simple(format!(
            "Contacts file must be a .csv or .vcf file: {}",
            path.display()
        )));
    }
    if ext == "vcf" || ext == "vcard" {
        return detect_vcf_format(path);
    }
    detect_csv_format(path)
}

/// Confirm a file has `BEGIN:VCARD` and `END:VCARD` lines.
fn detect_vcf_format(path: &Path) -> Result<ContactsFormat, ContactsInputError> {
    let text = fs::read_to_string(path).map_err(|e| {
        ContactsInputError::simple(format!("Could not read {}: {e}", path.display()))
    })?;
    let mut has_begin = false;
    let mut has_end = false;
    for line in text.lines() {
        let t = line.trim();
        if t.eq_ignore_ascii_case("BEGIN:VCARD") {
            has_begin = true;
        } else if t.eq_ignore_ascii_case("END:VCARD") {
            has_end = true;
        }
        if has_begin && has_end {
            return Ok(ContactsFormat::Vcf);
        }
    }
    let mut details = vec![format!("file={}", path.display())];
    if !has_begin {
        details.push("missing BEGIN:VCARD".into());
    }
    if !has_end {
        details.push("missing END:VCARD".into());
    }
    details.push("expected at least one BEGIN:VCARD … END:VCARD block".into());
    Err(ContactsInputError::unrecognized(details))
}

/// Confirm a CSV has the vCard CSV columns this reader needs.
fn detect_csv_format(path: &Path) -> Result<ContactsFormat, ContactsInputError> {
    let file = File::open(path).map_err(|e| {
        ContactsInputError::simple(format!("Could not open {}: {e}", path.display()))
    })?;
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .has_headers(true)
        .from_reader(file);
    let headers = rdr.headers().map_err(|e| {
        ContactsInputError::unrecognized(vec![
            format!("file={}", path.display()),
            format!("could not read CSV header: {e}"),
        ])
    })?;
    let header_l: Vec<String> = headers.iter().map(normalize_vcard_csv_header).collect();
    let cols = VcardCsvColumns::from_headers(headers.iter());
    let has_first = cols.first.is_some();
    let has_last = cols.last.is_some();
    let phone_cols: Vec<&str> = cols
        .phones
        .iter()
        .filter_map(|&i| header_l.get(i).map(String::as_str))
        .collect();
    let has_phone = !phone_cols.is_empty();

    if has_first && has_last && has_phone {
        return Ok(ContactsFormat::VcardCsv);
    }

    let mut details = vec![
        format!("file={}", path.display()),
        format!("headers={}", header_l.join(" | ")),
    ];
    if !has_first {
        details.push("missing First Name column".into());
    }
    if !has_last {
        details.push("missing Last Name column".into());
    }
    if !has_phone {
        details.push("missing Phone column (Mobile Phone, Home Phone, …)".into());
    } else {
        details.push(format!("phone columns: {}", phone_cols.join(", ")));
    }
    details.push(
        "valid CSV needs First Name, Last Name, and at least one Phone column \
         (vCard CSV)"
            .into(),
    );
    Err(ContactsInputError::unrecognized(details))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use std::io::Write;

    fn write(dir: &tempfile::TempDir, name: &str, body: &str) -> PathBuf {
        let path = dir.path().join(name);
        let mut f = File::create(&path).unwrap();
        write!(f, "{body}").unwrap();
        path
    }

    #[test]
    fn detect_rejects_missing_wrong_ext_and_bad_format() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.csv");
        let err = detect_contacts_format(&missing).unwrap_err();
        assert!(err.message.contains("not found"));

        let wrong_ext = write(&dir, "contacts.txt", "BEGIN:VCARD\nEND:VCARD\n");
        let err = detect_contacts_format(&wrong_ext).unwrap_err();
        assert!(err.message.contains("must be a .csv or .vcf"));

        let bad_csv = write(&dir, "contacts.csv", "name,phone\nAda,123\n");
        let err = detect_contacts_format(&bad_csv).unwrap_err();
        assert_eq!(err.message, UNRECOGNIZED_CONTACTS_FORMAT);
        assert!(err.details.iter().any(|d| d.contains("First Name")));
        assert!(err.details.iter().any(|d| d.contains("Last Name")));

        let missing_last = write(
            &dir,
            "partial.csv",
            "First Name,Mobile Phone\nAda,+15551234567\n",
        );
        let err = detect_contacts_format(&missing_last).unwrap_err();
        assert_eq!(err.message, UNRECOGNIZED_CONTACTS_FORMAT);
        assert!(err.details.iter().any(|d| d.contains("Last Name")));

        let empty_vcf = write(&dir, "empty.vcf", "NOTE: not a vcard\n");
        let err = detect_contacts_format(&empty_vcf).unwrap_err();
        assert_eq!(err.message, UNRECOGNIZED_CONTACTS_FORMAT);
        assert!(err.details.iter().any(|d| d.contains("BEGIN:VCARD")));

        let vcard_csv = write(
            &dir,
            "vcard.csv",
            "First Name,Last Name,Mobile Phone\nAda,Lovelace,+15551234567\n",
        );
        assert_eq!(
            detect_contacts_format(&vcard_csv).unwrap(),
            ContactsFormat::VcardCsv
        );

        let vcf = write(&dir, "ok.vcf", "BEGIN:VCARD\nFN:Ada\nEND:VCARD\n");
        assert_eq!(detect_contacts_format(&vcf).unwrap(), ContactsFormat::Vcf);
    }

    /// A vCard needs both markers, and needs them in one file.
    ///
    /// `has_begin && has_end` could become `has_begin || has_end` with nothing
    /// failing, and the three `if !has_*` guards on the error detail could each
    /// be deleted. A half-written export — the tail of a file that was cut off
    /// mid-transfer — would then be accepted as a vCard and read as empty.
    #[test]
    fn a_vcard_needs_both_markers() {
        let dir = tempfile::tempdir().unwrap();

        let begin_only = write(&dir, "begin.vcf", "BEGIN:VCARD\nFN:Ada\n");
        let err = detect_contacts_format(&begin_only).unwrap_err();
        assert_eq!(err.message, UNRECOGNIZED_CONTACTS_FORMAT);
        assert!(
            err.details.iter().any(|d| d.contains("missing END:VCARD")),
            "the message must name what is missing: {:?}",
            err.details
        );
        assert!(
            !err.details
                .iter()
                .any(|d| d.contains("missing BEGIN:VCARD")),
            "BEGIN is present, so it must not be reported missing: {:?}",
            err.details
        );

        let end_only = write(&dir, "end.vcf", "FN:Ada\nEND:VCARD\n");
        let err = detect_contacts_format(&end_only).unwrap_err();
        assert!(
            err.details
                .iter()
                .any(|d| d.contains("missing BEGIN:VCARD")),
            "{:?}",
            err.details
        );
        assert!(
            !err.details.iter().any(|d| d.contains("missing END:VCARD")),
            "{:?}",
            err.details
        );

        // Neither, which must name both.
        let neither = write(&dir, "neither.vcf", "just some notes\n");
        let err = detect_contacts_format(&neither).unwrap_err();
        assert!(
            err.details
                .iter()
                .any(|d| d.contains("missing BEGIN:VCARD"))
        );
        assert!(err.details.iter().any(|d| d.contains("missing END:VCARD")));

        // The markers are matched ignoring case, as vCard allows.
        let lowercase = write(&dir, "lower.vcf", "begin:vcard\nfn:Ada\nend:vcard\n");
        assert_eq!(
            detect_contacts_format(&lowercase).unwrap(),
            ContactsFormat::Vcf
        );
    }

    /// A CSV needs all three column kinds, and the failure says which one is
    /// absent. Each `if !has_*` guard could be deleted without failing a test,
    /// so the person given the message learned nothing about their file.
    #[test]
    fn a_contacts_csv_needs_a_first_name_a_last_name_and_a_phone() {
        let dir = tempfile::tempdir().unwrap();

        let no_phone = write(&dir, "no-phone.csv", "First Name,Last Name\nAda,Lovelace\n");
        let err = detect_contacts_format(&no_phone).unwrap_err();
        assert!(
            err.details
                .iter()
                .any(|d| d.contains("missing Phone column")),
            "{:?}",
            err.details
        );
        assert!(
            !err.details.iter().any(|d| d.contains("missing First Name")),
            "First Name is present: {:?}",
            err.details
        );

        let no_first = write(
            &dir,
            "no-first.csv",
            "Last Name,Mobile Phone\nLovelace,+15551234567\n",
        );
        let err = detect_contacts_format(&no_first).unwrap_err();
        assert!(err.details.iter().any(|d| d.contains("missing First Name")));
        assert!(!err.details.iter().any(|d| d.contains("missing Last Name")));

        // When a phone column is there, the failure names which ones it found,
        // so the reader can see the header was understood.
        let named = write(
            &dir,
            "named.csv",
            "First Name,Home Phone\nAda,+15551234567\n",
        );
        let err = detect_contacts_format(&named).unwrap_err();
        assert!(
            err.details.iter().any(|d| d.contains("phone columns:")),
            "{:?}",
            err.details
        );
    }
}
