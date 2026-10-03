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
use message_crate_core::ExportReport;
use message_ir::{
    ConversationDocument, IrConversationType, IrDirection, IrMessage, IrMessageKind,
    give_each_document_its_own_file, trimmed,
};
use message_ir_format::{MergedArchive, load_attachment_bytes};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

use crate::flat_eml::UNKNOWN_EMAIL_DOMAIN;

/// The export report counter for messages left out because SMS Backup+
/// holds only SMS and MMS.
pub(crate) const LEFT_OUT: &str = "messages_not_sms_or_mms_left_out";

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

/// The log line saying how many messages the archive left out and why, or
/// `None` when it left out none.
#[must_use]
pub fn left_out_line(report: &ExportReport) -> Option<String> {
    let count = report.extra(LEFT_OUT);
    (count > 0).then(|| {
        format!(
            "Left out {count} message(s) that are not SMS or MMS, because SMS Backup+ holds \
             only SMS and MMS"
        )
    })
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
                report.bump(LEFT_OUT, left_out as u64);
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
            self.write_conversation(output_dir, doc)?;
        }
        Ok(output_dir.to_path_buf())
    }

    /// None: the archive writes only folders of `.eml` files, and the next
    /// clean of the folder removes every such folder as it does the EML
    /// format's (`mail::clean_previous_mail_output`).
    fn file_names(&self) -> Vec<String> {
        Vec::new()
    }
}

impl SmsBackupPlusArchive {
    /// One folder of `.eml` files, named and ordered as the EML archive
    /// names and orders its own.
    fn write_conversation(&self, output_dir: &Path, doc: &ConversationDocument) -> Result<()> {
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
            let bytes = self.build_mail(&conversation, message, output_dir)?;
            fs::write(&path, bytes).with_context(|| format!("write {}", path.display()))?;
        }
        Ok(())
    }

    /// One message as an SMS Backup+ mail.
    fn build_mail(
        &self,
        conversation: &Conversation<'_>,
        message: &IrMessage,
        output_dir: &Path,
    ) -> Result<Vec<u8>> {
        let mut attachments = Vec::with_capacity(message.attachments.len());
        for (i, attachment) in message.attachments.iter().enumerate() {
            let bytes = load_attachment_bytes(attachment, output_dir)?;
            // An attachment whose file is gone has nothing to carry, and
            // the importer skips an empty part.
            if bytes.is_empty() {
                continue;
            }
            let mime = attachment
                .mime_type
                .as_deref()
                .and_then(trimmed)
                .unwrap_or("application/octet-stream");
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
            .subject(format!("SMS with {}", conversation.name()))
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
        let owner = doc.export.owner_handle.as_deref().and_then(trimmed);
        let mut peers: Vec<&str> = doc
            .conversation
            .participants
            .iter()
            .filter_map(|p| p.handle.as_deref().and_then(trimmed))
            .filter(|handle| Some(*handle) != owner)
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

    /// The name after `SMS with`: a group's title or its peers' names, else
    /// the peer's name or handle.
    fn name(&self) -> String {
        if self.is_group()
            && let Some(title) = self
                .doc
                .conversation
                .group_title
                .as_deref()
                .and_then(trimmed)
        {
            return title.to_string();
        }
        let names: Vec<&str> = self
            .peers
            .iter()
            .map(|peer| self.display_name(peer).unwrap_or(peer))
            .collect();
        if names.is_empty() {
            "Unknown".to_string()
        } else {
            names.join(", ")
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
                let sender = message
                    .sender_handle
                    .as_deref()
                    .and_then(trimmed)
                    .or_else(|| self.peers.first().copied())
                    .unwrap_or("unknown");
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
