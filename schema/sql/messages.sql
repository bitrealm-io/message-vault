-- One chat thread per account + chat handle.
CREATE TABLE IF NOT EXISTS conversations (
    -- Surrogate primary key for this conversation.
    id INTEGER PRIMARY KEY,
    -- Owning vault account (`accounts.id`).
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Thread identity handle (`handles.id`); peer for 1:1, group chat id for groups.
    chat_handle_id INTEGER NOT NULL REFERENCES handles(id) ON DELETE CASCADE,
    -- Thread shape: 'individual' | 'group' (and any other values the importer writes).
    conversation_type TEXT NOT NULL,
    -- Group display title; NULL for 1:1 chats.
    group_title TEXT,
    -- When the source export was produced (if known).
    exported_at TEXT,
    -- Path or name of the source file this thread came from.
    source_file TEXT NOT NULL,
    UNIQUE(account_id, chat_handle_id)
);

CREATE INDEX IF NOT EXISTS ix_conversations_account_id ON conversations (account_id);

-- One other person listed in a conversation. The account holder is never a
-- participant (ADR-0015); a message records which of the holder's addresses it
-- used in `messages.owner_handle_id`.
CREATE TABLE IF NOT EXISTS participants (
    -- Surrogate primary key for this participant row.
    id INTEGER PRIMARY KEY,
    -- Parent conversation (`conversations.id`).
    conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    -- Participant identity (`handles.id`).
    -- Participant identity. NULL when the source named this person and
    -- recorded no address for them; `contact_id` then carries who they are.
    handle_id INTEGER REFERENCES handles(id) ON DELETE CASCADE,
    -- Resolved address-book contact when known (`contacts.id`).
    contact_id INTEGER REFERENCES contacts(id) ON DELETE SET NULL,
    -- Display name residue from the source for this participant.
    name_alias TEXT
);

-- A participant is one person's seat in one conversation. A participant
-- with an identity is that identity; one without is its contact. The two
-- indexes say so, because a UNIQUE over all three columns never matches a
-- row holding a NULL on either engine: a re-import added the name-only
-- participant again, and one whose contact an import replaced (ADR-0013)
-- got a second row for the same identity. `contact_id` stays out of the
-- first index because a handle's contact is read through `contact_handles`.
CREATE UNIQUE INDEX IF NOT EXISTS ux_participants_conversation_handle
    ON participants (conversation_id, handle_id)
    WHERE handle_id IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS ux_participants_conversation_name_only_contact
    ON participants (conversation_id, contact_id)
    WHERE handle_id IS NULL;

CREATE INDEX IF NOT EXISTS ix_participants_handle_id ON participants (handle_id);
CREATE INDEX IF NOT EXISTS ix_participants_contact_id ON participants (contact_id);

-- One message in a conversation.
CREATE TABLE IF NOT EXISTS messages (
    -- Surrogate primary key for this message.
    id INTEGER PRIMARY KEY,
    -- Parent conversation (`conversations.id`).
    conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    -- Owning vault account (`accounts.id`) denormalized for account-scoped queries.
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Backup/source family that produced this row (for example imessage, whatsapp).
    source TEXT NOT NULL,
    -- Source-native message id when available; used for exact dedupe with source.
    guid TEXT,
    -- The instant the message was sent, RFC 3339 in UTC with a Z suffix. Shown, searched
    -- and filed by day and year in the account's time zone (accounts.time_zone).
    timestamp TEXT NOT NULL,
    -- 1 = sent by the vault owner; 0 = received from someone else.
    is_from_me INTEGER NOT NULL,
    -- Sender identity (`handles.id`); NULL when unknown.
    sender_handle_id INTEGER REFERENCES handles(id) ON DELETE SET NULL,
    -- Per-message transport: sms / imessage / rcs / whatsapp / …
    service TEXT,
    -- Optional subject line (for example MMS subject).
    subject TEXT,
    -- Plain-text body; may be NULL for attachment-only or announcement rows.
    body TEXT,
    -- 1 = system/announcement bubble; 0 = normal user message.
    is_announcement INTEGER NOT NULL DEFAULT 0,
    -- 1 = this message is a threaded reply; 0 = top-level.
    is_reply INTEGER NOT NULL DEFAULT 0,
    -- GUID of the message this reply refers to (when is_reply = 1).
    thread_originator_guid TEXT,
    -- Part index within the originator message for multi-part replies.
    thread_originator_part INTEGER,
    -- Count of replies hanging off this message (denormalized from the source).
    num_replies INTEGER NOT NULL DEFAULT 0,
    -- Stable order within the conversation when timestamps collide.
    sort_order INTEGER NOT NULL,
    -- Hash fingerprint for cross-source duplicate detection (chat/direction/time/body/attachments).
    content_key TEXT,
    -- Points at the kept message when this row is a flagged duplicate.
    duplicate_of INTEGER REFERENCES messages(id) ON DELETE SET NULL,
    -- Import run that inserted this row (`vault_imports.id`).
    import_id INTEGER REFERENCES vault_imports(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS ix_messages_conversation_timestamp
    ON messages (conversation_id, timestamp);
CREATE INDEX IF NOT EXISTS ix_messages_conversation_source_timestamp
    ON messages (conversation_id, source, timestamp);
CREATE INDEX IF NOT EXISTS ix_messages_account_id ON messages (account_id);
-- `GET /v1/messages` and `GET /v1/export/messages` page an account's
-- messages by time; this serves the default sort without a scan.
CREATE INDEX IF NOT EXISTS ix_messages_account_timestamp
    ON messages (account_id, timestamp, id);
-- `GET /v1/contacts` computes when the vault last heard from each contact:
-- the newest message any of the contact's handles sent. This answers that
-- MAX(timestamp) per sender without a scan.
CREATE INDEX IF NOT EXISTS ix_messages_sender_timestamp
    ON messages (sender_handle_id, timestamp);
CREATE UNIQUE INDEX IF NOT EXISTS ix_messages_account_source_guid
    ON messages (account_id, source, guid)
    WHERE guid IS NOT NULL AND guid != '';
CREATE INDEX IF NOT EXISTS ix_messages_content_key
    ON messages (content_key)
    WHERE content_key IS NOT NULL AND content_key != '';
CREATE INDEX IF NOT EXISTS ix_messages_duplicate_of
    ON messages (duplicate_of)
    WHERE duplicate_of IS NOT NULL;
CREATE INDEX IF NOT EXISTS ix_messages_import_id
    ON messages (import_id)
    WHERE import_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS ix_messages_source ON messages (source);

-- File or media part attached to a message.
CREATE TABLE IF NOT EXISTS attachments (
    -- Surrogate primary key for this attachment.
    id INTEGER PRIMARY KEY,
    -- Parent message (`messages.id`).
    message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    -- Relative path from the export/staging layout when known.
    path TEXT,
    -- Original filename from the source.
    original_name TEXT,
    -- MIME type of the original bytes when known.
    mime_type TEXT,
    -- 1 = sticker; 0 = ordinary attachment.
    is_sticker INTEGER NOT NULL DEFAULT 0,
    -- Optional speech-to-text or OCR text for search.
    transcription TEXT,
    -- SHA-256 hex of the stored original bytes when present.
    sha256 TEXT,
    -- Path under the vault assets store for the original file.
    assets_path TEXT,
    -- Original file size in bytes when known.
    size_bytes INTEGER,
    -- Why bytes are absent (for example not_exported, decrypt_failed); NULL if present.
    missing_reason TEXT,
    -- SHA-256 hex of a converted/compressed derivative used by the browser.
    derived_sha256 TEXT,
    -- Path under the vault assets store for the derivative file.
    derived_assets_path TEXT,
    -- MIME type of the derivative file.
    derived_mime_type TEXT
);

CREATE INDEX IF NOT EXISTS ix_attachments_sha256 ON attachments (sha256);
CREATE INDEX IF NOT EXISTS ix_attachments_message_id ON attachments (message_id);

-- Reaction (tapback) on a message part.
CREATE TABLE IF NOT EXISTS tapbacks (
    -- Surrogate primary key for this reaction.
    id INTEGER PRIMARY KEY,
    -- Parent message (`messages.id`).
    message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    -- Which part of a multi-part message this reaction targets (0 = first/default).
    part_index INTEGER NOT NULL DEFAULT 0,
    -- Reaction kind from the source (for example loved, liked, emphasized).
    kind TEXT NOT NULL,
    -- Emoji glyph when the reaction is custom/emoji-based.
    emoji TEXT,
    -- 1 = reaction from the vault owner; 0 = from someone else.
    is_from_me INTEGER NOT NULL,
    -- Reactor identity (`handles.id`); NULL when unknown.
    sender_handle_id INTEGER REFERENCES handles(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS ix_tapbacks_message_id ON tapbacks (message_id);

-- Named tag a user can stamp on whole conversations.
CREATE TABLE IF NOT EXISTS message_tags (
    -- Surrogate primary key for this tag.
    id INTEGER PRIMARY KEY,
    -- Owning vault account (`accounts.id`).
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Tag text unique per account.
    name TEXT NOT NULL,
    UNIQUE(account_id, name)
);

-- Membership of a conversation in a message tag.
CREATE TABLE IF NOT EXISTS message_tag_members (
    -- Tagged conversation (`conversations.id`).
    conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    -- Tag that includes the conversation (`message_tags.id`).
    tag_id INTEGER NOT NULL REFERENCES message_tags(id) ON DELETE CASCADE,
    PRIMARY KEY (conversation_id, tag_id)
);

-- One message a running Export Run matched when it was created. The run's
-- pages read these rows rather than its scope, so an import, a trash, or a
-- new day between pages cannot move what the run hands over. The rows are
-- deleted when the run completes or is cancelled.
CREATE TABLE IF NOT EXISTS vault_export_messages (
    -- Export Run (`vault_exports.id`).
    export_id INTEGER NOT NULL REFERENCES vault_exports(id) ON DELETE CASCADE,
    -- The message's place in the run, from 1: oldest first by timestamp,
    -- then sort_order, then id, as they stood at creation.
    row_order INTEGER NOT NULL,
    -- Matched message (`messages.id`). NULL once the message is deleted: its
    -- place stays, empty, so the places after it do not move.
    message_id INTEGER REFERENCES messages(id) ON DELETE SET NULL,
    PRIMARY KEY (export_id, row_order)
);

CREATE INDEX IF NOT EXISTS ix_vault_export_messages_message
    ON vault_export_messages (message_id);
