---
title: WhatsApp on Android
description: Get WhatsApp data from an Android phone — database or encrypted backup plus key.
---

Android WhatsApp data lives in a local database on the phone. The desktop app needs that database — or an encrypted copy of it — along with the matching key.

## What you need

One of these:

- **An encrypted WhatsApp backup** (`msgstore.db.crypt*`) plus the matching key file or hexadecimal key
- **An already-extracted message database** (`msgstore.db`)
- Optionally, the `wa.db` contacts database for name resolution
- Optionally, the WhatsApp media folder for photos and videos

Use files from the same backup. A mismatched key will not decrypt.

Install `wtsexporter` before Import. Commands are on [Install the desktop app](/vault/user/get-started/install-the-desktop-app/).

## How to get the data

There is no single path because WhatsApp and Android versions vary. In general:

1. Copy the WhatsApp database from the phone. If the phone is not rooted, use a file manager that can access `/data/data/com.whatsapp/databases/` or create a WhatsApp backup that includes the database
2. If you have an encrypted crypt file, get the matching key — WhatsApp stores it in `/data/data/com.whatsapp/files/key` on the device
3. Transfer the files to your computer

See WhatsApp's official [backup documentation](https://faq.whatsapp.com/) for the current backup steps for your version.

## What Import does with it

The desktop app runs `wtsexporter` to read the database or encrypted backup, then imports the result. Conversation names use a `__whatsapp` suffix.

An Android backup does not carry the phone number the WhatsApp account is registered to, so the Import form asks for it in the **WhatsApp phone number** field, pre-filled from the profile's phone. Import records that number on every message as the address it was held at, so the conversations count toward that identity in Settings. An empty field stops the form before the import starts.

## Known limitations

- Getting the files from the phone varies by device and Android version. On modern phones without root access, the direct database path may be restricted
- The desktop app cannot cancel `wtsexporter` mid-run — wait for it to finish or stop it manually
- A key value that the app recognizes as a passphrase is never saved to disk

## Next step

Open **Import**, set the source to **WhatsApp**, set **Platform** to **Android**, and fill in the backup folder and key. See [Import from a backup](/vault/user/import-from-a-backup/).
