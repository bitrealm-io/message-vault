---
title: Export your messages
description: Save your messages to a folder on your computer, in the format you choose.
---

**Export** writes your messages, and their attachments, to a folder on your computer. It runs in the desktop app: Export appears in the sidebar once you log in.

An export covers every conversation, or only the conversations a search finds. The scope is the first choice on the screen.

Every export is recorded as an Export Run: when it started, what it asked for, how many messages and attachments matched, and how it ended. [Settings → Storage](/docs/user/features/settings/settings/) lists them under Export history.

## Before you start

- A running Message Crate server, and an account with messages already imported
- The desktop app, logged in
- An empty folder on disk to write into

## Export from the desktop app

1. Log in to Message Crate in the desktop app
2. Open **Export** in the sidebar
3. Choose the scope: **Everything** or **Search**
4. Choose the folder to save into
5. Choose a format
6. Select **Export** and wait for the log to finish

## Scope

**Everything** writes every conversation.

**Search** writes only the conversations a search finds, and shows a box for the search. It takes the same [search language](/docs/user/features/messages/search/) as the search bar, so `from:me last year` exports what that search would show. To export particular conversations, name them with `in:`, which takes a title, a handle, or an id: `in:#19,#22` exports those two conversations and nothing else, and `in:"Book Club"` exports the one with that title.

When you open Export while looking at a list of conversations, the screen starts in **Search** with that list's search already in the box, tag included, so exporting what you are looking at is one more click. Opened from Contacts or Trash, or from the sidebar with no search running, it starts in **Everything**.

## Formats

| Format | What you get |
|---|---|
| **JSON Lines** | One `.jsonl` file per conversation, attachments in an `attachments/` folder |
| **JSON** | One indented `.json` file per conversation, attachments in an `attachments/` folder |
| **CSV** | One `.csv` file per conversation, attachments in an `attachments/` folder. Columns: [CSV columns](/docs/developer/reference/csv-columns/) |
| **EML** | One folder per conversation, one `.eml` file per message, attachments embedded |
| **MBOX** | One `.mbox` file per conversation, attachments embedded |
| **Android XML** | A single `smses.xml`, attachments embedded. Apple-only fields are dropped |

JSON Lines is what Message Crate stores, so it is the fastest and the only format that loses nothing. Every other format is written by converting a JSON Lines export, which happens as part of the same run.

Folder layout: [Export structure](/docs/developer/reference/export-structure/).

## Where the temporary files go

Choosing any format other than JSON Lines takes two steps: the messages are written as JSON Lines first, then converted into the format you asked for. The intermediate copy goes in your staging directory — the same folder Import uses, `~/message-crate` by default, changeable in [Settings → System](/docs/user/features/settings/settings/). It is deleted when the export finishes, including when the conversion fails.

Make sure that folder has room for a second copy of your messages while an export runs.
