---
title: Why you provide backups
description: Why backups are manual — a platform limitation, not a Message Crate one.
---

To put your messages in Message Crate, you make a backup of the phone and point the desktop app at it. That step is manual because Apple, Google, and WhatsApp do not offer another path.

## No official download API

Those companies do not provide API access to message history. There is no supported way for a program to log in and download the full archive.

## Messages already live in local files

- An iPhone keeps messages in the device backup you make on a computer, or in `chat.db` on a Mac that uses Messages.
- An Android phone keeps SMS/MMS in a local database — and in the SMS Backup & Restore XML file you can export.
- WhatsApp messages live in the app's local database (or an encrypted backup plus key).

The desktop app reads those files on your computer. No third-party sync service sits in the middle.

## A platform limitation

Every tool that works with message history works this way today. If a supported cloud export ever appears, the app can grow to match. Until then, the backup steps are the only path.

## Where to start

[Try Message Crate](/docs/user/get-started/try-message-crate/) first if you have not seen the product yet. When you are ready for your own data, [prepare a backup](/docs/user/prepare-a-backup/).
