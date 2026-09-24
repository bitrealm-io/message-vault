//! Helpers shared by exporter command-line tools and in-process runners.
//!
//! This module keeps its dependency surface small (only `anyhow` for
//! context-rich path errors) so the desktop app stays lightweight. Callers map
//! `String` errors at the edge when needed.

use crate::config::OutputFormat;
use anyhow::{Context, bail};
use media::MediaReport;
use message_csv::{DateRange, Zone};
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

/// Export run statistics: what was counted while parsing and what the
/// write tail did. Per-exporter extension counters (PDU counts, dedupe
/// counts, etc.) are stored in the `extra` map.
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
    /// The convert or compress pass over the staged attachments.
    pub media: MediaReport,
    /// Documents whose handles, names and bodies were obfuscated.
    pub obfuscated_docs: u64,
    /// Human-readable error/warning lines (capped by each exporter).
    pub errors: Vec<String>,
    /// Per-exporter extension counters keyed by name.
    pub extra: std::collections::BTreeMap<String, u64>,
}

impl ExportReport {
    /// Refuse a run whose media pass failed on every file it tried, when
    /// the mode needed ffmpeg: that is a missing tool, not a bad file.
    ///
    /// # Errors
    ///
    /// Returns an error when `needs_tools` is set, the media pass reported
    /// errors, and it processed nothing.
    pub fn check_media(&self, needs_tools: bool) -> anyhow::Result<()> {
        if needs_tools && !self.media.errors.is_empty() && self.media.processed == 0 {
            bail!("media processing failed for all candidate files");
        }
        Ok(())
    }

    /// Human-readable lines about the write tail: the media pass and
    /// obfuscation. Empty when neither did anything.
    pub fn media_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if self.media.processed > 0 || self.media.skipped > 0 || !self.media.errors.is_empty() {
            lines.push(format!(
                "Media: processed {} file(s), skipped {}",
                self.media.processed, self.media.skipped
            ));
            for err in self.media.errors.iter().take(10) {
                lines.push(format!("  media warning: {err}"));
            }
            if self.media.errors.len() > 10 {
                lines.push(format!("  …and {} more", self.media.errors.len() - 10));
            }
        }
        if self.obfuscated_docs > 0 {
            lines.push(format!(
                "Obfuscated {} conversation(s)",
                self.obfuscated_docs
            ));
        }
        lines
    }

    /// Append the summary lines to `out`: where the export went, then every
    /// count that is not zero.
    pub fn summary_lines(
        &self,
        format: OutputFormat,
        output: &std::path::Path,
        out: &mut Vec<String>,
    ) {
        out.push(format!(
            "Wrote {} export under {}",
            format.as_str(),
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

/// Parse optional start/end date strings into a [`DateRange`] in the host's
/// local zone.
///
/// # Errors
///
/// Returns an error string when a date cannot be parsed.
pub fn parse_date_range(
    start_date: Option<&str>,
    end_date: Option<&str>,
) -> Result<DateRange, String> {
    DateRange::parse_in(Zone::Local, start_date, end_date)
        .map_err(|e| format!("invalid date range: {e}"))
}

/// Parse optional start/end dates in an optional zone (a `UTC±HH:MM` offset
/// or an IANA name; blank is the host's local zone), the iMazing path.
///
/// # Errors
///
/// Returns an error string when a date or the zone cannot be parsed.
pub fn parse_date_range_tz(
    start_date: Option<&str>,
    end_date: Option<&str>,
    timezone: Option<&str>,
) -> Result<DateRange, String> {
    let zone = Zone::parse(timezone).map_err(|e| format!("invalid date range: {e}"))?;
    DateRange::parse_in(zone, start_date, end_date).map_err(|e| format!("invalid date range: {e}"))
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

    fn report_with_media(processed: usize, errors: &[&str]) -> ExportReport {
        ExportReport {
            media: MediaReport {
                processed,
                errors: errors.iter().map(|e| (*e).to_string()).collect(),
                ..MediaReport::default()
            },
            ..ExportReport::default()
        }
    }

    #[test]
    fn check_media_refuses_a_pass_that_failed_on_every_file() {
        let err = report_with_media(0, &["a.heic: ffmpeg not found"])
            .check_media(true)
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "media processing failed for all candidate files"
        );
    }

    #[test]
    fn check_media_accepts_a_pass_that_processed_a_file() {
        report_with_media(1, &["a.heic: bad file"])
            .check_media(true)
            .unwrap();
    }

    #[test]
    fn check_media_accepts_a_pass_with_no_errors() {
        report_with_media(0, &[]).check_media(true).unwrap();
    }

    #[test]
    fn check_media_accepts_failures_when_the_mode_needs_no_tools() {
        report_with_media(0, &["a.heic: ffmpeg not found"])
            .check_media(false)
            .unwrap();
    }

    fn pending_message(sort_key: i64, attachment: bool) -> message_ir::PendingMessage {
        message_ir::PendingMessage {
            sort_key,
            is_from_me: false,
            sender_handle: "+15555550100".to_string(),
            sender_display_name: None,
            text: "hi".to_string(),
            attachments: if attachment {
                vec![message_ir::PendingAttachment {
                    rel_path: "attachments/a.jpg".to_string(),
                    content_type: "image/jpeg".to_string(),
                    extension: "jpg".to_string(),
                    digest_sha256: None,
                    name_hint: None,
                }]
            } else {
                Vec::new()
            },
            extra: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn prune_drops_and_counts_invalid_dates_and_keeps_the_rest() {
        let mut convo = PendingConversation::new("chat", false, None, Vec::new());
        convo.messages = vec![
            pending_message(i64::MAX, true),
            pending_message(1_700_000_000, false),
            pending_message(i64::MAX, false),
        ];
        convo.has_attachments = true;
        let mut report = ExportReport {
            skipped_invalid_date: 5,
            ..ExportReport::default()
        };

        assert!(prune_and_finish_conversation(
            &mut convo,
            &mut report,
            |k| k
        ));
        assert_eq!(report.skipped_invalid_date, 7);
        assert_eq!(convo.messages.len(), 1);
        assert_eq!(convo.messages[0].sort_key, 1_700_000_000);
        // The only attachment went with a dropped message.
        assert!(!convo.has_attachments);
    }

    #[test]
    fn prune_reports_a_conversation_with_no_valid_message_as_empty() {
        let mut convo = PendingConversation::new("chat", false, None, Vec::new());
        convo.messages = vec![pending_message(i64::MAX, false)];
        let mut report = ExportReport::default();

        assert!(!prune_and_finish_conversation(
            &mut convo,
            &mut report,
            |k| k
        ));
        assert_eq!(report.skipped_invalid_date, 1);
        assert!(convo.messages.is_empty());
    }

    #[test]
    fn prune_passes_sort_keys_through_to_secs() {
        // Milliseconds that are valid only once divided by 1000.
        let mut convo = PendingConversation::new("chat", false, None, Vec::new());
        convo.messages = vec![pending_message(i64::MAX / 10, true)];
        let mut report = ExportReport::default();

        assert!(prune_and_finish_conversation(
            &mut convo,
            &mut report,
            |_| 1_700_000_000
        ));
        assert_eq!(report.skipped_invalid_date, 0);
        assert!(convo.has_attachments);
    }

    #[test]
    fn absorb_tally_adds_every_count_to_the_report() {
        let mut report = ExportReport {
            messages: 10,
            sent: 4,
            received: 6,
            ..ExportReport::default()
        };
        report.bump("notifications", 2);

        report.absorb_tally(ProjectionTally {
            messages: 5,
            sent: 2,
            received: 3,
            notifications: 3,
        });

        assert_eq!(report.messages, 15);
        assert_eq!(report.sent, 6);
        assert_eq!(report.received, 9);
        assert_eq!(report.extra("notifications"), 5);
    }

    #[test]
    fn absorb_tally_adds_no_notifications_key_when_there_were_none() {
        let mut report = ExportReport::default();
        report.absorb_tally(ProjectionTally {
            messages: 1,
            sent: 1,
            received: 0,
            notifications: 0,
        });
        assert!(report.extra.is_empty());
    }

    #[test]
    fn bump_adds_to_a_counter_and_extra_reads_zero_when_unset() {
        let mut report = ExportReport::default();
        assert_eq!(report.extra("pdu"), 0);
        report.bump("pdu", 3);
        report.bump("pdu", 4);
        assert_eq!(report.extra("pdu"), 7);
    }

    #[test]
    fn discover_files_missing_root_errors() {
        let err = discover_files(Path::new("/no/such/dir"), &|_| true).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    /// Cleaning the output before a write would delete the backup being read.
    #[test]
    fn prepare_outputs_refuses_an_output_that_is_or_contains_an_input() {
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("backup");
        std::fs::create_dir_all(&input).unwrap();
        let inputs = [input.clone()];

        for output in [input.clone(), tmp.path().to_path_buf()] {
            let err = prepare_outputs(&inputs, &output).unwrap_err().to_string();
            assert!(
                err.contains("must not be the same as, or contain, the input"),
                "{}: {err}",
                output.display()
            );
        }

        let output = tmp.path().join("export");
        let (resolved, out) = prepare_outputs(&inputs, &output).unwrap();
        assert!(output.is_dir(), "the output folder is created");
        assert_eq!(out, std::fs::canonicalize(&output).unwrap());
        assert_eq!(resolved, [std::fs::canonicalize(&input).unwrap()]);
    }
}
