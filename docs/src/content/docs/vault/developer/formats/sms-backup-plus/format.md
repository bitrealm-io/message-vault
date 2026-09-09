---
title: "SMS Backup+ format"
description: "Layout of SMS Backup+ EML archives that the rescue converter reads."
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

The body is the `text/html` part when the mail has one, else `text/plain`. A flat
message carries no HTML part in practice, so this only ever decides an archive.
Non-text MIME parts are exported as attachments.

## Archive EML

| Header / body | Meaning |
|---------------|---------|
| `Subject` | `SMS archive {contact name}` |
| `From` | Often `{digits}@sms-backup-plus.local` |
| Body lines | `YYYY-MM-DD HH:MM:SS - {Sender}` then message text; Sender `Me` = sent |

Many archives carry the transcript only as `text/html`, with no `text/plain`
part at all. The HTML is a styled rendering of the same lines, one bubble per
message, and it is read by stripping the tags back to those lines. Where a mail
carries both, the HTML wins: the plain-text copy has been hard-wrapped by the
sending mail client, so a sentence arrives broken across lines while the HTML
keeps it whole.

An archive that yields no messages is counted as `empty_archive_eml` and named
in the run summary, because a silently skipped archive is a whole conversation
lost.

Optional MIME attachments are attached to messages in order.

## Import mapping and deduplication

Source-field mapping and online cover-key deduplication: [SMS Backup+ mapping](/vault/developer/formats/sms-backup-plus/mapping/).
