//! A vCard (VCF) parser.
//!
//! Message Crate does not read a vCard as an address book: its address book
//! is its own CSV, which the server writes and loads. [`parse_vcf`] is kept
//! for the conversion from a vCard to that CSV (issue #916), and nothing in
//! the workspace calls it until then.

mod vcf;

pub use vcf::{VcfCard, parse_vcf};
