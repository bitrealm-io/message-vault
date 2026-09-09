---
title: "SMS Backup+ format"
description: "Layout of the SMS Backup+ EML files the exporter reads."
---

Input messages come from [SMS Backup+](https://github.com/jberkel/sms-backup-plus) syncing Android SMS/MMS to Gmail/IMAP, then archived as `.eml` (this project does **not** talk to IMAP).

## Flat single-message EML

Typical headers:

| Header | Meaning |
|--------|---------|
| `X-smssync-type` | Android SMS type; sent ≈ `{2,128,4,135,6,5}`, received ≈ `{1,132,130}` |
| `X-smssync-address` | Counterparty phone(s); groups use `~` (or `;`, `,`, `\|`) separators |
| `X-smssync-date` | Unix epoch **milliseconds** (or seconds if small) |
| `X-smssync-id` | Stable sync id (optional) |
| `Subject` | `SMS with {contact name}` |
| `From` / `To` | Often `*@sms-backup-plus.local` or owner Gmail |

The body is the `text/plain` part. SMS Backup+ writes every message body as
plain text — zero of 20,000 sampled carry a `text/html` part — so there is
nothing else to read. Non-text MIME parts are exported as attachments.

## Import mapping and deduplication

Source-field mapping and online cover-key deduplication: [SMS Backup+ mapping](/vault/developer/formats/sms-backup-plus/mapping/).
