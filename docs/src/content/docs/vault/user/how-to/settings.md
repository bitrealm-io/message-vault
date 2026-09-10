---
title: Settings
description: Account, profile, storage, system, convert, and appearance settings.
---

Open **Settings** in the sidebar. Settings has a tab for each area below.
Administrators also see a **Users** tab, and the desktop app adds **System** and **Convert**.

## Account

- **Username** — read-only account id used for sign-in
- **Password** — change password when local auth is enabled
- **API tokens** — named Bearer secrets for programs that call the vault's [HTTP API](/vault/developer/reference/api/). When creating one, choose **import**, **export**, or **both**. Each secret is shown once at creation; revoke when finished. Signing in to the website uses a separate session token that changes on each login and does not revoke these tokens. Desktop **Import** and **Export** do not need an API token; they use the signed-in session.
- **Danger zone** — delete all messages for the account, or delete the account (the demo account cannot be deleted)

## Profile

- **Display name** — name shown for messages you sent
- **Time zone** — the zone every message time, day and year is shown in, and the one `date:` searches count days in. It is chosen at profile setup, starting from the browser's zone, and can be changed here at any time
- **Handles** — phone numbers or emails used to recognize you in imports

## Storage

- **Usage** — attachment storage for this account
- **Largest attachments** — top attachments by file size
- **Import history** and **Export history** — every Import Run and Export Run recorded for this account: when it started, what it asked for, how much it moved, and how it ended. An Import Run also lists every contact it touched and says what it did to each: created it, created it in place of one you had trashed, named it, or added a handle to it.

## System

Desktop app only.

- **Staging directory** — where Import and Export write their temporary files, `~/message-vault` by default
- **Remember importer paths** — Import restores the last backup path for each import source
- **ffmpeg directory** — a folder holding `ffmpeg` and `ffprobe` when they aren't on the system PATH. See [Media and privacy](/vault/user/how-to/media-and-privacy/)

## Convert

Desktop app only.
Convert rewrites a folder of exported files into another format, without reading a backup or the vault: an input folder, a different output folder, and the output format.
Steps and the formats it reads and writes: [Convert formats](/vault/user/how-to/convert-formats/).

## Appearance

- **Theme** — light, dark, or follow the system setting

## Demo reset

Resetting sample data is a server command: [`reset-demo`](/vault/developer/reference/server-cli/#reset-demo). For a Docker volume, see [Docker](/vault/developer/docker/).
