//! Convert an existing Message Crate output directory to another format.

use anyhow::{Context, Result, bail};
use media::{CompressOptions, MediaMode};
pub use message_crate_core::RunResult;
use message_crate_core::{
    ExportReport, ExportTransforms, ExporterConfig, MediaConfig, OutputFormat, document_messages,
    prepare_outputs, stage_conversation_attachments,
};
use message_ir::ConversationDocument;
use message_ir_format::{
    CSV_HEADERS, FormatSink, clean_previous_ir_output, read_conversation_csv,
    read_conversation_eml_dir, read_conversation_json, read_conversation_jsonl,
    read_conversation_mbox,
};
use message_staging::AttachmentSpool;
use sms_backup_restore_exporter::{ReadOptions, SbrArchive, read_backup};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Convert the prior export in `config.inputs[0]` to `config.output_format`.
///
/// # Errors
///
/// Returns an error when the input is missing or ambiguous, the output is or
/// holds the input directory, an artifact cannot be read, or the output cannot be written.
pub fn run(config: &ExporterConfig) -> Result<RunResult> {
    let input = config.require_input().map_err(anyhow::Error::msg)?;
    if config.output.as_os_str().is_empty() {
        bail!("output directory is required");
    }
    let report = convert_export(input, config)?;
    Ok(RunResult::new(report.log_lines(), &report.report))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DetectedExport {
    format: OutputFormat,
}

#[derive(Debug, Default)]
struct ReexportReport {
    detected_format: String,
    /// Conversations written, attachments a convert or compress pass
    /// staged, the media pass, and obfuscation.
    report: ExportReport,
}

impl ReexportReport {
    /// Lines for the run's log.
    fn log_lines(&self) -> Vec<String> {
        let mut lines = vec![
            format!("Detected input format: {}", self.detected_format),
            format!("Conversations: {}", self.report.conversations),
        ];
        if self.report.attachments_saved > 0 {
            lines.push(format!(
                "  saved {} attachments",
                self.report.attachments_saved
            ));
        }
        let missing = self.report.extra("attachments_missing");
        if missing > 0 {
            lines.push(format!("  {missing} attachments missing"));
        }
        lines.extend(self.report.media_lines());
        lines
    }
}

/// Detect the input format, copy attachments if needed, and write the new export.
fn convert_export(input_dir: &Path, config: &ExporterConfig) -> Result<ReexportReport> {
    // The output is cleaned below, so one that is or holds the input is
    // refused before anything is written.
    prepare_outputs(&[input_dir.to_path_buf()], &config.output)?;

    let detected = detect_ir_export(input_dir)?;
    let transforms = ExportTransforms::from_config(config);
    let copy_attachments = transforms.copies_attachments();

    // Conversation files are read before the output is cleaned, so a file
    // the read refuses, such as one of another schema version, stops the
    // run with the previous output left as it was. An SMS backup is read
    // after: its read stages attachments into the output.
    let read_first = if detected.format == OutputFormat::Xml {
        None
    } else {
        Some(read_conversation_files(input_dir, detected.format)?)
    };

    clean_previous_ir_output(&config.output)?;

    if copy_attachments {
        copy_attachments_dir(input_dir, &config.output)?;
    }

    let mut documents = match read_first {
        Some(documents) => documents,
        None => read_sms_backup(input_dir, config, copy_attachments)?,
    };
    if documents.is_empty() {
        bail!("no conversations loaded from {}", input_dir.display());
    }
    let mut report = ExportReport::default();
    // A mail export's reader holds the attachments in memory, and the
    // export has no `attachments/` folder to copy, so its attachments are
    // staged even when the media is only cloned.
    if matches!(transforms.media, MediaMode::Convert | MediaMode::Compress)
        || (copy_attachments && detected.format.is_mail_archive())
    {
        apply_reexport_convert(&mut documents, config, &transforms, &mut report)?;
    }

    report.conversations = documents.len() as u64;
    let mut sink = FormatSink::open(&config.output, config.output_format, transforms)?;
    if config.output_format == OutputFormat::Xml {
        sink = sink.with_archive(Box::new(SbrArchive));
    }
    for document in documents {
        sink.write_document(document)?;
    }
    sink.finish(&mut report)?;

    Ok(ReexportReport {
        detected_format: detected.format.as_str().to_string(),
        report,
    })
}

/// Where one attachment's bytes come from when it is staged again.
enum Source {
    /// Held in memory, as a mail export's reader hands them over.
    Bytes(Vec<u8>),
    /// A file copied into the output from the input's `attachments/`.
    File(PathBuf),
    /// Nothing to stage.
    None,
}

/// Stage the attachments again through the shared step: the files copied
/// from the input, and the bytes a mail export held in memory. A convert or
/// compress pass then rewrites each document's paths, hashes and MIME
/// types. Adds the distinct files written to `report.attachments_saved`
/// and the attachments left without a file to `attachments_missing`.
///
/// An attachment the input already gave a `missing_reason` keeps that
/// reason when there is still nothing to stage, rather than becoming
/// `file_missing`.
fn apply_reexport_convert(
    documents: &mut [ConversationDocument],
    config: &ExporterConfig,
    transforms: &ExportTransforms,
    report: &mut ExportReport,
) -> Result<()> {
    let output_dir = &config.output;
    let mut sources: Vec<Source> = Vec::new();
    let mut reasons: Vec<Option<String>> = Vec::new();
    for att in documents
        .iter_mut()
        .flat_map(|doc| doc.messages.iter_mut())
        .flat_map(|msg| msg.attachments.iter_mut())
    {
        reasons.push(att.missing_reason.clone());
        sources.push(match (att.bytes.take(), att.path.as_deref()) {
            (Some(bytes), _) => Source::Bytes(bytes),
            (None, Some(rel)) => {
                Source::File(output_dir.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR)))
            }
            (None, None) => Source::None,
        });
    }
    let saved = stage_conversation_attachments(
        document_messages(documents),
        &output_dir.join("attachments"),
        &MediaConfig {
            mode: transforms.media,
            compress: transforms.compress.clone(),
        },
        |i| match sources
            .get_mut(i)
            .map(|s| std::mem::replace(s, Source::None))
        {
            Some(Source::Bytes(bytes)) => Ok(Some(bytes)),
            Some(Source::File(path)) => Ok(fs::read(path).ok()),
            Some(Source::None) | None => Ok(None),
        },
        config.log.as_ref(),
        config.progress.as_ref(),
        config.cancel.as_ref(),
    )
    .map_err(anyhow::Error::msg)?;
    report.attachments_saved += saved;

    let mut missing = 0;
    for (att, reason) in documents
        .iter_mut()
        .flat_map(|doc| doc.messages.iter_mut())
        .flat_map(|msg| msg.attachments.iter_mut())
        .zip(reasons)
    {
        if att.missing_reason.as_deref() == Some("file_missing") {
            missing += 1;
            if reason.is_some() {
                att.missing_reason = reason;
            }
        }
    }
    if missing > 0 {
        report.bump("attachments_missing", missing);
    }
    Ok(())
}

/// Read every conversation in an SMS Backup & Restore export, staging its
/// attachments into the output when `copy_attachments` is set.
fn read_sms_backup(
    input_dir: &Path,
    config: &ExporterConfig,
    copy_attachments: bool,
) -> Result<Vec<ConversationDocument>> {
    let attachments_dir = config.output.join("attachments");
    // Each payload goes to disk as its record is read, so the backup's
    // attachments are never all in memory; the spool is removed when
    // the read has staged them.
    let spool = if copy_attachments {
        Some(AttachmentSpool::open(&config.output)?)
    } else {
        None
    };
    let (documents, report) = read_backup(
        input_dir,
        ReadOptions {
            owner_phones: &[],
            attachments_dir: Some(&attachments_dir),
            spool: spool.as_ref(),
            stage_attachments: true,
            media: if copy_attachments {
                MediaMode::Clone
            } else {
                MediaMode::Disabled
            },
            compress: CompressOptions::default(),
            log: None,
            progress: None,
            cancel: config.cancel.as_ref(),
        },
    )?;
    for error in report.errors.iter().take(5) {
        config.emit_log(format!("xml warning: {error}"));
    }
    Ok(documents)
}

/// Read every conversation file or EML folder of `format` in `input_dir`.
/// Any file the read refuses stops the whole read.
fn read_conversation_files(
    input_dir: &Path,
    format: OutputFormat,
) -> Result<Vec<ConversationDocument>> {
    list_artifacts(input_dir, format)?
        .into_iter()
        .map(|path| read_artifact(&path, format))
        .collect()
}

/// Read one conversation file or EML folder in the detected format.
fn read_artifact(path: &Path, format: OutputFormat) -> Result<ConversationDocument> {
    match format {
        OutputFormat::Json => read_conversation_json(path),
        OutputFormat::Jsonl => read_conversation_jsonl(path),
        OutputFormat::Csv => read_conversation_csv(path),
        OutputFormat::Mbox => read_conversation_mbox(path),
        OutputFormat::Eml => read_conversation_eml_dir(path),
        OutputFormat::Xml => unreachable!("XML handled above"),
    }
}

/// Detect a single Message Crate export format in `input_dir`.
fn detect_ir_export(input_dir: &Path) -> Result<DetectedExport> {
    if !input_dir.is_dir() {
        bail!("input is not a directory: {}", input_dir.display());
    }

    let mut present = Vec::new();
    let mut samples = Vec::new();
    for entry in fs::read_dir(input_dir).with_context(|| format!("read {}", input_dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if ignored_artifact(&name) {
            continue;
        }
        let format = if path.is_file() {
            let extension = path
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            match extension.as_str() {
                "xml" if name.eq_ignore_ascii_case("smses.xml") || looks_like_smses(&path) => {
                    Some(OutputFormat::Xml)
                }
                "json" if looks_like_ir_json(&path)? => Some(OutputFormat::Json),
                "jsonl" | "ndjson" if looks_like_ir_jsonl(&path)? => Some(OutputFormat::Jsonl),
                "csv" if looks_like_ir_csv(&path)? => Some(OutputFormat::Csv),
                "mbox" => Some(OutputFormat::Mbox),
                _ => None,
            }
        } else if path.is_dir() && dir_has_eml(&path)? {
            Some(OutputFormat::Eml)
        } else {
            None
        };
        if let Some(format) = format {
            if !present.contains(&format) {
                present.push(format);
            }
            samples.push(if path.is_dir() {
                format!("{name}/")
            } else {
                name
            });
        }
    }
    present.sort_by_key(|format| match format {
        OutputFormat::Xml => 0,
        OutputFormat::Json => 1,
        OutputFormat::Jsonl => 2,
        OutputFormat::Csv => 3,
        OutputFormat::Mbox => 4,
        OutputFormat::Eml => 5,
    });

    match present.as_slice() {
        [format] => Ok(DetectedExport { format: *format }),
        [] => bail!(
            "unsupported input: no Message Crate IR export found in {} \
             (expected smses.xml, *.json, *.jsonl, *.csv, *.mbox, or EML folders)",
            input_dir.display()
        ),
        formats => bail!(
            "unsupported input: mixed formats in {} ({}); found: {}",
            input_dir.display(),
            formats
                .iter()
                .map(|format| format.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            samples.join(", ")
        ),
    }
}

/// List conversation files or EML folders for the detected format.
fn list_artifacts(input_dir: &Path, format: OutputFormat) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(input_dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if ignored_artifact(&name) {
            continue;
        }
        let matches = match format {
            OutputFormat::Xml => {
                path.is_file()
                    && (name.eq_ignore_ascii_case("smses.xml") || looks_like_smses(&path))
            }
            OutputFormat::Json => {
                path.is_file()
                    && path.extension().and_then(|extension| extension.to_str()) == Some("json")
                    && looks_like_ir_json(&path)?
            }
            OutputFormat::Jsonl => {
                path.is_file()
                    && path
                        .extension()
                        .and_then(|extension| extension.to_str())
                        .is_some_and(|extension| extension == "jsonl" || extension == "ndjson")
                    && looks_like_ir_jsonl(&path)?
            }
            OutputFormat::Csv => {
                path.is_file()
                    && path.extension().and_then(|extension| extension.to_str()) == Some("csv")
                    && looks_like_ir_csv(&path)?
            }
            OutputFormat::Mbox => {
                path.is_file()
                    && path.extension().and_then(|extension| extension.to_str()) == Some("mbox")
            }
            OutputFormat::Eml => path.is_dir() && dir_has_eml(&path)?,
        };
        if matches {
            paths.push(path);
        }
    }
    paths.sort();
    if paths.is_empty() {
        bail!(
            "no {} artifacts found in {}",
            format.as_str(),
            input_dir.display()
        );
    }
    Ok(paths)
}

/// True for sidecar files that are not conversation artifacts. `.tmp`
/// covers `.xml.tmp` as well.
fn ignored_artifact(name: &str) -> bool {
    name == "attachments"
        || name.starts_with('.')
        || name.ends_with(".meta.json")
        || name.ends_with(".tmp")
        || name.ends_with(".xml.sbrbody")
}

/// True when the first line of `path` looks like `<smses`.
fn looks_like_smses(path: &Path) -> bool {
    let Ok(file) = File::open(path) else {
        return false;
    };
    let mut first_line = String::new();
    let _ = BufReader::new(file).read_line(&mut first_line);
    first_line.to_ascii_lowercase().contains("<smses")
}

/// True when `path` is a conversation JSON file: an object with a numeric
/// `schema_version` and `export`, `conversation` and `messages` keys.
///
/// The version is not compared, so a file of another version is an export
/// here and its read refuses it by name.
fn looks_like_ir_json(path: &Path) -> Result<bool> {
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let value: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(_) => return Ok(false),
    };
    Ok(has_ir_header(&value) && value.get("messages").is_some())
}

/// True when the first line of `path` is a JSON Lines conversation header:
/// an object with a numeric `schema_version` and `export` and
/// `conversation` keys, and no `messages`.
///
/// The version is not compared, so a file of another version is an export
/// here and its read refuses it by name.
fn looks_like_ir_jsonl(path: &Path) -> Result<bool> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let Some(Ok(first_line)) = BufReader::new(file).lines().next() else {
        return Ok(false);
    };
    let value: serde_json::Value = match serde_json::from_str(&first_line) {
        Ok(value) => value,
        Err(_) => return Ok(false),
    };
    Ok(has_ir_header(&value) && value.get("messages").is_none())
}

/// True when `value` has a numeric `schema_version` and `export` and
/// `conversation` keys, whatever the version.
fn has_ir_header(value: &serde_json::Value) -> bool {
    value
        .get("schema_version")
        .is_some_and(serde_json::Value::is_u64)
        && value.get("export").is_some()
        && value.get("conversation").is_some()
}

/// True when `path` has every column in [`CSV_HEADERS`].
fn looks_like_ir_csv(path: &Path) -> Result<bool> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(BufReader::new(file));
    let Ok(headers) = reader.headers() else {
        return Ok(false);
    };
    let headers: HashSet<&str> = headers.iter().collect();
    Ok(CSV_HEADERS.iter().all(|header| headers.contains(header)))
}

/// True when `dir` contains at least one `.eml` file.
fn dir_has_eml(dir: &Path) -> Result<bool> {
    for entry in fs::read_dir(dir)? {
        if entry?
            .path()
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("eml"))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Copy `input_dir/attachments` into `output_dir/attachments` when present.
fn copy_attachments_dir(input_dir: &Path, output_dir: &Path) -> Result<()> {
    let source = input_dir.join("attachments");
    if !source.is_dir() {
        return Ok(());
    }
    let destination = output_dir.join("attachments");
    fs::create_dir_all(&destination)?;
    copy_dir_recursive(&source, &destination)
}

/// Recursively copy files from `source` into `destination`.
fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<()> {
    for entry in fs::read_dir(source).with_context(|| format!("read {}", source.display()))? {
        let entry = entry?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        if from.is_dir() {
            fs::create_dir_all(&to)?;
            copy_dir_recursive(&from, &to)?;
        } else {
            fs::copy(&from, &to)
                .with_context(|| format!("copy {} → {}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
