---
title: WhatsApp on iPhone
description: Get WhatsApp data from an iPhone backup for Import.
---

WhatsApp data on iPhone lives inside the device backup you make with iTunes or Finder.

## What you need

- An iPhone backup (unencrypted or encrypted) that includes WhatsApp data
- The desktop app, with `wtsexporter` installed — see [Install the desktop app](/vault/user/get-started/install-the-desktop-app/)

## How to get the data

Follow Apple's official guide to [back up your iPhone](https://support.apple.com/en-us/108369). WhatsApp data is included in the backup — no special settings are required.

WhatsApp-on-iPhone Import points at the Finder/iTunes backup folder. It does not ask for the Apple backup password. Encrypted device backups are a `wtsexporter` limitation, not a field on this form.

## What Import does with it

The desktop app runs `wtsexporter` to extract WhatsApp messages from the backup, then imports the result. Conversation names use a `__whatsapp` suffix so they stay separate from Apple Messages threads.

The backup carries the phone number the WhatsApp account is registered to, in WhatsApp's own preferences file (`group.net.whatsapp.WhatsApp.shared.plist`, key `OwnJabberID`). Import reads it from there and records it on every message as the address it was held at, so the conversations count toward that identity in Settings. When the backup has no such entry, Import uses the **WhatsApp phone number** field under **Processing Options (Advanced)** instead, and stops with a message when that field is empty too.

## Known limitations

- The desktop app cannot cancel `wtsexporter` mid-run — wait for it to finish or stop it manually
- WhatsApp import needs `wtsexporter` on `PATH`

## Next step

Open **Import**, set the source to **WhatsApp**, set **Platform** to **iPhone**, and point it at the backup folder. See [Import from a backup](/vault/user/import-from-a-backup/).
