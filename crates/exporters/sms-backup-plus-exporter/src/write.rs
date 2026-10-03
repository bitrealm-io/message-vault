//! Write conversations as SMS Backup+ mail: one folder per conversation and
//! one `.eml` per SMS or MMS, with the `X-smssync-*` headers this crate's
//! importer reads (#543, ADR 0021).
//!
//! The mail carries only what the database keeps. The phone's row and
//! thread ids, read and status flags, protocol and the app's build are not
//! kept, so `X-smssync-id`, `-thread`, `-read`, `-status`, `-protocol` and
//! `-version` are never written; Gmail's `X-GM-THRID` and `X-Gmail-Labels`
//! belong to Gmail, not the app, and are never written either.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use mail::{attachment_part, eml_file_name, text_body_part};
use mail_builder::MessageBuilder;
use mail_builder::headers::address::Address;
use mail_builder::headers::date::Date;
use mail_builder::headers::text::Text;
use mail_builder::mime::MimePart;
use message_crate_core::{ATTACHMENTS_MISSING, ExportReport, NOT_SMS_OR_MMS_LEFT_OUT};
use message_ir::{
    ConversationDocument, HandleType, IrConversationType, IrDirection, IrMessage, IrMessageKind,
    give_each_document_its_own_file, trimmed,
};
use message_ir_format::{MergedArchive, load_attachment_bytes};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

use crate::flat_eml::UNKNOWN_EMAIL_DOMAIN;

/// Domain of the `Message-ID` and `References` the writer makes up. `.local`
/// is never routed.
const DOMAIN: &str = "sms-backup-plus.local";

/// Writes the SMS and MMS of every conversation as SMS Backup+ mail. Hand it
/// to [`message_ir_format::FormatSink::with_archive`] for
/// [`message_crate_core::OutputFormat::SmsBackupPlus`].
#[derive(Debug, Clone, Copy)]
pub struct SmsBackupPlusArchive {
    /// `X-smssync-backup-time` on every mail: the Export Run's start.
    backup_time_ms: i64,
}

impl SmsBackupPlusArchive {
    /// An archive whose mail records `backup_time`, the Export Run's start,
    /// as the time of the backup.
    #[must_use]
    pub fn new(backup_time: DateTime<Utc>) -> Self {
        Self {
            backup_time_ms: backup_time.timestamp_millis(),
        }
    }
}

impl MergedArchive for SmsBackupPlusArchive {
    /// Write one folder per conversation that has an SMS or MMS, and count
    /// every other message in `report`. Returns `output_dir`.
    fn write(
        &self,
        output_dir: &Path,
        documents: &[ConversationDocument],
        report: &mut ExportReport,
    ) -> Result<PathBuf> {
        fs::create_dir_all(output_dir)
            .with_context(|| format!("create {}", output_dir.display()))?;
        let mut kept = Vec::with_capacity(documents.len());
        for doc in documents {
            let messages: Vec<IrMessage> = doc
                .messages
                .iter()
                .filter(|message| message.is_sms_or_mms())
                .cloned()
                .collect();
            let left_out = doc.messages.len() - messages.len();
            if left_out > 0 {
                report.bump(NOT_SMS_OR_MMS_LEFT_OUT, left_out as u64);
            }
            if messages.is_empty() {
                continue;
            }
            kept.push(ConversationDocument {
                schema_version: doc.schema_version,
                export: doc.export.clone(),
                conversation: doc.conversation.clone(),
                messages,
                packaging_stem_suffix: doc.packaging_stem_suffix.clone(),
            });
        }
        let mut docs: Vec<&mut ConversationDocument> = kept.iter_mut().collect();
        give_each_document_its_own_file(&mut docs).map_err(anyhow::Error::msg)?;
        for doc in &kept {
            self.write_conversation(output_dir, doc, report)?;
        }
        Ok(output_dir.to_path_buf())
    }

    /// None: the archive writes only folders of `.eml` files, and the next
    /// clean of the folder removes every such folder as it does the EML
    /// format's (`mail::clean_previous_mail_output`).
    fn file_names(&self) -> Vec<String> {
        Vec::new()
    }

    fn format_name(&self) -> &'static str {
        "SMS Backup+"
    }
}

impl SmsBackupPlusArchive {
    /// One folder of `.eml` files, named and ordered as the EML archive
    /// names and orders its own.
    fn write_conversation(
        &self,
        output_dir: &Path,
        doc: &ConversationDocument,
        report: &mut ExportReport,
    ) -> Result<()> {
        let folder = output_dir.join(doc.filename_stem());
        fs::create_dir_all(&folder).with_context(|| format!("create {}", folder.display()))?;
        let conversation = Conversation::of(doc);
        let mut ordered: Vec<&IrMessage> = doc.messages.iter().collect();
        ordered.sort_by(|a, b| {
            a.timestamp_unix_ms
                .cmp(&b.timestamp_unix_ms)
                .then_with(|| a.guid.cmp(&b.guid))
        });
        for (index, message) in ordered.into_iter().enumerate() {
            let sequence = u32::try_from(index + 1).context("too many messages")?;
            let path = folder.join(eml_file_name(sequence, message)?);
            let bytes = self.build_mail(&conversation, message, output_dir, report)?;
            fs::write(&path, bytes).with_context(|| format!("write {}", path.display()))?;
        }
        Ok(())
    }

    /// One message as an SMS Backup+ mail. An attachment whose file is gone
    /// is counted in `report` as missing.
    fn build_mail(
        &self,
        conversation: &Conversation<'_>,
        message: &IrMessage,
        output_dir: &Path,
        report: &mut ExportReport,
    ) -> Result<Vec<u8>> {
        let mut attachments = Vec::with_capacity(message.attachments.len());
        for (i, attachment) in message.attachments.iter().enumerate() {
            let bytes = load_attachment_bytes(attachment, output_dir)?;
            // An attachment whose file is gone has nothing to carry, and
            // the importer skips an empty part, so it is counted and the
            // run's log says how many. One the staging step already found
            // missing (`missing_reason`) was counted there.
            if bytes.is_empty() {
                if attachment.missing_reason.is_none() {
                    report.bump(ATTACHMENTS_MISSING, 1);
                }
                continue;
            }
            let mime = attachment
                .mime_type
                .as_deref()
                .and_then(trimmed)
                .unwrap_or("application/octet-stream");
            // Every `text/plain` part of an MMS is message text to the
            // importer, as it is in an MMS on the phone, so a text file goes
            // as plain bytes under its own name and stays a file.
            let mime = if mms_parts::is_text(mime) {
                "application/octet-stream"
            } else {
                mime
            };
            let name = attachment
                .original_name
                .clone()
                .unwrap_or_else(|| format!("attachment-{i}"));
            attachments.push(attachment_part(mime, name, &bytes));
        }
        // An SMS that carries a file can only be told as an MMS.
        let is_mms = message.message_kind == IrMessageKind::Mms || !attachments.is_empty();
        let android_type = match (is_mms, message.direction) {
            (false, IrDirection::Incoming) => "1",
            (false, IrDirection::Outgoing) => "2",
            (true, IrDirection::Incoming) => "132",
            (true, IrDirection::Outgoing) => "128",
        };
        let (from, to) = conversation.envelope(message);
        let builder = MessageBuilder::new()
            .from(from)
            .to(to)
            .subject(conversation.subject(message))
            .date(Date::new(message.timestamp_unix_ms.div_euclid(1000)))
            .message_id(format!("{}@{DOMAIN}", message.guid))
            .references(format!("{}@{DOMAIN}", conversation.thread))
            .header(
                "X-smssync-datatype",
                Text::new(if is_mms { "MMS" } else { "SMS" }),
            )
            .header("X-smssync-address", Text::new(conversation.address()))
            .header(
                "X-smssync-date",
                Text::new(message.timestamp_unix_ms.to_string()),
            )
            .header("X-smssync-type", Text::new(android_type))
            .header(
                "X-smssync-backup-time",
                Text::new(self.backup_time_ms.to_string()),
            );
        let text = text_body_part(&message.text);
        let body = if is_mms {
            let mut parts = Vec::with_capacity(attachments.len() + 1);
            parts.push(text);
            parts.extend(attachments);
            MimePart::new("multipart/mixed", parts).transfer_encoding("7bit")
        } else {
            text
        };
        builder
            .body(body)
            .write_to_vec()
            .context("serialize SMS Backup+ mail")
    }
}

/// What every mail of one conversation shares.
struct Conversation<'a> {
    doc: &'a ConversationDocument,
    /// The other people's handles, the owner's left out.
    peers: Vec<&'a str>,
    /// `References` local part: the same for every mail of the conversation,
    /// so a mail program threads them.
    thread: String,
}

impl<'a> Conversation<'a> {
    fn of(doc: &'a ConversationDocument) -> Self {
        let owner = doc
            .export
            .owner_handle
            .as_deref()
            .and_then(trimmed)
            .map(handle_key);
        let mut peers: Vec<&str> = doc
            .conversation
            .participants
            .iter()
            .filter_map(|p| p.handle.as_deref().and_then(trimmed))
            .filter(|handle| Some(handle_key(handle)) != owner)
            .collect();
        if peers.is_empty()
            && let Some(id) = trimmed(&doc.conversation.chat_identifier)
        {
            peers.push(id);
        }
        let digest = hex::encode(Sha256::digest(doc.conversation.chat_identifier.as_bytes()));
        Self {
            doc,
            peers,
            thread: digest[..32].to_string(),
        }
    }

    fn is_group(&self) -> bool {
        self.doc.conversation.conversation_type == IrConversationType::Group
    }

    /// `X-smssync-address`: the peer, or a group's peers joined by `~` as
    /// SMS Backup+ joins them.
    fn address(&self) -> String {
        self.peers.join("~")
    }

    /// The display name the roster gives `handle`, if any.
    fn display_name(&self, handle: &str) -> Option<&'a str> {
        self.doc
            .conversation
            .participants
            .iter()
            .find(|p| p.handle.as_deref() == Some(handle))
            .and_then(|p| p.display_name.as_deref())
            .and_then(trimmed)
    }

    /// Who a received message is from: its sender, or for a one-to-one
    /// message, which can only be from its one peer, that peer. `None` for a
    /// group message whose sender is unknown.
    fn sender_of<'m>(&'m self, message: &'m IrMessage) -> Option<&'m str> {
        message
            .sender_handle
            .as_deref()
            .and_then(trimmed)
            .or_else(|| {
                (!self.is_group())
                    .then(|| self.peers.first().copied())
                    .flatten()
            })
    }

    /// The `Subject`: `SMS with <name>`. The importer takes the name as the
    /// name of the person the mail is with, and credits the message to it, so
    /// it names the sender of a received message and the peer of a sent
    /// one-to-one message. A sent group message, or a received one whose
    /// sender is unknown, names no one: a group title or a list of names
    /// would be given to one person. It is `SMS with` a member's number,
    /// written as its key (`+15555550101`, or a short code's digits), which
    /// the importer never takes as a name, or plain `SMS` when no member has
    /// a number. The importer knows the mail by `X-smssync-type`, not by
    /// its subject.
    fn subject(&self, message: &IrMessage) -> String {
        let sender = self.sender_of(message);
        let named = match message.direction {
            IrDirection::Incoming => sender.map(|handle| {
                message
                    .sender_display_name
                    .as_deref()
                    .and_then(trimmed)
                    .or_else(|| self.display_name(handle))
                    .unwrap_or(handle)
                    .to_string()
            }),
            IrDirection::Outgoing if !self.is_group() => self
                .peers
                .first()
                .map(|peer| self.display_name(peer).unwrap_or(peer).to_string()),
            IrDirection::Outgoing => None,
        };
        let unnamed = || {
            self.peers.iter().find_map(|peer| {
                phone::Handle::parse(peer)
                    .filter(|handle| handle.kind() == HandleType::Phone)
                    .map(phone::Handle::into_key)
            })
        };
        match named.or_else(unnamed) {
            Some(name) => format!("SMS with {name}"),
            None => "SMS".to_string(),
        }
    }

    /// `From` and `To`: the sender to the owner for an incoming message,
    /// the owner to the peers for an outgoing one.
    fn envelope(&self, message: &IrMessage) -> (Address<'static>, Address<'static>) {
        let owner_handle = message
            .owner_handle
            .as_deref()
            .and_then(trimmed)
            .or_else(|| self.doc.export.owner_handle.as_deref().and_then(trimmed))
            .unwrap_or("me");
        let owner_name = self
            .doc
            .export
            .owner_display_name
            .as_deref()
            .and_then(trimmed)
            .unwrap_or("Me");
        let owner = address(owner_handle, Some(owner_name));
        match message.direction {
            IrDirection::Incoming => {
                // A group message whose sender is unknown keeps it unknown: an
                // address no peer has, which the importer reads as no sender.
                let sender = self.sender_of(message).unwrap_or("unknown");
                let name = message
                    .sender_display_name
                    .as_deref()
                    .and_then(trimmed)
                    .or_else(|| self.display_name(sender));
                (address(sender, name), owner)
            }
            IrDirection::Outgoing => {
                let peers: Vec<Address<'static>> = self
                    .peers
                    .iter()
                    .map(|peer| address(peer, self.display_name(peer)))
                    .collect();
                let to = if peers.len() == 1 {
                    peers.into_iter().next().expect("one peer")
                } else {
                    Address::new_list(peers)
                };
                (owner, to)
            }
        }
    }
}

/// The key two spellings of one handle share: `5555550100` and
/// `+15555550100` are one number. A handle that is no phone number or email
/// address is its own key.
fn handle_key(handle: &str) -> String {
    phone::Handle::parse(handle).map_or_else(|| handle.to_string(), phone::Handle::into_key)
}

/// `"Name" <address>`, as SMS Backup+ writes it: an email address as it is,
/// and any other handle, such as a phone number, as `<handle>@unknown.email`.
/// The importer reads the handle back from the part before `@unknown.email`
/// and any other address whole, so each handle comes back as it went out.
fn address(handle: &str, name: Option<&str>) -> Address<'static> {
    let address = if handle.contains('@') {
        handle.to_string()
    } else {
        format!("{handle}@{UNKNOWN_EMAIL_DOMAIN}")
    };
    Address::new_address(name.map(str::to_string), address)
}

#[cfg(test)]
mod tests;
