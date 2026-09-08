-- Named query a user stores to run again from the sidebar.
--
-- A saved search collects nothing: it holds a query string, not members.
-- The rows an `import` search points at outlive it — deleting a saved
-- search never touches `vault_imports`.
CREATE TABLE IF NOT EXISTS saved_searches (
    -- Surrogate primary key for this saved search.
    id INTEGER PRIMARY KEY,
    -- Owning vault account (`accounts.id`).
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Display name, unique per account.
    name TEXT NOT NULL,
    -- Query string, run against the conversation list. Stored verbatim and
    -- never validated; the two search grammars disagree about what is legal.
    query TEXT NOT NULL,
    -- How the row was born: 'manual' when a person wrote it, 'import' when
    -- the server created it at the end of an import run.
    kind TEXT NOT NULL DEFAULT 'manual',
    UNIQUE(account_id, name)
);
