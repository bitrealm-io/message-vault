-- Import scratch copy of conversations; cleared/promoted per account during import.
CREATE TABLE IF NOT EXISTS staging_conversations (
    -- Surrogate primary key for this staging conversation.
    id INTEGER PRIMARY KEY,
    -- Owning vault account (`accounts.id`).
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Thread identity handle id (resolved into handles during staging).
    chat_handle_id INTEGER NOT NULL,
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

-- Import scratch copy of participants.
CREATE TABLE IF NOT EXISTS staging_participants (
    -- Surrogate primary key for this staging participant.
    id INTEGER PRIMARY KEY,
    -- Parent staging conversation (`staging_conversations.id`).
    conversation_id INTEGER NOT NULL REFERENCES staging_conversations(id) ON DELETE CASCADE,
    -- Participant identity handle id (resolved during staging). NULL when the
    -- source named the person and recorded no address for them.
    handle_id INTEGER,
    -- Optional contact id when already resolved during staging.
    contact_id INTEGER,
    -- Display name residue from the source for this participant.
    name_alias TEXT,
    UNIQUE(conversation_id, handle_id, contact_id)
);

-- Import scratch copy of messages (no content_key / duplicate_of until promote).
CREATE TABLE IF NOT EXISTS staging_messages (
    -- Surrogate primary key for this staging message.
    id INTEGER PRIMARY KEY,
    -- Parent staging conversation (`staging_conversations.id`).
    conversation_id INTEGER NOT NULL REFERENCES staging_conversations(id) ON DELETE CASCADE,
    -- Owning vault account (`accounts.id`).
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Backup/source family that produced this row.
    source TEXT NOT NULL,
    -- Source-native message id when available.
    guid TEXT,
    -- The instant the message was sent, RFC 3339 in UTC with a Z suffix. Shown, searched
    -- and filed by day and year in the account's time zone (accounts.time_zone).
    timestamp TEXT NOT NULL,
    -- 1 = sent by the vault owner; 0 = received from someone else.
    is_from_me INTEGER NOT NULL,
    -- Sender identity handle id; NULL when unknown.
    sender_handle_id INTEGER,
    -- Per-message transport: sms / imessage / rcs / whatsapp / …
    service TEXT,
    -- Optional subject line.
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
    -- Import run that staged this row (`vault_imports.id`).
    import_id INTEGER REFERENCES vault_imports(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS ix_staging_messages_conversation_timestamp
    ON staging_messages (conversation_id, timestamp);
CREATE INDEX IF NOT EXISTS ix_staging_messages_account_id ON staging_messages (account_id);
CREATE UNIQUE INDEX IF NOT EXISTS ix_staging_messages_account_source_guid
    ON staging_messages (account_id, source, guid)
    WHERE guid IS NOT NULL AND guid != '';

-- Import scratch copy of attachments.
CREATE TABLE IF NOT EXISTS staging_attachments (
    -- Surrogate primary key for this staging attachment.
    id INTEGER PRIMARY KEY,
    -- Parent staging message (`staging_messages.id`).
    message_id INTEGER NOT NULL REFERENCES staging_messages(id) ON DELETE CASCADE,
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
    -- Why bytes are absent; NULL if present.
    missing_reason TEXT,
    -- SHA-256 hex of a converted/compressed derivative.
    derived_sha256 TEXT,
    -- Path under the vault assets store for the derivative file.
    derived_assets_path TEXT,
    -- MIME type of the derivative file.
    derived_mime_type TEXT
);

CREATE INDEX IF NOT EXISTS ix_staging_attachments_sha256 ON staging_attachments (sha256);
CREATE INDEX IF NOT EXISTS ix_staging_attachments_message_id ON staging_attachments (message_id);

-- Import scratch copy of tapbacks.
CREATE TABLE IF NOT EXISTS staging_tapbacks (
    -- Surrogate primary key for this staging reaction.
    id INTEGER PRIMARY KEY,
    -- Parent staging message (`staging_messages.id`).
    message_id INTEGER NOT NULL REFERENCES staging_messages(id) ON DELETE CASCADE,
    -- Which part of a multi-part message this reaction targets (0 = first/default).
    part_index INTEGER NOT NULL DEFAULT 0,
    -- Reaction kind from the source (for example loved, liked, emphasized).
    kind TEXT NOT NULL,
    -- Emoji glyph when the reaction is custom/emoji-based.
    emoji TEXT,
    -- 1 = reaction from the vault owner; 0 = from someone else.
    is_from_me INTEGER NOT NULL,
    -- Reactor identity handle id; NULL when unknown.
    sender_handle_id INTEGER
);

CREATE INDEX IF NOT EXISTS ix_staging_tapbacks_message_id ON staging_tapbacks (message_id);
