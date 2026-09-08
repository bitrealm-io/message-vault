---
title: Export structure
description: The JSONL format the vault imports — schema version 4, one file per conversation.
---

The vault imports JSONL (JSON Lines) exports at schema version 4. Version 3 is refused, never upgraded. This page describes the format for CLI users and tool authors.

## Happy path

**Phone backup → JSONL export → vault CLI `import` or `POST /v1/imports/{id}/batches` → SQLite**

The JSONL files are plain text — one JSON object per line. The format is the same whether you import through the desktop app or post to the import API directly.

## File shape

One `*.jsonl` per conversation, plus media files under `attachments/`:

1. **Line 1** — Conversation header (`schema_version`, `export`, `conversation`)
2. **Following lines** — One message per line (`timestamp_unix_ms`, `direction`, `service`, `text`, `attachments`, and optional fields)

`service` is the channel (`sms`, `imessage`, `rcs`, …). Apple-specific fields such as tapbacks, replies, and message effects live under an optional `imessage` object.

Attachment records may include `digest_sha256` so clients can upload by hash (`PUT /v1/assets/{sha256}`) before importing the JSONL.

## Clients

- **Same machine**: CLI `import` against a local export folder
- **Remote**: The desktop app **Import** screen posts JSONL to the import API. Batched requests may concatenate multiple conversations (header + messages, repeated) in one body.

## Schema compatibility

The vault reads one schema version, currently 4. A file written at any other version is refused, with an error naming both the file's version and the version the vault expects. To import an older export, re-export it with the current desktop app.

## Related

- [CSV columns reference](/vault/developer/reference/csv-columns/)
- [Import API reference](/vault/developer/reference/api/)
