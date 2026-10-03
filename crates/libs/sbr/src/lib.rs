//! SMS Backup & Restore (SyncTech) XML codec.
//!
//! Writers produce a single backup file (`smses.xml`) with root
//! `<smses count="N">`. See the [SMS Backup & Restore XML output](https://messagecrate.app/docs/developer/formats/sms-backup-restore-xml/).

mod read;

pub use read::{
    AttachmentBlob, ConversationKind, ParseStats, Record, SourceFields, contact_name,
    infer_owner_phones, parse_file_with,
};

use anyhow::{Context, Result};
use base64::Engine;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

/// One `<sms>` or `<mms>` element ready to serialize.
#[derive(Debug, Clone)]
pub enum SbrMessage {
    /// One `<sms>` element carrying a raw attribute map.
    Sms {
        /// Raw XML attributes for the `<sms>` element.
        attrs: BTreeMap<String, String>,
    },
    /// One `<mms>` element carrying attrs, parts, and addrs.
    Mms {
        /// Raw XML attributes for the `<mms>` element.
        attrs: BTreeMap<String, String>,
        /// Raw `<part>` attribute maps.
        parts: Vec<BTreeMap<String, String>>,
        /// Raw `<addr>` attribute maps.
        addrs: Vec<BTreeMap<String, String>>,
    },
}

impl SbrMessage {
    /// Wrap a raw attribute map as an SMS element.
    pub fn sms(attrs: BTreeMap<String, String>) -> Self {
        Self::Sms { attrs }
    }

    /// Wrap attrs/parts/addrs as an MMS element.
    pub fn mms(
        attrs: BTreeMap<String, String>,
        parts: Vec<BTreeMap<String, String>>,
        addrs: Vec<BTreeMap<String, String>>,
    ) -> Self {
        Self::Mms {
            attrs,
            parts,
            addrs,
        }
    }
}

/// Streaming writer for a SyncTech-style `smses.xml` backup.
///
/// Message bodies are buffered in a sidecar temp file; [`finish`](Self::finish)
/// writes the final document with the correct `count`.
#[derive(Debug)]
pub struct SbrBackupWriter {
    path: PathBuf,
    body_path: PathBuf,
    body: BufWriter<File>,
    count: u64,
    characters_left_out: u64,
}

impl SbrBackupWriter {
    /// Create a new backup at `path` (typically `…/smses.xml`).
    ///
    /// # Errors
    ///
    /// Returns an error when the output directory cannot be created, a stale
    /// body file cannot be removed, or the body file cannot be opened.
    pub fn create(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        let body_path = body_path_for(path);
        if body_path.exists() {
            fs::remove_file(&body_path)
                .with_context(|| format!("remove stale {}", body_path.display()))?;
        }
        let body = BufWriter::new(
            File::create(&body_path).with_context(|| format!("create {}", body_path.display()))?,
        );
        Ok(Self {
            path: path.to_path_buf(),
            body_path,
            body,
            count: 0,
            characters_left_out: 0,
        })
    }

    /// Number of messages written so far.
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Characters left out so far because XML 1.0 cannot carry them.
    pub fn characters_left_out(&self) -> u64 {
        self.characters_left_out
    }

    /// Serialize one SMS/MMS element into the sidecar body file and increment
    /// the count.
    ///
    /// # Errors
    ///
    /// Returns an error when the body write fails.
    pub fn write_message(&mut self, msg: &SbrMessage) -> Result<()> {
        match msg {
            SbrMessage::Sms { attrs } => {
                self.characters_left_out += write_empty_element(&mut self.body, "sms", attrs)?;
            }
            SbrMessage::Mms {
                attrs,
                parts,
                addrs,
            } => {
                self.characters_left_out += write_mms(&mut self.body, attrs, parts, addrs)?;
            }
        }
        self.count += 1;
        Ok(())
    }

    /// Finalize `count`, close `</smses>`, and replace `path`.
    ///
    /// # Errors
    ///
    /// Returns an error when flushing, reading back, writing, or renaming the
    /// backup files fails.
    pub fn finish(mut self) -> Result<PathBuf> {
        self.body.flush().context("flush sbr body")?;
        drop(self.body);

        // The body carries every attachment as base64, so it is copied to
        // the backup as a stream rather than read into memory.
        let mut body = File::open(&self.body_path)
            .with_context(|| format!("open {}", self.body_path.display()))?;

        let tmp = tmp_path_for(&self.path);
        {
            let mut out = BufWriter::new(
                File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?,
            );
            writeln!(
                out,
                r"<?xml version='1.0' encoding='UTF-8' standalone='yes' ?>"
            )?;
            writeln!(out, r#"<smses count="{}">"#, self.count)?;
            // Every element written to the body ends with a line break, so
            // the closing tag starts a line of its own.
            io::copy(&mut body, &mut out)
                .with_context(|| format!("copy {}", self.body_path.display()))?;
            writeln!(out, "</smses>")?;
            out.flush()?;
        }
        drop(body);
        fs::rename(&tmp, &self.path)
            .with_context(|| format!("rename {} → {}", tmp.display(), self.path.display()))?;
        let _ = fs::remove_file(&self.body_path);
        Ok(self.path)
    }
}

/// Standard base64 for MMS `data` attributes.
pub fn encode_part_data(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Write attributes as ` key="value"` with XML escaping, and return how
/// many characters were left out because XML 1.0 cannot carry them.
fn write_attrs(w: &mut impl Write, attrs: &BTreeMap<String, String>) -> Result<u64> {
    let mut left_out = 0;
    for (k, v) in attrs {
        let (value, dropped) = escape_attr(v);
        left_out += dropped;
        write!(w, r#" {k}="{value}""#)?;
    }
    Ok(left_out)
}

/// The value escaped for a double-quoted XML attribute, and the number of
/// characters left out of it.
///
/// An XML reader turns a literal line break or tab in an attribute into a
/// space, so `\n`, `\r` and `\t` are written as character references. XML
/// 1.0 cannot carry any other character below U+0020, not even as a
/// reference, so those are left out.
fn escape_attr(value: &str) -> (String, u64) {
    let mut out = String::with_capacity(value.len());
    let mut left_out = 0;
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' => out.push_str("&#9;"),
            '\u{0}'..='\u{1f}' => left_out += 1,
            c => out.push(c),
        }
    }
    (out, left_out)
}

/// Write a self-closing element with its attributes, and return how many
/// characters were left out.
fn write_empty_element(
    w: &mut impl Write,
    name: &str,
    attrs: &BTreeMap<String, String>,
) -> Result<u64> {
    write!(w, "  <{name}")?;
    let left_out = write_attrs(w, attrs)?;
    writeln!(w, " />")?;
    Ok(left_out)
}

/// Write an `<mms>` element with its `<parts>` and `<addrs>` children, and
/// return how many characters were left out.
fn write_mms(
    w: &mut impl Write,
    attrs: &BTreeMap<String, String>,
    parts: &[BTreeMap<String, String>],
    addrs: &[BTreeMap<String, String>],
) -> Result<u64> {
    write!(w, "  <mms")?;
    let mut left_out = write_attrs(w, attrs)?;
    writeln!(w, ">")?;
    writeln!(w, "    <parts>")?;
    for part in parts {
        write!(w, "      <part")?;
        left_out += write_attrs(w, part)?;
        writeln!(w, " />")?;
    }
    writeln!(w, "    </parts>")?;
    writeln!(w, "    <addrs>")?;
    for addr in addrs {
        write!(w, "      <addr")?;
        left_out += write_attrs(w, addr)?;
        writeln!(w, " />")?;
    }
    writeln!(w, "    </addrs>")?;
    writeln!(w, "  </mms>")?;
    Ok(left_out)
}

/// Default filename for a full-backup projection.
const DEFAULT_BACKUP_FILENAME: &str = "smses.xml";

/// Join `smses.xml` onto an output directory (the default full-backup filename).
pub fn default_backup_path(output_dir: &Path) -> PathBuf {
    output_dir.join(DEFAULT_BACKUP_FILENAME)
}

/// The file [`SbrBackupWriter`] buffers message bodies in beside `path`.
fn body_path_for(path: &Path) -> PathBuf {
    path.with_extension("xml.sbrbody")
}

/// The file [`SbrBackupWriter::finish`] writes before it renames it to `path`.
fn tmp_path_for(path: &Path) -> PathBuf {
    path.with_extension("xml.tmp")
}

/// Remove the default backup in `output_dir` and the two temporary files a
/// writer that stopped before it finished leaves beside it. A file that is
/// not there is skipped.
///
/// # Errors
///
/// Returns an error when one of the files exists and cannot be removed.
pub fn remove_backup(output_dir: &Path) -> Result<()> {
    let path = default_backup_path(output_dir);
    for file in [tmp_path_for(&path), body_path_for(&path), path] {
        if file.is_file() {
            fs::remove_file(&file)
                .with_context(|| format!("remove previous {}", file.display()))?;
        }
    }
    Ok(())
}

/// Insert `key` only when it is not already present.
pub fn ensure_attr(attrs: &mut BTreeMap<String, String>, key: &str, value: impl Into<String>) {
    attrs.entry(key.to_string()).or_insert_with(|| value.into());
}

/// Overwrite an attribute (IR authoritative fields like date/body).
pub fn set_attr(attrs: &mut BTreeMap<String, String>, key: &str, value: impl Into<String>) {
    attrs.insert(key.to_string(), value.into());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_sms_and_mms_with_count() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("smses.xml");
        let mut w = SbrBackupWriter::create(&path).unwrap();

        let mut sms = BTreeMap::new();
        sms.insert("protocol".into(), "0".into());
        sms.insert("address".into(), "+15555550101".into());
        sms.insert("date".into(), "1400773261000".into());
        sms.insert("type".into(), "1".into());
        sms.insert("body".into(), r#"hello & "world""#.into());
        sms.insert("read".into(), "1".into());
        w.write_message(&SbrMessage::sms(sms)).unwrap();

        let mut mms_attrs = BTreeMap::new();
        mms_attrs.insert("date".into(), "1400773400000".into());
        mms_attrs.insert("msg_box".into(), "1".into());
        mms_attrs.insert("address".into(), "+15555550101".into());
        let mut part = BTreeMap::new();
        part.insert("seq".into(), "0".into());
        part.insert("ct".into(), "text/plain".into());
        part.insert("text".into(), "mms hi".into());
        let mut part2 = BTreeMap::new();
        part2.insert("seq".into(), "1".into());
        part2.insert("ct".into(), "image/jpeg".into());
        part2.insert("name".into(), "pic.jpg".into());
        part2.insert("data".into(), encode_part_data(b"xxxx"));
        let mut addr = BTreeMap::new();
        addr.insert("address".into(), "+15555550101".into());
        addr.insert("type".into(), "137".into());
        w.write_message(&SbrMessage::mms(mms_attrs, vec![part, part2], vec![addr]))
            .unwrap();

        let out = w.finish().unwrap();
        let text = fs::read_to_string(&out).unwrap();
        assert!(text.contains(r#"<smses count="2">"#));
        assert!(text.contains("hello &amp; &quot;world&quot;"));
        assert!(text.contains("<mms "));
        assert!(text.contains(r#"ct="image/jpeg""#));
        assert!(text.contains("</smses>"));
        assert!(!path.with_extension("xml.sbrbody").exists());
    }

    #[test]
    fn empty_backup_still_valid() {
        let tmp = tempfile::tempdir().unwrap();
        let path = default_backup_path(tmp.path());
        let w = SbrBackupWriter::create(&path).unwrap();
        w.finish().unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(r#"count="0""#));
        assert!(text.contains("</smses>"));
    }

    #[test]
    fn a_character_xml_cannot_carry_is_left_out_and_counted() {
        let tmp = tempfile::tempdir().unwrap();
        let path = default_backup_path(tmp.path());
        let mut w = SbrBackupWriter::create(&path).unwrap();
        let mut sms = BTreeMap::new();
        sms.insert("address".into(), "+15555550101".into());
        sms.insert("body".into(), "a\u{1}b\u{1b}c\r\nd\te".into());
        w.write_message(&SbrMessage::sms(sms)).unwrap();
        assert_eq!(w.characters_left_out(), 2);
        w.finish().unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(r#"body="abc&#13;&#10;d&#9;e""#), "{text}");
        assert!(
            !text.chars().any(|c| c.is_control() && c != '\n'),
            "every control character is written as a reference or left out"
        );
    }
}
