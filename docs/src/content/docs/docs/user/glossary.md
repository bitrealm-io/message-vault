---
title: Glossary
description: Plain-language definitions of the formats and terms you will meet in this guide.
---

Short definitions of terms used in the User Guide. Command flags and vendor field names live under [Developer](/docs/developer/).

## Message Crate

**Message Crate** — One installation of the product: the thing a person claims, owns, and logs in to. One Message Crate holds many accounts.

**Owner** — The person who claimed a Message Crate. The owner manages its accounts and its server settings, and reads no messages.

**Account** — One person's login to a Message Crate, with that person's own messages and contacts. Each account sees only its own.

**Server** — The program that runs a Message Crate. It stores the messages and serves the website in the browser.

## Formats

**JSONL (JSON Lines)** — One JSON object per line of text. Each conversation in a file export is one `.jsonl` file. [JSON Lines](https://jsonlines.org/) calls it a convenient format for data processed one record at a time. Import never asks for this folder. [Export](/docs/user/features/messages/export/) writes it.

**JSON** — Pretty-printed JSON with indentation. The same data as JSONL, easier for a human to read. One `.json` file per conversation.

**CSV** — Comma-separated values, the spreadsheet format. One `.csv` file per conversation.

**EML** — The standard file format for a single email. Each message becomes one `.eml` file.

**MBOX** — A single file that holds many emails. One `.mbox` file per conversation.

**XML** — The SyncTech SMS Backup & Restore format (`smses.xml`), used by Android backup and restore tools.

**VCF** — vCard, the standard format for contact cards.

**E.164** — The international phone number format, for example `+15555550100`.

## Software

**Docker** — Runs the Message Crate server in a container, so you do not install the server toolchain on the host.

**SQLite** — The database that stores the messages. It is a file on your computer.
