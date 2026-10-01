---
title: What is Message Crate?
description: Extract messages from phone backups, import them into Message Crate on your own computer, and browse them in an interface you control.
---

Message Crate helps you extract messages from phone backups and browse them on a machine you control. Nothing is uploaded to a Message Crate cloud service.

## Two pieces

- **The server** — a small program, usually run in Docker. It stores messages in a SQLite database on your computer and serves a website in the browser.
- **The desktop app** — a program on your computer. It reads phone backups and imports them into Message Crate. It can also write files to disk without talking to the server.

The two talk over a local URL (typically **http://localhost:8080**). The website is enough to look around. Importing a backup needs the desktop app.

The Message Crate you run has a **local** username and password. That login is not a Bitrealm (or other) cloud account.

## What you can do

- **Try sample conversations** by logging in as `demo` (see [Try Message Crate](/docs/user/get-started/try-message-crate/))
- **Import** your own backups with the desktop app
- **Browse and search** conversations, contacts, and media in the browser or the app
- **Convert** exports between JSONL (JSON Lines), JSON, CSV, EML, MBOX, and XML when you need files on disk
- **Keep media** — photos and videos from conversations can be stored with the messages

## Where to go next

- [Why you provide backups](/docs/user/get-started/why-you-provide-backups/)
- [Try Message Crate](/docs/user/get-started/try-message-crate/)
- [Use your own messages](/docs/user/get-started/your-own-messages/) if the sample data is enough to skip ahead
