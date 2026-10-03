---
title: Database tables
description: SQLite tables in Message Crate and how chats, contacts, and messages link through typed handles.
---

The Message Crate SQLite database falls into four groups:

1. **Chats and texts** — threads, participants, messages, files, reactions
2. **People and groups** — handles, contacts, contact groups, accounts
3. **Staging** — temporary copies used while importing
4. **Trash markers** — soft-delete lists without removing chat data

Chats and people are **not** joined by a shared person ID. They meet through
the **`handles`** table: every phone number, email, or username appears once
per account **per platform** (`phone` or `whatsapp`) as a typed handle, and
conversations, participants, messages, and contacts all point at the same
handle rows.

```mermaid
erDiagram
    conversations ||--o{ participants : "has"
    conversations ||--o{ messages : "has"
    messages ||--o{ attachments : "has"
    messages ||--o{ tapbacks : "has"
    messages ||--o| messages : "duplicate_of"
    handles ||--o{ conversations : "chat_handle_id"
    handles ||--o{ participants : "handle_id"
    handles ||--o{ messages : "sender_handle_id"
    handles ||--o{ tapbacks : "sender_handle_id"
    handles ||--o{ contact_handles : "has"
    contacts ||--o{ contact_handles : "has"
    contacts ||--o{ contact_group_members : "in"
    contact_groups ||--o{ contact_group_members : "has"
    conversations ||--o{ message_tag_members : "tagged"
    message_tags ||--o{ message_tag_members : "has"
```

## Chats and texts

### `conversations`

One row = one chat thread (`account_id`, `chat_handle_id` → `handles`,
`conversation_type`, `group_title`, and related fields). There is no
conversation-level messaging transport: SMS vs iMessage vs RCS varies per
message. Thread chrome for “which backup” uses distinct `messages.source`
values.

### `participants`

One row = one handle in one chat (`handle_id` → `handles`, required, and an
optional `name_alias`). A person the backup names with no address has a handle
of type `other` holding the name. The participant's contact is the one its
handle is on in `contact_handles`.

### `messages`

One row = one message (`source`, `guid`, timestamps, `is_from_me`, optional
`service` for per-message transport such as `sms` / `imessage` / `rcs` /
`whatsapp`, `body`, `content_key`, optional `sender_handle_id` → `handles`,
optional `duplicate_of`).

### `attachments` / `tapbacks`

Files and reactions tied to a message. Attachments may store `sha256` and
derived (converted) paths for the browser. Reactions record `sender_handle_id`
→ `handles`.

## People and accounts

### `accounts` / `account_emails` / `account_handles` / `account_session_tokens` / `account_api_tokens`

Web accounts log in with **user ID** (`username`) and optional password.
`preferred_name` is the display name. `account_handles` (and optional
`account_emails`) are handles used to recognize “you” in messages — emails are
never used for login. GUI **session** tokens live in `account_session_tokens` (one
per account; rotated on login; prefix `mc-user-`). Named **API tokens** for program
import/export live in `account_api_tokens` (many per account; prefix `mc-api-`).

### `handles`

One row = one **platform** identity per account: `raw` (as the source wrote
it), `normalized`, `handle_type` (`phone` / `email` / `username` / `other`),
required `service` (`phone` | `whatsapp` — UI labels “Text message” /
“WhatsApp”), and an optional `normalized_note`. Handles are deduplicated per
account by `(account_id, normalized, handle_type, service)`, so the same phone
number on Text message and WhatsApp is two rows. SMS / iMessage / RCS are
**not** handle platforms; those are per-message transport values on
`messages.service`. Everywhere else in the schema, identities are referenced by
`handle_id` — never by text.

`normalized_note` is the needs-review flag: phone numbers are written as
E.164 only when unambiguous. Ambiguous values (e.g. a trunk-zero national
number like `020 7946 0000` without a country code) keep their digits as
`normalized` — never a fabricated `+0…` — and carry a human-readable reason
in `normalized_note` so the UI can surface them for review. Only an import
writes a note. An address book load refuses a phone it cannot key, so a row a
load makes never carries one.

`origin` records what made the row: `import`, `user`, or `address_book` for
an identity an address book load created. A load removes an `address_book`
identity again when Edit takes it off its contact and no conversation,
message, or account uses it.

### `contacts` / `contact_handles`

The people an account knows; display name is `preferred_name` only.
`last_modified` is a SQLite `datetime('now')` string bumped when the contact's
name, identities, or Contact Groups change (create, rename, handle
add/update/remove, group membership, merge survivor, import sibling platform
link, address book load) — not when messages arrive.
`contact_handles` links a contact to its `handles` rows per account (one contact
per handle per account).

`origin` on both tables records what made the row: `import`, `user`, or
`address_book`. An address book load writes `address_book` on a contact it
creates and on every link it makes or moves. A contact a load only renames
keeps its origin. Nothing reads `origin` to decide what a load may change:
a load changes the contacts its file names, and no others.

### The address book file

The address book is not a table. It is a CSV the server writes from these
tables and reads back into them, one row per `contact_handles` link:

| Column | Source |
|---|---|
| `contact_id` | `contacts.id` |
| `display_name` | `contacts.preferred_name`, trimmed |
| `groups` | the names of the contact's `contact_groups`, joined with `;` |
| `service` | `handles.service` |
| `identity_type` | `handles.handle_type` |
| `identity` | `handles.normalized` |

`POST /v1/contacts/address-book` writes it and `POST /v1/contacts` loads it;
`crates/server/server/src/db/address_book.rs` holds both directions. The
rules of a load are in
[`docs/architecture/contacts-identities-and-messages.md`](https://github.com/messagecrate/message-crate/blob/main/docs/architecture/contacts-identities-and-messages.md).

### `contact_groups` / `contact_group_members`

Named groups and membership. Groups are ordinary memberships with no reserved
status names.

### `message_tags` / `message_tag_members`

Named stamps on whole conversation threads (not on individual messages). A
thread can have several tags. New messages in a tagged thread stay under that
tag. A new thread is untagged until someone adds a tag.

## How chats meet people

There is no `contact_id` on conversations. The link is the `handles` table:

- 1:1 `conversations.chat_handle_id` and `participants.handle_id` on the chat
  side
- `contact_handles.handle_id` on the contact side

Chat-side and contact-side reference the same per-account handle rows, so when
a chat handle and a contact handle are the same identity, the UI treats that
chat as belonging to that contact.

```mermaid
flowchart LR
  chat["conversations / participants"] -->|"handle_id"| h["handles"]
  h -->|"handle_id via contact_handles"| person["contacts"]
```

## Staging and trash

Import writes into `staging_*` tables first, then promotes into lasting tables
(cleared per account during import). Staging rows carry the same `handle_id`
columns; import resolves handles to ids while rows are being staged.

`trashed_conversations` and `trashed_contacts` mark items as trashed without
deleting underlying rows.

## How storage sizes are measured

The owner's Dashboard reports what the database takes on disk. Every
figure is measured from the database. The queries are in
`crates/server/server/src/db/storage.rs`.

| Figure | How it is measured |
|---|---|
| Database size | `page_count * page_size` |
| Messages on disk | `dbstat` pages of `messages` and every index on it |
| Full-text search index | `dbstat` pages of the four `messages_fts_*` shadow tables |
| Text bytes per account | `sum(length(cast(body as blob)) + length(cast(subject as blob)))` grouped by `account_id` |

The bundled SQLite is compiled with `SQLITE_ENABLE_DBSTAT_VTAB`
(`libsqlite3-sys`'s `build.rs` sets the flag), so the `dbstat` virtual table
is available. A build without it must turn the flag back on rather than
estimate, because every other figure on the Dashboard is measured and one
estimate among them would be read as measured too. Text is counted in bytes,
not characters: `length()` of text counts characters, so the text is read as
a blob. The full-text search index is tables of its own, so the messages
figure never includes it. The database size never counts attachment files on
disk.

The one estimate is each account's share of message storage: the
messages-on-disk figure times the account's share of all text bytes. The
shares are computed so they add up to the measured figure exactly, with the
last account that has any text absorbing the rounding. An account with no
messages reports zero. The full-text search index is one shared structure on
SQLite, so it is reported once for the whole database and never per account.

## Quick map

| You want… | Look in… |
|-----------|----------|
| A chat thread | `conversations` |
| Who is listed in a chat | `participants` |
| An identity (phone, email, username) | `handles` |
| The texts | `messages` |
| Photos and files | `attachments` |
| Reactions | `tapbacks` |
| A person you named | `contacts` |
| Web login | `accounts` |
| Soft-deleted items | `trashed_*` |
| Import scratch space | `staging_*` |
| One import attempt, or one export attempt | `imports`, `exports` |
| The messages a running export matched when it started | `export_messages` |
| What an import did to each contact | `import_contacts` |

Baseline table definitions live in
[`schema/sql/`](https://github.com/messagecrate/message-crate/blob/main/schema/sql/).
Rust loads them from [`src/db/schema.rs`](https://github.com/messagecrate/message-crate/blob/main/crates/server/server/src/db/schema.rs).
Every column carries a `--` comment on the line above it; a server test fails
when one is missing.

Related: [Import](/docs/user/features/messages/import/) (desktop).
