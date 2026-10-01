---
title: Export formats
description: What each of the six export formats produces on disk.
---

Message Crate writes an export in one of six formats. Which one you get is chosen on the [Export](/docs/user/features/messages/export/) screen in the desktop app. This page describes what each format produces, so you can pick knowing what lands in the folder.

## What each format writes

| Format | Shape | Media |
|---|---|---|
| **JSON Lines** | One `.jsonl` per conversation, one JSON object per line | `attachments/` folder |
| **JSON** | One indented `.json` per conversation | `attachments/` folder |
| **CSV** | One `.csv` per conversation | `attachments/` folder. Columns: [CSV columns](/docs/developer/reference/csv-columns/) |
| **EML** | One folder per conversation, one `.eml` per message | Embedded in each message |
| **MBOX** | One `.mbox` per conversation | Embedded |
| **Android XML** | A single `smses.xml` | Embedded. Apple-only fields are dropped |

Folder layout for the formats that use an `attachments/` folder: [Export structure](/docs/developer/reference/export-structure/).

## Which to pick

**JSON Lines** is Message Crate's own storage shape. It is the only format that loses nothing, and the only one it can read back in. Choose it for anything you may want to re-import.

**JSON** holds the same fields as JSON Lines, indented for reading. It is larger and slower to parse.

**CSV** is the format to open in a spreadsheet. It flattens each message to a row, so structure that does not fit a table — reactions, edit history, and per-attachment detail beyond the filename — is not represented.

**EML** and **MBOX** are mail formats, so a conversation opens in a mail client with its media attached rather than sitting beside it. MBOX puts a conversation in one file; EML puts each message in its own.

**Android XML** matches the SMS Backup & Restore schema, which is what makes it useful for moving messages back onto an Android phone. It is the lossiest option: fields that exist only on Apple platforms are dropped.

## How conversion works

Every export is written as JSON Lines first, then rewritten into the format you asked for. Only JSON Lines skips the second step.

The intermediate copy goes in the staging directory, `~/message-crate` by default and changeable in [Settings → System](/docs/user/features/settings/settings/). It is deleted when the export finishes, including when the conversion fails, so a folder with room for one extra copy of the exported messages is enough.

Reading an existing export back in is a separate operation with no screen yet — see [issue 275](https://github.com/messagecrate/message-crate/issues/275). The `message-reexport` library still reads all six formats and converts between them; nothing in the app calls it that way today.
