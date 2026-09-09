---
title: "SMS Backup+ import mapping"
description: "How SMS Backup+ EML fields become the shared conversation structure."
---

How flat and archive `.eml` messages become shared `ConversationDocument` values, including identity resolution, deduplication, and retained source data.

Input layouts: [format](/vault/developer/formats/sms-backup-plus/format/). Shared model: [message-ir](/vault/developer/architecture/common-message/). CSV projection: [CSV columns](/vault/developer/reference/csv-columns/) and [`message_ir_format::CSV_HEADERS`](https://github.com/bitrealm-io/message-vault/blob/main/crates/libs/ir-format/src/write.rs).

## Goal / non-goal

- **Goal:** Document how SMS Backup+ fields fill shared conversation fields and the retained source-data bag.
- **Non-goal:** Define EML source layouts or a private CSV schema. Source layout belongs in [format](/vault/developer/formats/sms-backup-plus/format/), and every output format is projected from the shared model.

## Pipeline / output

Source EML → `ConversationDocument` → [`message_ir_format::FormatSink`](https://github.com/bitrealm-io/message-vault/blob/main/crates/libs/ir-format/src/format_sink.rs) (`--format json|jsonl|csv|eml|mbox|xml`; default `json`).

With `--format csv`: one file per conversation (header + one row per message after dedupe). MIME attachments under `attachments/` when copying/embedding. Filenames: 1:1 → `+E164.csv`; untitled groups → `group_+A_+B_….csv` (max 10 phones, then a hash). Peers with no usable phone number are written to `unknown.csv`. `--format xml` writes a single SyncTech `smses.xml`.

## Source → shared fields

| Shared field | EML source |
|---------------|------------|
| `chat_identifier` | Peer E.164 or `chat-group-…` |
| `conversation_type` | `individual` / `group` from address list |
| `group_title` | Derived for groups (empty for 1:1) |
| `participants_json` | Peer handles for the conversation |
| `guid` | Deterministic SHA-256 fingerprint |
| `timestamp` / `timestamp_utc` / `timestamp_display` / `timestamp_unix_ms` | Flat: `X-smssync-date` / `Date`; archive: body timestamp |
| `direction` | `incoming` / `outgoing` from `X-smssync-type` or archive sender |
| `service` | Always `SMS` |
| `sender_handle` / `sender_display_name` | Outgoing uses export owner; incoming may use Subject / name hint |
| `text` | The `text/html` part stripped to text when there is one, else `text/plain` |
| `attachments_json` | Non-text MIME parts under `attachments/` |
| `message_kind` | `sms` or `mms` |
| `export_source` / `export_tool` / `export_tool_version` | `sms-backup-plus` / `SMS Backup+` / `1.5.11` |
| `owner_handle` / `owner_display_name` | Export owner |
| `android_type` | Raw `X-smssync-type` when present |
| `source_fields_json` | Vendor bag (below) |

Apple-only columns stay empty.

## `source_fields_json`

| Bag key | Meaning |
|---------|---------|
| `smssync_id` | `X-smssync-id` when present |
| `eml_path` | Relative path to the source `.eml` |

## Deduplication

Duplicates are collapsed **while scanning** with a cover key (`cover_identity`):

`{chat_id}|{timestamp_ms_floored_to_second}|{0|1}|{normalized_text}`

That ignores sub-second time and `X-smssync-id`, so two exports of one message meet even where their timestamps disagree below the second and only one kept the id. When two copies collide, the one carrying `X-smssync-id` wins, because only some export routes preserve it; attachments are merged by content digest so MMS media is not dropped. Otherwise the earlier timestamp wins. Rows are sorted by time before writing.

Text normalization collapses whitespace.
