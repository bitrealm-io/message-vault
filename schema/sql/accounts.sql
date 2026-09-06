-- Vault login account (web UI + API owner).
CREATE TABLE IF NOT EXISTS accounts (
    -- Stable account id (opaque string primary key).
    id TEXT PRIMARY KEY,
    -- Login user id; unique case-insensitively.
    username TEXT NOT NULL UNIQUE COLLATE NOCASE,
    -- Password verifier hash; NULL when password auth is unused.
    password_hash TEXT,
    -- Display name for “you” in the UI.
    preferred_name TEXT,
    -- IANA time zone (for example America/New_York) every message time, day and year is
    -- read in. Chosen at profile setup; a message records only the instant it arrived.
    time_zone TEXT NOT NULL DEFAULT 'UTC',
    -- 1 = may not sign in and existing sessions are refused; 0 = active.
    disabled INTEGER NOT NULL DEFAULT 0,
    -- 1 = the vault owner chose this password, so the account holder must
    -- replace it before the session goes anywhere; cleared on the change.
    must_change_password INTEGER NOT NULL DEFAULT 0,
    -- 1 = the account holder has not set up their profile yet, so the session
    -- owes profile setup before it goes anywhere; cleared on the first save.
    must_set_up_profile INTEGER NOT NULL DEFAULT 0,
    -- 1 = may call the import endpoints.
    can_import INTEGER NOT NULL DEFAULT 1,
    -- 1 = may call the export endpoints.
    can_export INTEGER NOT NULL DEFAULT 1,
    -- 1 = may destroy message data (trash, purge, delete-messages, attachments).
    can_delete INTEGER NOT NULL DEFAULT 1
);

-- Email addresses attached to an account (not used for login).
CREATE TABLE IF NOT EXISTS account_emails (
    -- Owning vault account (`accounts.id`).
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Email address; unique case-insensitively across the vault.
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
    -- Owning vault account (`accounts.id`).
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Self identity (`handles.id`).
    handle_id INTEGER NOT NULL REFERENCES handles(id) ON DELETE CASCADE,
    PRIMARY KEY (account_id, handle_id)
);

-- GUI session Bearer (one per account; rotates on login). Prefix: mv-user-
CREATE TABLE IF NOT EXISTS account_session_tokens (
    -- Owning vault account (`accounts.id`); also the primary key (one session).
    account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    -- Hash of the session Bearer secret (never store the raw token).
    token_hash TEXT NOT NULL UNIQUE,
    -- Unix-seconds string for when this session token was issued.
    created_at TEXT NOT NULL,
    -- Unix-seconds string; session rejected after this time.
    expires_at TEXT NOT NULL DEFAULT '0'
);

-- Named CLI API tokens (many per account). Prefix: mv-api-
CREATE TABLE IF NOT EXISTS account_api_tokens (
    -- Opaque token id (primary key).
    id TEXT PRIMARY KEY,
    -- Owning vault account (`accounts.id`).
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- User-visible label in Settings.
    label TEXT NOT NULL,
    -- Hash of the API Bearer secret (never store the raw token).
    token_hash TEXT NOT NULL UNIQUE,
    -- 1 = this token may call the import endpoints.
    can_import INTEGER NOT NULL DEFAULT 1,
    -- 1 = this token may call the export endpoints.
    can_export INTEGER NOT NULL DEFAULT 1,
    -- 1 = this token may destroy message data. Off unless asked for.
    can_delete INTEGER NOT NULL DEFAULT 0,
    -- Masked form for Settings (e.g. mv-api-Sd..mE). Not enough to recover the secret.
    token_hint TEXT NOT NULL DEFAULT 'mv-api-..',
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
    -- Owning vault account (`accounts.id`).
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Preference name (for example theme or feature flags).
    key TEXT NOT NULL,
    -- Preference value stored as text.
    value TEXT NOT NULL,
    PRIMARY KEY (account_id, key)
);

-- Settings that belong to the whole vault rather than to one account. Exactly
-- one row, so the vault owner reads and writes it without an id.
CREATE TABLE IF NOT EXISTS vault_settings (
    -- Always 1: the vault has one settings record.
    id INTEGER PRIMARY KEY CHECK (id = 1),
    -- 1 = anyone reaching the vault may create their own account; 0 = only
    -- the vault owner creates accounts. Off until the owner turns it on.
    public_registration INTEGER NOT NULL DEFAULT 0
);

-- Process-wide schema markers (for example FTS trigger install flag).
CREATE TABLE IF NOT EXISTS schema_meta (
    -- Marker name (for example messages_fts_triggers_v1).
    key TEXT PRIMARY KEY,
    -- Marker value (usually '1' when installed).
    value TEXT NOT NULL
);

-- One row per import run into the vault.
CREATE TABLE IF NOT EXISTS vault_imports (
    -- Surrogate primary key for this import run.
    id INTEGER PRIMARY KEY,
    -- Owning vault account (`accounts.id`).
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Backup/source family (for example imessage, whatsapp, sms-backup-restore).
    source TEXT NOT NULL,
    -- Client/tool name that performed the import (optional).
    tool TEXT,
    -- Import mode string recorded by the importer.
    mode TEXT NOT NULL,
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
    -- awaiting_gate_2, or pushing. NULL once the run is over, and on rows
    -- written before sessions existed. `status` says how a run ended;
    -- `stage` says where it is.
    stage TEXT,
    -- Absolute path to this session's staging folder on the client. The
    -- database holds the pointer so resuming means asking the vault where
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

CREATE INDEX IF NOT EXISTS ix_vault_imports_account_started
    ON vault_imports(account_id, started_at DESC);

-- At most one live import session per account. A partial unique index
-- rather than application logic, so it holds against a racing client.
CREATE UNIQUE INDEX IF NOT EXISTS ux_vault_imports_active_account
    ON vault_imports(account_id) WHERE status = 'running';

-- Per-item warning or error recorded during an import run.
CREATE TABLE IF NOT EXISTS vault_import_issues (
    -- Surrogate primary key for this issue row.
    id INTEGER PRIMARY KEY,
    -- Parent import run (`vault_imports.id`).
    import_id INTEGER NOT NULL REFERENCES vault_imports(id) ON DELETE CASCADE,
    -- Issue class (for example warning or error).
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

CREATE INDEX IF NOT EXISTS ix_vault_import_issues_import
    ON vault_import_issues(import_id);
