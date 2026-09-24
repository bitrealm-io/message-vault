//! Parse iMazing Messages and WhatsApp CSV exports.

use anyhow::{Context, Result, bail};
use message_csv::{col, field};
use message_vault_io_core::discover_files;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Which iMazing export tree a CSV came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceKind {
    Messages,
    WhatsApp,
}

impl SourceKind {
    /// The source kind as written into the vendor bag.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Messages => "messages",
            Self::WhatsApp => "whatsapp",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DiscoveredCsv {
    pub path: PathBuf,
    pub kind: SourceKind,
}

#[derive(Debug, Clone)]
pub(crate) struct RawRow {
    pub chat_session: String,
    pub message_date: String,
    pub delivered_date: String,
    pub read_date: String,
    pub edited_date: String,
    pub deleted_date: String,
    pub sent_date: String,
    pub service: String,
    pub msg_type: String,
    pub sender_id: String,
    pub sender_name: String,
    pub status: String,
    pub forwarded: String,
    pub replying_to: String,
    pub subject: String,
    pub text: String,
    pub reactions: String,
    pub attachment: String,
    pub attachment_type: String,
    pub attachment_info: String,
}

/// Discover iMazing Messages or WhatsApp CSV files under `input` (file or directory tree).
pub(crate) fn discover_csv_files(input: &Path) -> Result<Vec<DiscoveredCsv>> {
    if input.is_file() {
        return match classify_imazing_csv(input)? {
            Some(kind) => Ok(vec![DiscoveredCsv {
                path: input.to_path_buf(),
                kind,
            }]),
            None => bail!(
                "{} is not an iMazing Messages or WhatsApp CSV \
                 (need Chat Session + Message Date + Sender ID)",
                input.display()
            ),
        };
    }
    if !input.is_dir() {
        bail!("input path not found: {}", input.display());
    }

    let mut files = Vec::new();
    let paths = discover_files(input, &|p| {
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
            return false;
        };
        let lower = name.to_ascii_lowercase();
        lower.ends_with(".csv")
            // Skip Contacts exports and attachment sidecars by name.
            && !lower.starts_with("contacts")
            && !lower.contains("attachment")
    })
    .with_context(|| format!("read_dir {}", input.display()))?;
    for path in paths {
        if let Some(kind) = classify_imazing_csv(&path)? {
            files.push(DiscoveredCsv { path, kind });
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    if files.is_empty() {
        bail!(
            "no iMazing Messages or WhatsApp CSV files found under {}",
            input.display()
        );
    }
    Ok(files)
}

/// Sniff the CSV header to tell a Messages export from a WhatsApp export, or `None` for neither.
fn classify_imazing_csv(path: &Path) -> Result<Option<SourceKind>> {
    let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut buf = vec![0u8; 8192];
    let n = file.read(&mut buf)?;
    let text = String::from_utf8_lossy(&buf[..n]);
    let header = text
        .lines()
        .next()
        .unwrap_or("")
        .trim_start_matches('\u{feff}');
    let lower = header.to_ascii_lowercase();
    if !(lower.contains("chat session")
        && lower.contains("message date")
        && lower.contains("sender id"))
    {
        return Ok(None);
    }
    // WhatsApp exports omit Service and include Forwarded / Attachment info / Sent Date.
    // Messages exports include Service (SMS/iMessage) and Apple date columns.
    if lower.contains("service") {
        return Ok(Some(SourceKind::Messages));
    }
    if lower.contains("forwarded")
        || lower.contains("attachment info")
        || lower.contains("sent date")
    {
        return Ok(Some(SourceKind::WhatsApp));
    }
    // Fallback: treat as Messages if it has the shared core columns.
    Ok(Some(SourceKind::Messages))
}

/// Read every row of one iMazing CSV, stripping a UTF-8 BOM first.
pub(crate) fn parse_csv_file(path: &Path, kind: SourceKind) -> Result<Vec<RawRow>> {
    let (mut rdr, headers) = message_csv::open_csv_lowercase(path)?;

    let chat_i = col(&headers, "chat session")?;
    let date_i = col(&headers, "message date")?;
    let type_i = col(&headers, "type")?;
    let sender_id_i = col(&headers, "sender id")?;
    let sender_name_i = col(&headers, "sender name")?;
    let text_i = col(&headers, "text")?;
    let service_i = headers.iter().position(|h| h == "service");
    let subject_i = headers.iter().position(|h| h == "subject");
    let reactions_i = headers.iter().position(|h| h == "reactions");
    let status_i = headers.iter().position(|h| h == "status");
    let att_i = headers.iter().position(|h| h == "attachment");
    let att_type_i = headers.iter().position(|h| h == "attachment type");
    let att_info_i = headers.iter().position(|h| h == "attachment info");
    let forwarded_i = headers.iter().position(|h| h == "forwarded");
    let replying_i = headers.iter().position(|h| h == "replying to");
    let delivered_i = headers.iter().position(|h| h == "delivered date");
    let read_i = headers.iter().position(|h| h == "read date");
    let edited_i = headers.iter().position(|h| h == "edited date");
    let deleted_i = headers.iter().position(|h| h == "deleted date");
    let sent_i = headers.iter().position(|h| h == "sent date");

    let mut rows = Vec::new();
    for (idx, rec) in rdr.records().enumerate() {
        let rec = rec.with_context(|| format!("row {} in {}", idx + 2, path.display()))?;
        let chat_session = field(&rec, chat_i);
        let message_date = field(&rec, date_i);
        let text = field(&rec, text_i);
        let attachment = att_i.map(|i| field(&rec, i)).unwrap_or_default();
        if chat_session.is_empty()
            && message_date.is_empty()
            && text.is_empty()
            && attachment.is_empty()
        {
            continue;
        }
        let service = match kind {
            SourceKind::WhatsApp => "WhatsApp".to_string(),
            SourceKind::Messages => service_i.map(|i| field(&rec, i)).unwrap_or_default(),
        };
        rows.push(RawRow {
            chat_session,
            message_date,
            delivered_date: delivered_i.map(|i| field(&rec, i)).unwrap_or_default(),
            read_date: read_i.map(|i| field(&rec, i)).unwrap_or_default(),
            edited_date: edited_i.map(|i| field(&rec, i)).unwrap_or_default(),
            deleted_date: deleted_i.map(|i| field(&rec, i)).unwrap_or_default(),
            sent_date: sent_i.map(|i| field(&rec, i)).unwrap_or_default(),
            service,
            msg_type: field(&rec, type_i),
            sender_id: field(&rec, sender_id_i),
            sender_name: field(&rec, sender_name_i),
            status: status_i.map(|i| field(&rec, i)).unwrap_or_default(),
            forwarded: forwarded_i.map(|i| field(&rec, i)).unwrap_or_default(),
            replying_to: replying_i.map(|i| field(&rec, i)).unwrap_or_default(),
            subject: subject_i.map(|i| field(&rec, i)).unwrap_or_default(),
            text,
            reactions: reactions_i.map(|i| field(&rec, i)).unwrap_or_default(),
            attachment,
            attachment_type: att_type_i.map(|i| field(&rec, i)).unwrap_or_default(),
            attachment_info: att_info_i.map(|i| field(&rec, i)).unwrap_or_default(),
        });
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write(dir: &tempfile::TempDir, rel: &str, body: &str) -> PathBuf {
        let path = dir.path().join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let mut f = File::create(&path).unwrap();
        write!(f, "{body}").unwrap();
        path
    }

    #[test]
    fn classifies_messages_and_whatsapp_headers() {
        let dir = tempfile::tempdir().unwrap();
        let messages = write(
            &dir,
            "m.csv",
            "Chat Session,Message Date,Service,Type,Sender ID,Sender Name,Text\n",
        );
        let whatsapp = write(
            &dir,
            "w.csv",
            "Chat Session,Message Date,Sent Date,Type,Sender ID,Sender Name,Status,Forwarded,Text,Attachment info\n",
        );
        assert_eq!(
            classify_imazing_csv(&messages).unwrap(),
            Some(SourceKind::Messages)
        );
        assert_eq!(
            classify_imazing_csv(&whatsapp).unwrap(),
            Some(SourceKind::WhatsApp)
        );
    }

    /// An iMazing export needs all three core columns; any other CSV found
    /// in the tree is not one. Among exports, any one of WhatsApp's own
    /// columns marks a WhatsApp export, since iMazing versions differ in
    /// which of them they write; with none of them, it is Messages.
    #[test]
    fn a_header_is_classified_by_its_core_and_whatsapp_columns() {
        let dir = tempfile::tempdir().unwrap();
        let classify = |header: &str| {
            let path = write(&dir, "x.csv", &format!("{header}\n"));
            classify_imazing_csv(&path).unwrap()
        };
        for missing_one in [
            "Message Date,Type,Sender ID,Text",
            "Chat Session,Type,Sender ID,Text",
            "Chat Session,Message Date,Type,Text",
        ] {
            assert_eq!(classify(missing_one), None, "{missing_one}");
        }
        for whatsapp_column in ["Forwarded", "Attachment info", "Sent Date"] {
            assert_eq!(
                classify(&format!(
                    "Chat Session,Message Date,Type,Sender ID,Text,{whatsapp_column}"
                )),
                Some(SourceKind::WhatsApp),
                "{whatsapp_column}"
            );
        }
        assert_eq!(
            classify("Chat Session,Message Date,Type,Sender ID,Text"),
            Some(SourceKind::Messages)
        );
    }

    #[test]
    fn discovers_nested_messages_and_whatsapp() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir,
            "Contacts/All/Contacts - x.csv",
            "First Name,Mobile Phone\nAda,+15555550100\n",
        );
        write(
            &dir,
            "Messages/2020-01-01 12 00 00 - Bob/Messages - Bob.csv",
            "Chat Session,Message Date,Service,Type,Sender ID,Sender Name,Text\n\
Bob,2020-01-01 12:00:00,SMS,Incoming,+15555550111,Bob,Hi\n",
        );
        write(
            &dir,
            "WhatsApp/2020-01-01 12 00 00 - Bob/WhatsApp - Bob.csv",
            "Chat Session,Message Date,Sent Date,Type,Sender ID,Sender Name,Status,Forwarded,Text,Attachment info\n\
Bob,2020-01-01 12:00:00,,Incoming,+15555550111,Bob,Read,,Hi,\n",
        );
        let found = discover_csv_files(dir.path()).unwrap();
        assert_eq!(found.len(), 2);
        assert!(found.iter().any(|f| f.kind == SourceKind::Messages));
        assert!(found.iter().any(|f| f.kind == SourceKind::WhatsApp));
    }
}
