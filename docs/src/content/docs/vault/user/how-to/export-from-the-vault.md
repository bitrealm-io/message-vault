---
title: Export from the vault
description: Save the messages in your vault to a folder on your computer, in the format you choose.
---

**Export** writes the messages in your vault, and their attachments, to a folder on your computer. It runs in the desktop app: Export appears in the sidebar once you sign in.

Export always writes the whole vault. There is no way yet to export only what you are browsing or only the conversations you have selected.

Every export is recorded as an Export Run: when it started, what it asked for, how many messages and attachments matched, and how it ended. [Settings → Storage](/vault/user/how-to/settings/) lists them under Export history.

## Before you start

- A vault that is running, and an account with messages already imported
- The desktop app, signed in
- An empty folder on disk to write into

## Export from the desktop app

1. Sign in to the vault in the desktop app
2. Open **Export** in the sidebar
3. Choose the folder to save into
4. Choose a format
5. Select **Export** and wait for the log to finish

## Formats

| Format | What you get |
|---|---|
| **JSON Lines** | One `.jsonl` file per conversation, attachments in an `attachments/` folder |
| **JSON** | One indented `.json` file per conversation, attachments in an `attachments/` folder |
| **CSV** | One `.csv` file per conversation, attachments in an `attachments/` folder. Columns: [CSV columns](/vault/developer/reference/csv-columns/) |
| **EML** | One folder per conversation, one `.eml` file per message, attachments embedded |
| **MBOX** | One `.mbox` file per conversation, attachments embedded |
| **Android XML** | A single `smses.xml`, attachments embedded. Apple-only fields are dropped |

JSON Lines is what the vault stores, so it is the fastest and the only format that loses nothing. Every other format is written by converting a JSON Lines export, which happens as part of the same run.

Folder layout: [Export structure](/vault/developer/reference/export-structure/).

## Where the temporary files go

Choosing any format other than JSON Lines takes two steps: the vault is written as JSON Lines first, then converted into the format you asked for. The intermediate copy goes in your staging directory — the same folder Import uses, `~/message-vault` by default, changeable in [Settings → System](/vault/user/how-to/settings/). It is deleted when the export finishes, including when the conversion fails.

Make sure that folder has room for a second copy of your vault while an export runs.
