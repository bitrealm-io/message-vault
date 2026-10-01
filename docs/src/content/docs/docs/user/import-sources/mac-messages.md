---
title: Messages on a Mac
description: Import straight from the Messages app's database on a Mac, without a phone backup.
---

:::caution[Not tested on a Mac]
The field names on this page are read from the product. The macOS steps follow Apple's documentation, and nobody on the project has run them on a Mac. A wrong step is worth [an issue](https://github.com/messagecrate/message-crate/issues).
:::

A Mac that uses the Messages app keeps its own copy of the conversations in a database file.
The desktop app reads that file directly, so no phone backup is needed.

## When to use it

- There is no iPhone to back up, or no way to back it up.
- The Mac holds conversations the phone no longer has.

An [iPhone backup](/docs/user/your-messages/back-up-an-iphone/) is the better first import, because the phone usually holds more.
The Mac has only what Messages on that Mac received.
A Mac that was signed in recently, or that doesn't receive the phone's SMS texts, holds part of the history.

Both can be imported into the same account.
Messages the Message Crate already holds are skipped.

## What it needs

- The desktop app running on the Mac. The app is built for Apple Silicon only.
- **Full Disk Access** for the desktop app, under **System Settings → Privacy & Security → Full Disk Access**. macOS refuses to let any app read the Messages database without it. The app must be restarted after the setting changes.

## Fill in the Import form

1. Open **Import** and choose **iMessage** as the source.
2. **Platform**: **Mac Messages**.
3. **Messages database**: on a Mac the field already holds `~/Library/Messages/chat.db`.
4. **Attachment folder** stays empty. The attachments sit next to `chat.db`, and the app finds them there.
5. **Apple Contacts file** stays empty. The app reads the Mac's own Contacts for names.

The two optional fields exist for a `chat.db` that was copied to another computer.
In that case **Attachment folder** takes the copied `Attachments` folder, and **Apple Contacts file** takes a copied `AddressBook-v22.abcddb`.

The run itself is the same as in [Import your backup](/docs/user/your-messages/import-your-backup/#start-the-import).
