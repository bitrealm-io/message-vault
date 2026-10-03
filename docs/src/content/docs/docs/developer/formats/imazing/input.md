---
title: "iMazing input format"
description: "CSV columns and limitations in iMazing Messages and WhatsApp exports."
---

This reference describes the iMazing Messages, WhatsApp, and Contacts CSV files consumed by `imazing-exporter`. It records facts observed in iMazing 3.5.5, validated against a full **All messages** device export on 2026-07-19. It is not a complete specification for every iMazing version.

Conversion behavior and parser decisions are documented in [design](/docs/developer/formats/imazing/design/).

## Export tree

A full device export root typically contains:

```text title="Device export tree"
Device-Info.txt
Contacts/.../Contacts - {stamp}.csv
Messages/{YYYY-MM-DD HH MM SS} - {label}/Messages - {export-stamp} - {label}.csv
WhatsApp/{YYYY-MM-DD HH MM SS} - {label}/WhatsApp - {export-stamp} - {label}.csv
```

Media files sit beside the CSV in each chat folder. There is no `Attachments/` subdirectory. A media file's name has this shape:

```text title="Attachment filename"
{YYYY-MM-DD HH MM SS} - {label} - {name}
```

The timestamp is the row's `Message Date` with each `:` replaced by a space, and it matches to the second: no file is off by a second or by whole hours.
A `Message Date` written without seconds stands for second `00`, because iMazing writes the seconds into every file name.

The label comes from the chat, and a row can't rebuild it.
On the maintainer's iMazing 3.5.5 export, measured for [#1082](https://github.com/messagecrate/message-crate/issues/1082) under "How iMazing names the files", the label differs from the chat folder's label in 71 of 298 Messages folders and from `Chat Session` in 142.
The name is the row's `Attachment` cell, which is always a bare basename, as iMazing changed it when it wrote the file.
It can itself hold ` - `, so the file name can't be split on ` - `.

iMazing changes the basename in four ways.

1. It converts the extension: heic to jpg, caf to mp3, opus to mp3, and webp to png.
2. It cuts a stem longer than 40 characters to its first 40.
3. It removes non-ASCII characters from the stem. U+202F, U+2019, U+202D, U+2026 and U+00AE occur.
4. It numbers the files of rows that would get one name, `X.ext`, `X 2.ext`, and on to `X N.ext`, because one folder can't hold two files of one name.

The written name is the name iMazing writes for a cell: the stem without non-ASCII characters, cut to 40, with the extension converted.
The numbering name is the written name in lower case.
The importer numbers rows of one CSV whose `Message Date` and numbering name are the same.
It assumes iMazing numbers two names that differ only in case together, because iMazing writes to a file system that ignores case by default, as macOS and Windows do.
On that assumption, `photo.JPG` and `photo.jpg` of one second get the files `photo.JPG` and `photo 2.jpg`.
The rows are numbered in CSV order: the first takes the plain name and the k-th the name whose stem ends with ` k`.
Two cells can share a written name, such as two long names that cut to the same 40 characters, so they are numbered together.

The importer looks for a row's file in the row's own chat folder only, never in another conversation's, because a file of the same name elsewhere belongs to another message.
The file's name must start with the row's timestamp and ` - `, and end with ` - ` and a name iMazing may have written for the row.
The label between the two is not compared, because a row can't rebuild it.
The names are tried in this order, and the first that any file ends with is the one used:

1. the `Attachment` cell as written;
2. the cell with its extension converted;
3. either of these with every non-ASCII character removed from the stem and the stem cut to its first 40 characters.

A file whose name ends with a longer name that another row of the same second gives is that row's.
So a row naming `photo.jpg` takes `{timestamp} - {label} - photo.jpg` and leaves `{timestamp} - {label} - Holiday - photo.jpg` to the row naming `Holiday - photo.jpg`.
The rule holds because the label can't be compared: without it, both files would end with ` - photo.jpg` and the shorter name would match two files.

Names and extensions are compared to files exactly, with no case folding and no timestamp tolerance, because the export measured for #1082 needs neither and nothing measured shows what iMazing writes for an upper-case `HEIC`.

The importer gives a row no file, and marks its attachment `file_missing`, in four cases, because in each nothing tells which file is the row's:

- the row's folder holds no file of the row's shape, or two or more;
- rows of one second share a numbering name and the folder lacks the file of any of them, so all of them are marked, because a missing file moves the ` 2`, ` 3` numbers;
- two or more rows of one CSV would take one file, so none of them gets it;
- the row is a Location row, which names a `.vcf` while its file is a `.url` with another stem.

Six points are not confirmed:

- that the ` 2`, ` 3` files follow the order of the rows in the CSV;
- that iMazing numbers rows by the written name, after conversion, removal of non-ASCII characters and the 40-character cut, rather than by the `Attachment` cell as written;
- that iMazing numbers two names that differ only in case together;
- the order in which iMazing removes non-ASCII characters and cuts to 40;
- that a `Message Date` written without seconds stands for second `00`;
- any version of iMazing other than 3.5.5.

Two kinds of file in a Messages chat folder are named by no row:

- **A Live Photo's video**, `{message timestamp} - {label} - {stem}.mov`, sits beside its picture, `{message timestamp} - {label} - {stem}.jpg` or `.jpeg`. Only the extension differs.
- **A link preview**, `{message timestamp} - {label} - Web link.url`, or `Web link 2` and on when two share a second, is one `[InternetShortcut]` section holding a single `URL=` line. It has no title and no image, and its address appears in the `Text` of a row at the same second.

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

## Contacts

iMazing can emit contacts as a wide CSV with vCard-style fields such as `First Name` and `Mobile Phone`.
The importer does not read it: discovery skips Contacts exports so they are never parsed as conversations, and Message Crate's address book is its own CSV, not a vendor's.

## Source limitations

These limitations come from the exported files. The importer cannot recover information that iMazing did not include:

1. Outgoing rows do not contain the owner’s number or name.
2. Many one-to-one chats use a display name as `Chat Session` instead of a phone number.
3. A silent Messages group member has no phone, because no row pairs their display name with an address.
4. WhatsApp has no complete group roster. Participants are inferred from senders.
5. `Message Date` values do not contain a timezone.
6. Long folder names and chat labels can end mid-name with `-`.
7. The CSV attachment basename can differ from the filename on disk.
8. Contacts can omit phone columns and retain a phone only in `Notes`.
9. Replies and reactions are free text instead of structured records. Observed reaction timestamps use the US `M/D/YYYY` format.
10. Edited and deleted details are limited to rare columns and statuses such as `Recently deleted`.
11. Group conversations have no stable group identifier. A group's key is made from its earliest message instead (see [design](/docs/developer/formats/imazing/design/#groups)). The key changes when the oldest messages are gone from the phone, for instance under **Keep Messages: 1 year**, or when an export covers only a date range. `Message Date` holds no time zone (item 5), so it also changes when two exports write the dates in different time zones. The group then comes in as a second conversation beside the first, with a second copy of the messages both exports hold. Groups that start with the same row are merged only when they have the same session name. Only one iMazing export has been measured, so the behaviour across two exports is reasoned, not observed.
12. `Sender ID` can contain an email address for an iMessage conversation.
