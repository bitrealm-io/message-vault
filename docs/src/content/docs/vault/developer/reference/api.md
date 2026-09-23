---
title: HTTP API
description: Import Runs, Export Runs, and the search language, for people writing tools against the vault.
---

Three places describe the vault's `/v1` interface, and this page is the smallest of them:

- The generated [HTTP API reference](/vault/developer/rustdoc/http/) states what each route takes and answers today: paths, JSON fields, status codes, and which credential and scope each route accepts.
- The [HTTP interface rules](https://github.com/bitrealm-io/message-vault/blob/main/docs/architecture/http-api.md) state what every route must do, each rule with its reason: route shape, lists and paging, failures as problem documents, credentials and what each reaches, and runs.
- This page walks through an Import Run and an Export Run from start to finish, and lists the words of the search language.

Day-to-day import uses the desktop [Import](/vault/user/import-from-a-backup/) screen and download uses [Export](/vault/user/how-to/export-from-the-vault/). Both call this API with [JSONL](/vault/developer/reference/export-structure/) and attachment bytes keyed by SHA-256, through the `vault-push` and `vault-pull` libraries.

## Tokens

A program of its own calls the vault with a named **API token**, created under **Settings → Account** and shown once. A website login uses a **session** token that changes on each login, and the desktop app uses that session rather than an API token. A session token pasted into a program stops working at the next login, so a program should hold an API token.

Either token travels as a Bearer header:

```http title="Bearer header"
Authorization: Bearer <token>
```

An API token carries the `import` scope, the `export` scope, or both. It never browses and never deletes, so outside an Export Run it started, a program reads no messages. The full rules are under "Credentials and reach" in the HTTP interface rules.

`GET /v1/session` answers which account a token belongs to, which is a quick way to check a new token:

```bash title="Verify a token"
curl -sS "http://127.0.0.1:8080/v1/session" \
  -H "Authorization: Bearer <api-token-from-settings>"
```

Setting `[server] openapi_ui = true` in `config/config.toml` turns on a local explorer at `/docs` on that vault. It is off by default. "Try it" still sends the Bearer header.

## Import Run

Every import is an Import Run, and there is no import without one. A run takes four steps.

1. `POST /v1/imports` creates the run and answers `201 Created` with its id. The body names the `source`, the `mode` (`replace` or `append`, default `append`), and whether to `dedupe` across sources after each batch (default false). These settings belong to the run and are stated once, so no batch repeats them. An account has at most one running Import Run, so a second `POST` answers `409 Conflict` while the first is live. `GET /v1/imports?status=running` finds the live run.
2. Each attachment goes up first, by its SHA-256, through `/v1/assets`. A message points at its attachment by that fingerprint, so the vault must already hold the file when the message arrives.
3. Each `POST /v1/imports/{id}/batches` adds one JSONL body to the run. The run's row says how the batch is imported, so the request carries nothing but the body. A `replace` run wipes the source once, on its first batch, and appends every batch after that.
4. `POST /v1/imports/{id}/complete` records how the run ended: `completed`, `completed_with_issues`, or `failed`, with its counts and issues. `POST /v1/imports/{id}/discard` gives a live run up instead and records it as `cancelled`. A run that has finished answers `409 Conflict` to a batch, a second close, or a change of stage, because its record is the history the person reads and is never rewritten.

Between those steps, `PATCH /v1/imports/{id}` moves a live run to another stage, carrying the plan approved at the gate it just passed in `summary` when there is one. The desktop app uses the stage to resume an import after a restart. The stage is a field of the run, so it is written with a `PATCH` rather than posted to a `stage` sub-resource. The answer is the run, the same record `GET /v1/imports/{id}` returns.

`GET /v1/imports` is a page of every run, newest first (`sort=started_at` for oldest first), narrowed by `status` to one of `running`, `completed`, `completed_with_issues`, `failed`, `cancelled`. `GET /v1/imports/{id}/contacts` lists the contacts a run created or changed. Messages promoted in a run store its id in `messages.import_id`, which is what the `import:` search word matches.

### Import body

A batch body is `Content-Type: application/jsonl` or `application/x-ndjson`. Any other media type answers `415 Unsupported Media Type`, because attachments never travel in a batch. A body larger than `[server] asset_max_bytes` (default 512 MiB) answers `413 Payload Too Large`.

A file the vault cannot read answers `400 Bad Request` with a `malformed-body` problem document. Its `detail` names the line where reading stopped. For a file of the wrong schema version, `detail` names the version the file has and the version the vault reads: nothing is upgraded, so the file must be exported again with current tools. Every other failure is a problem document too, described under "Failures" in the HTTP interface rules.

### Batches and the database

A batch holds one pooled database connection for the whole of its work: parsing the JSONL, placing attachments, and promoting messages. At most two batches run at once across the vault, so the rest of the pool stays free for logins, browsing, and export while an import runs. Batches for the same account run one at a time. The same holds on SQLite and on Postgres.

`message-vault-server import` reads a folder of JSONL without the HTTP interface. It defaults to `replace` and runs dedupe unless given `--skip-dedupe`.

## Export Run

Every export is an Export Run, and there is no unrecorded export. `POST /v1/exports` creates one and answers `201 Created` with the run: what was asked for, and the four counts the vault computed for it at creation — messages, conversations, distinct attachments, and their bytes. The body names a `scope` in one of three forms, stored as given, and an optional `tool`:

```json title="POST /v1/exports"
{ "scope": { "kind": "everything" }, "tool": "vault-pull" }
{ "scope": { "kind": "query", "q": "from:me date:>2024" } }
{ "scope": { "kind": "selection", "conversation_ids": [12, 40], "message_ids": [913] } }
```

`everything` is every non-trashed message the account holds. `query` is the [search language](#search-operators-q) against the Messages list. A blank `q` is refused, because that is the `everything` form. `selection` is conversations and messages picked by hand: a message is selected when its conversation is listed or it is listed itself. Either list may be empty but not both, each list holds at most 500 ids, and an id the account does not hold is refused by name. A selection hides trashed conversations and duplicates the way a browse does.

A run is a snapshot. When it is created, the vault lists the messages the scope matches, and `GET /v1/exports/{id}/messages` pages that list, oldest first (`sort=-date` for newest first), with a default page of 100 and no offset cap. An import, a trash, or a new day while the run is open changes nothing it hands over: `import:last` and relative dates keep the meaning they had at creation, and a message whose conversation is trashed afterwards is still handed over.

A page's `total` is always the run's `message_count`. A message deleted permanently afterwards leaves its place empty, so its page holds fewer than `limit` items. A client steps `offset` by `limit` until it reaches `total`, and doesn't stop on a short or empty page. Each page read raises the run's `messages_delivered` to how far into the list the pages have read, so an abandoned run shows how far it got. A run that is no longer `running` answers `409 Conflict`.

The client closes the run. `POST /v1/exports/{id}/complete` or `POST /v1/exports/{id}/cancel` sets the status and `finished_at`, deletes the run's list of messages, and answers the run. A second close answers `409 Conflict`. `GET /v1/exports` is a page of every run, newest first (`sort=started_at` for oldest first), narrowed by `status` to one of `running`, `completed`, `failed`, `cancelled`. `GET /v1/exports/{id}` is one run. The record holds what was asked for and how much matched, never what the messages said.

Every export route takes the `export` scope on a session or an API token. A program holding an export token reads messages only through a run it started, because `GET /v1/messages` and the other browse routes refuse a token.

## Search operators (`q`)

`q` is the same search language the website uses. [Search](/vault/user/how-to/search/) has the full grammar: quoting, `none`/`any`, date and size ranges, `-` to exclude, `or` and parentheses, `avoc*` prefixes. An Export Run's `query` scope compiles `q` against the Messages list, with the same compiler Contacts and Conversations search use elsewhere in the vault, full-text index included for free text. `GET /v1/search-fields` lists the words each list accepts. These are the words the Messages list has:

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
- `date:`, `first-message:`, `last-message:` — a day, month, year, or relative span, with comparisons and ranges. A message's `timestamp` is a UTC instant. The span's edges are midnight in the account's `time_zone` (`GET /v1/accounts/{id}`), turned into instants before the comparison, so the same rule serves SQLite and Postgres and the `year=` filter on a conversation's messages.
- `attachment:` — `image`, `video`, `audio`, `document`, `pdf`, `contact`, `other`, `any`, `none`.
- `filename:` — an attachment's file name; text or a `pre*` prefix.
- `size:` — an attachment's size, with comparisons and ranges.
- `participants:` — how many people are in the conversation, with comparisons and ranges.
- `attachments:` — how many attachments are on the message, with comparisons and ranges.
- `trashed:` — `yes`, `no`, or `any`. Trash is excluded by default; `trashed:yes` or `trashed:any` lifts that.

`messages:`, `conversations:`, and `groups:` belong to the Contacts and Conversations lists, not Messages, so export refuses them.

Health check: <http://127.0.0.1:8080/health>
