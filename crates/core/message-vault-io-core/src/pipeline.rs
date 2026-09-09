//! Helpers shared by exporter command-line tools and in-process runners.
//!
//! This module keeps its dependency surface small (only `anyhow` for
//! context-rich path errors) so the desktop app stays lightweight. Callers map
//! `String` errors at the edge when needed.

use anyhow::{Context, bail};
use message_csv::DateRange;
use message_ir::{
    ConversationDocument, PendingConversation, ProjectionHooks, ProjectionTally,
    pending_to_document, prepare_conversation,
};
use std::fs;
use std::path::{Path, PathBuf};

/// Recursively walk `root`, collecting files that match `predicate`.
/// Skips symlinks (both files and directories). Directories are
/// traversed depth-first with no explicit depth limit (callers
/// should use this on trusted local input trees).
///
/// # Errors
///
/// Returns an I/O error when `root` cannot be read.
pub fn discover_files(
    root: &Path,
    predicate: &dyn Fn(&Path) -> bool,
) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut out = Vec::new();
    discover_files_into(root, predicate, &mut out)?;
    Ok(out)
}

/// Append matching files under `dir` onto `out`.
///
/// # Errors
///
/// Returns an I/O error when a directory cannot be read.
fn discover_files_into(
    dir: &Path,
    predicate: &dyn Fn(&Path) -> bool,
    out: &mut Vec<PathBuf>,
) -> Result<(), std::io::Error> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        if ft.is_symlink() {
            continue;
        }
        let path = entry.path();
        if ft.is_dir() {
            discover_files_into(&path, predicate, out)?;
        } else if ft.is_file() && predicate(&path) {
            out.push(path);
        }
    }
    Ok(())
}

/// Result of a successful exporter `run`: human-readable log lines.
#[derive(Debug, Default)]
pub struct RunResult {
    /// Human-readable log lines (summary lines plus mid-run notes).
    pub messages: Vec<String>,
}

/// Export run statistics. Per-exporter extension counters (PDU counts,
/// dedupe counts, etc.) are stored in the `extra` map.
#[derive(Debug, Default, Clone)]
pub struct ExportReport {
    /// Conversations exported.
    pub conversations: u64,
    /// Conversations a resumed run found already written and did not write
    /// again. Counted in `conversations` as well, because they are part of
    /// the export; this says how much of it this run did not have to do.
    pub conversations_skipped: u64,
    /// Messages exported.
    pub messages: u64,
    /// Outgoing messages exported.
    pub sent: u64,
    /// Incoming messages exported.
    pub received: u64,
    /// Rows skipped because their date could not be parsed.
    pub skipped_invalid_date: u64,
    /// Rows skipped because they fell outside the date range.
    pub skipped_out_of_range: u64,
    /// Duplicate rows dropped during dedupe.
    pub duplicates_dropped: u64,
    /// Attachment files saved to the output.
    pub attachments_saved: u64,
    /// Human-readable error/warning lines (capped by each exporter).
    pub errors: Vec<String>,
    /// Per-exporter extension counters keyed by name.
    pub extra: std::collections::BTreeMap<String, u64>,
}

impl ExportReport {
    /// Append one or more summary lines to `out`.
    pub fn summary_lines(&self, output: &std::path::Path, out: &mut Vec<String>) {
        out.push(format!(
            "Wrote {} export under {}",
            crate::name_stem(output.to_string_lossy().as_ref()),
            output.display()
        ));
        if self.conversations_skipped > 0 {
            out.push(format!(
                "  resumed: {} conversation(s) were already written",
                self.conversations_skipped
            ));
        }
        if self.skipped_invalid_date > 0 {
            out.push(format!(
                "  skipped {} invalid-date rows",
                self.skipped_invalid_date
            ));
        }
        if self.skipped_out_of_range > 0 {
            out.push(format!(
                "  skipped {} out-of-range rows",
                self.skipped_out_of_range
            ));
        }
        if self.duplicates_dropped > 0 {
            out.push(format!(
                "  dropped {} duplicate rows",
                self.duplicates_dropped
            ));
        }
        if self.attachments_saved > 0 {
            out.push(format!("  saved {} attachments", self.attachments_saved));
        }
        for (key, count) in &self.extra {
            out.push(format!("  {key}: {count}"));
        }
        for err in &self.errors {
            out.push(format!("  error: {err}"));
        }
    }

    /// Bump a per-exporter extension counter in the `extra` map.
    pub fn bump(&mut self, key: &str, by: u64) {
        *self.extra.entry(key.to_string()).or_insert(0) += by;
    }

    /// Read a per-exporter extension counter from the `extra` map (0 when unset).
    pub fn extra(&self, key: &str) -> u64 {
        self.extra.get(key).copied().unwrap_or(0)
    }

    /// Fold the counts from one projected conversation into this report.
    pub fn absorb_tally(&mut self, tally: ProjectionTally) {
        self.messages += tally.messages;
        self.sent += tally.sent;
        self.received += tally.received;
        if tally.notifications > 0 {
            self.bump("notifications", tally.notifications);
        }
    }
}

/// Turn one pending conversation into a document, or drop it.
///
/// This is the step every exporter runs after parsing: sort the messages the
/// way the exporter's hooks say, drop rows whose timestamp cannot be
/// represented, project the rest with [`pending_to_document`], and fold the
/// counts into `report`. Returns `None` when no message survives.
pub fn project_conversation<H: ProjectionHooks + ?Sized>(
    chat_id: &str,
    convo: &mut PendingConversation,
    hooks: &H,
    report: &mut ExportReport,
) -> Option<ConversationDocument> {
    let unit = hooks.sort_key_unit();
    let (keep, skipped) = prepare_conversation(
        convo,
        |a, b| hooks.message_order(a, b),
        |key| unit.to_secs(key),
    );
    report.skipped_invalid_date += skipped;
    if !keep {
        return None;
    }
    let (doc, tally) = pending_to_document(chat_id, convo, hooks);
    report.absorb_tally(tally);
    Some(doc)
}

/// Print `RunResult` lines with the standard stdout/stderr split:
/// media/obfuscate/warning lines → stderr, summary lines → stdout.
pub fn print_result(result: &RunResult) {
    for line in &result.messages {
        if line.starts_with("Media:")
            || line.starts_with("  media ")
            || line.starts_with("Obfuscated ")
            || line.starts_with("warning:")
        {
            eprintln!("{line}");
        } else {
            println!("{line}");
        }
    }
}

/// Parse optional start/end date strings into a [`DateRange`].
///
/// # Errors
///
/// Returns an error string when a date cannot be parsed.
pub fn parse_date_range(
    start_date: Option<&str>,
    end_date: Option<&str>,
) -> Result<DateRange, String> {
    DateRange::parse(start_date, end_date).map_err(|e| format!("invalid date range: {e}"))
}

/// Parse optional start/end dates with an optional timezone name (iMazing path).
///
/// # Errors
///
/// Returns an error string when a date or timezone cannot be parsed.
pub fn parse_date_range_tz(
    start_date: Option<&str>,
    end_date: Option<&str>,
    timezone: Option<&str>,
) -> Result<DateRange, String> {
    DateRange::parse_optional_tz(start_date, end_date, timezone)
        .map_err(|e| format!("invalid date range: {e}"))
}

/// Filesystem-safe stem from a display name or handle (alnum / `-` / `_` / `+`).
pub fn name_stem(value: &str) -> String {
    let mut raw = String::with_capacity(value.len());
    for c in value.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '+' {
            raw.push(c);
        } else {
            raw.push('_');
        }
    }
    if raw.is_empty() || raw.chars().all(|c| c == '_') {
        "unknown".to_string()
    } else {
        raw
    }
}

/// Create and canonicalize the output directory, canonicalize every input,
/// and bail when the output is the same as, or contains, an input.
///
/// Returns the canonicalized `(inputs, output)` paths.
///
/// # Errors
///
/// Returns an error when the output directory cannot be created, a path
/// cannot be resolved, or the output overlaps an input.
pub fn prepare_outputs(
    inputs: &[std::path::PathBuf],
    output: &std::path::Path,
) -> anyhow::Result<(Vec<std::path::PathBuf>, std::path::PathBuf)> {
    fs::create_dir_all(output).with_context(|| format!("create {}", output.display()))?;
    let output =
        fs::canonicalize(output).with_context(|| format!("resolve {}", output.display()))?;
    let mut resolved = Vec::with_capacity(inputs.len());
    for input in inputs {
        let input =
            fs::canonicalize(input).with_context(|| format!("resolve {}", input.display()))?;
        if output == input || input.starts_with(&output) {
            bail!(
                "output {} must not be the same as, or contain, the input {}",
                output.display(),
                input.display()
            );
        }
        resolved.push(input);
    }
    Ok((resolved, output))
}

/// Drop messages with unrepresentable timestamps and finalize a pending
/// conversation. Returns whether any message remains.
///
/// `to_secs` converts a message sort key to Unix seconds (exporters that
/// store milliseconds pass `|k| k / 1000`).
pub fn prune_and_finish_conversation(
    convo: &mut PendingConversation,
    report: &mut ExportReport,
    to_secs: impl Fn(i64) -> i64,
) -> bool {
    convo.messages.retain(|m| {
        if message_csv::format_local_ts(to_secs(m.sort_key)).is_some() {
            true
        } else {
            report.skipped_invalid_date += 1;
            false
        }
    });
    convo.has_attachments = convo.messages.iter().any(|m| !m.attachments.is_empty());
    !convo.messages.is_empty()
}

/// Standard export metadata: source / tool / version plus the owner identity.
pub fn export_meta(
    source: &str,
    tool: &str,
    tool_version: &str,
    owner_handle: Option<String>,
    owner_display_name: Option<String>,
) -> message_ir::ExportMeta {
    message_ir::ExportMeta {
        source: source.to_string(),
        tool: tool.to_string(),
        tool_version: tool_version.to_string(),
        owner_handle,
        owner_display_name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_stem_sanitizes() {
        assert_eq!(name_stem("Alice Bob"), "Alice_Bob");
        assert_eq!(name_stem("+15555550100"), "+15555550100");
        assert_eq!(name_stem("!!!"), "unknown");
        assert_eq!(name_stem(""), "unknown");
    }

    #[test]
    fn parse_date_range_rejects_bad() {
        let err = parse_date_range(Some("not-a-date"), None).unwrap_err();
        assert!(err.starts_with("invalid date range:"));
    }

    #[test]
    fn discover_files_walks_and_filters() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("a.xml"), b"<x/>").unwrap();
        std::fs::write(root.join("b.txt"), b"x").unwrap();
        std::fs::write(root.join("sub").join("c.xml"), b"<x/>").unwrap();
        std::fs::write(root.join("sub").join("d.eml"), b"").unwrap();
        let files = discover_files(root, &|p| {
            p.extension().and_then(|e| e.to_str()) == Some("xml")
        })
        .unwrap();
        let mut names: Vec<PathBuf> = files
            .iter()
            .map(|p| p.strip_prefix(root).unwrap().to_path_buf())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec![PathBuf::from("a.xml"), PathBuf::from("sub").join("c.xml")]
        );
    }

    #[test]
    fn discover_files_missing_root_errors() {
        let err = discover_files(Path::new("/no/such/dir"), &|_| true).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }
}
