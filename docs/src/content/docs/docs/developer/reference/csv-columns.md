---
title: Understand CSV columns
description: Read the shared columns written for every conversation CSV.
---

CSV output contains one row per message. Conversation and export identity are repeated on every row, so there is no separate metadata file.

## Conversation and message identity

| Column | Meaning |
| --- | --- |
| `chat_identifier` | Identifier for the conversation, usually a peer handle or group identifier. |
| `conversation_type` | `individual` or `group`. |
| `group_title` | Group name, or empty when none is known. |
| `participants_json` | JSON array of participant handles and display names. |
| `guid` | Stable message identifier. |
| `message_kind` | Kind such as `sms`, `mms`, `imessage`, or `tapback`. |

## Time, direction, and content

| Column | Meaning |
| --- | --- |
| `timestamp` | Local RFC 3339 time. |
| `timestamp_utc` | UTC RFC 3339 time. |
| `timestamp_display` | Human-readable time. |
| `timestamp_unix_ms` | Unix time in milliseconds. |
| `direction` | `incoming` or `outgoing`. |
| `service` | `sms`, `imessage`, `whatsapp`, `rcs`, or `unknown`. |
| `sender_handle` | Sender phone number, email, or other handle. Outgoing rows use the export owner when known. |
| `sender_display_name` | Sender name. Outgoing rows default to `Me` when an owner handle is known. |
| `subject` | Message subject when present. |
| `text` | Message body. |
| `attachments_json` | JSON array with attachment path, original name, media type, and available file fingerprints and media details. |

## Source and owner

| Column | Meaning |
| --- | --- |
| `export_source` | Source family used for the import. |
| `export_tool` | Name of the source tool or format. |
| `export_tool_version` | Source version recorded by the importer. |
| `owner_handle` | Phone number or email for the person whose backup was exported. |
| `owner_display_name` | Display name for the owner. |
| `android_type` | Original Android SMS type or MMS box number, or empty for other sources. |
| `source_fields_json` | Compact JSON containing source-specific fields that do not have shared columns. |

## Apple Messages details

The remaining columns hold iMessage features. They are empty or `false` for sources that do not provide those features:

- `read_receipt`, `is_deleted`, `send_effect`, and `shared_location`;
- `is_announcement` and `announcement`;
- `is_reply`, `thread_originator_guid`, `thread_originator_part`, and `num_replies`;
- `parts_json`, `edits_json`, `tapbacks_json`, and `app_json`;
- `balloon_bundle_id` and `balloon_kind`; and
- `associated_guid`, `associated_part`, `tapback_kind`, `tapback_emoji`, and `tapback_action`.

## How values are written

Nested values use compact JSON. When `source_fields_json`, `parts_json`, `edits_json`, `tapbacks_json`, or `app_json` has no value, the cell is empty rather than the word `null`.

The boolean columns `is_deleted`, `is_reply`, and `is_announcement` contain `true` or `false`. A simple Apple text part that only repeats the `text` column is omitted from `parts_json`; richer or multi-part bodies keep it.

Older columns named `date_ms`, `contact_name`, and `xml_fields_json` are not written.
