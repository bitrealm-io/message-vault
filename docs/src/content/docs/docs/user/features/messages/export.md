---
title: Export
description: What the desktop app's Export screen writes, what an export covers, and the formats it offers.
---

**Export** writes an account's messages and their attachments to a folder on the computer.
It reads the Message Crate, never a phone backup.

Export is in the desktop app's sidebar, under **Messages**.
The browser doesn't show it, because writing a folder of files needs the desktop app.

Every export is recorded as an Export Run.
[**Settings → Storage**](/docs/user/features/settings/storage/) lists them under **Export history**.
Each row has the date, the scope, the status, the count of messages that matched, the count delivered, the count of attachments, and their size.
The record holds what was asked for, never what the messages said.

## The form

The Export screen has four fields.

| Field | What it sets |
|---|---|
| **Scope** | **Everything** or **Search**. |
| **Search** | The search an export is limited to. Shown only when **Scope** is **Search**. |
| **Save to** | The folder the export is written into. |
| **Format** | One of six formats. **JSON Lines (.jsonl)** is the default. |

**Export** starts the run and **Cancel** stops it.
The **Export** button stays disabled until **Save to** holds a folder, and under **Search** until the search box holds a search.
A log under the buttons shows what the run is doing.
A finished run reads `Export complete.` followed by the format and the folder.

## Scope

**Everything** writes every conversation the account holds.

**Search** writes only the conversations a search finds.
The box takes the same [search language](/docs/user/features/messages/search/) as the search bar, so `from:me last year` exports what that search shows.
`in:` names particular conversations by title, by identity, or by id: `in:#19,#22` exports those two conversations and nothing else.

Export opened from the conversation list starts in **Search**, with the search that list is showing already in the box.
Opened from any other screen, or with no search running, it starts in **Everything**.

An Export Run hands over the messages that matched when it started.
A message imported or trashed while the run is being read does not change what the run writes.

## Formats

| Format | What is written |
|---|---|
| **JSON Lines (.jsonl)** | One `.jsonl` file per conversation, attachments in an `attachments/` folder |
| **JSON (.json)** | One indented `.json` file per conversation, attachments in an `attachments/` folder |
| **CSV (.csv)** | One `.csv` file per conversation, attachments in an `attachments/` folder. Columns: [CSV columns](/docs/developer/reference/csv-columns/) |
| **EML (one file per message)** | One folder per conversation, one `.eml` file per message, attachments embedded |
| **MBOX (.mbox)** | One `.mbox` file per conversation, attachments embedded |
| **Android XML (smses.xml)** | A single `smses.xml`, attachments embedded |

Export always fetches the messages as JSON Lines first.
Any other format is written by converting that JSON Lines copy, as part of the same run.

The JSON Lines layout is described in [Export structure](/docs/developer/reference/export-structure/).

## The folder an export writes into

A JSON Lines export writes straight into the **Save to** folder.
It also keeps a file named `.message-crate-pull-state.jsonl` there, which records the attachments already downloaded.
A later JSON Lines export into the same folder skips those attachments.

An export in any other format first deletes the files of an earlier export from the **Save to** folder.
It refuses a folder that holds other files and no export, so that nothing unrelated is deleted.
An empty folder, or one a previous export wrote, is accepted.

## The Staging Directory

An export in any format other than JSON Lines needs two folders, because the conversion reads one folder and writes another.
The JSON Lines copy goes into the Staging Directory, and the converted files go into the **Save to** folder.

The Staging Directory is `~/message-crate` by default, the same folder Import uses.
[**Settings → System**](/docs/user/features/settings/system/) changes it under **Staging directory**.
The export's folder inside it is deleted when the export finishes, including when the conversion fails.

The disk that holds the Staging Directory needs room for a second copy of the exported messages and attachments while such an export runs.
