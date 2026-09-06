---
title: HTTP API
description: Tokens, import sessions, search syntax, and JSONL upload for people writing tools against the vault.
---

Route schemas, status codes, and JSON fields live in the generated [HTTP API reference](/vault/developer/rustdoc/http/). Crate types and functions live in [Rust crate docs](/vault/developer/rustdoc/). This page is the prose those tools need that is not a JSON schema.

`message-vault-server serve` reads `[server]` in `config/config.toml` (`bind`). Day-to-day import uses the desktop [Import](/vault/user/import-from-a-backup/) screen and download uses [Export](/vault/user/how-to/export-from-the-vault/). Both call this API with [JSONL](/vault/developer/reference/export-structure/) and attachment bytes keyed by SHA-256, through the `vault-push` and `vault-pull` libraries.

## One shape for every route

- A list takes `?offset=&limit=` and answers `{items, total, limit, offset}`. `limit` is at most 500 and at least 1; `offset` is at most 50 000 on the Contacts and Conversations lists and unlimited on Export.
- A failure answers `{"error": "<sentence>"}` with the HTTP status. That includes a malformed query parameter, path, or JSON body, an unknown `/v1` path (404), and a wrong method (405). There is no `ok` field on any response.
- A route with nothing to say on success answers `204 No Content`.
- Every id is an integer, except API token ids and account ids, which are opaque strings.

Why: [ADR-0005](https://github.com/bitrealm-io/message-vault/blob/main/docs/adr/0005-one-shape-for-every-route-on-the-http-interface.md).

## Trash is the only door to deletion

`POST /v1/conversations/{id}/trash` and `POST /v1/contacts/{id}/trash` set a marker; `/restore` clears it. Nothing is deleted until one of three routes runs, and each needs a signed-in session whose account may delete:

- `DELETE /v1/conversations/{id}` removes a trashed conversation, its messages, and any attachment file no remaining message references. A conversation that is not in the trash answers 409.
- `DELETE /v1/contacts/{id}` does what a phone's Delete Contact does: the name, the person's edits and their Contact Group memberships go, the contact becomes Unknown and leaves the trash, and every conversation stays as it is, showing the handle. A contact that is not in the trash answers 409.
- `DELETE /v1/trash` does both for everything in the trash.

All three answer `204`. The demo account may delete like any other account. The one deletion it is refused is its own: `POST /v1/auth/delete-account` answers 400 for it, and `reset-demo` restores the vault instead. An Import Run's record on `/v1/imports` does not change when messages it brought in are later deleted.

## Tokens

Auth is per-account. There is no host-wide admin token.

Create a named **API token** under **Settings → Account** (shown once) for a program of your own that calls this API. A website login uses a **session** Bearer that rotates on each login, and the desktop app uses that session rather than a token. Do not paste a session token into a program expecting a long-lived token.

Send either token as:

```http title="Bearer header"
Authorization: Bearer <token>
```

An API token may import (write) and export messages and assets (read). It may not change profile, settings, or browse-only website routes. Export routes never delete vault data.

Turn on a local explorer with `[server] openapi_ui = true`, then open `/docs` on that vault. The explorer is off by default. “Try it” still sends this header.

## Import session

Import starts a session with `POST /v1/imports`, passes `import_id` on each `POST /v1/import`, then `POST /v1/imports/{id}/complete` so Settings → Storage can list history. Messages promoted in that session store `messages.import_id`.

If `import_id` is omitted on `POST /v1/import`, the server starts and finishes a one-shot session so Storage still records the import.

Bulk `POST /v1/import` opens its own SQLite connection so it does not hold the serve process’s short session mutex across JSONL and asset work. Same-account imports stay serialized. Export and auth open their own connections and can proceed under WAL while an import runs.

## Import body

- `Content-Type: application/jsonl` or `application/x-ndjson` — body only; attachments already uploaded by SHA-256 through `/v1/assets`. Any other media type is refused with 415.

Request body limit matches `[server] asset_max_bytes` (default 512 MiB).

HTTP `mode` defaults to `append` (CLI `import` defaults to `replace`). HTTP `dedupe` defaults to false (CLI runs dedupe unless `--skip-dedupe`). HTTP `source` is a required query parameter. `account` is optional when the Bearer token already identifies the tenant.

A file the vault cannot read comes back as a 400 whose `error` names the line, or the schema version the file has and the version the vault reads.

## Messages across conversations

`GET /v1/messages?q=` answers one row per message matching `q`, paged like every other list, behind a signed-in session. It is a read route: opening a conversation is `GET /v1/conversations/{id}/messages`, downloading is `GET /v1/export/messages`, and searching across messages is this. The thread's find box uses it with `in:#id` so a find reaches every message in the conversation, not the page the browser holds.

## Search operators (`q`)

`q` is the same search language the website uses — see [Search](/vault/user/how-to/search/) for the full grammar: quoting, `none`/`any`, date and size ranges, `-` to exclude, `or` and parentheses, `avoc*` prefixes. Export compiles `q` against the Messages list, with the same compiler Contacts and Conversations search use elsewhere in the vault, full-text index included for free text. These are the words the Messages list has:

- Free text and `"quoted phrases"` match the message body, the subject, and any attachment file name.
- `body:`, `subject:` — text, `none`, `any`, restricted to that one field.
- `name:`, `handle:` — a participant's name or handle; text, `none`, `any`.
- `title:` — the conversation's title; text, `none`, `any`.
- `with:` — a participant, by name, handle, or `#id`.
- `from:`, `to:` — who sent it or who it went to; `me`, name, handle, or `#id`.
- `in:` — this one conversation; title, handle, or `#id`.
- `group:` — this Contact Group, on the contact or on a participant; name, `#id`, `none`, `unknown`.
- `tag:` — this Message Tag; name, `#id`, `none`.
- `kind:` — `direct` or `group`.
- `service:` — `imessage`, `sms`, `mms`, `rcs`, `whatsapp`.
- `source:` — the backup family it was imported from: `imessage`, `whatsapp`, `sms`.
- `import:` — the Import Run that brought it in; `#id` or `last`.
- `date:`, `first-message:`, `last-message:` — a day, month, year, or relative span, with comparisons and ranges. A message's `timestamp` is a UTC instant; the span's edges are midnight in the account's `time_zone` (`GET /v1/account/profile`), turned into instants before the comparison, so the same rule serves SQLite and Postgres and the `year=` filter on a conversation's messages.
- `attachment:` — `image`, `video`, `audio`, `document`, `pdf`, `contact`, `other`, `any`, `none`.
- `filename:` — an attachment's file name; text or a `pre*` prefix.
- `size:` — an attachment's size, with comparisons and ranges.
- `participants:` — how many people are in the conversation, with comparisons and ranges.
- `attachments:` — how many attachments are on the message, with comparisons and ranges.
- `trashed:` — `yes`, `no`, or `any`. Trash is excluded by default; `trashed:yes` or `trashed:any` lifts that.

`messages:`, `conversations:`, and `groups:` belong to the Contacts and Conversations lists, not Messages, so export refuses them.

## Verify a token

```bash title="Verify a token"
curl -sS "http://127.0.0.1:8080/v1/auth/check" \
  -H "Authorization: Bearer <import-api-token-from-settings>"
```

Health check: <http://127.0.0.1:8080/health>
