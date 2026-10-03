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
addresses and none of the owner's is not read as a group. It goes to the
one-to-one conversation of the address in `From`, who wrote it. When `From`
gives no address, it goes to the `X-smssync-address` conversation with no
sender, since that number is not known to have written it. Either way it is
counted as `group_messages_owner_not_named` in the run summary. The counter
counts stored messages, after duplicates are dropped.

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

The **EML (SMS Backup+)** export format writes SMS and MMS back out as this mail, through `SmsBackupPlusArchive` in `sms-backup-plus-exporter` (ADR 0021). A message that is not SMS or MMS is left out and counted, and the run's log says how many.
A message whose service is unknown counts as SMS or MMS when its kind says so, as it does for the Android XML export.

Each conversation is one folder, named as the EML archive names its folders, holding one `.eml` per message, named as the EML archive names its files.

Every mail carries `Subject` (`SMS with <name>`: the sender of a received message, the other person of a sent one-to-one message, and otherwise a member's number, since the import takes the name as the sender's), `From`, `To`, `Date`, `Message-ID` and `References` (`<…@sms-backup-plus.local>`), `MIME-Version`, `Content-Type`, `Content-Transfer-Encoding`, and:

| Header | Value |
|---|---|
| `X-smssync-datatype` | `SMS`, or `MMS` for an MMS or a message with an attachment |
| `X-smssync-address` | The other person's address; in a group, every other person's, joined by `~` |
| `X-smssync-date` | The message time in epoch milliseconds |
| `X-smssync-type` | `1` received or `2` sent for SMS; `132` received or `128` sent for MMS |
| `X-smssync-backup-time` | The start of the Export Run that wrote the file, or of the Convert run, in epoch milliseconds |

`From` and `To` name a phone number as `<number>@unknown.email` and an email address as itself, as SMS Backup+ does, so the import reads each handle back as it went out.
A received group message whose sender is unknown names an address no member has, and comes back with no sender.

An SMS is `text/plain`. An MMS is `multipart/mixed`: the text, then each stored attachment with its content type and file name.
Every `text/plain` part of an MMS is message text to the import, as it is on the phone, so a text file attached to a message is written as `application/octet-stream` under its own name, and comes back as a file of that type.
An attachment whose file is gone is left out of the mail and counted as missing in the run's log.
A carriage return in the text comes back from the import as a newline, because the import reads a mail's line breaks as newlines.

The database does not keep the phone's row and thread ids, read and status flags, protocol, or the app's build, so `X-smssync-id`, `-thread`, `-read`, `-status`, `-protocol` and `-version` are never written. `X-GM-THRID` and `X-Gmail-Labels` are Gmail's, not the app's, and are never written either.
