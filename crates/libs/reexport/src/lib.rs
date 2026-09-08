//! Convert an existing Message Vault output directory to another format.

use anyhow::{Context, Result, bail};
use media::{CompressOptions, MediaMode};
use message_ir::ConversationDocument;
use message_ir_format::{
    CSV_HEADERS, ExportTransforms, FormatSink, FormatSinkResult, SbrReadOptions,
    clean_previous_ir_output, read_conversation_csv, read_conversation_eml_dir,
    read_conversation_json, read_conversation_jsonl, read_conversation_mbox, read_sbr_documents,
};
pub use message_vault_io_core::RunResult;
use message_vault_io_core::{
    AttachmentJob, ExporterConfig, MediaConfig, OutputFormat, run_attachment_jobs,
};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Convert the prior export in `config.inputs[0]` to `config.output_format`.
///
/// # Errors
///
/// Returns an error when the input is missing or ambiguous, the output is the
/// input directory, an artifact cannot be read, or the output cannot be written.
pub fn run(config: &ExporterConfig) -> Result<RunResult> {
    let input = config.require_input().map_err(anyhow::Error::msg)?;
    if config.output.as_os_str().is_empty() {
        bail!("output directory is required");
    }
    let report = convert_export(input, config)?;
    Ok(RunResult {
        messages: report.log_lines(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DetectedExport {
    format: OutputFormat,
}

#[derive(Debug, Default)]
struct ReexportReport {
    detected_format: String,
    conversations: usize,
    sink: FormatSinkResult,
}

impl ReexportReport {
    /// Lines for CLI / GUI logs.
    fn log_lines(&self) -> Vec<String> {
        let mut lines = vec![
            format!("Detected input format: {}", self.detected_format),
            format!("Conversations: {}", self.conversations),
        ];
        lines.extend(self.sink.log_lines());
        lines
    }
}

/// Detect the input format, copy attachments if needed, and write the new export.
fn convert_export(input_dir: &Path, config: &ExporterConfig) -> Result<ReexportReport> {
    let input_canon = fs::canonicalize(input_dir)
        .with_context(|| format!("canonicalize input {}", input_dir.display()))?;
    if config.output.exists() {
        let output_canon = fs::canonicalize(&config.output)
            .with_context(|| format!("canonicalize output {}", config.output.display()))?;
        if input_canon == output_canon {
            bail!("input and output directories must be different");
        }
    }

    let detected = detect_ir_export(input_dir)?;
    let transforms = ExportTransforms::from_config(config);
    let copy_attachments = transforms.copies_attachments();

    fs::create_dir_all(&config.output)
        .with_context(|| format!("create {}", config.output.display()))?;
    clean_previous_ir_output(&config.output)?;

    if copy_attachments {
        copy_attachments_dir(input_dir, &config.output)?;
    }

    let mut documents = load_documents(input_dir, detected, config, copy_attachments)?;
    if documents.is_empty() {
        bail!("no conversations loaded from {}", input_dir.display());
    }
    if matches!(transforms.media, MediaMode::Convert | MediaMode::Compress) {
        apply_reexport_convert(&mut documents, &config.output, &transforms)?;
    }

    let conversations = documents.len();
    let mut sink = FormatSink::open(&config.output, config.output_format, transforms)?;
    for document in documents {
        sink.write_document(document)?;
    }
    let sink = sink.finish()?;

    Ok(ReexportReport {
        detected_format: detected.format.as_str().to_string(),
        conversations,
        sink,
    })
}

/// Transcode copied attachments and update document paths, hashes, and MIME.
fn apply_reexport_convert(
    documents: &mut [ConversationDocument],
    output_dir: &Path,
    transforms: &ExportTransforms,
) -> Result<()> {
    let attachments_dir = output_dir.join("attachments");
    let mut jobs = Vec::new();
    for doc in documents.iter_mut() {
        for msg in &mut doc.messages {
            let ts = msg.timestamp_unix_ms;
            for att in &mut msg.attachments {
                let size_hint = att.size_bytes;
                jobs.push(AttachmentJob {
                    attachment: att,
                    timestamp_unix_ms: ts,
                    size_hint,
                });
            }
        }
    }
    let sources: Vec<Option<PathBuf>> = jobs
        .iter()
        .map(|job| {
            job.attachment
                .path
                .as_ref()
                .map(|rel| output_dir.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR)))
        })
        .collect();
    run_attachment_jobs(
        &mut jobs,
        &attachments_dir,
        &MediaConfig {
            mode: transforms.media,
            compress: transforms.compress.clone(),
        },
        |i| {
            let Some(path) = sources.get(i).and_then(|p| p.as_ref()) else {
                return Ok(None);
            };
            fs::read(path).map(Some).or(Ok(None))
        },
        |_| {},
        None,
        None,
    )
    .map_err(anyhow::Error::msg)?;
    Ok(())
}

/// Load every conversation document from a detected export directory.
fn load_documents(
    input_dir: &Path,
    detected: DetectedExport,
    config: &ExporterConfig,
    copy_attachments: bool,
) -> Result<Vec<ConversationDocument>> {
    if detected.format == OutputFormat::Xml {
        let attachments_dir = config.output.join("attachments");
        let (documents, report) = read_sbr_documents(
            input_dir,
            SbrReadOptions {
                owner_phones: &[],
                attachments_dir: Some(&attachments_dir),
                copy_attachments,
                keep_attachment_bytes: false,
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
        return Ok(documents);
    }

    list_artifacts(input_dir, detected.format)?
        .into_iter()
        .map(|path| read_artifact(&path, detected.format))
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

/// Detect a single Message Vault export format in `input_dir`.
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
            "unsupported input: no Message Vault IR export found in {} \
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

/// True for sidecar files that are not conversation artifacts.
fn ignored_artifact(name: &str) -> bool {
    name == "attachments"
        || name.starts_with('.')
        || name.ends_with(".meta.json")
        || name.ends_with(".tmp")
        || name.ends_with(".xml.tmp")
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

/// True when `path` is a schema-version-3 conversation JSON file.
fn looks_like_ir_json(path: &Path) -> Result<bool> {
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let value: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(_) => return Ok(false),
    };
    Ok(value.get("schema_version").and_then(|value| value.as_u64())
        == Some(message_ir::SCHEMA_VERSION as u64)
        && value.get("export").is_some()
        && value.get("conversation").is_some()
        && value.get("messages").is_some())
}

/// True when `path` is a schema-version-3 JSON Lines conversation file.
fn looks_like_ir_jsonl(path: &Path) -> Result<bool> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let Some(Ok(first_line)) = BufReader::new(file).lines().next() else {
        return Ok(false);
    };
    let value: serde_json::Value = match serde_json::from_str(&first_line) {
        Ok(value) => value,
        Err(_) => return Ok(false),
    };
    Ok(value.get("schema_version").and_then(|value| value.as_u64())
        == Some(message_ir::SCHEMA_VERSION as u64)
        && value.get("export").is_some()
        && value.get("conversation").is_some()
        && value.get("messages").is_none())
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
