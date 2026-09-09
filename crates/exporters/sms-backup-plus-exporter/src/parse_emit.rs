//! Parse pipeline: discover `.eml` inputs and turn each file into
//! [`ParsedMessage`]s in parallel chunks.

use crate::emit::is_eml_file;
use crate::flat_eml::{MailHeaders, is_flat_sms_eml, parse_flat_eml_mail};
use crate::types::ParsedMessage;
use anyhow::{Result, bail};
use message_vault_io_core::CancelFlag;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Collect `.eml` paths from files and directories, skipping `duplicate` /
/// `exclude` / `.git` folders.
///
/// # Errors
///
/// Returns an error when an input is neither a file nor a directory, a file is
/// not `.eml`, no `.eml` files are found, or the user cancels.
pub(super) fn collect_eml_paths<P: AsRef<Path>>(
    inputs: &[P],
    cancel: Option<&CancelFlag>,
) -> Result<Vec<PathBuf>> {
    if inputs.is_empty() {
        bail!("at least one --input path is required");
    }

    let mut paths = Vec::new();
    for input in inputs {
        message_vault_io_core::check_cancel(cancel)?;
        let input = input.as_ref();
        if input.is_file() {
            if is_eml_file(input) {
                paths.push(input.to_path_buf());
            } else {
                bail!("input file is not .eml: {}", input.display());
            }
            continue;
        }
        if !input.is_dir() {
            bail!("input is not a file or directory: {}", input.display());
        }
        let mut found = message_vault_io_core::discover_files(input, &is_eml_file)?;
        found.retain(|p| !in_skipped_dir(p));
        paths.extend(found);
    }

    // Stable order for deterministic CSV dedupe winners when timestamps tie.
    paths.sort();
    paths.dedup();
    if paths.is_empty() {
        let listed = inputs
            .iter()
            .map(|p| p.as_ref().display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        bail!("no .eml files under: {listed}");
    }
    Ok(paths)
}

/// True for paths under folders the walk never enters (`duplicate`,
/// `exclude`, `.git`).
fn in_skipped_dir(path: &Path) -> bool {
    path.components().any(|c| {
        matches!(
            c.as_os_str()
                .to_str()
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("duplicate" | "exclude" | ".git")
        )
    })
}

/// Per-file parse result produced in parallel; merged serially into conversations.
pub(super) enum ParsedEmlKind {
    Flat {
        msg: Box<ParsedMessage>,
    },
    FlatNone,
    NotSms,
    IoError(String),
    ParseError(String),
    /// Cooperative cancel observed at the start of a parallel worker.
    Cancelled,
}

/// Read one EML: a single SMS Backup+ message, or something to skip.
pub(super) fn parse_one_eml(
    eml_path: &Path,
    rel_path: String,
    owner_digits: &HashSet<String>,
    owner_emails_lc: &[String],
) -> ParsedEmlKind {
    let bytes = match std::fs::read(eml_path) {
        Ok(b) => b,
        Err(err) => {
            return ParsedEmlKind::IoError(format!("{}: {err}", eml_path.display()));
        }
    };
    let mail = match mailparse::parse_mail(&bytes) {
        Ok(m) => m,
        Err(err) => {
            return ParsedEmlKind::ParseError(format!("{}: parse EML: {err}", eml_path.display()));
        }
    };
    let headers = MailHeaders::from_mail(&mail);

    if is_flat_sms_eml(&headers) {
        match parse_flat_eml_mail(eml_path, &mail, &headers, owner_digits, owner_emails_lc) {
            Some(mut msg) => {
                msg.eml_path = rel_path;
                ParsedEmlKind::Flat { msg: Box::new(msg) }
            }
            None => ParsedEmlKind::FlatNone,
        }
    } else {
        ParsedEmlKind::NotSms
    }
}
