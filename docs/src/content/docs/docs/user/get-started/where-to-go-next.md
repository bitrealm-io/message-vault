---
title: Where to go next
description: Other import sources, the feature pages, and the tasks of running a Message Crate.
---

The Message Crate now holds one phone's messages.
What follows depends on what else there is to bring in.

## More messages

| Source | Page |
|---|---|
| The Messages app on a Mac | [Messages on a Mac](/docs/user/import-sources/mac-messages/) |
| WhatsApp, on Android or iPhone | [WhatsApp](/docs/user/import-sources/whatsapp/). Advanced: it needs a separate program and, on Android, a decryption key. |
| An old backup from SMS Backup+, GO SMS Pro, iMazing, or OpenExtract | [Old backups](/docs/user/import-sources/old-backups/) |
| A second phone | Steps [6](/docs/user/get-started/back-up-an-iphone/) and [7](/docs/user/get-started/import-your-backup/) again, into the same account |
| Another person's messages | A new account for that person, added by the Owner as in [step 4](/docs/user/get-started/create-the-owner-and-an-account/#add-an-account) |

## Using what is there

- [Browse](/docs/user/features/messages/browse/) describes the conversation list and the thread view.
- [Search](/docs/user/features/messages/search/) lists every search word.
- [Contacts](/docs/user/features/contacts/contacts/) covers naming people and grouping them.
- [Attachments and media](/docs/user/features/messages/attachments-and-media/) covers converting photos and video so every browser shows them.
- [Export](/docs/user/features/messages/export/) writes conversations back to files.

## Running the Message Crate

- [Owner Home](/docs/user/features/owner/owner-home/) is where accounts and server settings are managed.
- [Update](/docs/user/features/owner/update/) moves the server to a new version.
- [Troubleshooting](/docs/user/features/owner/troubleshooting/) lists the common failures.

The messages live in the Docker volume `message-crate-data`.
A copy of that volume is the backup of the Message Crate.
