---
title: "iMazing importer design"
description: "Parser decisions and validation for the iMazing rescue converter."
---

Living design notes for [`imazing-exporter`](../). Append dated findings; do not erase prior validation rows.

The observed iMazing 3.5.5 directory layout, CSV headers, and source limitations are documented separately in [input format](/docs/developer/formats/imazing/input/). This file explains how the importer discovers, interprets, and converts those files.

## Goals

- Accept either one iMazing Messages/WhatsApp CSV **or** a folder at any level of a device export tree.
- Emit the common message → packaging via `FormatSink` (CSV uses shared [`CSV_HEADERS`](https://github.com/messagecrate/message-crate/blob/main/crates/libs/ir-format/src/write.rs); default JSON), with WhatsApp kept separate from SMS/iMessage.

## Input discovery

Discovery walks the selected path recursively without following directory symbolic links. Matching files are sorted before parsing so repeated runs process them in the same order. Header classification separates Messages CSV files from WhatsApp CSV files and prevents Contacts exports from being parsed as conversations. See [input format](/docs/developer/formats/imazing/input/) for the accepted paths and identifying headers.

## Output policy

- Pipeline: iMazing CSV → `ConversationDocument` → [`message_ir_format::FormatSink`](https://github.com/messagecrate/message-crate/blob/main/crates/libs/ir-format/src/format_sink.rs) Import writes JSON Lines; the other formats come from [Convert](/docs/developer/formats/convert/). Shared header: [`CSV_HEADERS`](https://github.com/messagecrate/message-crate/blob/main/crates/libs/ir-format/src/write.rs) / [CSV columns](/docs/developer/reference/csv-columns/).
- SMS + iMessage for the same peer merge into one conversation (Messages family).
- WhatsApp for the same peer is a **separate** file (`…__whatsapp.csv` / matching stem suffix for other formats).
- Notification rows keep `imazing_type=Notification` in `source_fields_json`; direction is emitted as `incoming`.
- Vendor-lossy fields live in `source_fields_json` (not top-level CSV columns): `imazing_type`,
  `imazing_status`, `replying_to`, `forwarded`, `attachment_info`, `reactions`, `delivered_date`,
  `read_date`, `edited_date`, `deleted_date`, `sent_date`, plus `group_title` when the session
  string is a display title.
- `participants_json` is always written (unified header).
- The shared dedupe step (`message_ir::one_copy_per_message`) compares attachments by content digest, or by path when the file was not found, so rows with one time and text and different media are kept.
- When the Import form's **Attachments** choice copies media (and always for mail / Xml), attachments are resolved by basename or
  suffix-match against files beside the source CSV and copied under `output/attachments/`.
- When media is copied, the files beside a CSV that no row names are sorted after every CSV is read.
  A Live Photo's video (`.mov` beside a `.jpg` or `.jpeg` an Image row names) becomes the second
  attachment of that row's message, named as the row's picture with the video's extension. When two
  rows name the picture, the first in CSV order takes the video and the report names the picture.
  The report counts the videos as `live_photo_videos`.
- A link preview (`.url`) whose `URL=` address appears in the text of a row at the second the file
  name starts with is not imported, because it holds nothing the message does not already show. The
  report counts it as `link_previews_already_in_message`. Every other file no row names, a `.url`
  whose address no row shows included, is counted as `files_named_by_no_row`.
- Untitled group files are `group_+A_+B_….csv` (max 10 phones; if more, append a 16-hex hash of
  the full roster). WhatsApp adds `__whatsapp` before `.csv`. The `chat_identifier` cell is unchanged.

## Chat identity and participants

Each chat session has a `message_ir::ConversationKey`: a one-to-one chat with an address, a group, or a one-to-one chat known by a name only.
The key's chat id is the conversation's `chat_identifier`.
The rows of one session in one CSV are one conversation.

### Group or one-to-one

A Messages session named as a roster (`Name A & Name B`) is a group, even when one member wrote.
Any session in which two or more people wrote is a group.
The rows decide who is one person.
In Messages a row's `Sender ID` and `Sender Name` belong to one person, so two addresses under one `Sender Name` are one person: one contact writing from a number and an email address, or from two numbers, is a one-to-one chat.
Two different people saved under one name in a chat with a typed title are then counted as one person, and the chat is filed as one-to-one.
A WhatsApp account has one number, so in WhatsApp two numbers are always two people, whatever their `Sender Name`.

### One-to-one chats

The chat's address is the number in `Chat Session`, a `Chat Session` that is itself an address, or else the address of the earliest received row that has one, so new messages from a second address do not change it.
The chat lists that one address as its participant. A person's other address is not a participant: it is the sender of the messages written from it, with the same `Sender Name`.
Without any of these, the chat is known by its name: its `chat_identifier` is a name stem, it is counted as `name_only_chat`, and the server matches the name to a contact on import.
A received row with no `Sender ID` is from the chat's number or short code.

### Groups

iMazing gives a group no id, and nothing it names is a usable key.
A session name repeats across different groups, and it changes when a contact is renamed or a member joins or leaves.
The folder name carries the time of the latest message, the file name carries the export's own time, and both cut the label at 40 characters.

A group's `chat_identifier` is `group:` and the SHA-256 of its earliest row: `Message Date` as written, `Type`, `Sender ID`, `Text` and `Attachment`.
Where rows share the earliest time, the smallest of them is taken, so the order of the rows does not change the key.
Two groups can start with the same row, when the account holder sends one message to two new groups in the same second.
Of the groups that share an earliest row, one whose rows are the first rows of another is the same group read from an older export in the same input folder, and the two are one conversation.
Each other one's key hashes its earliest rows, as few as tell it apart from every one of the others.
That key changes when a group it was told apart from is gone from the phone, or a new group shares more of its earliest rows.
The key does not change when someone new writes, and it never equals a person's address.
Its limit is in [input format](/docs/developer/formats/imazing/input/#source-limitations).

A group's members are data on the conversation, never read back out of its `chat_identifier`:

1. Every address that sent a row, with its `Sender Name`.
2. Every `+digits` number in the session name.
3. For a Messages roster, every label split on ` & `. A label that is an address is that address. A name-only label resolves through the rows: a row whose `Sender Name` matches the label gives its `Sender ID`.
4. A label no row matches is a member who never wrote. The export holds no address for them, so the member has the name and no address, and the server gives them an identity of type `other` holding the name. Each is counted as `unresolved_group_participants`.

A group message's sender comes only from its row. A received row with no `Sender ID` has no sender.

WhatsApp `Chat Session` is a **title**, not a roster, so a WhatsApp group's members are the people who wrote.
Non-senders are invisible in the CSV.

## Validation matrix

| Date | iMazing | Sample | Result |
|------|---------|--------|--------|
| 2026-07-19 | 3.5.5 | Full device export (Messages + WhatsApp + Contacts) | Headers/layout confirmed; silent-roster limitation quantified on Messages groups; WhatsApp schema differs as above |
| 2026-07-19 | 3.5.5 | Synthetic fixtures in `tests/fixtures/` | Recursive discovery, service separation, silent-member contact recovery |
| 2026-10-02 | 3.5.5 | Full device export, read only (681 Messages and 66 WhatsApp conversations) | One CSV per conversation; five pairs of different groups share a session name; the earliest-row group key has no collision among the 747 conversations |

## Future work (not yet implemented)

- An optional owner phone number on the Import form to annotate the outgoing sender handle.
- Structured parse of reactions / replies if a stable grammar is confirmed.

## Related docs

- Input format and source limitations: [iMazing input format](/docs/developer/formats/imazing/input/)
- Shared model and output contracts: [message-ir architecture](/docs/developer/architecture/common-message/), [export structure](/docs/developer/reference/export-structure/), [CSV columns](/docs/developer/reference/csv-columns/)
