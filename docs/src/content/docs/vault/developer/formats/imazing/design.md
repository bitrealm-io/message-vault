---
title: "iMazing importer design"
description: "Parser decisions and validation for the iMazing rescue converter."
---

Living design notes for [`imazing-exporter`](../). Append dated findings; do not erase prior validation rows.

The observed iMazing 3.5.5 directory layout, CSV headers, and source limitations are documented separately in [input format](/vault/developer/formats/imazing/input/). This file explains how the importer discovers, interprets, and converts those files.

## Goals

- Accept either one iMazing Messages/WhatsApp CSV **or** a folder at any level of a device export tree.
- Emit the common message → packaging via `FormatSink` (CSV uses shared [`CSV_HEADERS`](https://github.com/bitrealm-io/message-vault/blob/main/crates/libs/ir-format/src/write.rs); default JSON), with WhatsApp kept separate from SMS/iMessage.
- Resolve phones/names through an optional vCard CSV.

## Input discovery

Discovery walks the selected path recursively without following directory symbolic links. Matching files are sorted before parsing so repeated runs process them in the same order. Header classification separates Messages CSV files from WhatsApp CSV files and prevents Contacts exports from being parsed as conversations. See [input format](/vault/developer/formats/imazing/input/) for the accepted paths and identifying headers.

## Output policy

- Pipeline: iMazing CSV → `ConversationDocument` → [`message_ir_format::FormatSink`](https://github.com/bitrealm-io/message-vault/blob/main/crates/libs/ir-format/src/format_sink.rs) Import writes JSON Lines; the other formats come from [Convert](/vault/developer/formats/convert/). Shared header: [`CSV_HEADERS`](https://github.com/bitrealm-io/message-vault/blob/main/crates/libs/ir-format/src/write.rs) / [CSV columns](/vault/developer/reference/csv-columns/).
- SMS + iMessage for the same peer merge into one conversation (Messages family).
- WhatsApp for the same peer is a **separate** file (`…__whatsapp.csv` / matching stem suffix for other formats).
- Notification rows keep `imazing_type=Notification` in `source_fields_json`; direction is emitted as `incoming`.
- Vendor-lossy fields live in `source_fields_json` (not top-level CSV columns): `imazing_type`,
  `imazing_status`, `replying_to`, `forwarded`, `attachment_info`, `reactions`, `delivered_date`,
  `read_date`, `edited_date`, `deleted_date`, `sent_date`, plus `group_title` when the session
  string is a display title.
- `participants_json` is always written (unified header).
- Deduplication key includes attachment identity so same-time/text with different media are kept.
- When the Import form's **Attachments** choice copies media (and always for mail / Xml), attachments are resolved by basename or
  suffix-match against files beside the source CSV and copied under `output/attachments/`.
- Untitled group files are `group_+A_+B_….csv` (max 10 phones; if more, append a 16-hex hash of
  the full roster). WhatsApp adds `__whatsapp` before `.csv`. The `chat_identifier` cell is unchanged.

## Chat identity and participants

### Individual chats

Prefer `Sender ID` phones/emails; else normalize a phone-like `Chat Session`; else Contacts
name→phone; else a sanitized name stem (reported as unresolved).

### Messages groups

`Chat Session` often encodes a roster as `Name A & Name B & Name C`.

1. Collect phones/emails from sender rows and `+digits` in the session string.
2. Split roster labels on ` & `.
3. Resolve name-only labels through Contacts.
4. Group `chat_identifier` = sorted, comma-joined resolved handles when any exist.

### WhatsApp groups

`Chat Session` is a **title**, not a roster. Participants are inferred only from distinct senders.
Non-senders are invisible in the CSV.

## Validation matrix

| Date | iMazing | Sample | Result |
|------|---------|--------|--------|
| 2026-07-19 | 3.5.5 | Full device export (Messages + WhatsApp + Contacts) | Headers/layout confirmed; silent-roster limitation quantified on Messages groups; WhatsApp schema differs as above |
| 2026-07-19 | 3.5.5 | Synthetic fixtures in `tests/fixtures/` | Recursive discovery, service separation, silent-member contact recovery |

## Future work (not yet implemented)

- An optional owner phone number on the Import form to annotate the outgoing sender handle.
- Structured parse of reactions / replies if a stable grammar is confirmed.

## Related docs

- Input format and source limitations: [iMazing input format](/vault/developer/formats/imazing/input/)
- Contacts helper: [`crates/libs/contacts`](https://github.com/bitrealm-io/message-vault/tree/main/crates/libs/contacts)
- Shared model and output contracts: [message-ir architecture](/vault/developer/architecture/common-message/), [export structure](/vault/developer/reference/export-structure/), [CSV columns](/vault/developer/reference/csv-columns/)
