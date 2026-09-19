---
title: iPhone or iPad
description: Get Messages data from an iPhone or iPad — what you need, where to find it, and what Import expects.
---

The desktop app can read iPhone or iPad messages from a device backup or from Messages on a Mac.

## What you need

One of these:

- **An unencrypted iPhone backup** made with iTunes (Windows) or Finder (macOS)
- **An encrypted iPhone backup** whose password you know
- **A Mac with Messages** signed into your Apple Account — the app can read `chat.db` directly

## How to get the data

### Make an iPhone backup

Follow Apple's official guide to [back up your iPhone](https://support.apple.com/en-us/108369). An unencrypted backup is fine — messages do not require encryption. If the backup is encrypted, you need the password.

The backup is a folder on your computer. On macOS it is at `~/Library/Application Support/MobileSync/Backup/`. On Windows it is at `%APPDATA%\Apple Computer\MobileSync\Backup\`.

### Copy chat.db from a Mac

If you use Messages on a Mac:

1. Open the Messages app on the Mac — this keeps the database current
2. The database is at `~/Library/Messages/chat.db`
3. In Import, choose **iMessage**, then **Platform** **Mac Messages**, and point at this file

On a live Mac, `Attachments` and `StickerCache` sit next to `chat.db`. Leave **Attachment folder** empty. Leave **Apple Contacts file** empty to use the local AddressBook.

## What Import does with it

The desktop app reads SMS, iMessage, and attachments. For an iPhone backup it uses the AddressBook inside the backup. For Mac Messages it can use the local AddressBook, or a Contacts file you pick.

## Known limitations

- You need the device or a Mac with Messages logged in. There is no cloud access.
- Android XML cannot store Apple-only fields like message effects and Tapbacks. That matters only if you later [convert](/vault/user/how-to/convert-formats/) to XML.

## Next step

Open the desktop app, log in to the vault, and go to **Import**. Choose **iMessage**, then the **Platform** that matches the files. See [Import from a backup](/vault/user/import-from-a-backup/).
