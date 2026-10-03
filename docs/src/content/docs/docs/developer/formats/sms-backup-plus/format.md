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
| `X-smssync-address` | The other party's number. For an MMS, only the first address the phone stored for it, so a group MMS names one of its participants here. Older exports may list several, separated by `~` (or `;`, `,`, `\|`) |
| `X-smssync-date` | Unix epoch **milliseconds** (or seconds if small) |
| `X-smssync-id` | Stable sync id (optional) |
| `Subject` | `SMS with {contact name}` |
| `From` / `To` | The owner is their Gmail address. Anyone else is their contact's email address when the contact has one, else `<number>@unknown.email`. A sent MMS names every recipient in `To`; a received MMS names its sender in `From` and every other recipient, the owner included, in `To` |

The `X-smssync-address`, `From` and `To` rows come from SMS Backup+ at commit
`fd33c32`: `messageFromMapMms` in
`app/src/main/java/sms/backup/plus/mail/MessageGenerator.java` (lines 144–150
and 182), `getDetails` in `MmsSupport.java` (lines 91–153), and `getAddress`
and `getUnknownEmail` in `PersonRecord.java` (lines 36–52 and 83–86).

A received MMS names the owner in `To` under the address on the owner's own
contact card, which need not be the account the backup went to. The run
knows the owner only by the phone numbers and email addresses it is given, so
they should include every number the archive spans and any email address on
the owner's own contact card. A received MMS whose `To` names two or more
addresses and none of the owner's is not read as a group: it is keyed by
`X-smssync-address` and counted as `group_messages_owner_not_named` in the run
summary.

The body is the `text/plain` part. SMS Backup+ writes every message body as
plain text — zero of 20,000 sampled carry a `text/html` part — so there is
nothing else to read. Non-text MIME parts are exported as attachments.

## Call-log mails

SMS Backup+ can also back up the phone's call log, one mail per call, into a label of its own ("Call log").
Such a mail carries `X-smssync-datatype: CALLLOG`, and its `X-smssync-type` holds the call's type, not a message type.
Message Crate has no model for a call, so the exporter skips every `CALLLOG` mail and counts it as `skipped_call_log` in the run summary.

## Import mapping and deduplication

Source-field mapping and online cover-key deduplication: [SMS Backup+ mapping](/docs/developer/formats/sms-backup-plus/mapping/).
