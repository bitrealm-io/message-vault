---
title: What is Message Vault?
description: Extract messages from phone backups, import them into a local vault, and browse them in an interface you control.
---

Message Vault helps you extract messages from phone backups and browse them on a machine you control. Nothing is uploaded to a Message Vault cloud service.

## Two pieces

- **The vault** — a small server, usually in Docker. It stores messages in a SQLite database on your computer and serves a website in the browser.
- **The desktop app** — a program on your computer. It reads phone backups and imports them into the vault. It can also write files to disk without talking to the vault.

The two talk over a local URL (typically **http://localhost:8080**). The website is enough to look around. Importing a backup needs the desktop app.

The vault you run has a **local** username and password. That login is not a Bitrealm (or other) cloud account.

## What you can do

- **Try sample conversations** by logging in as `demo` (see [Try the vault](/vault/user/get-started/try-the-vault/))
- **Import** your own backups with the desktop app
- **Browse and search** conversations, contacts, and media in the browser or the app
- **Convert** exports between JSONL (JSON Lines), JSON, CSV, EML, MBOX, and XML when you need files on disk
- **Keep media** — photos and videos from conversations can be stored with the messages

## Where to go next

- [Why you provide backups](/vault/user/get-started/why-you-provide-backups/)
- [Try the vault](/vault/user/get-started/try-the-vault/)
- [Use your own messages](/vault/user/get-started/your-own-messages/) if the sample data is enough to skip ahead
