//! Write [`ConversationDocument`] messages as SMS Backup & Restore XML.

use crate::format_sink::MergedArchive;
use crate::util::load_attachment_bytes_strict;
use anyhow::{Context, Result};
use message_ir::{
    ConversationDocument, IrAttachment, IrConversationType, IrDirection, IrMessage, IrMessageKind,
    nonempty,
};
use sbr::{
    SbrBackupWriter, SbrMessage, SourceFields, default_backup_path, encode_part_data, ensure_attr,
    set_attr,
};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

const MMS_ADDR_FROM: &str = "137";
const MMS_ADDR_TO: &str = "151";

/// Session that appends conversations into a single `{output}/smses.xml`.
pub(crate) struct SbrBackupSession {
    writer: SbrBackupWriter,
    output_dir: PathBuf,
}

impl SbrBackupSession {
    /// Start a new `smses-*.xml` backup under `output_dir`, replacing any earlier one.
    pub fn create(output_dir: &Path) -> Result<Self> {
        fs::create_dir_all(output_dir)
            .with_context(|| format!("create {}", output_dir.display()))?;
        let path = default_backup_path(output_dir);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("replace existing {}", path.display()))?;
        }
        Ok(Self {
            writer: SbrBackupWriter::create(&path)?,
            output_dir: output_dir.to_path_buf(),
        })
    }

    /// Write every message of one conversation as SBR `<sms>` or `<mms>` elements.
    pub fn append_document(&mut self, doc: &ConversationDocument) -> Result<()> {
        for msg in document_to_sbr_messages(doc, &self.output_dir)? {
            self.writer.write_message(&msg)?;
        }
        Ok(())
    }

    /// Close the XML and return the backup file path.
    pub fn finish(self) -> Result<PathBuf> {
        self.writer.finish()
    }
}

/// Map one conversation's messages into SBR XML elements (lossy for iMessage).
pub(crate) fn document_to_sbr_messages(
    doc: &ConversationDocument,
    output_dir: &Path,
) -> Result<Vec<SbrMessage>> {
    let owner = doc
        .export
        .owner_handle
        .as_deref()
        .and_then(nonempty)
        .unwrap_or_default();
    let mut out = Vec::with_capacity(doc.messages.len());
    for msg in &doc.messages {
        out.push(ir_message_to_sbr(doc, msg, &owner, output_dir)?);
    }
    Ok(out)
}

/// One IR message as an SBR element: restored from the vendor bag when the export came
/// from SBR, else synthesized.
fn ir_message_to_sbr(
    doc: &ConversationDocument,
    msg: &IrMessage,
    owner: &str,
    output_dir: &Path,
) -> Result<SbrMessage> {
    // The raw source bag is [`SourceFields`] serialized into the IR's
    // `source.fields`; deserializing it back recovers the typed bag without
    // any manual JSON walking. Anything that does not parse as a complete
    // SMS/MMS bag (a different `kind`, or a truncated bag) falls back to
    // synthesis, as before.
    if let Some(fields) = msg.source.as_ref().map(|s| &s.fields)
        && fields.contains_key("kind")
    {
        match serde_json::from_value::<SourceFields>(Value::Object(fields.clone())) {
            Ok(SourceFields::Sms { attrs }) => return Ok(restore_sms(attrs, msg)),
            Ok(SourceFields::Mms {
                attrs,
                parts,
                addrs,
            }) => {
                if let Some(restored) =
                    restore_mms(attrs, parts, addrs, doc, msg, owner, output_dir)?
                {
                    return Ok(restored);
                }
            }
            Err(_) => {}
        }
    }
    synthesize_sbr(doc, msg, owner, output_dir)
}

/// An `<sms>` from the original attributes, with the fields IR owns written back over them.
fn restore_sms(mut attrs: BTreeMap<String, String>, msg: &IrMessage) -> SbrMessage {
    set_attr(&mut attrs, "date", msg.timestamp_unix_ms.to_string());
    set_attr(
        &mut attrs,
        "type",
        match msg.direction {
            IrDirection::Incoming => "1",
            IrDirection::Outgoing => "2",
        },
    );
    set_attr(&mut attrs, "body", msg.text.clone());
    if let Some(subj) = msg.subject.as_deref() {
        set_attr(&mut attrs, "subject", subj);
    }
    ensure_attr(&mut attrs, "protocol", "0");
    ensure_attr(&mut attrs, "read", "1");
    SbrMessage::sms(attrs)
}

/// An `<mms>` from the original attributes, parts, and addresses, with the IR-owned fields written back.
#[allow(clippy::too_many_arguments)]
fn restore_mms(
    mut attrs: BTreeMap<String, String>,
    mut parts: Vec<BTreeMap<String, String>>,
    mut addrs: Vec<BTreeMap<String, String>>,
    doc: &ConversationDocument,
    msg: &IrMessage,
    owner: &str,
    output_dir: &Path,
) -> Result<Option<SbrMessage>> {
    if parts.is_empty() && addrs.is_empty() {
        // Incomplete bag — fall back to synthesis.
        return Ok(None);
    }
    set_attr(&mut attrs, "date", msg.timestamp_unix_ms.to_string());
    set_attr(
        &mut attrs,
        "msg_box",
        match msg.direction {
            IrDirection::Incoming => "1",
            IrDirection::Outgoing => "2",
        },
    );
    inject_attachment_data(&mut parts, &msg.attachments, output_dir)?;
    if addrs.is_empty() {
        addrs = synthesize_addrs(doc, msg, owner);
    }
    ensure_attr(&mut attrs, "read", "1");
    Ok(Some(SbrMessage::mms(attrs, parts, addrs)))
}

/// An SBR element for a message that did not come from SBR: `<mms>` when it has attachments, else `<sms>`.
fn synthesize_sbr(
    doc: &ConversationDocument,
    msg: &IrMessage,
    owner: &str,
    output_dir: &Path,
) -> Result<SbrMessage> {
    let is_group = doc.conversation.conversation_type == IrConversationType::Group;
    let use_mms =
        is_group || !msg.attachments.is_empty() || matches!(msg.message_kind, IrMessageKind::Mms);

    if use_mms {
        synthesize_mms(doc, msg, owner, output_dir)
    } else {
        Ok(synthesize_sms(doc, msg))
    }
}

/// A minimal `<sms>` for a text-only message.
fn synthesize_sms(doc: &ConversationDocument, msg: &IrMessage) -> SbrMessage {
    let peer = peer_address(doc, msg);
    let mut attrs = BTreeMap::new();
    set_attr(&mut attrs, "protocol", "0");
    set_attr(&mut attrs, "address", peer);
    set_attr(&mut attrs, "date", msg.timestamp_unix_ms.to_string());
    set_attr(
        &mut attrs,
        "type",
        match msg.direction {
            IrDirection::Incoming => "1",
            IrDirection::Outgoing => "2",
        },
    );
    if let Some(subj) = msg.subject.as_deref().filter(|s| !s.is_empty()) {
        set_attr(&mut attrs, "subject", subj);
    } else {
        set_attr(&mut attrs, "subject", "null");
    }
    set_attr(&mut attrs, "body", msg.text.clone());
    set_attr(&mut attrs, "toa", "null");
    set_attr(&mut attrs, "sc_toa", "null");
    set_attr(&mut attrs, "service_center", "null");
    set_attr(&mut attrs, "read", "1");
    set_attr(&mut attrs, "status", "-1");
    if let Some(name) = contact_name_alias(doc, msg) {
        set_attr(&mut attrs, "contact_name", name);
    }
    SbrMessage::sms(attrs)
}

/// A minimal `<mms>` with a text part and one part per attachment, reading attachment bytes from the output folder.
fn synthesize_mms(
    doc: &ConversationDocument,
    msg: &IrMessage,
    owner: &str,
    output_dir: &Path,
) -> Result<SbrMessage> {
    let address = mms_address_field(doc);
    let mut attrs = BTreeMap::new();
    set_attr(&mut attrs, "date", msg.timestamp_unix_ms.to_string());
    set_attr(
        &mut attrs,
        "msg_box",
        match msg.direction {
            IrDirection::Incoming => "1",
            IrDirection::Outgoing => "2",
        },
    );
    set_attr(&mut attrs, "address", address);
    set_attr(&mut attrs, "read", "1");
    if let Some(name) = contact_name_alias(doc, msg) {
        set_attr(&mut attrs, "contact_name", name);
    }
    if let Some(subj) = msg.subject.as_deref().filter(|s| !s.is_empty()) {
        set_attr(&mut attrs, "sub", subj);
    }

    let mut parts = Vec::new();
    let mut seq = 0i32;
    if !msg.text.trim().is_empty() {
        let mut part = BTreeMap::new();
        set_attr(&mut part, "seq", seq.to_string());
        set_attr(&mut part, "ct", "text/plain");
        set_attr(&mut part, "name", format!("text_{seq}.txt"));
        set_attr(&mut part, "chset", "106");
        set_attr(&mut part, "text", msg.text.clone());
        parts.push(part);
        seq += 1;
    }
    for att in &msg.attachments {
        let bytes = load_attachment_bytes_strict(att, output_dir)?;
        let mime = att
            .mime_type
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("application/octet-stream");
        let name = att
            .original_name
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("attachment");
        let mut part = BTreeMap::new();
        set_attr(&mut part, "seq", seq.to_string());
        set_attr(&mut part, "ct", mime);
        set_attr(&mut part, "name", name);
        set_attr(&mut part, "cl", name);
        if !bytes.is_empty() {
            set_attr(&mut part, "data", encode_part_data(&bytes));
        }
        parts.push(part);
        seq += 1;
    }
    if parts.is_empty() {
        let mut part = BTreeMap::new();
        set_attr(&mut part, "seq", "0");
        set_attr(&mut part, "ct", "text/plain");
        set_attr(&mut part, "text", msg.text.clone());
        parts.push(part);
    }

    let addrs = synthesize_addrs(doc, msg, owner);
    Ok(SbrMessage::mms(attrs, parts, addrs))
}

/// The `<addr>` list for a synthesized MMS: the sender as From, everyone else as To.
fn synthesize_addrs(
    doc: &ConversationDocument,
    msg: &IrMessage,
    owner: &str,
) -> Vec<BTreeMap<String, String>> {
    let mut addrs = Vec::new();
    let from = match msg.direction {
        IrDirection::Incoming => {
            if let Some(h) = msg.sender_handle.as_deref().filter(|s| !s.is_empty()) {
                h.to_string()
            } else {
                peer_address(doc, msg)
            }
        }
        IrDirection::Outgoing => {
            if owner.is_empty() {
                "insert-address-token".into()
            } else {
                owner.to_string()
            }
        }
    };
    addrs.push(addr_entry(&from, MMS_ADDR_FROM));

    match msg.direction {
        IrDirection::Incoming => {
            if !owner.is_empty() {
                addrs.push(addr_entry(owner, MMS_ADDR_TO));
            }
        }
        IrDirection::Outgoing => {
            if doc.conversation.conversation_type == IrConversationType::Group {
                for p in &doc.conversation.participants {
                    let Some(handle) = p.handle.as_deref() else {
                        continue;
                    };
                    if handle != owner {
                        addrs.push(addr_entry(handle, MMS_ADDR_TO));
                    }
                }
            } else {
                let peer = peer_address(doc, msg);
                addrs.push(addr_entry(&peer, MMS_ADDR_TO));
            }
        }
    }
    addrs
}

/// One `<addr>` element's attributes.
fn addr_entry(address: &str, addr_type: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    set_attr(&mut m, "address", address);
    set_attr(&mut m, "type", addr_type);
    set_attr(&mut m, "charset", "106");
    m
}

/// The other party's address: the sender for incoming messages, else the chat identifier.
fn peer_address(doc: &ConversationDocument, msg: &IrMessage) -> String {
    if let Some(h) = msg
        .sender_handle
        .as_deref()
        .filter(|s| !s.is_empty())
        .filter(|_| msg.direction == IrDirection::Incoming)
    {
        return h.to_string();
    }
    if let Some(handle) = doc
        .conversation
        .participants
        .first()
        .and_then(|p| p.handle.clone())
    {
        return handle;
    }
    doc.conversation.chat_identifier.clone()
}

/// The `address` attribute of an MMS: every participant joined with `~` for groups, else the peer.
fn mms_address_field(doc: &ConversationDocument) -> String {
    if doc.conversation.conversation_type == IrConversationType::Group {
        doc.conversation
            .participants
            .iter()
            .filter_map(|p| p.handle.as_deref())
            .collect::<Vec<_>>()
            .join("~")
    } else if let Some(handle) = doc
        .conversation
        .participants
        .first()
        .and_then(|p| p.handle.clone())
    {
        handle
    } else {
        doc.conversation.chat_identifier.clone()
    }
}

/// The `contact_name` value: the sender's display name for incoming messages, else the peer's.
fn contact_name_alias(doc: &ConversationDocument, msg: &IrMessage) -> Option<String> {
    if msg.direction == IrDirection::Incoming
        && let Some(n) = msg.sender_display_name.as_deref().filter(|s| !s.is_empty())
    {
        return Some(n.to_string());
    }
    doc.conversation
        .participants
        .first()
        .and_then(|p| p.display_name.clone())
        .filter(|s| !s.is_empty())
}

/// Rehydrate base64 `data` on MMS parts whose payloads were staged as files.
///
/// Parts are matched to attachments by payload digest rather than by position.
/// The reader (`part_fields` in `message-sbr`) records the decoded payload
/// digest as `data_sha256` on each part and drops the base64 string, and
/// staged files are content-addressed by that same digest, so a digest lookup
/// is exact. Positional pairing drifts whenever the part list and attachment
/// list diverge: parts with empty or undecodable base64 never produced an
/// attachment, and identical payloads dedupe into a single attachment that
/// several parts share. Parts whose digest matches no attachment (e.g. media
/// transforms rewrote the bytes and rehashed the digest) fall back to the next
/// unconsumed attachment in list order.
fn inject_attachment_data(
    parts: &mut [BTreeMap<String, String>],
    attachments: &[IrAttachment],
    output_dir: &Path,
) -> Result<()> {
    // Digest → attachment index for exact matching. An attachment is keyed by
    // the digest that named its staged file; transforms clear it and rehash,
    // which is exactly when the fallback below takes over.
    let mut by_digest: HashMap<&str, usize> = HashMap::new();
    let mut consumed = vec![false; attachments.len()];
    for (index, att) in attachments.iter().enumerate() {
        if let Some(digest) = att.digest_sha256.as_deref().filter(|d| !d.is_empty()) {
            by_digest.entry(digest).or_insert(index);
        }
    }
    // First unconsumed attachment for the positional fallback.
    let mut next_unconsumed = 0usize;
    for part in parts.iter_mut() {
        let ct = part.get("ct").map_or("", String::as_str);
        let is_text = ct.starts_with("text/") || ct.eq_ignore_ascii_case("application/smil");
        let decode_error = part.get("data_decode_error").is_some_and(|v| v == "true");
        let digest = part.get("data_sha256").cloned();
        // Drop CSV-only digest placeholders.
        part.remove("data_len");
        part.remove("data_sha256");
        part.remove("data_decode_error");
        if is_text || decode_error || digest.as_deref().is_none_or(|s| s.trim().is_empty()) {
            // Text parts carry their own text. Parts whose base64 was empty or
            // undecodable have no staged attachment; leave `data` unset instead
            // of consuming another part's attachment.
            continue;
        }
        let index = if let Some(index) = by_digest.get(digest.as_deref().unwrap_or("")).copied() {
            Some(index)
        } else {
            // No exact digest match (attachment rewritten by a media
            // transform, or attachment list shorter than the part list).
            while next_unconsumed < attachments.len() && consumed[next_unconsumed] {
                next_unconsumed += 1;
            }
            (next_unconsumed < attachments.len()).then(|| {
                let index = next_unconsumed;
                next_unconsumed += 1;
                index
            })
        };
        let Some(index) = index else {
            continue;
        };
        consumed[index] = true;
        let bytes = load_attachment_bytes_strict(&attachments[index], output_dir)?;
        if !bytes.is_empty() {
            set_attr(part, "data", encode_part_data(&bytes));
        }
    }
    Ok(())
}

/// The SMS Backup & Restore backup as a [`MergedArchive`]: every
/// conversation into one `smses.xml`, attachment bytes inside it.
#[derive(Debug, Clone, Copy, Default)]
pub struct SbrArchive;

impl MergedArchive for SbrArchive {
    fn write(&self, output_dir: &Path, documents: &[ConversationDocument]) -> Result<PathBuf> {
        let mut session = SbrBackupSession::create(output_dir)?;
        for doc in documents {
            session.append_document(doc)?;
        }
        session.finish()
    }
}
