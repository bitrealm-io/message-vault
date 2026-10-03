---
title: "SMS Backup & Restore import mapping"
description: "How SyncTech SMS and MMS fields become the shared conversation structure."
---

How SyncTech `<sms>` and `<mms>` elements become `ConversationDocument` values, including validation, skipped records, and the source-specific data retained for later output.

Input structure and attribute meanings: [input format](/docs/developer/formats/sms-backup-restore/input/). Shared model: [message-ir](/docs/developer/architecture/common-message/). CSV projection: [CSV columns](/docs/developer/reference/csv-columns/).

## Pipeline

Source XML → `ConversationDocument` → packaging.

The desktop app's Import screen always writes JSON Lines, one file per conversation, because Import and Push read conversation files in that form. Every other format (JSON, CSV, EML, MBOX, SMS Backup & Restore XML) is a rewrite of that output through [Convert](/docs/developer/formats/convert/), which Export and **Settings → Convert** run.

In CSV form: one file per conversation. Decoded MMS media under `attachments/` when copying/embedding. Filenames: 1:1 → `+E164.csv`; untitled groups → `group_+A_+B_….csv`. The XML form is a single SyncTech `smses.xml`.
## Source → shared fields

| Shared field | SMS / MMS source |
|---------------|------------------|
| `chat_identifier` | Peer's handle key, or `chat-group-…` for groups |
| `conversation_type` | `individual` / `group` |
| `group_title` | Derived for groups; empty for 1:1 |
| `participants_json` | Peer handles from SMS address / MMS `<addr>` list |
| `guid` | SHA-256 of the message identity (`MessageGuid`): chat id, direction, sender, UTC milliseconds, collapsed text, sorted attachment digests |
| `timestamp` / `timestamp_utc` / `timestamp_display` / `timestamp_unix_ms` | From `date` (Unix epoch milliseconds, UTC) |
| `direction` | `incoming` / `outgoing` from SMS `type` or MMS `msg_box` / From addr |
| `service` | Always `sms` |
| `sender_handle` / `sender_display_name` | Incoming peer; outgoing uses export owner (`owner_*`) |
| `subject` | SMS `subject`, or MMS `sub` |
| `text` | SMS `body`, or MMS text/plain parts (HTML entities decoded) |
| `attachments_json` | Extracted MMS media paths |
| `message_kind` | `sms` or `mms` |
| `export_source` / `export_tool` / `export_tool_version` | `sms-backup-restore` / `SMS Backup & Restore` / `10.26.003` |
| `owner_handle` / `owner_display_name` | Export owner |
| `android_type` | SMS `type`, or MMS `msg_box` |
| `source_fields_json` | Full fidelity JSON (below) |

Apple-only columns (`parts_json`, tapbacks, balloons, …) stay empty.

## How the exporter uses SMS fields

- `address` → `chat_identifier` / participant handle, classified once by `phone::Handle::parse`: a number keeps the country its `+` names and is otherwise read as a US number, an address with `@` is an email address, and anything else (such as `AMAZON`) is a sender name, an identity of type `other`. Only a blank `address` is skipped, as `skipped_unknown_address`
- `date` → `timestamp*` and `timestamp_unix_ms` (invalid or missing dates are skipped)
- `type` `1` / `2` → `direction` incoming / outgoing; `3` (draft) and `4` (outbox) are skipped and counted as `skipped_draft_or_outbox`; other types are skipped and counted as `skipped_unknown_type`; raw value in `android_type`
- `body` → `text` (character references and HTML entities decoded; a surrogate pair written as two references, such as `&#55357;&#56832;`, becomes one character; a reference that is not a character, such as `&#0;` or a lone surrogate, is dropped and counted as `dropped_character_references`)
- `subject` → `subject` when present
- `contact_name` → `sender_display_name` for incoming and the peer's participant name (not a separate CSV column). The app writes `null` or `(Unknown)` where the phone has no contact, so an empty value, `null` and `(Unknown)`, compared without case, give no name and the peer stays Unknown
- **Every** `<sms>` attribute → `source_fields_json.attrs`

Example: `<sms address="+15555550101" date="1400773261000" type="1" body="hello &amp; hi" contact_name="Sam" />` becomes an incoming row with `chat_identifier=+15555550101` and text `hello & hi`.

## How the exporter uses MMS fields

- `date` → `timestamp*` / `timestamp_unix_ms` (bad dates skipped)
- `msg_box` `2` → outgoing; `1` → incoming. The From addr (`type="137"`) is the sender, wherever that number sits in the `address` list. Without one, a 1:1 message is from its one peer and a group message has no sender: the app writes a From addr on every MMS, and guessing a group's sender from the address order would be wrong more often than right. Raw `msg_box` in `android_type`
- `msg_box` `3` (draft) and `4` (outbox) are skipped and counted as `skipped_draft_or_outbox` (not `skipped_unknown_type`, which is for unknown SMS `type` only)
- `sub` → `subject`
- `contact_name` names the peer and the sender of a 1:1 MMS, read as on an `<sms>`. On a group MMS it is the members' names joined by a comma and a space (such as `Ana, Lee`), which names the group rather than the sender, so a group message takes no name from it
- `address` plus `<addr>` list → participants; one other person is a 1:1 chat, more than one is a group
- `text/plain` parts → `text`; SMIL (`application/smil`) controls text/image order when present
- Non-text `data` → files under `attachments/` and `attachments_json`; in `source_fields_json.parts`, `data` is replaced with `data_len` + `data_sha256`
- Every `<mms>` / `<part>` / `<addr>` attribute → `source_fields_json`
- Empty participant lists and undecodable attachment base64 are skipped and counted in the run report

Example group address string: `+15555550101~+15555550102` with two From/To addrs becomes a group conversation titled from those two numbers.

**Group chat identity limitation:** the format has no stable thread ID, so a group conversation is keyed by the sorted set of participant numbers (`chat-group-…`). When the roster changes (someone is added or removed), messages before and after the change are grouped into two separate conversations. This is inherent to the source data and cannot be recovered.

## `source_fields_json`

### SMS

```json title="SMS source_fields_json"
{ "kind": "sms", "attrs": { /* every <sms> attribute */ } }
```

### MMS

```json title="MMS source_fields_json"
{
  "kind": "mms",
  "attrs": { /* every <mms> attribute */ },
  "parts": [ { /* every <part> attribute */ } ],
  "addrs": [ { /* every <addr> attribute */ } ]
}
```

For each `<part>` that has a `data` attribute, the bag stores `data_len` and `data_sha256` of the **decoded** bytes and **omits** the base64 `data` string (binaries live under `attachments/` or are embedded for mail/Xml). Other part attributes (`seq`, `ct`, `name`, `cl`, `chset`, `text`, …) are kept as-is.

The reverse `ConversationDocument` → `smses.xml` rules are documented in [SMS Backup & Restore XML output](/docs/developer/formats/sms-backup-restore-xml/).
