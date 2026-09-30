---
title: Glossary
description: Plain-language definitions of the formats and terms you will meet in this guide.
---

Short definitions of terms used in the User Guide. Command flags and vendor field names live under [Developer](/docs/developer/).

## Formats

**JSONL (JSON Lines)** — One JSON object per line of text. Each conversation in a file export is one `.jsonl` file. [JSON Lines](https://jsonlines.org/) calls it a convenient format for data processed one record at a time. Happy-path Import does not ask you to produce this folder; the [Extract to files](/docs/user/how-to/extract-to-files/) command-line exporters do.

**JSON** — Pretty-printed JSON with indentation. The same data as JSONL, easier for a human to read. One `.json` file per conversation.

**CSV** — Comma-separated values, the spreadsheet format. One `.csv` file per conversation.

**EML** — The standard file format for a single email. Each message becomes one `.eml` file.

**MBOX** — A single file that holds many emails. One `.mbox` file per conversation.

**XML** — The SyncTech SMS Backup & Restore format (`smses.xml`), used by Android backup and restore tools.

**VCF** — vCard, the standard format for contact cards.

**E.164** — The international phone number format, for example `+15555550100`.

## Software

**Docker** — Runs the vault server in a container, so you do not install the server toolchain on the host.

**SQLite** — The database that stores messages in the vault. It is a file on your computer.
