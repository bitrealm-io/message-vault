//! Contact books and shared VCF / vCard-CSV parsers.
//!
//! A VCF file is a vCard address book. A vCard CSV is the same data exported
//! as a spreadsheet (First Name, Last Name, phone columns).
//!
//! - [`parse_vcf`] / [`read_vcard_csv_rows`] — parse APIs used by the server's import
//!   and by backup converters
//! - [`ContactsBook`] — name and phone indexes used when a converter looks up
//!   display names
//!
//! Accepted inputs: VCF, or vCard CSV.

mod book;
mod format;
mod name;
mod vcard_csv;
mod vcf;

pub use book::{ContactsBook, resolve_contacts_cli};
pub use format::{ContactsFormat, ContactsInputError, detect_contacts_format};
pub use vcard_csv::{ContactCsvRow, read_vcard_csv_rows};
pub use vcf::{VcfCard, extract_tags, parse_vcf, strip_tags};
