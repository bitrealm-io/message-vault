-- Address-book person for one vault account.
CREATE TABLE IF NOT EXISTS contacts (
    -- Surrogate primary key for this contact row.
    id INTEGER PRIMARY KEY,
    -- Owning vault account (`accounts.id`).
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Display name shown in the UI. Empty until something supplies a name;
    -- a contact with identities and no preferred name is Unknown.
    preferred_name TEXT NOT NULL,
    -- Where this row came from: 'address_book', 'import', or 'user'. Loading
    -- an address book replaces only the rows the address book owns.
    origin TEXT NOT NULL DEFAULT 'user',
    -- When the row was first recorded. Stored and queryable; not displayed.
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    -- Address-book shape last changed (not message activity).
    last_modified TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS ix_contacts_account_id ON contacts (account_id);

-- One platform identity (phone, email, username, or other) per account.
CREATE TABLE IF NOT EXISTS handles (
    -- Surrogate primary key for this handle row.
    id INTEGER PRIMARY KEY,
    -- Owning vault account (`accounts.id`).
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Identity string exactly as the backup/source wrote it.
    raw TEXT NOT NULL,
    -- Dedup key: E.164 when unambiguous for phones; otherwise cleaned digits/text.
    normalized TEXT NOT NULL,
    -- Human-readable reason the number needs review; NULL when normalization is trusted.
    normalized_note TEXT,
    -- Shape of the identity: 'phone' | 'email' | 'username' | 'other'.
    handle_type TEXT NOT NULL,
    -- Platform identity: 'phone' | 'whatsapp' (not per-message SMS/iMessage/RCS).
    service TEXT NOT NULL,
    -- Where this row came from: 'address_book', 'import', or 'user'.
    origin TEXT NOT NULL DEFAULT 'import',
    -- When the identity was first recorded. Queryable; not displayed.
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    -- When the identity last changed. Queryable; not displayed.
    last_modified TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(account_id, normalized, handle_type, service)
);

CREATE INDEX IF NOT EXISTS ix_handles_account_id ON handles (account_id);
CREATE INDEX IF NOT EXISTS ix_handles_normalized ON handles (account_id, normalized);

-- Links one handle to at most one contact within an account.
CREATE TABLE IF NOT EXISTS contact_handles (
    -- Owning vault account (`accounts.id`).
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Linked identity (`handles.id`).
    handle_id INTEGER NOT NULL REFERENCES handles(id) ON DELETE CASCADE,
    -- Address-book person that owns this handle (`contacts.id`).
    contact_id INTEGER NOT NULL REFERENCES contacts(id) ON DELETE CASCADE,
    -- Where this link came from: 'address_book', 'import', or 'user'.
    origin TEXT NOT NULL DEFAULT 'import',
    PRIMARY KEY (account_id, handle_id)
);

CREATE INDEX IF NOT EXISTS ix_contact_handles_contact_id
    ON contact_handles (contact_id);

-- Named group a user can attach to contacts.
CREATE TABLE IF NOT EXISTS contact_groups (
    -- Surrogate primary key for this group.
    id INTEGER PRIMARY KEY,
    -- Owning vault account (`accounts.id`).
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Group text unique per account.
    name TEXT NOT NULL,
    -- How the row was born: 'manual' when a person made it, 'import' when the
    -- server created it at the end of an import run. An import group is a
    -- shortcut pointing at that run and the person may delete it; the run's own
    -- record is permanent. Unknown is not stored here — its membership is
    -- computed from contact state.
    kind TEXT NOT NULL DEFAULT 'manual',
    UNIQUE(account_id, name)
);

-- Membership of a contact in a group.
CREATE TABLE IF NOT EXISTS contact_group_members (
    -- Contact in the group (`contacts.id`).
    contact_id INTEGER NOT NULL REFERENCES contacts(id) ON DELETE CASCADE,
    -- Group that includes the contact (`contact_groups.id`).
    group_id INTEGER NOT NULL REFERENCES contact_groups(id) ON DELETE CASCADE,
    PRIMARY KEY (contact_id, group_id)
);

-- Soft-delete marker for a conversation; chat rows stay until purge.
CREATE TABLE IF NOT EXISTS trashed_conversations (
    -- Owning vault account (`accounts.id`).
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Conversation marked trash (`conversations.id`, no FK so chat can remain).
    conversation_id INTEGER NOT NULL,
    -- When the conversation entered trash (SQLite datetime string).
    trashed_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (account_id, conversation_id)
);

-- Soft-delete marker for a contact; contact row stays until purge.
CREATE TABLE IF NOT EXISTS trashed_contacts (
    -- Owning vault account (`accounts.id`).
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Contact marked trash (`contacts.id`, no FK so contact can remain).
    contact_id INTEGER NOT NULL,
    -- When the contact entered trash (SQLite datetime string).
    trashed_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (account_id, contact_id)
);
