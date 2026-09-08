//! Write conversation files for the demo dataset.
//!
//! Each file is JSON Lines: a header object, then one JSON object per message.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::{Context, Result};
use chrono::{Duration, FixedOffset, TimeZone, Utc};
use message_ir::{
    ConversationHeader, ConversationMeta, ConversationStats, ExportMeta, IrAttachment,
    IrConversationType, IrDirection, IrImessage, IrMessage, IrMessageKind, IrParticipant,
    IrService, SCHEMA_VERSION,
};
use rand::Rng;
use rand::RngExt;
use rand::seq::{IndexedRandom, SliceRandom};
use serde_json::json;

use crate::assets::{JPG_PHOTOS, OTHER_ATTACHMENTS};
use crate::config::SeedConfig;
use crate::corpus::Corpus;
use crate::personas::{
    Contact, EMPTY_GROUP_HANDLE, EMPTY_THREAD_HANDLE, ORPHAN_SENDER, OWNER_PHONE, Roster,
    Unassigned,
};

const IMESSAGE_SOURCE: &str = "imessage";
const SBR_SOURCE: &str = "sms-backup-restore";
const WHATSAPP_SOURCE: &str = "whatsapp";

#[derive(Clone, Copy)]
enum SourceFlavor {
    IMessage,
    SmsBackupRestore,
    Whatsapp,
}

/// Counts of contacts, conversations, messages, and attachments written this run.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct GenStats {
    /// Contacts invented.
    pub contacts: usize,
    /// Conversation files written across every backup folder.
    pub conversation_files: usize,
    /// Messages written.
    pub messages: usize,
    /// Attachment references written into messages.
    pub attachment_refs: usize,
    /// Group conversations written.
    pub groups: usize,
}

const TAPBACK_KINDS: &[&str] = &[
    "loved",
    "liked",
    "disliked",
    "laughed",
    "emphasized",
    "questioned",
    "emoji",
];

const PHOTO_CAPTIONS: &[&str] = &[
    "Check this out",
    "Thought you'd like this",
    "From yesterday",
    "Saw this and thought of you",
    "",
];

const EMOJI_ONLY: &[&str] = &["👍", "😂", "❤️", "🎉", "😊"];

/// Export metadata stamped on every conversation header.
fn export_meta(source: &str) -> ExportMeta {
    ExportMeta {
        source: source.into(),
        tool: "demo-seed".into(),
        tool_version: "0.2.0".into(),
        owner_handle: Some(OWNER_PHONE.into()),
        owner_display_name: Some("Me".into()),
    }
}

/// The three backup folders a run writes into.
pub struct StagingDirs<'a> {
    pub imessage: &'a Path,
    pub sbr: &'a Path,
    pub whatsapp: &'a Path,
}

/// Write every conversation file into the three backup folders and return counts.
///
/// One-to-one contacts are split into iMessage-only, Android-only, and overlap.
/// Groups, unassigned handles, empty threads, and WhatsApp copies are written
/// after that.
///
/// # Errors
///
/// Returns an error if a conversation file cannot be created or written.
pub fn write_all<R: Rng>(
    staging: &StagingDirs<'_>,
    roster: &Roster,
    cfg: &SeedConfig,
    corpus: &Corpus,
    rng: &mut R,
    attachment_digests: &HashMap<String, (String, u64)>,
) -> Result<GenStats> {
    clear_jsonl(staging.imessage)?;
    clear_jsonl(staging.sbr)?;
    clear_jsonl(staging.whatsapp)?;
    let mut seeder = Seeder {
        cfg,
        corpus,
        rng,
        attachment_digests,
        stats: GenStats {
            contacts: roster.contacts.len(),
            groups: roster.groups.len(),
            ..Default::default()
        },
    };
    seeder.write_all(staging, roster)?;
    Ok(seeder.stats)
}

/// Everything the writers share: the config, the corpus, the random source,
/// the attachment fingerprints, and the running counts. One value per run;
/// every writer is a method on it.
struct Seeder<'a, R: Rng> {
    cfg: &'a SeedConfig,
    corpus: &'a Corpus,
    rng: &'a mut R,
    attachment_digests: &'a HashMap<String, (String, u64)>,
    stats: GenStats,
}

impl<R: Rng> Seeder<'_, R> {
    /// Write the whole dataset in the order the doc on [`write_all`] gives.
    fn write_all(&mut self, staging: &StagingDirs<'_>, roster: &Roster) -> Result<()> {
        let mut one_to_one = contacts_with_one_to_one(roster);
        one_to_one.shuffle(&mut *self.rng);

        let overlap_count = self.cfg.sources.overlap_count.min(one_to_one.len());
        let (overlap, rest) = one_to_one.split_at(overlap_count);
        let android_only_count =
            crate::rounded_fraction(rest.len(), self.cfg.sources.android_only_fraction);
        let (android_only, imessage_only) = rest.split_at(android_only_count);

        for contact in imessage_only {
            let spec = Individual::for_contact(contact, SourceFlavor::IMessage);
            self.individual(staging.imessage, &spec)?;
        }
        for contact in android_only {
            let spec = Individual::for_contact(contact, SourceFlavor::SmsBackupRestore);
            self.individual(staging.sbr, &spec)?;
        }
        for contact in overlap {
            self.overlap_individual(staging, contact)?;
        }
        for ua in &roster.unassigned {
            let count = self.rng.random_range(4..16);
            self.unassigned(staging.imessage, ua, count)?;
        }
        for group in &roster.groups {
            self.group(staging.imessage, roster, group)?;
        }
        self.orphaned(staging.imessage)?;

        if self.cfg.edge_cases.empty_individual {
            write_header_only(
                staging.imessage,
                EMPTY_THREAD_HANDLE,
                IrConversationType::Individual,
                &[],
                IMESSAGE_SOURCE,
            )?;
            self.stats.conversation_files += 1;
        }
        if self.cfg.edge_cases.empty_group {
            write_header_only(
                staging.imessage,
                EMPTY_GROUP_HANDLE,
                IrConversationType::Group,
                &["+12125554503", "+13035555604"],
                IMESSAGE_SOURCE,
            )?;
            self.stats.conversation_files += 1;
        }

        // WhatsApp threads reuse the contact's phone number. Import treats them as
        // a separate platform on the same person.
        for contact in roster.contacts.iter().filter(|c| c.has_whatsapp) {
            let spec = Individual::for_contact(contact, SourceFlavor::Whatsapp);
            self.individual(staging.whatsapp, &spec)?;
        }
        Ok(())
    }
}

/// Contacts that have a phone number and a one-to-one conversation.
fn contacts_with_one_to_one(roster: &Roster) -> Vec<&Contact> {
    let mut contacts = Vec::new();
    for contact in &roster.contacts {
        if contact.phones.is_empty() {
            continue;
        }
        if !contact.has_one_to_one() {
            continue;
        }
        contacts.push(contact);
    }
    contacts
}

/// Delete existing `.jsonl` and `.json` files in `staging` so a regenerate does not leave leftovers.
///
/// # Errors
///
/// Returns an error if the directory cannot be listed or a file cannot be deleted.
fn clear_jsonl(staging: &Path) -> Result<()> {
    if !staging.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(staging)? {
        let entry = entry?;
        let path = entry.path();
        if is_json_conversation_file(&path) {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// True when `path` is a conversation file (`.jsonl` or `.json`).
fn is_json_conversation_file(path: &Path) -> bool {
    match path.extension() {
        Some(extension) => extension == "jsonl" || extension == "json",
        None => false,
    }
}

/// One one-to-one conversation to write: whose, how long, how many messages,
/// and for which backup source.
struct Individual<'a> {
    chat_id: &'a str,
    display: String,
    span_years: f64,
    msg_count: usize,
    flavor: SourceFlavor,
}

impl<'a> Individual<'a> {
    /// The contact's conversation as `flavor` would hold it. WhatsApp copies
    /// are shorter and more recent than the phone's own history.
    fn for_contact(contact: &'a Contact, flavor: SourceFlavor) -> Self {
        let (span_years, msg_count) = match flavor {
            SourceFlavor::Whatsapp => (
                contact.span_years.min(3.0),
                contact.message_count().clamp(20, 80),
            ),
            SourceFlavor::IMessage | SourceFlavor::SmsBackupRestore => {
                (contact.span_years, contact.message_count())
            }
        };
        Self {
            chat_id: contact.primary_phone(),
            display: contact.display_hint(),
            span_years,
            msg_count,
            flavor,
        }
    }
}

/// A message written to both sides of an overlapping conversation with the
/// same text and time, so import can treat the two copies as duplicates.
struct SharedMessage {
    timestamp: i64,
    from_me: bool,
    text: String,
}

impl SharedMessage {
    /// The plain message for one side.
    fn message(
        &self,
        guid: String,
        chat_id: &str,
        service: IrService,
        message_kind: IrMessageKind,
    ) -> IrMessage {
        IrMessage {
            guid,
            timestamp_unix_ms: self.timestamp,
            direction: if self.from_me {
                IrDirection::Outgoing
            } else {
                IrDirection::Incoming
            },
            service,
            message_kind,
            sender_handle: if self.from_me {
                None
            } else {
                Some(chat_id.into())
            },
            sender_display_name: None,
            subject: None,
            text: self.text.clone(),
            attachments: vec![],
            imessage: None,
            source: None,
        }
    }
}

/// An overlapping conversation: the shared messages both sides carry, the
/// iMessage side's full timeline, and how many extra Android-only messages
/// follow it.
struct Overlap<'a> {
    chat_id: &'a str,
    display_name: Option<String>,
    msg_count: usize,
    timestamps: &'a [i64],
    shared: &'a [SharedMessage],
    extra_n: usize,
}

impl Overlap<'_> {
    /// Timestamp to start extra Android messages from: the last shared
    /// message, else the last iMessage time, else the reference time.
    fn android_base_timestamp(&self, cfg: &SeedConfig) -> i64 {
        if let Some(shared) = self.shared.last() {
            return shared.timestamp;
        }
        if let Some(&timestamp) = self.timestamps.last() {
            return timestamp;
        }
        cfg.reference_time.timestamp_millis()
    }
}

impl<R: Rng> Seeder<'_, R> {
    /// Write one one-to-one conversation for a single backup source.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be created or a line cannot be written.
    fn individual(&mut self, staging: &Path, spec: &Individual<'_>) -> Result<()> {
        let chat_id = spec.chat_id;
        let msg_count = spec.msg_count;
        let flavor = spec.flavor;
        let participants =
            individual_participants(chat_id, optional_display_name(spec.display.clone()));
        let path = staging.join(sanitize_filename(chat_id) + ".jsonl");
        let mut file = open_jsonl(&path)?;
        write_conversation_header(
            &mut file,
            chat_id,
            IrConversationType::Individual,
            None,
            participants,
            msg_count,
            source_id(flavor),
        )?;

        let timestamps = self.timestamps(msg_count, spec.span_years, sample_direct_day_burst);
        let mut origin_guid: Option<String> = None;
        for (i, &ts) in timestamps.iter().enumerate() {
            let from_me = i % 3 != 0;
            let guid = format!("{}1to1-{chat_id}-{i}", guid_prefix(flavor));
            let mut msg = self.text_message(&guid, ts, from_me, chat_id, flavor);
            match flavor {
                SourceFlavor::IMessage => {
                    self.decorate_message(
                        &mut msg,
                        i,
                        msg_count,
                        chat_id,
                        from_me,
                        &mut origin_guid,
                    );
                }
                SourceFlavor::SmsBackupRestore => {
                    self.decorate_android_message(&mut msg, i, msg_count);
                }
                SourceFlavor::Whatsapp => {
                    // WhatsApp threads skip iMessage-only fields such as tapbacks and replies.
                }
            }
            self.emit(&mut file, msg)?;
        }
        self.stats.conversation_files += 1;
        Ok(())
    }

    /// Write the same contact into both the iMessage and Android folders.
    ///
    /// Shared messages use the same text and time so import can treat them as
    /// duplicates. The Android copy also gets extra messages that only exist there.
    ///
    /// # Errors
    ///
    /// Returns an error if either conversation file cannot be written.
    fn overlap_individual(&mut self, staging: &StagingDirs<'_>, contact: &Contact) -> Result<()> {
        let chat_id = contact.primary_phone();
        let msg_count = contact.message_count().max(1);
        let shared_raw = (msg_count as f64) * self.cfg.sources.overlap_shared_fraction;
        let shared_n = shared_raw.round().clamp(1.0, msg_count as f64) as usize;
        let extra_lo = self.cfg.sources.overlap_android_extra_min;
        let extra_hi = self.cfg.sources.overlap_android_extra_max.max(extra_lo + 1);
        let extra_n = self.rng.random_range(extra_lo..extra_hi);

        let timestamps = self.timestamps(msg_count, contact.span_years, sample_direct_day_burst);
        let shared: Vec<SharedMessage> = timestamps
            .iter()
            .take(shared_n)
            .enumerate()
            .map(|(i, &timestamp)| SharedMessage {
                timestamp,
                from_me: i % 3 != 0,
                text: format!("Shared demo message {i} with {chat_id}"),
            })
            .collect();
        let overlap = Overlap {
            chat_id,
            display_name: optional_display_name(contact.display_hint()),
            msg_count,
            timestamps: &timestamps,
            shared: &shared,
            extra_n,
        };
        self.overlap_imessage(staging.imessage, &overlap)?;
        self.overlap_android(staging.sbr, &overlap)
    }

    /// The iMessage side of an overlapping conversation: shared rows, then the rest.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written.
    fn overlap_imessage(&mut self, staging: &Path, overlap: &Overlap<'_>) -> Result<()> {
        let chat_id = overlap.chat_id;
        let path = staging.join(sanitize_filename(chat_id) + ".jsonl");
        let mut file = open_jsonl(&path)?;
        write_conversation_header(
            &mut file,
            chat_id,
            IrConversationType::Individual,
            None,
            individual_participants(chat_id, overlap.display_name.clone()),
            overlap.msg_count,
            IMESSAGE_SOURCE,
        )?;
        let mut origin_guid: Option<String> = None;
        for (i, shared) in overlap.shared.iter().enumerate() {
            let msg = shared.message(
                format!("1to1-{chat_id}-{i}"),
                chat_id,
                IrService::IMessage,
                IrMessageKind::IMessage,
            );
            self.emit(&mut file, msg)?;
        }
        let shared_n = overlap.shared.len();
        for (i, &timestamp) in overlap
            .timestamps
            .iter()
            .enumerate()
            .skip(shared_n)
            .take(overlap.msg_count.saturating_sub(shared_n))
        {
            let from_me = i % 3 != 0;
            let guid = format!("1to1-{chat_id}-{i}");
            let mut msg =
                self.text_message(&guid, timestamp, from_me, chat_id, SourceFlavor::IMessage);
            self.decorate_message(
                &mut msg,
                i,
                overlap.msg_count,
                chat_id,
                from_me,
                &mut origin_guid,
            );
            self.emit(&mut file, msg)?;
        }
        self.stats.conversation_files += 1;
        Ok(())
    }

    /// The Android side of an overlapping conversation: shared rows, then extra messages.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written.
    fn overlap_android(&mut self, staging: &Path, overlap: &Overlap<'_>) -> Result<()> {
        let chat_id = overlap.chat_id;
        let android_total = overlap.shared.len() + overlap.extra_n;
        let path = staging.join(sanitize_filename(chat_id) + ".jsonl");
        let mut file = open_jsonl(&path)?;
        write_conversation_header(
            &mut file,
            chat_id,
            IrConversationType::Individual,
            None,
            individual_participants(chat_id, overlap.display_name.clone()),
            android_total,
            SBR_SOURCE,
        )?;
        for (i, shared) in overlap.shared.iter().enumerate() {
            let msg = shared.message(
                format!("sbr-shared-{chat_id}-{i}"),
                chat_id,
                IrService::Sms,
                IrMessageKind::Sms,
            );
            self.emit(&mut file, msg)?;
        }
        let base_ts = overlap.android_base_timestamp(self.cfg);
        for j in 0..overlap.extra_n {
            let timestamp = base_ts + ((j as i64) + 1) * 60_000;
            let from_me = j % 4 == 0;
            let guid = format!("sbr-extra-{chat_id}-{j}");
            let mut msg = self.text_message(
                &guid,
                timestamp,
                from_me,
                chat_id,
                SourceFlavor::SmsBackupRestore,
            );
            self.decorate_android_message(&mut msg, j, overlap.extra_n);
            self.emit(&mut file, msg)?;
        }
        self.stats.conversation_files += 1;
        Ok(())
    }
}

/// `None` when the display name is empty, otherwise `Some`.
fn optional_display_name(display: String) -> Option<String> {
    if display.is_empty() {
        None
    } else {
        Some(display)
    }
}

/// One participant for a one-to-one conversation: the other person's phone or email.
fn individual_participants(chat_id: &str, display_name: Option<String>) -> Vec<IrParticipant> {
    vec![IrParticipant {
        handle: Some(chat_id.into()),
        display_name,
        handle_type: None,
    }]
}

/// Folder name for this backup source (`imessage`, `sms-backup-restore`, or `whatsapp`).
fn source_id(flavor: SourceFlavor) -> &'static str {
    match flavor {
        SourceFlavor::IMessage => IMESSAGE_SOURCE,
        SourceFlavor::SmsBackupRestore => SBR_SOURCE,
        SourceFlavor::Whatsapp => WHATSAPP_SOURCE,
    }
}

/// Prefix on message IDs so Android and WhatsApp IDs do not collide with iMessage.
fn guid_prefix(flavor: SourceFlavor) -> &'static str {
    match flavor {
        SourceFlavor::IMessage => "",
        SourceFlavor::SmsBackupRestore => "sbr-",
        SourceFlavor::Whatsapp => "wa-",
    }
}

impl<R: Rng> Seeder<'_, R> {
    /// Write a conversation for a phone or email that has no contact card.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written.
    fn unassigned(&mut self, staging: &Path, ua: &Unassigned, msg_count: usize) -> Result<()> {
        let chat_id = &ua.handle;
        let participants = individual_participants(chat_id, ua.name_alias.clone());
        let fname = if ua.email_only {
            format!("email-{}.jsonl", chat_id.replace('@', "_at_"))
        } else {
            sanitize_filename(chat_id) + ".jsonl"
        };
        let mut file = open_jsonl(&staging.join(fname))?;
        write_conversation_header(
            &mut file,
            chat_id,
            IrConversationType::Individual,
            None,
            participants,
            msg_count,
            IMESSAGE_SOURCE,
        )?;

        let timestamps = self.timestamps(msg_count, 1.5, sample_direct_day_burst);
        for (i, &ts) in timestamps.iter().enumerate() {
            let from_me = i % 4 == 0;
            let guid = format!("unassigned-{chat_id}-{i}");
            let mut msg = self.text_message(&guid, ts, from_me, chat_id, SourceFlavor::IMessage);
            if i == 2 && ua.name_alias.is_some() && !from_me {
                msg.sender_handle = Some(String::new());
            }
            if should_attach_jpg(i, msg_count, self.cfg) {
                self.add_jpg_attachment(&mut msg, i);
            }
            self.emit(&mut file, msg)?;
        }
        self.stats.conversation_files += 1;
        Ok(())
    }

    /// Write one group conversation. The first group starts with a rename announcement.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written.
    fn group(
        &mut self,
        staging: &Path,
        roster: &Roster,
        group: &crate::personas::GroupSpec,
    ) -> Result<()> {
        let chat_id = group_chat_id(group.index);
        let participants = group_participants(roster, group);
        if participants.len() < 2 && !group.phone_only {
            return Ok(());
        }

        // Demo group members always carry an address; a name-only participant has
        // nothing to send from.
        let handles: Vec<String> = participants
            .iter()
            .filter_map(|participant| participant.handle.clone())
            .collect();
        let msg_count = ((group.msgs_per_year * group.span_years).round() as isize).max(1) as usize;
        let timestamps = self.timestamps(msg_count, group.span_years, sample_group_day_burst);
        let mut file = open_jsonl(&staging.join(format!("group-{:03}.jsonl", group.index)))?;
        let header_message_count = if group.index == 0 {
            msg_count + 1
        } else {
            msg_count
        };
        write_conversation_header(
            &mut file,
            &chat_id,
            IrConversationType::Group,
            group.title.clone(),
            participants,
            header_message_count,
            IMESSAGE_SOURCE,
        )?;

        // The first group starts with a rename announcement so the UI has one to show.
        if group.index == 0 {
            let first_message_ts = timestamps
                .first()
                .copied()
                .unwrap_or_else(|| self.cfg.reference_time.timestamp_millis());
            let mut ann = self.text_message(
                "grp-0-rename",
                first_message_ts - 60_000,
                true,
                OWNER_PHONE,
                SourceFlavor::IMessage,
            );
            ann.text.clear();
            ann.message_kind = IrMessageKind::Announcement;
            let im = ann.imessage.get_or_insert_with(IrImessage::default);
            im.announcement = Some("Demo User named the conversation “Weekend Trip”.".into());
            self.emit(&mut file, ann)?;
        }

        let mut origin_guid: Option<String> = None;
        for i in 0..msg_count {
            let from_me = i % 7 == 0;
            let sender = if from_me {
                None
            } else {
                Some(handles[i % handles.len()].clone())
            };
            let guid = format!("grp-{}-{i}", group.index);
            let peer = sender.as_deref().unwrap_or(OWNER_PHONE);
            let mut msg =
                self.text_message(&guid, timestamps[i], from_me, peer, SourceFlavor::IMessage);
            msg.sender_handle = sender;
            if should_attach_jpg(i, msg_count, self.cfg) {
                self.add_jpg_attachment(&mut msg, i + group.index);
            } else if should_attach_other(i, msg_count, self.cfg) {
                self.add_attachment(&mut msg, i, OTHER_ATTACHMENTS);
            }
            let messages = &self.cfg.messages;
            if messages.tapback_stride > 0
                && i % messages.tapback_stride == 0
                && msg_count >= 10
                && !handles.is_empty()
            {
                let reactor = &handles[(i + 1) % handles.len()];
                let kind = TAPBACK_KINDS.choose(&mut *self.rng).unwrap();
                let emoji = tapback_emoji(kind, &mut *self.rng);
                push_tapback(&mut msg, kind, emoji, reactor, false);
            }
            if messages.reply_stride > 0 && i % messages.reply_stride == 0 && origin_guid.is_some()
            {
                let im = msg.imessage.get_or_insert_with(IrImessage::default);
                im.is_reply = true;
                im.in_reply_to_guid.clone_from(&origin_guid);
                im.thread_originator_part = Some(0);
            }
            if i % (messages.reply_stride.max(1) + 17) == 0 {
                origin_guid = Some(guid.clone());
            }
            self.emit(&mut file, msg)?;
        }
        self.stats.conversation_files += 1;
        Ok(())
    }
}

/// Participants for a group: named contacts, or phone numbers with no names.
fn group_participants(roster: &Roster, group: &crate::personas::GroupSpec) -> Vec<IrParticipant> {
    if group.phone_only {
        return phone_only_participants(&group.phone_only_handles);
    }
    named_group_participants(roster, &group.member_idxs)
}

/// Group members who are only phone numbers, with no display names.
fn phone_only_participants(handles: &[String]) -> Vec<IrParticipant> {
    let mut participants = Vec::with_capacity(handles.len());
    for handle in handles {
        participants.push(IrParticipant {
            handle: Some(handle.clone()),
            display_name: None,
            handle_type: None,
        });
    }
    participants
}

/// Group members looked up from the roster by index.
fn named_group_participants(roster: &Roster, member_idxs: &[usize]) -> Vec<IrParticipant> {
    let mut participants = Vec::new();
    for &index in member_idxs {
        let Some(contact) = roster.contacts.get(index) else {
            continue;
        };
        let hint = contact.display_hint();
        let display_name = optional_display_name(hint);
        participants.push(IrParticipant {
            handle: Some(contact.primary_phone().into()),
            display_name,
            handle_type: None,
        });
    }
    participants
}

impl<R: Rng> Seeder<'_, R> {
    /// Write messages that have a sender but no conversation to attach them to.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written.
    fn orphaned(&mut self, staging: &Path) -> Result<()> {
        let n = self.cfg.edge_cases.orphaned_messages.max(1);
        let mut file = open_jsonl(&staging.join("orphaned.jsonl"))?;
        write_conversation_header(
            &mut file,
            "orphaned",
            IrConversationType::Individual,
            None,
            vec![],
            n,
            IMESSAGE_SOURCE,
        )?;
        let timestamps = self.timestamps(n, 2.0, sample_direct_day_burst);
        for (i, &ts) in timestamps.iter().enumerate() {
            let guid = format!("orphan-{i}");
            let mut msg =
                self.text_message(&guid, ts, i % 2 == 0, ORPHAN_SENDER, SourceFlavor::IMessage);
            msg.text = format!("Orphaned message #{i} (no conversation association)");
            self.emit(&mut file, msg)?;
        }
        self.stats.conversation_files += 1;
        Ok(())
    }
}

/// Write a conversation header with no messages (empty individual or empty group).
///
/// # Errors
///
/// Returns an error if the file cannot be written.
fn write_header_only(
    staging: &Path,
    chat_id: &str,
    conv_type: IrConversationType,
    member_phones: &[&str],
    source: &str,
) -> Result<()> {
    let path = staging.join(format!("empty-{}.jsonl", sanitize_filename(chat_id)));
    let mut file = open_jsonl(&path)?;
    let mut participants = Vec::with_capacity(member_phones.len());
    for handle in member_phones {
        participants.push(IrParticipant {
            handle: Some((*handle).into()),
            display_name: None,
            handle_type: None,
        });
    }
    write_conversation_header(&mut file, chat_id, conv_type, None, participants, 0, source)?;
    Ok(())
}

impl<R: Rng> Seeder<'_, R> {
    /// Add photos, other files, tapbacks, replies, and occasional SMS/RCS
    /// labels to an iMessage. `origin_guid` is the thread every few messages
    /// reply to; this call may replace it.
    fn decorate_message(
        &mut self,
        msg: &mut IrMessage,
        i: usize,
        msg_count: usize,
        peer: &str,
        from_me: bool,
        origin_guid: &mut Option<String>,
    ) {
        if should_attach_jpg(i, msg_count, self.cfg) {
            self.add_jpg_attachment(msg, i);
            if i > 0 && i.is_multiple_of(40) {
                msg.text = PHOTO_CAPTIONS.choose(&mut *self.rng).unwrap().to_string();
            }
        } else if should_attach_photo_only(i, msg_count, self.cfg) {
            msg.text.clear();
            self.add_jpg_attachment(msg, i + 1);
        } else if should_attach_other(i, msg_count, self.cfg) {
            self.add_attachment(msg, i, OTHER_ATTACHMENTS);
        }
        let messages = &self.cfg.messages;
        if messages.tapback_stride > 0
            && i.is_multiple_of(messages.tapback_stride)
            && !from_me
            && msg_count >= 20
        {
            push_tapback(msg, "loved", None, peer, false);
        }
        if messages.reply_stride > 0
            && i.is_multiple_of(messages.reply_stride)
            && origin_guid.is_some()
            && msg_count >= 25
        {
            let im = msg.imessage.get_or_insert_with(IrImessage::default);
            im.is_reply = true;
            im.in_reply_to_guid.clone_from(origin_guid);
            im.thread_originator_part = Some(0);
        }
        if i.is_multiple_of(messages.reply_stride.max(1) + 29) {
            *origin_guid = Some(msg.guid.clone());
            let im = msg.imessage.get_or_insert_with(IrImessage::default);
            im.num_replies = Some(self.rng.random_range(1..4));
        }
        maybe_mark_as_sms_or_rcs(
            msg,
            self.cfg.messages.apple_fallback_transport_fraction,
            &mut *self.rng,
        );
    }
}

/// Sometimes mark an iMessage as SMS or RCS so the conversation view can show those labels.
fn maybe_mark_as_sms_or_rcs(msg: &mut IrMessage, fraction: f64, rng: &mut impl Rng) {
    if !rng.random_bool(fraction.clamp(0.0, 1.0)) {
        return;
    }
    if rng.random_bool(0.5) {
        msg.service = IrService::Sms;
    } else {
        msg.service = IrService::Rcs;
    }
    if matches!(
        msg.message_kind,
        IrMessageKind::Mms | IrMessageKind::Announcement
    ) {
        return;
    }
    msg.message_kind = IrMessageKind::Sms;
}

/// Create a buffered writer for a new conversation file.
///
/// # Errors
///
/// Returns an error if the file cannot be created.
fn open_jsonl(path: &Path) -> Result<BufWriter<File>> {
    let f = File::create(path).with_context(|| format!("create {}", path.display()))?;
    Ok(BufWriter::new(f))
}

/// Write the first line of a conversation file: schema, export info, and participants.
///
/// # Errors
///
/// Returns an error if the header cannot be serialized or written.
fn write_conversation_header(
    file: &mut BufWriter<File>,
    chat_id: &str,
    conv_type: IrConversationType,
    group_title: Option<String>,
    participants: Vec<IrParticipant>,
    message_count: usize,
    source: &str,
) -> Result<()> {
    let header = ConversationHeader {
        schema_version: SCHEMA_VERSION,
        export: export_meta(source),
        conversation: ConversationMeta {
            chat_identifier: chat_id.into(),
            conversation_type: conv_type,
            group_title,
            participants,
            stats: ConversationStats {
                message_count: message_count as u64,
                attachment_count: 0,
                first_timestamp_unix_ms: None,
                last_timestamp_unix_ms: None,
            },
        },
    };
    writeln!(file, "{}", serde_json::to_string(&header)?)?;
    Ok(())
}

/// Write one message as a JSON line. Empty iMessage extras are dropped first.
///
/// # Errors
///
/// Returns an error if the message cannot be serialized or written.
fn write_message(file: &mut BufWriter<File>, mut msg: IrMessage) -> Result<()> {
    if let Some(im) = msg.imessage.take() {
        msg.imessage = im.into_option();
    }
    writeln!(file, "{}", serde_json::to_string(&msg)?)?;
    Ok(())
}

impl<R: Rng> Seeder<'_, R> {
    /// Build a text message with a body from the book, or sometimes only an emoji.
    fn text_message(
        &mut self,
        guid: &str,
        timestamp_unix_ms: i64,
        from_me: bool,
        peer: &str,
        flavor: SourceFlavor,
    ) -> IrMessage {
        let text = if self.rng.random_bool(self.cfg.messages.emoji_probability) {
            (*EMOJI_ONLY.choose(&mut *self.rng).unwrap()).to_string()
        } else {
            self.corpus.pick_message(&mut *self.rng)
        };
        let (service, message_kind) = match flavor {
            SourceFlavor::IMessage => (IrService::IMessage, IrMessageKind::IMessage),
            SourceFlavor::SmsBackupRestore => (IrService::Sms, IrMessageKind::Sms),
            SourceFlavor::Whatsapp => (IrService::Whatsapp, IrMessageKind::Sms),
        };
        IrMessage {
            guid: guid.into(),
            timestamp_unix_ms,
            direction: if from_me {
                IrDirection::Outgoing
            } else {
                IrDirection::Incoming
            },
            service,
            message_kind,
            sender_handle: if from_me { None } else { Some(peer.into()) },
            sender_display_name: None,
            subject: None,
            text,
            attachments: vec![],
            imessage: None,
            source: None,
        }
    }

    /// Add photos or other files to an Android message and mark it as MMS when it has an attachment.
    fn decorate_android_message(&mut self, msg: &mut IrMessage, i: usize, msg_count: usize) {
        msg.service = IrService::Sms;
        if should_attach_jpg(i, msg_count, self.cfg) {
            self.add_jpg_attachment(msg, i);
            if i > 0 && i.is_multiple_of(40) {
                msg.text = PHOTO_CAPTIONS.choose(&mut *self.rng).unwrap().to_string();
            }
            msg.message_kind = IrMessageKind::Mms;
        } else if should_attach_other(i, msg_count, self.cfg) {
            self.add_attachment(msg, i, OTHER_ATTACHMENTS);
            msg.message_kind = IrMessageKind::Mms;
        } else {
            msg.message_kind = IrMessageKind::Sms;
        }
    }

    /// Write one message line and count it.
    fn emit(&mut self, file: &mut BufWriter<File>, msg: IrMessage) -> Result<()> {
        write_message(file, msg)?;
        self.stats.messages += 1;
        Ok(())
    }

    /// Spread `total` messages across the `span_years` before the reference
    /// time, with `sampler` deciding how busy each day is.
    fn timestamps(
        &mut self,
        total: usize,
        span_years: f64,
        sampler: fn(&mut R) -> usize,
    ) -> Vec<i64> {
        bursty_timestamps(
            total,
            span_years,
            self.cfg.reference_time,
            sampler,
            &mut *self.rng,
        )
    }
}

/// Spread messages across the date range with busy days, quiet gaps, and occasional floods.
fn bursty_timestamps<R: Rng, F: FnMut(&mut R) -> usize>(
    total: usize,
    span_years: f64,
    reference_time: chrono::DateTime<Utc>,
    mut sample_burst: F,
    rng: &mut R,
) -> Vec<i64> {
    if total == 0 {
        return Vec::new();
    }
    let span_days = ((span_years * 365.25).round() as i64).max(1);
    let start = reference_time - Duration::days(span_days);
    let offset = FixedOffset::west_opt(4 * 3600).unwrap();

    let per_day = assign_messages_to_days(total, span_days, &mut sample_burst, rng);
    let mut days: Vec<(i64, usize)> = per_day.into_iter().collect();
    days.sort_by_key(|(day, _)| *day);

    let mut out = Vec::with_capacity(total);
    for (day, count) in days {
        let day_start = start + Duration::days(day);
        append_day_timestamps(&mut out, day_start, count, offset, reference_time, rng);
    }
    out.sort_unstable();
    out
}

/// Assign bursts of messages to days, preferring recent days.
fn assign_messages_to_days<R: Rng, F: FnMut(&mut R) -> usize>(
    total: usize,
    span_days: i64,
    sample_burst: &mut F,
    rng: &mut R,
) -> HashMap<i64, usize> {
    let mut per_day: HashMap<i64, usize> = HashMap::new();
    let mut remaining = total;
    while remaining > 0 {
        let burst = sample_burst(rng).min(remaining);
        // Prefer recent days. Most days in the span get no messages.
        let unit: f64 = rng.random::<f64>().powf(0.65);
        let day_index = ((span_days - 1) as f64 * unit).round() as i64;
        let day = day_index.clamp(0, span_days - 1);
        *per_day.entry(day).or_default() += burst;
        remaining -= burst;
    }
    per_day
}

/// Place a day's messages between 8am and 11pm, a few seconds apart.
fn append_day_timestamps<R: Rng>(
    out: &mut Vec<i64>,
    day_start: chrono::DateTime<Utc>,
    count: usize,
    offset: FixedOffset,
    reference_time: chrono::DateTime<Utc>,
    rng: &mut R,
) {
    let mut seconds = Vec::with_capacity(count);
    for _ in 0..count {
        seconds.push(rng.random_range(8 * 3600..23 * 3600));
    }
    seconds.sort_unstable();
    for (i, secs) in seconds.into_iter().enumerate() {
        // Nudge messages a few seconds apart so a burst is not one identical timestamp.
        let spacing = (i as i64) * rng.random_range(8..45);
        let spaced = secs + spacing;
        let latest_second = 23 * 3600 + 3599;
        let mut dt = day_start + Duration::seconds(spaced.min(latest_second));
        if let Some(&prev) = out.last()
            && dt.timestamp_millis() <= prev
        {
            let prev_time = Utc
                .timestamp_millis_opt(prev)
                .single()
                .unwrap_or(reference_time);
            let gap = Duration::seconds(rng.random_range(12..90));
            dt = prev_time + gap;
        }
        let local = offset.from_utc_datetime(&dt.naive_utc());
        out.push(local.timestamp_millis());
    }
}

/// How many messages land on one active day in a one-to-one conversation.
fn sample_direct_day_burst(rng: &mut impl Rng) -> usize {
    let roll: f64 = rng.random();
    if roll < 0.08 {
        rng.random_range(12..=45)
    } else if roll < 0.20 {
        rng.random_range(1..=2)
    } else {
        rng.random_range(3..=10)
    }
}

/// How many messages land on one active day in a group conversation.
fn sample_group_day_burst(rng: &mut impl Rng) -> usize {
    let roll: f64 = rng.random();
    if roll < 0.10 {
        rng.random_range(16..=70)
    } else if roll < 0.22 {
        rng.random_range(1..=2)
    } else {
        rng.random_range(3..=12)
    }
}

/// Chat identifier for a group: a mix of `chat…` IDs and phone-looking IDs.
fn group_chat_id(index: usize) -> String {
    match index % 5 {
        0 => format!("chat{:010}", 1_000_000_000u64 + index as u64),
        1 => format!("+1800555{:04}", 1000 + (index % 9000)),
        2 => format!("+4477009{:05}", 10000 + (index % 80000)),
        3 => format!("+1212555{:04}", 2000 + (index % 7000)),
        _ => format!("chat{:010}", 2_000_000_000u64 + index as u64),
    }
}

/// True when this message index should get a JPEG with a caption.
fn should_attach_jpg(i: usize, total: usize, cfg: &SeedConfig) -> bool {
    if total < 8 {
        return false;
    }
    let stride = (cfg.messages.jpg_base_stride + total / 50).max(20);
    i > 0 && i.is_multiple_of(stride)
}

/// True when this message should be a photo with no text.
fn should_attach_photo_only(i: usize, total: usize, cfg: &SeedConfig) -> bool {
    if total < 20 {
        return false;
    }
    let stride = (cfg.messages.jpg_base_stride * 2 + total / 40).max(40);
    i % stride == 5
}

/// True when this message should get a non-JPEG attachment (PDF, audio, sticker, and so on).
fn should_attach_other(i: usize, total: usize, cfg: &SeedConfig) -> bool {
    if total < 30 {
        return false;
    }
    let stride = (cfg.messages.other_base_stride + total / 30).max(50);
    i > 0 && i.is_multiple_of(stride)
}

impl<R: Rng> Seeder<'_, R> {
    /// Attach a JPEG from the demo photo list and count it.
    fn add_jpg_attachment(&mut self, msg: &mut IrMessage, idx: usize) {
        let photo = &JPG_PHOTOS[idx % JPG_PHOTOS.len()];
        let (digest_sha256, size_bytes) = digest_fields(self.attachment_digests, photo.path);
        msg.attachments.push(IrAttachment {
            path: Some(photo.path.into()),
            original_name: Some(photo.original_name.into()),
            mime_type: Some("image/jpeg".into()),
            digest_sha256,
            is_sticker: false,
            transcription: None,
            sticker_effect: None,
            bytes: None,
            size_bytes,
            missing_reason: None,
        });
        self.stats.attachment_refs += 1;
    }

    /// Attach a non-JPEG file. Audio files get a short transcription.
    fn add_attachment(&mut self, msg: &mut IrMessage, idx: usize, files: &[(&str, &str, bool)]) {
        let (path, mime, is_sticker) = files[idx % files.len()];
        let (digest_sha256, size_bytes) = digest_fields(self.attachment_digests, path);
        let transcription = if mime.starts_with("audio/") {
            Some("Hey, just leaving a quick voice note.".into())
        } else {
            None
        };
        msg.attachments.push(IrAttachment {
            path: Some(path.into()),
            original_name: Some(filename_from_relative_path(path).into()),
            mime_type: Some(mime.into()),
            digest_sha256,
            is_sticker,
            transcription,
            sticker_effect: None,
            bytes: None,
            size_bytes,
            missing_reason: None,
        });
        self.stats.attachment_refs += 1;
    }
}

/// Look up the content fingerprint and size for an attachment path, if the file was written.
fn digest_fields(
    digests: &HashMap<String, (String, u64)>,
    path: &str,
) -> (Option<String>, Option<u64>) {
    match digests.get(path) {
        Some((digest, byte_len)) if !digest.is_empty() => (Some(digest.clone()), Some(*byte_len)),
        _ => (None, None),
    }
}

/// Last path segment of `path`, or the whole string if it has no `/`.
fn filename_from_relative_path(path: &str) -> &str {
    match path.rsplit('/').next() {
        Some(name) => name,
        None => path,
    }
}

/// Optional emoji for a tapback of kind `"emoji"`.
fn tapback_emoji(kind: &str, rng: &mut impl Rng) -> Option<String> {
    if kind != "emoji" {
        return None;
    }
    if !rng.random_bool(0.5) {
        return None;
    }
    let emoji = *EMOJI_ONLY.choose(rng).unwrap();
    Some(emoji.to_string())
}

/// Add a tapback (heart, thumbs-up, and similar) from `sender` onto `msg`.
fn push_tapback(
    msg: &mut IrMessage,
    kind: &str,
    emoji: Option<String>,
    sender: &str,
    from_me: bool,
) {
    let im = msg.imessage.get_or_insert_with(IrImessage::default);
    let mut taps = match im.tapbacks.take() {
        Some(serde_json::Value::Array(items)) => items,
        Some(other) if !other.is_null() => vec![other],
        _ => Vec::new(),
    };
    let sender_value = if from_me {
        serde_json::Value::Null
    } else {
        json!(sender)
    };
    taps.push(json!({
        "part_index": 0,
        "kind": kind,
        "emoji": emoji,
        "is_from_me": from_me,
        "sender": sender_value,
    }));
    im.tapbacks = Some(serde_json::Value::Array(taps));
}

/// Turn a phone or email into a safe file name (`+` becomes `p`, `@` becomes `a`).
fn sanitize_filename(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '+' => 'p',
            '@' => 'a',
            ':' | '/' | '\\' => '_',
            _ if c.is_ascii_alphanumeric() => c,
            _ => '_',
        })
        .collect()
}
