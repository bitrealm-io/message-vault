---
title: SMS and MMS on Android
description: Get SMS and MMS from an Android phone using SMS Backup & Restore XML.
---

The recommended path for Android SMS and MMS uses the [SMS Backup &amp; Restore](https://play.google.com/store/apps/details?id=com.riteshsahu.SMSBackupRestore) app (SyncTech) from Google Play. It produces an XML file the desktop app can import.

## What you need

- An Android phone with SMS Backup &amp; Restore installed
- At least one XML backup file transferred to your computer
- The phone numbers that belonged to the device — so the app can tell sent messages from received ones

A contacts file (VCF or vCard CSV) is optional but helps fill in display names.

## How to get the data

1. Install [SMS Backup &amp; Restore](https://play.google.com/store/apps/details?id=com.riteshsahu.SMSBackupRestore) from Google Play
2. Open the app and create a backup — choose XML format
3. Transfer the `.xml` file to your computer (email, USB, or cloud storage)
4. If the backup is an encrypted ZIP, unlock it first — the desktop app does not open encrypted archives

You can also provide a folder of multiple XML files. The desktop app combines them.

## What Import does with it

The desktop app reads SMS and MMS from the XML, resolves contacts when you provide them, and stores messages in the vault. MMS attachments — photos, videos, audio — follow the attachment setting on the Import form.

## Known limitations

- Only SMS and MMS are supported. Call logs, drafts, failed, and outbox messages are skipped
- Incorrect owner phone numbers can swap senders or mis-group conversations. Enter every number that belonged to the device

## Next step

Open **Import**, set the source to **SMS Backup & Restore**, enter the owner phone numbers, and point at the XML file or folder. See [Import from a backup](/docs/user/import-from-a-backup/).
