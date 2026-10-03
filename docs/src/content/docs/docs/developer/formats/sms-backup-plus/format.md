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

## Writing SMS Backup+ mail

The **EML (SMS Backup+)** export format writes SMS and MMS back out as this mail, through `SmsBackupPlusArchive` in `sms-backup-plus-exporter` (ADR 0021). A message whose service is not SMS is left out and counted, and the run's log says how many.

Each conversation is one folder, named as the EML archive names its folders, holding one `.eml` per message, named as the EML archive names its files.

Every mail carries `Subject` (`SMS with <name>`), `From`, `To`, `Date`, `Message-ID` and `References` (`<…@sms-backup-plus.local>`), `MIME-Version`, `Content-Type`, `Content-Transfer-Encoding`, and:

| Header | Value |
|---|---|
| `X-smssync-datatype` | `SMS`, or `MMS` for an MMS or a message with an attachment |
| `X-smssync-address` | The other person's address; in a group, every other person's, joined by `~` |
| `X-smssync-date` | The message time in epoch milliseconds |
| `X-smssync-type` | `1` received or `2` sent for SMS; `132` received or `128` sent for MMS |
| `X-smssync-backup-time` | The start of the run that wrote the file, in epoch milliseconds |

An SMS is `text/plain`. An MMS is `multipart/mixed`: the text, then each stored attachment with its content type and file name.

The database does not keep the phone's row and thread ids, read and status flags, protocol, or the app's build, so `X-smssync-id`, `-thread`, `-read`, `-status`, `-protocol` and `-version` are never written. `X-GM-THRID` and `X-Gmail-Labels` are Gmail's, not the app's, and are never written either.
