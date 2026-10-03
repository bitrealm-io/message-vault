-- Login account (web UI + API owner).
CREATE TABLE IF NOT EXISTS accounts (
    -- Account id. Ids below 100 are reserved for accounts the server makes
    -- itself: the owner is 1 and the demo account 2. Every other account takes
    -- the next id above both that range and the highest id the table has ever
    -- held. AUTOINCREMENT keeps that high mark in sqlite_sequence, so a
    -- deleted account's id, and any folder of its files left behind, never
    -- passes to a new account.
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    -- Login user id; unique case-insensitively.
    username TEXT NOT NULL UNIQUE COLLATE NOCASE,
    -- Password verifier hash; NULL when password auth is unused.
    password_hash TEXT,
    -- Display name for “you” in the UI.
    preferred_name TEXT,
    -- IANA time zone (for example America/New_York) every message time, day and year is
    -- read in. Chosen at profile setup; a message records only the instant it arrived.
    time_zone TEXT NOT NULL DEFAULT 'UTC',
    -- 1 = may not log in and existing sessions are refused; 0 = active.
    disabled INTEGER NOT NULL DEFAULT 0,
    -- 1 = the account holder has not set up their profile yet, so the session
    -- owes profile setup before it goes anywhere; cleared on the first save.
    must_set_up_profile INTEGER NOT NULL DEFAULT 0,
    -- 1 = may call the import endpoints.
    can_import INTEGER NOT NULL DEFAULT 1,
    -- 1 = may call the export endpoints.
    can_export INTEGER NOT NULL DEFAULT 1,
    -- 1 = may destroy message data for good (delete from the trash, empty it,
    -- and delete the account's messages).
    can_delete INTEGER NOT NULL DEFAULT 1,
    -- RFC 3339 UTC instant of the last successful login (login, claiming
    -- Message Crate, or registering); NULL until the account has logged in once.
    last_login_at TEXT
);

-- Email addresses attached to an account (not used for login).
CREATE TABLE IF NOT EXISTS account_emails (
    -- Owning account (`accounts.id`).
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Email address; unique case-insensitively across all accounts.
    email TEXT NOT NULL UNIQUE COLLATE NOCASE,
    -- 1 = primary email for this account; at most one per account via partial index.
    is_primary INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (account_id, email)
);

CREATE UNIQUE INDEX IF NOT EXISTS ix_account_emails_one_primary
    ON account_emails(account_id)
    WHERE is_primary = 1;

-- Handles that mean “me” when matching message participants.
CREATE TABLE IF NOT EXISTS account_handles (
    -- Owning account (`accounts.id`).
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Self identity (`handles.id`).
    handle_id INTEGER NOT NULL REFERENCES handles(id) ON DELETE CASCADE,
    PRIMARY KEY (account_id, handle_id)
);

-- GUI session Bearer (one per account; rotates on login). Prefix: mc-user-
CREATE TABLE IF NOT EXISTS account_session_tokens (
    -- Owning account (`accounts.id`); also the primary key (one session).
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Hash of the session Bearer secret (never store the raw token).
    token_hash TEXT NOT NULL UNIQUE,
    -- Unix-seconds string for when this session token was issued.
    created_at TEXT NOT NULL,
    -- Unix-seconds string; session rejected after this time.
    expires_at TEXT NOT NULL DEFAULT '0',
    -- Which app last used this session: 'desktop' or 'website'. NULL until a
    -- request names one.
    app_kind TEXT,
    -- The Build that app reported, e.g. '0.9.0+343fe0d8'. Set with app_kind,
    -- and rewritten only when either differs from what a request sends.
    app_build TEXT,
    -- The Audit Trail's `logged_in` entry for this session
    -- (`audit_entries.id`), so the entry that ends it can name it. NULL for a
    -- session made outside a login, such as a test's.
    login_entry_id INTEGER REFERENCES audit_entries(id),
    PRIMARY KEY (account_id)
);

-- Named API tokens (many per account). Prefix: mc-api-
CREATE TABLE IF NOT EXISTS account_api_tokens (
    -- Token id.
    id INTEGER PRIMARY KEY,
    -- Owning account (`accounts.id`).
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- User-visible label in Settings.
    label TEXT NOT NULL,
    -- Hash of the API Bearer secret (never store the raw token).
    token_hash TEXT NOT NULL UNIQUE,
    -- 1 = this token may call the import endpoints.
    can_import INTEGER NOT NULL DEFAULT 1,
    -- 1 = this token may call the export endpoints.
    can_export INTEGER NOT NULL DEFAULT 1,
    -- Masked form for Settings (e.g. mc-api-Sd..mE). Not enough to recover the secret.
    token_hint TEXT NOT NULL DEFAULT 'mc-api-..',
    -- Unix-seconds string for when this API token was created.
    created_at TEXT NOT NULL,
    -- Unix-seconds string; NULL until first successful Bearer use.
    last_accessed_at TEXT,
    -- Unix-seconds string; NULL means no expiry.
    expires_at TEXT,
    -- Soft-disable without deleting the row.
    disabled INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS ix_account_api_tokens_account
    ON account_api_tokens(account_id);

-- Per-account key/value preferences for the UI and server.
CREATE TABLE IF NOT EXISTS account_prefs (
    -- Owning account (`accounts.id`).
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Preference name (for example theme or feature flags).
    key TEXT NOT NULL,
    -- Preference value stored as text.
    value TEXT NOT NULL,
    PRIMARY KEY (account_id, key)
);

-- Settings that belong to the whole Message Crate rather than to one account. Exactly
-- one row, so the owner reads and writes it without an id.
CREATE TABLE IF NOT EXISTS server_settings (
    -- Always 1: a Message Crate has one settings record.
    id INTEGER PRIMARY KEY CHECK (id = 1),
    -- 1 = anyone reaching the server may create their own account; 0 = only
    -- the owner creates accounts. Off until the owner turns it on.
    public_registration INTEGER NOT NULL DEFAULT 0,
    -- The attachment size limit: the largest asset the server accepts, in
    -- bytes. 512 MiB until the owner sets it.
    asset_max_bytes BIGINT NOT NULL DEFAULT 536870912
);

-- A Demo Account build that has not finished. A build writes this row before
-- it writes anything else and removes it after its last write, so a row found
-- when the server starts is a build the server stopped part-way: the server
-- removes the Demo Account the build left and reports the build as failed.
CREATE TABLE IF NOT EXISTS demo_account_build (
    -- Always 1: one Demo Account build runs at a time.
    id INTEGER PRIMARY KEY CHECK (id = 1),
    -- When the build started, in Unix seconds as text.
    started_at TEXT NOT NULL
);

-- Process-wide schema markers (for example FTS trigger install flag).
CREATE TABLE IF NOT EXISTS schema_meta (
    -- Marker name (for example messages_fts_triggers_v1).
    key TEXT PRIMARY KEY,
    -- Marker value (usually '1' when installed).
    value TEXT NOT NULL
);

-- One row per import run into Message Crate.
CREATE TABLE IF NOT EXISTS imports (
    -- Surrogate primary key for this import run.
    id INTEGER PRIMARY KEY,
    -- Owning account (`accounts.id`). Set NULL when the account is deleted:
    -- the run is part of the Audit Trail, which outlives the account
    -- (docs/adr/0020-the-audit-trail-outlives-the-account.md).
    account_id INTEGER REFERENCES accounts(id) ON DELETE SET NULL,
    -- The username the account had, written when the account is deleted and
    -- `account_id` becomes NULL, so the run still says whose it was.
    username TEXT,
    -- What started the run: 'session' or 'api_token'. NULL for a run the
    -- server started itself (the Demo Account build, the CLI import).
    credential TEXT,
    -- The app the starting Session named: 'desktop' or 'website'. NULL when
    -- the request named none or a token started the run.
    app_kind TEXT,
    -- That app's Build, such as '0.9.0+343fe0d8'; set with app_kind.
    app_build TEXT,
    -- The starting API token's label as it was then, so the run names the
    -- token after it is renamed or deleted.
    api_token_label TEXT,
    -- The starting API token's masked hint (mc-api-Sd..mE) as it was then.
    -- Never the token or its hash.
    api_token_hint TEXT,
    -- Backup/source family (for example imessage, whatsapp, sms-backup-restore).
    source TEXT NOT NULL,
    -- Client/tool name that performed the import (optional).
    tool TEXT,
    -- Import mode string recorded by the importer.
    mode TEXT NOT NULL,
    -- 1 when cross-source dedupe runs after each batch of this run. Stated
    -- once, when the run is created, so two batches cannot disagree.
    dedupe INTEGER NOT NULL DEFAULT 0,
    -- Run status (for example running, completed, failed, cancelled).
    status TEXT NOT NULL,
    -- When the import started.
    started_at TEXT NOT NULL,
    -- When the import finished; NULL while still running.
    finished_at TEXT,
    -- Messages accepted during this run.
    message_count INTEGER NOT NULL DEFAULT 0,
    -- Attachments accepted during this run.
    attachment_count INTEGER NOT NULL DEFAULT 0,
    -- Bytes uploaded for assets during this run.
    bytes_uploaded INTEGER NOT NULL DEFAULT 0,
    -- Wall-clock duration of the whole run in milliseconds.
    duration_ms INTEGER,
    -- Time spent parsing input in milliseconds.
    parse_ms INTEGER,
    -- Time spent copying, converting, or skipping attachments in milliseconds.
    attachments_ms INTEGER,
    -- Time spent preparing conversation files in milliseconds.
    prepare_ms INTEGER,
    -- Time spent uploading in milliseconds.
    upload_ms INTEGER,
    -- JSON blob with a human-readable run summary for Import History.
    summary_json TEXT,
    -- Where a live session is: parse, write, awaiting_gate_1, transcode,
    -- awaiting_gate_2, or pushing. NULL once the run is over. `status` says
    -- how a run ended; `stage` says where it is.
    stage TEXT,
    -- Absolute path to this session's staging folder on the client. The
    -- database holds the pointer so resuming means asking the server where
    -- to go, rather than guessing from a directory listing.
    staging_dir TEXT,
    -- Which install created the session, so another machine can say where
    -- it belongs instead of failing to open a path that was never local.
    device_id TEXT,
    -- Import form snapshot: restores the screen, and restarts the run with
    -- the same settings.
    form_json TEXT,
    -- Source path, size, mtime, and message count. A backup that grew
    -- between attempts has different conversation boundaries.
    source_fingerprint TEXT,
    -- Addresses the backup's device sent from (JSON array), read by the
    -- client before parsing. Lets a resumed Gate 1 show the identity list
    -- without re-reading the backup.
    source_identities TEXT
);

CREATE INDEX IF NOT EXISTS ix_imports_account_started
    ON imports(account_id, started_at DESC);

-- At most one live import session per account. A partial unique index
-- rather than application logic, so it holds against a racing client.
CREATE UNIQUE INDEX IF NOT EXISTS ux_imports_active_account
    ON imports(account_id) WHERE status = 'running';

-- One row per Export Run: what was asked for and how much matched, never
-- message content. Recorded permanently whether the run completed, failed
-- or was cancelled, so every read of message data by a program leaves a
-- record.
CREATE TABLE IF NOT EXISTS exports (
    -- Surrogate primary key for this export run.
    id INTEGER PRIMARY KEY,
    -- Owning account (`accounts.id`). Set NULL when the account is deleted,
    -- as for `imports`; the deletion also clears `scope_query`,
    -- `scope_conversation_ids` and `scope_message_ids`, which describe the
    -- person's messages.
    account_id INTEGER REFERENCES accounts(id) ON DELETE SET NULL,
    -- The username the account had, written when the account is deleted and
    -- `account_id` becomes NULL, so the run still says whose it was.
    username TEXT,
    -- What started the run: 'session' or 'api_token'. NULL for a run the
    -- server started itself (the Demo Account build, the CLI import).
    credential TEXT,
    -- The app the starting Session named: 'desktop' or 'website'. NULL when
    -- the request named none or a token started the run.
    app_kind TEXT,
    -- That app's Build, such as '0.9.0+343fe0d8'; set with app_kind.
    app_build TEXT,
    -- The starting API token's label as it was then, so the run names the
    -- token after it is renamed or deleted.
    api_token_label TEXT,
    -- The starting API token's masked hint (mc-api-Sd..mE) as it was then.
    -- Never the token or its hash.
    api_token_hint TEXT,
    -- Which of the three scope forms the run asked for: everything, query,
    -- or selection.
    scope_kind TEXT NOT NULL,
    -- Which list the query is for, when `scope_kind` is query: conversations
    -- (every message of the conversations the query shows) or messages (the
    -- messages the query matches); NULL otherwise.
    scope_list TEXT,
    -- The search-language query, when `scope_kind` is query; NULL otherwise.
    scope_query TEXT,
    -- JSON array of the conversation ids picked by hand, when `scope_kind`
    -- is selection; NULL otherwise.
    scope_conversation_ids TEXT,
    -- JSON array of the message ids picked by hand, when `scope_kind` is
    -- selection; NULL otherwise.
    scope_message_ids TEXT,
    -- Client/tool name that ran the export (optional).
    tool TEXT,
    -- Run status: running, completed, failed, or cancelled.
    status TEXT NOT NULL,
    -- When the export started.
    started_at TEXT NOT NULL,
    -- When the export finished; NULL while still running.
    finished_at TEXT,
    -- Messages the scope matched when the run was created.
    message_count INTEGER NOT NULL,
    -- Distinct conversations with at least one matching message, at creation.
    conversation_count INTEGER NOT NULL,
    -- Distinct attachment fingerprints among the matching messages, at creation.
    attachment_count INTEGER NOT NULL,
    -- Sum of the known sizes of those distinct attachments, at creation.
    total_bytes INTEGER NOT NULL,
    -- How far the run's message pages have read into its list, in places,
    -- so an abandoned run shows how far it got.
    messages_delivered INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS ix_exports_account_started
    ON exports(account_id, started_at DESC);

-- Per-item error or skip recorded during an import run.
CREATE TABLE IF NOT EXISTS import_issues (
    -- Surrogate primary key for this issue row.
    id INTEGER PRIMARY KEY,
    -- Parent import run (`imports.id`).
    import_id INTEGER NOT NULL REFERENCES imports(id) ON DELETE CASCADE,
    -- Issue class: 'error' or 'skip'.
    kind TEXT NOT NULL,
    -- Pipeline step where the issue happened.
    step TEXT NOT NULL,
    -- Item identifier (path, guid, or similar).
    item TEXT NOT NULL,
    -- Human-readable explanation.
    reason TEXT NOT NULL,
    -- When the issue was recorded.
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS ix_import_issues_import
    ON import_issues(import_id);

-- What one import run did to one contact. Written while the run stages, so
-- the record says what the import decided rather than what timestamps
-- suggest afterwards. The run's Contact Group and its new/changed counts
-- read this table.
CREATE TABLE IF NOT EXISTS import_contacts (
    -- Import run (`imports.id`).
    import_id INTEGER NOT NULL REFERENCES imports(id) ON DELETE CASCADE,
    -- Contact the run touched (`contacts.id`); the row goes with the contact.
    contact_id INTEGER NOT NULL REFERENCES contacts(id) ON DELETE CASCADE,
    -- One of replaced_trashed, created, named, handle_added. When a run does
    -- more than one of these to a contact, the earlier one in that list is
    -- kept.
    reason TEXT NOT NULL,
    PRIMARY KEY (import_id, contact_id)
);

CREATE INDEX IF NOT EXISTS ix_import_contacts_contact
    ON import_contacts(contact_id);

-- The Audit Trail: what each user did on this Message Crate, and when, for
-- everything `imports` and `exports` do not already record. An entry is
-- written once and never changed by a route; deleting an account sets its
-- `account_id` NULL and keeps the entry
-- (docs/adr/0020-the-audit-trail-outlives-the-account.md). A password, a
-- session token and an API token never enter it, hashed or not, and nothing
-- here says what a message said or which conversation it was in.
CREATE TABLE IF NOT EXISTS audit_entries (
    -- Entry id.
    id INTEGER PRIMARY KEY,
    -- When it happened, RFC 3339 UTC, as `imports.started_at` is written.
    at TEXT NOT NULL,
    -- What happened: logged_in, session_ended, login_refused,
    -- account_created, account_disabled, account_enabled, password_set,
    -- permissions_changed, messages_deleted, conversation_deleted,
    -- trash_emptied, account_deleted, registration_opened,
    -- registration_closed, api_token_created, api_token_deleted,
    -- address_book_loaded or address_book_exported.
    action TEXT NOT NULL,
    -- Who acted: owner, holder (the account's own holder), command_line
    -- (a server command), server (the server on its own), or anonymous
    -- (someone at the login card).
    actor TEXT NOT NULL,
    -- The account the entry is about (`accounts.id`). NULL when it is about
    -- no account (a refused login for an unknown username, opening or closing
    -- registration) or once the account is deleted.
    account_id INTEGER REFERENCES accounts(id) ON DELETE SET NULL,
    -- That account's username when the entry was written; for a refused
    -- login, the username as typed, trimmed and cut to 128 characters.
    username TEXT,
    -- session_ended: logged_out, replaced or revoked. login_refused:
    -- unknown_username, wrong_password or account_disabled. NULL otherwise.
    reason TEXT,
    -- logged_in and login_refused: the app the request named, 'desktop' or
    -- 'website'; NULL when the request named none.
    app_kind TEXT,
    -- That app's Build; set with app_kind.
    app_build TEXT,
    -- session_ended: the logged_in entry of the session that ended.
    session_entry_id INTEGER REFERENCES audit_entries(id),
    -- logged_in: when the session expires, RFC 3339 UTC. A session with no
    -- session_ended entry reads as expired from this time, so expiry needs
    -- no entry and no sweeper. Moved forward when a password change renews
    -- the session.
    session_expires_at TEXT,
    -- JSON object of the entry's counts and names: permissions added and
    -- removed, conversations and attachments deleted, an API token's label
    -- and hint, an address book's mode and contact counts.
    details TEXT
);

CREATE INDEX IF NOT EXISTS ix_audit_entries_account_at
    ON audit_entries(account_id, at);

CREATE INDEX IF NOT EXISTS ix_audit_entries_at
    ON audit_entries(at);

CREATE INDEX IF NOT EXISTS ix_audit_entries_session
    ON audit_entries(session_entry_id);
