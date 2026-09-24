---
title: "iMazing input format"
description: "CSV columns and limitations in iMazing Messages and WhatsApp exports."
---

This reference describes the iMazing Messages, WhatsApp, and Contacts CSV files consumed by `imazing-exporter`. It records facts observed in iMazing 3.5.5, validated against a full **All messages** device export on 2026-07-19. It is not a complete specification for every iMazing version.

Conversion behavior and parser decisions are documented in [design](/vault/developer/formats/imazing/design/).

## Export tree

A full device export root typically contains:

```text title="Device export tree"
Device-Info.txt
Contacts/.../Contacts - {stamp}.csv
Messages/{YYYY-MM-DD HH MM SS} - {label}/Messages - {export-stamp} - {label}.csv
WhatsApp/{YYYY-MM-DD HH MM SS} - {label}/WhatsApp - {export-stamp} - {label}.csv
```

Media files sit beside the CSV in each chat folder. There is no `Attachments/` subdirectory. Filenames often use:

```text title="Attachment filename"
{message timestamp} - {truncated chat label} - {original basename}
```

The CSV `Attachment` cell usually contains only the original basename.

## Accepted input paths

The path given on the Import form accepts:

| Path | Files available to the importer |
|------|---------------------------------|
| One `.csv` | One Messages or WhatsApp CSV when its headers match a supported layout |
| Chat folder | Matching CSV files found below that folder |
| `Messages/` or `WhatsApp/` | Matching CSV files found recursively in that tree |
| Device export root | Messages and WhatsApp CSV files found recursively; Contacts CSV files are skipped as message input |

Discovery does not follow directory symbolic links. The importer sorts discovered paths and classifies CSV files by their headers:

- **Messages:** contains `Service` together with shared fields such as `Chat Session`, `Message Date`, and `Sender ID`.
- **WhatsApp:** lacks `Service` and contains one or more of `Forwarded`, `Attachment info`, and `Sent Date`.

## Messages CSV

The verified layout contains 17 columns:

```text title="Messages CSV headers"
Chat Session, Message Date, Delivered Date, Read Date, Edited Date, Deleted Date,
Service, Type, Sender ID, Sender Name, Status, Replying to, Subject, Text, Reactions,
Attachment, Attachment type
```

Observed `Service` values are `SMS` and `iMessage`. One chat can contain both values.

## WhatsApp CSV

The verified layout contains 14 columns:

```text title="WhatsApp CSV headers"
Chat Session, Message Date, Sent Date, Type, Sender ID, Sender Name, Status, Forwarded,
Replying to, Text, Reactions, Attachment, Attachment type, Attachment info
```

The source has no complete group-roster field. A group member who never sent a message is absent from the CSV.

## Contacts (vCard CSV)

iMazing can emit contacts as a wide address-book CSV with vCard-style fields such as `First Name` and `Mobile Phone`. Some phone numbers appear only in `Notes` as `PROP-ID: +…`. The shared `message-contacts` parser treats this as a **vCard CSV** (not an iMazing-specific format).

## Source limitations

These limitations come from the exported files. The importer cannot recover information that iMazing did not include:

1. Outgoing rows do not contain the owner’s number or name.
2. Many one-to-one chats use a display name as `Chat Session` instead of a phone number.
3. A silent Messages group member has no phone unless their display name can be resolved through Contacts.
4. WhatsApp has no complete group roster. Participants are inferred from senders.
5. `Message Date` values do not contain a timezone.
6. Long folder names and chat labels can end mid-name with `-`.
7. The CSV attachment basename can differ from the filename on disk.
8. Contacts can omit phone columns and retain a phone only in `Notes`.
9. Replies and reactions are free text instead of structured records. Observed reaction timestamps use the US `M/D/YYYY` format.
10. Edited and deleted details are limited to rare columns and statuses such as `Recently deleted`.
11. Group conversations have no stable group identifier.
12. `Sender ID` can contain an email address for an iMessage conversation.
