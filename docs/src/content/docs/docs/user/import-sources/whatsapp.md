---
title: WhatsApp
description: Import WhatsApp from an Android phone or an iPhone backup. Advanced, because it needs a separate program and, on Android, a decryption key.
---

WhatsApp is the hardest source to import.
It belongs after a first import of [iPhone](/docs/user/get-started/back-up-an-iphone/) or [Android](/docs/user/get-started/back-up-an-android-phone/) messages has worked, not before.

Three things make it hard:

- The desktop app doesn't include the program that reads WhatsApp data. It must be installed separately ([issue 940](https://github.com/messagecrate/message-crate/issues/940)).
- On Android, WhatsApp's backup file is encrypted, and Import needs the key.
- On iPhone, Import can't read WhatsApp from an encrypted iPhone backup ([issue 941](https://github.com/messagecrate/message-crate/issues/941)).

## Install wtsexporter

The desktop app runs [`wtsexporter`](https://github.com/KnugiHK/WhatsApp-Chat-Exporter), from the open-source WhatsApp Chat Exporter project.
It needs Python and [`pipx`](https://pipx.pypa.io/).

```bash title="Install wtsexporter"
pipx install "whatsapp-chat-exporter[android_backup,crypt15]"
```

`wtsexporter --help` prints the program's options once it is installed.

The desktop app has no setting for where `wtsexporter` is.
It finds the program when it is on the `PATH` the app was started with, or when the environment variable `WTSEXPORTER` holds the full path to it.

## WhatsApp on Android

### What Import needs

A folder that holds one of:

- `msgstore.db`, the WhatsApp message database, or
- `msgstore.db.crypt15` (or the older `.crypt14` or `.crypt12`), the encrypted backup of it, together with its key.

The phone number the WhatsApp account is registered to is also required, because an Android backup doesn't record it.

### Without root: the 64-digit key

WhatsApp can protect its backup with a key the person holds.

1. In WhatsApp, open **Settings → Chats → Chat backup → End-to-end encrypted backup** and turn it on.
2. Choose the **64-digit encryption key** in place of a password, and write the key down. A password can't be used for Import.
3. Run a backup from **Chat backup**.
4. Copy `msgstore.db.crypt15` to the computer. On the phone it is under `Android/media/com.whatsapp/WhatsApp/Databases/`. A WhatsApp `Media` folder beside `Databases` holds the photos and videos, and it is copied too when they are wanted.

The labels inside WhatsApp change between versions.
[WhatsApp's own help](https://faq.whatsapp.com/) describes the current ones.

### With root: the key file

A rooted phone gives access to WhatsApp's private storage, where the unencrypted `msgstore.db` and the `key` file live under `/data/data/com.whatsapp/`.
Rooting differs for every phone model and is outside this guide.
The [WhatsApp Chat Exporter documentation](https://github.com/KnugiHK/WhatsApp-Chat-Exporter) describes which files to copy.

### Fill in the Import form

1. Open **Import** and choose **WhatsApp** as the source.
2. **Platform**: **Android**.
3. **Backup folder**: the folder that holds the `msgstore` file.
4. **Decryption key**: the 64-digit key, or the path to the key file. It is required when the folder has a `.crypt` file and no `msgstore.db`. The app never saves it.
5. **WhatsApp phone number**: the number the account is registered to. It is filled in from the profile.
6. **Contacts database**, **Media folder**, and **Message database** stay empty when those files are inside the backup folder.

## WhatsApp on iPhone

WhatsApp's data is inside the iPhone backup made in [Back up an iPhone](/docs/user/get-started/back-up-an-iphone/).

The backup must be made **without** encryption.
The Import form has no password field for WhatsApp, so it can't open an encrypted backup.
A backup that was encrypted for importing iPhone messages can't be reused here. A second, unencrypted backup is needed.

### Fill in the Import form

1. Open **Import** and choose **WhatsApp** as the source.
2. **Platform**: **iPhone**.
3. **Backup folder**: the device folder, the one that contains `Manifest.plist`.
4. **WhatsApp Business** is turned on only for a WhatsApp Business backup.

The phone number is read from the backup.
When the backup doesn't hold it, Import uses **WhatsApp phone number** under **Processing Options (Advanced)**.

## Limits

- A run can't be cancelled while `wtsexporter` is working. It ends when the program finishes.
- Using files from two different backups together fails, because a key decrypts only the backup it was made with.

The run itself is the same as in [Import your backup](/docs/user/get-started/import-your-backup/#start-the-import).
