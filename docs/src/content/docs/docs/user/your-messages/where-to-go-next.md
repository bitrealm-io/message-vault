---
title: Where to go next
description: More messages and more people, reading from another device, hosting with Docker, and the feature pages.
---

The Message Crate now holds one phone's messages.
What follows depends on what else there is to bring in.

## More messages

| Source | Page |
|---|---|
| The Messages app on a Mac | [Messages on a Mac](/docs/user/import-sources/mac-messages/) |
| WhatsApp, on Android or iPhone | [WhatsApp](/docs/user/import-sources/whatsapp/). Advanced: it needs a separate program and, on Android, a decryption key. |
| An old backup from SMS Backup+, GO SMS Pro, iMazing, or OpenExtract | [Old backups](/docs/user/import-sources/old-backups/) |
| A second phone | [Back up the phone](/docs/user/your-messages/back-up-an-iphone/) and [import the backup](/docs/user/your-messages/import-your-backup/) again, into the same account |
| Another person's messages | A new account for that person, added by the Owner as in [Add an account](/docs/user/your-messages/create-the-owner-and-an-account/#add-an-account). That person then imports their own backup. |

## Using what is there

- [Browse](/docs/user/features/messages/browse/) describes the conversation list and the thread view.
- [Search](/docs/user/features/messages/search/) lists every search word.
- [Contacts](/docs/user/features/contacts/contacts/) covers naming people and grouping them.
- [Attachments and media](/docs/user/features/messages/attachments-and-media/) covers converting photos and video so every browser shows them.
- [Export](/docs/user/features/messages/export/) writes conversations back to files.

## Read from a phone or another computer

The Message Crate the app starts answers this computer only.
**Settings → System → Let other devices on this network connect** opens it to the home network, and a phone or another computer then reaches it in a browser at this computer's address on port 8080.

Two limits apply.
The Message Crate is reachable only while the app is open.
The connection is plain HTTP, so anyone on the same network can read what is sent, passwords included.

## Host it with Docker

A Message Crate that is on at all hours, or that lives on a home server, runs in Docker instead of inside the app.
[Run Message Crate with Docker](/docs/user/features/owner/run-with-docker/) starts one, and the desktop app connects to it for importing.
[Run on another machine](/docs/user/features/owner/run-on-another-machine/) covers a server on a second computer.

A Message Crate in Docker starts empty of real messages: the backups are imported again into it.

## Running the Message Crate

- [Owner Home](/docs/user/features/owner/owner-home/) is where accounts and server settings are managed.
- [Update](/docs/user/features/owner/update/) moves the Message Crate to a new version.
- [Troubleshooting](/docs/user/features/owner/troubleshooting/) lists the common failures.

The messages live in the app's data folder, which **Settings → System → Open data folder** opens.
A copy of that folder, made while the app is closed, is the backup of the Message Crate.
