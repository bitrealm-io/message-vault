---
title: Convert
description: What the Convert tab of Settings does, rewriting a folder of exported files into another format without reading a backup or Message Crate.
---

**Convert** rewrites a folder of already-exported files into a different format.
It reads files and writes files.
It never opens a phone backup, and it never reads or changes anything in Message Crate.

The **Convert** tab of **Settings** appears in the desktop app only, because the conversion runs on the computer, not on the server.
A browser does not show the tab.
The Owner's own Settings has no **Convert** tab either, because the Owner holds no messages.

Convert sits under Settings, not beside Import and Export, because most people never need it.
[Import](/docs/user/features/messages/import/) never requires it, and [Export](/docs/user/features/messages/export/) already writes every format Convert does.
It serves the case where an export exists in one format and a copy in another is wanted, such as JSON Lines rewritten as MBOX for a mail client.

## The form

The tab holds three fields and two buttons.

| Field | Holds |
|---|---|
| **Input folder** | The folder holding one existing export |
| **Output folder** | A different folder to write into |
| **Output format** | The format to write. The default is **JSON Lines (.jsonl)** |

**Convert** starts the conversion.
It stays disabled until both folders are filled in and differ.
**Cancel** stops a running conversion.

A log under the buttons fills in as the conversion runs.
Its first line names the format found in the input folder, such as `Detected input format: json`.
Its second line gives the number of conversations, such as `Conversations: 384`.

A finished conversion shows a green panel that names the format and the output folder, such as "Conversion complete. MBOX (.mbox) written to /home/sam/exports/mbox."
A failed one shows the reason in a red panel and as an `Error:` line in the log.

## What it reads

The input format is detected from the folder, not chosen.
Convert reads the six formats Export writes:

| Format | Recognised by |
|---|---|
| JSON Lines | `.jsonl` or `.ndjson` files |
| JSON | `.json` files |
| CSV | `.csv` files |
| MBOX | `.mbox` files |
| EML | Folders that hold `.eml` files |
| Android XML | A file named `smses.xml`, or an `.xml` file that starts with `<smses` |

A `.json`, `.jsonl`, or `.csv` file counts only when its contents look like a Message Crate export, so an unrelated file with the same extension is passed over.

Detection ignores the `attachments` folder, any name that starts with a dot, and names that end in `.meta.json`, `.tmp`, or `.xml.sbrbody`.

A folder must hold exactly one format.
A folder that holds more than one is refused with `unsupported input: mixed formats`, followed by the formats and files found, because Convert can't tell which export to read.
A folder that holds none is refused with `unsupported input: no Message Crate IR export found`.

A `.json` or `.jsonl` export of another schema version, such as an export written before version 4, is refused with "This file is schema version 3; Message Crate reads version 4" and the file's name.
Nothing is upgraded: export the conversations again with the current app.
Convert reads every file before it writes, so a refused file stops the whole run and the output folder is left as it was.

## What it writes

| **Output format** | Shape | Media |
|---|---|---|
| **JSON Lines (.jsonl)** | One `.jsonl` per conversation | `attachments/` folder |
| **JSON (.json)** | One indented `.json` per conversation | `attachments/` folder |
| **CSV (.csv)** | One `.csv` per conversation | `attachments/` folder. Columns: [CSV columns](/docs/developer/reference/csv-columns/) |
| **EML (one file per message)** | One folder per conversation, one `.eml` per message | Embedded |
| **MBOX (.mbox)** | One `.mbox` per conversation | Embedded |
| **Android XML (smses.xml)** | One `smses.xml` holding only SMS and MMS | Embedded |

The folder layout is described in [Export structure](/docs/developer/reference/export-structure/).

## Why the two folders must differ

Convert clears an earlier export out of the output folder before it writes.
A second run into the same output folder therefore replaces the first instead of mixing with it.
Writing into the input folder, or into a folder that holds it, would delete the files being read.

Two checks prevent that.
The screen keeps **Convert** disabled while both fields name the same folder, and says "Choose a different output folder."
The conversion itself refuses an output folder that resolves to the input folder or to a folder above it, with `output <folder> must not be the same as, or contain, the input <folder>`.
The second check also catches a symbolic link that points at the input folder.

## What the output folder may hold

An output folder that does not exist is created.
An empty folder, or one that holds an earlier export, is used as it is.

A folder that holds only unrelated files is refused, because the clean-up step deletes only what looks like an export and has no way to know the folder was chosen on purpose.
The message ends "Use an empty directory or one previously used for exports."

The clean-up removes earlier export files and the whole `attachments` folder.
It leaves a hidden file named `.message-crate-export` behind, which marks the folder as one Convert may clear on a later run.
When Convert writes an SMS Backup & Restore file, that hidden file lists `smses.xml`, so the next export into the folder removes it. Other XML files in the folder are kept.

## Limits

- Attachments reach the output only when they are present in the input folder, because Convert copies them from its `attachments` folder and never fetches them from the server.
- Android XML holds only SMS and MMS, because SMS Backup & Restore cannot describe an iMessage, a WhatsApp message or any other kind. Convert leaves every other message out and its log says how many: `Left out 3 message(s) that are not SMS or MMS, because SMS Backup & Restore holds only SMS and MMS`. JSON or JSON Lines is the format to choose when those messages matter.
