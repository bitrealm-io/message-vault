//! Session caches (chats, handles, contacts, tapbacks).
//!
//! Addresses and chat members come from the reader's own reads of
//! `handle(ROWID, id)` and `chat_handle_join`, never from the handle cache of
//! `imessage-database`. That cache is built for exports a person reads: it
//! joins every address of one person into one string, maps handle 0 to "Me",
//! and has no entry for a chat with no handle rows. The library still reads
//! the chats, the message bodies and the attachments.

use std::collections::{BTreeSet, HashMap};

use imessage_database::{
    tables::{
        chat::Chat,
        messages::Message,
        table::{Cacheable, ME, UNKNOWN},
    },
    util::{dates::get_offset, platform::Platform},
};
use rusqlite::Connection;

use crate::{
    data_source::DataSource, error::RuntimeError, identities::OwnerAddresses,
    options::ReaderOptions,
};

/// Setup steps `MailSession::new` runs before any message is read.
const CACHE_STEPS: u64 = 4;

/// Apple's `chat.style` for a group chat.
const STYLE_GROUP: i64 = 43;
/// Apple's `chat.style` for a one-to-one chat.
const STYLE_ONE_TO_ONE: i64 = 45;

/// Cached chats, handles, contacts, and tapbacks for one conversion run.
pub(crate) struct MailSession {
    pub options: ReaderOptions,
    pub offset: i64,
    pub data_source: DataSource,
    pub chatrooms: HashMap<i32, Chat>,
    /// Apple's `chat.style` by chat rowid; empty when the column is absent.
    chat_styles: HashMap<i32, i64>,
    /// The handle rowids `chat_handle_join` lists for each chat.
    pub chat_members: HashMap<i32, BTreeSet<i32>>,
    /// One address per handle rowid: that row's `id`.
    handles: HashMap<i32, String>,
    /// The contact name for each handle whose address the contacts know.
    handle_names: HashMap<i32, String>,
    /// The addresses the backup names as the owner's.
    owner: OwnerAddresses,
    /// Tapbacks keyed by target message GUID → part index → reactions.
    pub tapbacks: HashMap<String, HashMap<usize, Vec<Message>>>,
}

impl MailSession {
    /// Load chats, handles, contacts, and tapbacks from the Messages database.
    ///
    /// # Errors
    ///
    /// Returns an error when the data source cannot be opened or a cache query
    /// fails.
    pub fn new(options: ReaderOptions) -> Result<Self, RuntimeError> {
        let data_source = DataSource::from(&options)?;
        let db = data_source.db();

        options.emit_log("Building cache...");
        options.setup_step(1, CACHE_STEPS, "Caching chats");
        let chatrooms = Chat::cache(db)?;
        let chat_styles = chat_styles(db);

        options.setup_step(2, CACHE_STEPS, "Caching chatrooms");
        let chat_members = chat_members(db)?;

        options.setup_step(3, CACHE_STEPS, "Caching participants");
        let handles = handles(db)?;
        let handle_names = handles
            .iter()
            .filter_map(|(&rowid, address)| {
                contact_name(&data_source, address).map(|name| (rowid, name))
            })
            .collect();
        let backup_root = matches!(options.platform, Platform::iOS).then_some(&options.db_path);
        let owner = OwnerAddresses::read(db, backup_root.map(|p| p.as_path()));

        options.setup_step(4, CACHE_STEPS, "Caching tapbacks");
        let tapbacks = Message::cache(db)?;
        options.emit_log("Cache built!");

        Ok(Self {
            chatrooms,
            chat_styles,
            chat_members,
            handles,
            handle_names,
            owner,
            tapbacks,
            options,
            offset: get_offset(),
            data_source,
        })
    }

    /// The chat row a message belongs to, or `None` for a message in no chat.
    pub fn conversation(&self, message: &Message) -> Option<&Chat> {
        let chat_id = message.chat_id.or(message.deleted_from)?;
        let chat = self.chatrooms.get(&chat_id);
        if chat.is_none() {
            self.options
                .emit_log(format!("Chat ID {chat_id} does not exist in chat table!"));
        }
        chat
    }

    /// Whether a chat is a group. Apple's `chat.style` decides; where the
    /// column is absent or holds another value, a group id (`chat` and
    /// digits) makes a group. Members and title decide nothing.
    pub fn is_group(&self, chat: &Chat) -> bool {
        match self.chat_styles.get(&chat.rowid) {
            Some(&STYLE_GROUP) => true,
            Some(&STYLE_ONE_TO_ONE) => false,
            _ => is_group_id(&chat.chat_identifier),
        }
    }

    /// The address of a handle: its row's `id`. Handle 0 is no handle.
    pub fn handle_address(&self, handle_id: i32) -> Option<&str> {
        self.handles.get(&handle_id).map(String::as_str)
    }

    /// The contact name of a handle, when the contacts know its address.
    pub fn handle_name(&self, handle_id: i32) -> Option<&str> {
        self.handle_names.get(&handle_id).map(String::as_str)
    }

    /// The contact name of an address, when the contacts know it.
    pub fn address_name(&self, address: &str) -> Option<String> {
        contact_name(&self.data_source, address)
    }

    /// Whether an address is one of the owner's.
    pub fn is_owner(&self, address: &str) -> bool {
        self.owner.contains(address)
    }

    /// Display name for the sender: Me / caller id, a contact name, the
    /// handle's address, or Unknown.
    pub fn who<'a, 'b: 'a>(
        &'a self,
        handle_id: Option<i32>,
        is_from_me: bool,
        destination_caller_id: Option<&'b str>,
    ) -> &'a str {
        if is_from_me {
            if self.options.use_caller_id {
                return destination_caller_id.unwrap_or(ME);
            }
            return ME;
        }
        handle_id
            .and_then(|id| self.handle_name(id).or_else(|| self.handle_address(id)))
            .unwrap_or(UNKNOWN)
    }
}

/// Whether a `chat_identifier` is Apple's group id: `chat` followed by
/// digits.
fn is_group_id(identifier: &str) -> bool {
    identifier
        .strip_prefix("chat")
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
}

/// The contact's full name for one address, if the contacts have one.
fn contact_name(data_source: &DataSource, address: &str) -> Option<String> {
    data_source
        .contacts_index
        .lookup(address)
        .map(|name| name.full)
        .filter(|full| !full.is_empty())
}

/// Every handle row's address, by rowid. A row with no `id` is left out.
fn handles(db: &Connection) -> Result<HashMap<i32, String>, RuntimeError> {
    let mut statement = db.prepare("SELECT ROWID, id FROM handle")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, i32>(0)?, row.get::<_, Option<String>>(1)?))
    })?;
    let mut out = HashMap::new();
    for row in rows {
        if let (rowid, Some(id)) = row?
            && !id.trim().is_empty()
        {
            out.insert(rowid, id);
        }
    }
    Ok(out)
}

/// The handle rowids `chat_handle_join` lists for each chat.
fn chat_members(db: &Connection) -> Result<HashMap<i32, BTreeSet<i32>>, RuntimeError> {
    let mut statement = db.prepare("SELECT chat_id, handle_id FROM chat_handle_join")?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, i32>(0)?, row.get::<_, i32>(1)?)))?;
    let mut out: HashMap<i32, BTreeSet<i32>> = HashMap::new();
    for row in rows {
        let (chat_id, handle_id) = row?;
        out.entry(chat_id).or_default().insert(handle_id);
    }
    Ok(out)
}

/// Apple's `chat.style` by chat rowid. An older database without the
/// column gives an empty map, and the identifier decides instead.
fn chat_styles(db: &Connection) -> HashMap<i32, i64> {
    let Ok(mut statement) = db.prepare("SELECT ROWID, style FROM chat") else {
        return HashMap::new();
    };
    let Ok(rows) = statement.query_map([], |row| {
        Ok((row.get::<_, i32>(0)?, row.get::<_, Option<i64>>(1)?))
    }) else {
        return HashMap::new();
    };
    rows.flatten()
        .filter_map(|(rowid, style)| style.map(|style| (rowid, style)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::FixtureDb;
    use chat_db_fixture::{
        FRIEND_EMAIL, FRIEND_PHONE, FRIEND_PHONE_EMAIL, GROUP_CHAT_IDENTIFIER, OWNER, OWNER_EMAIL,
    };

    /// The caches hold what the fixture wrote: six chats with their styles,
    /// the join rows as written, and each handle with its own address, not
    /// the joined string `imessage-database` makes of one person's two
    /// addresses. Handle 0 is no handle.
    #[test]
    fn a_session_caches_the_chats_and_handles() {
        let fixture = FixtureDb::write();
        let session = fixture.session();

        assert_eq!(session.chatrooms.len(), 6);
        assert_eq!(session.chatrooms[&2].chat_identifier, GROUP_CHAT_IDENTIFIER);
        assert_eq!(session.chat_members[&1], BTreeSet::from([1]));
        assert_eq!(session.chat_members[&2], BTreeSet::from([1, 2, 4]));
        assert!(!session.chat_members.contains_key(&5));

        assert_eq!(session.handle_address(1), Some(FRIEND_PHONE));
        assert_eq!(session.handle_address(2), Some(FRIEND_EMAIL));
        assert_eq!(session.handle_address(3), Some(FRIEND_PHONE_EMAIL));
        assert_eq!(session.handle_address(0), None);
        assert_eq!(session.handle_address(99), None);
        assert_eq!(session.handle_name(1), None, "no contacts file, so no name");
        assert!(session.tapbacks.is_empty());
    }

    /// Every address the backup names as the owner's counts, from either
    /// column, and nobody else's does.
    #[test]
    fn the_owner_is_known_by_every_address_the_backup_names() {
        let fixture = FixtureDb::write();
        let session = fixture.session();
        assert!(session.is_owner(OWNER));
        assert!(session.is_owner(OWNER_EMAIL));
        assert!(!session.is_owner(FRIEND_PHONE));
    }

    /// A message finds its chat by `chat_id`; a message whose chat id names
    /// no chat row lands nowhere, and so does a message with no chat id. A
    /// chat with no handle rows is still found.
    #[test]
    fn a_message_resolves_to_its_conversation() {
        let fixture = FixtureDb::write();
        let session = fixture.session();
        let messages = FixtureDb::messages(&session);
        assert_eq!(messages.len(), 12);

        let rowid = |index: usize| session.conversation(&messages[index]).map(|c| c.rowid);
        assert_eq!(rowid(0), Some(1));
        assert_eq!(rowid(2), Some(2));
        assert_eq!(rowid(3), Some(3), "the chat on the owner's email account");
        assert_eq!(
            rowid(4),
            Some(1),
            "a row with no caller id still finds its chat by chat_id"
        );
        assert_eq!(rowid(6), Some(5), "a chat with no handle rows");
        assert_eq!(rowid(11), None, "a message in no chat");

        let mut orphan = FixtureDb::messages(&session).remove(0);
        orphan.chat_id = Some(42);
        orphan.deleted_from = None;
        assert!(session.conversation(&orphan).is_none());
    }

    /// Style 43 is a group and 45 one-to-one. Without a style, only a group
    /// id makes a group: an address that starts with "chat" does not.
    #[test]
    fn a_group_id_is_chat_and_digits() {
        assert!(is_group_id("chat100"));
        assert!(!is_group_id("chat"));
        assert!(!is_group_id("chatty@example.com"));
        assert!(!is_group_id(FRIEND_PHONE));
    }

    /// The sender's name: the owner by caller id, a contact by name, a bare
    /// handle when the book has no name for it, and Unknown for nobody,
    /// handle 0 included.
    #[test]
    fn who_names_the_owner_a_contact_or_unknown() {
        let fixture = FixtureDb::write();
        let session = fixture.session_with_contacts();

        assert_eq!(session.who(None, true, Some(OWNER)), OWNER);
        assert_eq!(session.who(None, true, None), ME);
        assert_eq!(session.who(Some(1), false, None), "Sam Example");
        assert_eq!(session.who(Some(2), false, None), "Robin");
        assert_eq!(session.who(Some(99), false, None), UNKNOWN);
        assert_eq!(session.who(Some(0), false, None), UNKNOWN);
        assert_eq!(session.who(None, false, None), UNKNOWN);

        // Without the caller id option the owner is always Me.
        let mut options = fixture.options();
        options.use_caller_id = false;
        let session = MailSession::new(options).unwrap();
        assert_eq!(session.who(None, true, Some(OWNER)), ME);
        assert_eq!(
            session.who(Some(1), false, None),
            FRIEND_PHONE,
            "no contacts file: the handle stands in for the name"
        );
    }
}
