---
title: "SMS Backup+ format"
description: "Layout of the SMS Backup+ EML files the exporter reads."
---

Input messages come from [SMS Backup+](https://github.com/jberkel/sms-backup-plus) syncing Android SMS/MMS to Gmail/IMAP, then archived as `.eml` (this project does **not** talk to IMAP).

## Flat single-message EML

Typical headers:

| Header | Meaning |
|--------|---------|
| `X-smssync-datatype` | `SMS`, `MMS` or `CALLLOG`; a `CALLLOG` mail is skipped (below) |
| `X-smssync-type` | Android SMS type; sent ≈ `{2,128,4,135,6,5}`, received ≈ `{1,132,130}` |
| `X-smssync-address` | Counterparty phone(s); groups use `~` (or `;`, `,`, `\|`) separators |
| `X-smssync-date` | Unix epoch **milliseconds** (or seconds if small) |
| `X-smssync-id` | Stable sync id (optional) |
| `Subject` | `SMS with {contact name}` |
| `From` / `To` | Often `*@sms-backup-plus.local` or owner Gmail |

The body is the `text/plain` part. SMS Backup+ writes every message body as
plain text — zero of 20,000 sampled carry a `text/html` part — so there is
nothing else to read. Non-text MIME parts are exported as attachments.

## Call-log mails

SMS Backup+ can also back up the phone's call log, one mail per call, into a label of its own ("Call log").
Such a mail carries `X-smssync-datatype: CALLLOG`, and its `X-smssync-type` holds the call's type, not a message type.
Message Crate has no model for a call, so the exporter skips every `CALLLOG` mail and counts it as `skipped_call_log` in the run summary.

## Import mapping and deduplication

Source-field mapping and online cover-key deduplication: [SMS Backup+ mapping](/docs/developer/formats/sms-backup-plus/mapping/).
