# Dashboard storage sizes

Date: 2026-09-22. Status: approved design, not yet built.

## Why

The vault owner's Dashboard shows one card today: attachment bytes and four
counts for the whole vault. It says nothing about the database itself. The
owner cannot see how big the database file is, how much of it the messages
take, how much the full-text search index adds, or which account's messages
take the most room. This work adds those figures to the Dashboard and turns
the page into a column of headed sections, so later dashboards can reuse the
same pieces.

Custom dashboards are out of scope. So is a per-account full-text search
figure: on SQLite the index is one shared structure, so a per-account number
there could only be a guess. The owner-visibility rule in
`docs/adr/0008-the-vault-owner-holds-no-messages.md` already permits every
number below, because each describes an account's data without being it.

## What the vault measures

Every figure is measured from the database, on both engines. The only
estimate is the per-account split, and it is labeled as one.

| Figure | SQLite | Postgres |
|---|---|---|
| Database size | `page_count * page_size` | `pg_database_size(current_database())` |
| Messages on disk | `dbstat` pages of `messages` plus every index on it | `pg_total_relation_size('messages')` minus the FTS figure |
| FTS size | `dbstat` pages of the four `messages_fts_*` shadow tables | `sum(pg_column_size(search_tsv))` plus `pg_relation_size('ix_messages_search_tsv')` |
| Text bytes per account | `sum(length(cast(body as blob)) + length(cast(subject as blob)))` grouped by `account_id` | `sum(octet_length(body) + octet_length(subject))` grouped by `account_id` |

Notes:

- The bundled SQLite is compiled with `SQLITE_ENABLE_DBSTAT_VTAB`, so the
  `dbstat` virtual table is available. If a future build drops it, the fix is
  to turn it back on, not to estimate.
- Database size never counts attachment files on disk. The existing card
  already reports attachment bytes.
- On Postgres the FTS vector is a column on `messages`, so the table's total
  size includes it. Subtracting FTS leaves a messages-only figure that means
  the same thing as the SQLite one.
- Estimated message bytes per account are the messages-on-disk figure times
  that account's share of all text bytes. The shares are computed so they add
  up to the messages-on-disk figure exactly; the last account with any text absorbs the
  rounding. An account with no messages reports zero. When no account has any
  text, every estimate is zero.

## API

`GET /v1/vault/storage` keeps its five fields and gains four. It stays
owner-only.

```json
{
  "message_count": 612893,
  "conversation_count": 1024,
  "contact_count": 300,
  "attachment_count": 50000,
  "total_bytes": 9000000000,
  "database_bytes": 581000000,
  "messages_bytes": 419000000,
  "fts_bytes": 149000000,
  "accounts": [
    {
      "account_id": 100,
      "username": "mbeisser",
      "message_count": 612893,
      "text_bytes": 54000000,
      "estimated_message_bytes": 419000000
    }
  ]
}
```

- `accounts` lists every account, including ones with no messages, in the
  order the User Accounts table uses: the owner first, then by username.
- Each row carries only an id, a username and numbers.
- No client compatibility is kept; the web app is updated in the same PR.

The queries live in `crates/vault/server/src/db/storage.rs` beside the
existing counts, branched on `DbEngine` like the rest of the database layer.
The handler in `vault_api.rs` assembles the response and computes the split.

## The Dashboard page

`OwnerDashboardPanel` becomes a column of headed sections. Each section is
its own component under `web/src/screens/owner/dashboard/`, and the panel
stacks them. All three read the one storage query, so the page shows one
loading line and one error line as it does today.

1. **Vault contents.** The existing card: attachment bytes over the four
   counts. Unchanged except that it sits under a header.
2. **Database.** One card with three figures side by side: database size,
   messages on disk, and full-text search index. A hint under the header says
   the database figure excludes attachment files.
3. **Messages by account.** A table with one row per account: username,
   messages, text, and estimated size on disk. A hint under the header says
   the last column is the messages-on-disk figure split by each account's
   share of text. Columns sort the way the User Accounts table sorts. A totals
   row at the bottom shows the whole-vault figures, so the split is visibly
   complete.

Bytes are formatted with the existing `formatBytes` helper.

## Tests

- Server, both engines: the vault storage tests gain a vault with two
  accounts and imported messages. Checks: the database figure is positive,
  FTS is positive once messages exist, `messages_bytes` is positive, the
  per-account estimates add up to `messages_bytes`, an account with no
  messages reports zero, and `accounts` lists every account in order. The
  existing owner-only checks cover the new fields.
- Web: the dashboard test checks that each section header renders, that the
  table has one row per account with formatted bytes, and that the totals
  row matches the vault-wide figures.
- `docs/src/assets/openapi.json` and `web/src/lib/vaultApi.types.ts` are
  regenerated; CI checks both.

## Docs

`docs/adr/0008` already permits bytes per account and needs no change. The
developer architecture notes gain a short section on how each size is
measured on each engine, and the owner's user guide page for the Dashboard
describes the three sections.
