---
title: Convert formats
description: Rewrite a folder of exported files into another format without re-reading the backup or touching the vault.
---

**Convert** rewrites a folder of already-exported files into a different format.
It reads files and writes files. It never opens a phone backup and never reads or changes the vault.
Convert lives in the desktop app under **Settings → Convert**, because it is a tool most people use rarely rather than a third sidebar entry beside Import and Export.

Happy-path [Import](/vault/user/import-from-a-backup/) never needs this step.
Convert is for the case where an export already exists in one format and a copy in another format is wanted, for example a JSON Lines export from [Export](/vault/user/how-to/export-from-the-vault/) rewritten as MBOX for a mail client.

## Before starting

- The desktop app, logged in
- A folder that holds one existing export
- A second, different folder to write into

## Run a conversion

1. Open **Settings** in the sidebar and choose the **Convert** tab
2. Choose the **Input folder**, the folder that holds the existing export
3. Choose the **Output folder**, a different folder to write into
4. Choose the **Output format**
5. Select **Convert** and wait for the log to finish

The log's first line names the format Convert found in the input folder, for example `Detected input format: json`.
The last line reports the conversion as complete.

## What it reads

The input format is detected from the folder, not chosen.
Convert reads any of the six formats Export writes:

- JSON Lines (`.jsonl` files, one per conversation)
- JSON (`.json` files, one per conversation)
- CSV (`.csv` files, one per conversation)
- MBOX (`.mbox` files, one per conversation)
- EML (one folder per conversation, `.eml` files inside)
- Android XML (a single `smses.xml`)

The detector ignores an `attachments/` folder and older metadata files.
A folder that holds files in more than one of these shapes is reported as ambiguous and the run stops, because Convert can't tell which export to read.

## What it writes

| Format | Shape | Media |
|---|---|---|
| **JSON Lines** | One `.jsonl` per conversation | `attachments/` folder |
| **JSON** | One indented `.json` per conversation | `attachments/` folder |
| **CSV** | One `.csv` per conversation | `attachments/` folder. Columns: [CSV columns](/vault/developer/reference/csv-columns/) |
| **EML** | One folder per conversation, one `.eml` per message | Embedded |
| **MBOX** | One `.mbox` per conversation | Embedded |
| **Android XML** | One `smses.xml` | Embedded. Apple-only fields are dropped |

Folder layout: [Export structure](/vault/developer/reference/export-structure/).

## Why the two folders must differ

Convert clears earlier export files out of the output folder before it writes, so that a second run over the same output folder replaces the first rather than mixing with it.
Writing into the input folder would therefore delete the very files being read.
The screen keeps **Convert** disabled while both fields name the same folder, and the conversion itself refuses a pair of paths that resolve to one folder, which also catches a symbolic link to the input.

An output folder that holds unrelated files is refused too, because the clean-up step deletes only what looks like an export and stops when it meets anything else.
An empty folder, or one holding a previous export, works.

## Limitations

- Attachments come across only when they are still present in the input folder, because Convert copies them rather than fetching them from the vault
- Android XML can't store Apple-only fields such as message effects and Tapbacks, so JSON or JSON Lines is the right target when iMessage detail matters
- Convert runs in the desktop app only, because the conversion runs in the desktop process rather than on the vault
