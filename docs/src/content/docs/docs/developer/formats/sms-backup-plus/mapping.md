---
title: "SMS Backup+ import mapping"
description: "How SMS Backup+ EML fields become the shared conversation structure."
---

How flat and archive `.eml` messages become shared `ConversationDocument` values, including identity resolution, deduplication, and retained source data.

Input layouts: [format](/docs/developer/formats/sms-backup-plus/format/). Shared model: [message-ir](/docs/developer/architecture/common-message/). CSV projection: [CSV columns](/docs/developer/reference/csv-columns/) and [`message_ir_format::CSV_HEADERS`](https://github.com/messagecrate/message-crate/blob/main/crates/libs/ir-format/src/write.rs).

## Goal / non-goal

- **Goal:** Document how SMS Backup+ fields fill shared conversation fields and the retained source-data bag.
- **Non-goal:** Define EML source layouts or a private CSV schema. Source layout belongs in [format](/docs/developer/formats/sms-backup-plus/format/), and every output format is projected from the shared model.

## Pipeline / output

Source EML → `ConversationDocument` → [`message_ir_format::FormatSink`](https://github.com/messagecrate/message-crate/blob/main/crates/libs/ir-format/src/format_sink.rs) ).

The desktop app's Import screen always writes JSON Lines, one file per conversation, because Import and Push read conversation files in that form. Every other format (JSON, CSV, EML, MBOX, SMS Backup & Restore XML) is a rewrite of that output through [Convert](/docs/developer/formats/convert/), which Export and **Settings → Convert** run.

In CSV form: one file per conversation (header + one row per message after dedupe). MIME attachments under `attachments/` when copying/embedding. Filenames: 1:1 → `+E164.csv`; untitled groups → `group_+A_+B_….csv` (max 10 phones, then a hash). A mail with no `X-smssync-address` is keyed by the name in its subject, trimmed and otherwise unchanged, so "José" and "Josè" are two conversations; its file name is made from that name like any other, with a short suffix when two names give the same file name. The XML form is a single SyncTech `smses.xml`.

## Source → shared fields

| Shared field | EML source |
|---------------|------------|
| `chat_identifier` | Peer's handle key (a number, an email address or a sender name, classified by `phone::Handle::parse`) or `chat-group-…` |
| `conversation_type` | `group` when two or more other participants are named, else `individual`. A mail whose `To` names two or more addresses (in practice an MMS) takes its participants from `To`, plus `From` when it was received, leaving out the owner; any other mail takes them from `X-smssync-address`. See the `From` / `To` row in [format](/docs/developer/formats/sms-backup-plus/format/) for why |
| `group_title` | Derived for groups (empty for 1:1) |
| `participants_json` | Peer handles for the conversation |
| `guid` | SHA-256 of the message identity (`MessageGuid`): chat id, direction, sender, UTC milliseconds, collapsed text, sorted attachment digests |
| `timestamp` / `timestamp_utc` / `timestamp_display` / `timestamp_unix_ms` | Flat: `X-smssync-date` / `Date`; archive: body timestamp |
| `direction` | `incoming` / `outgoing` from `X-smssync-type` or archive sender |
| `service` | Always `sms` |
| `sender_handle` / `sender_display_name` | Outgoing uses export owner. Incoming one-to-one: the other participant. Incoming group: the address inside `<…>` of `From` (the part before `@unknown.email`, or the whole email address) when it is one of the participants, else no sender, counted as `group_messages_without_sender` in the run summary. The display name may come from Subject |
| `text` | The first `text/plain` part |
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

Every parsed mail is kept until the conversation is projected.
The shared dedupe step (`message_ir::one_copy_per_message`) then keeps one copy of each message.
Two copies are one message when the chat, the direction, the sender, the collapsed text and the attachment digests agree and the times are compatible.
Times are compatible when they are equal to the millisecond, or when one copy has whole seconds only and falls in the same second as the other.

A mail timed by `X-smssync-date` in milliseconds has milliseconds.
A mail timed by `Date`, or by `X-smssync-date` in seconds, has whole seconds only.
A whole-second copy yields to a millisecond copy, so the message keeps the millisecond time and one `guid` whichever file is read first.
Two millisecond copies with different times are two messages, such as the same text sent twice 300 ms apart.

`X-smssync-id` never decides whether two copies are one message, because one copy of a message can carry it and another lack it, and Android reuses the id after a wipe.
Between two copies nothing else tells apart, the one that carries `X-smssync-id` is kept, then the one whose `.eml` path sorts first.
Rows are sorted by time before writing.
