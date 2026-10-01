//! Read message-ir JSONL files (one JSON object per line) into import records.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{Context, Result};

use crate::models::{self, ExportRecord};

/// Read a message-ir JSON Lines conversation file (one JSON object per line)
/// into import records.
///
/// Parses line-by-line so the full file is not held as a second string buffer
/// before deserialization (records still accumulate in memory).
///
/// # Errors
///
/// Returns an error when the file cannot be opened, a line cannot be read, or
/// a line is not valid message-ir JSON.
pub fn read_records(path: &Path) -> Result<Vec<ExportRecord>> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut lines = Vec::new();
    for (line_no, line) in reader.lines().enumerate() {
        let line = line.with_context(|| {
            format!("failed to read line {} of {}", line_no + 1, path.display())
        })?;
        // Drop empty lines early so the parse buffer stays smaller.
        if !line.trim().is_empty() {
            lines.push(line);
        }
    }
    models::parse_ir_lines(lines)
        .with_context(|| format!("failed to parse message-ir JSONL in {}", path.display()))
}
