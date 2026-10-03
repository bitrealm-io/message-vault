---
title: Import
description: Every field of the desktop app's Import form, every stage of an Import Run, and what resuming does.
---

**Import** reads a backup on the computer and stores its messages in the Message Crate.
One import is one Import Run.

Import is in the desktop app's sidebar, under **Messages**.
The browser doesn't show it, because reading a backup needs the desktop app.
The Owner's login doesn't show it either, because the Owner holds no messages.
Importing also needs the account's import permission, which the Owner sets.

A first import is walked through in [Import your backup](/docs/user/your-messages/import-your-backup/).
This page describes every field and every stage.

## Without the import permission

**Import** stays in the sidebar for an account that may not import.
Opening it shows a message in place of the form, and nothing is read from a backup.

| Account | What the screen says |
|---|---|
| An account with **Import** off | The Owner has not allowed this account to import. The Owner turns it on under **Message Permissions**. |
| The Demo Account, which can never import | Importing needs a personal account. **Log out** goes to the login card. |

An Import Run already in progress when the Owner turns **Import** off stays on screen until it ends.
A notice above the run says the Owner has turned Import off.
The server refuses the run's next request.
A run in Staging or Media then ends as failed, and a run in Upload is paused.
Leaving the run shows the message above.

## The form

The form has two sections, **Import Messages** and **Processing Options (Advanced)**, and an **Import** button.
**Processing Options (Advanced)** is shown only for a source that has a field in it.

The first list in **Import Messages** is the source.
The fields under it depend on the source.

| Source | What it reads |
|---|---|
| **iMessage** | An iPhone backup folder, or `chat.db` from a Mac. Covers SMS and MMS as well as iMessage. |
| **WhatsApp** | A WhatsApp database from Android, or an iPhone backup that includes WhatsApp. |
| **SMS Backup & Restore** | A folder of SMS Backup & Restore XML files. |
| **GO SMS Pro** | A folder of GO SMS Pro backup files. |
| **iMazing** | An iMazing export. |
| **SMS Backup+** | A folder of `.eml` files archived from SMS Backup+. |
| **OpenExtract** | An OpenExtract export. |

[Old backups](/docs/user/import-sources/old-backups/) says what GO SMS Pro, iMazing, SMS Backup+, and OpenExtract backups hold and where they fall short.

A red asterisk marks a field that must be filled.
**(Optional)** marks a field that may stay empty.
The **Import** button stays disabled until every needed field is filled and every path exists.
A path of the wrong kind gets a message under its field, for example `Pick the backup folder.` when a file is chosen where a folder is needed.

**Settings → System** has **Remember importer paths**.
When it is on, the form restores the last paths used for each source.
Passwords and keys are never remembered.

### iMessage fields

**Platform** chooses **Mac Messages** or **iPhone backup**.
It starts on **iPhone backup**.

With **iPhone backup**:

| Field | What it takes |
|---|---|
| **iPhone Backup Directory** | The root folder of a device backup made by Finder or iTunes. Required. |
| **Encryption password** | The backup's password. The app checks the chosen folder, and marks the field required when the backup is encrypted and **(Optional)** when it is not. |

[Back up an iPhone](/docs/user/your-messages/back-up-an-iphone/) shows where the backup folder is.

With **Mac Messages**:

| Field | What it takes |
|---|---|
| **Messages database** | The path to `chat.db`. Required. On a Mac that has `~/Library/Messages/chat.db`, the field is filled in with it. |
| **Attachment folder (Optional)** | The folder that contains `Attachments` and `StickerCache`. Left empty, the folders next to `chat.db` are used. |
| **Apple Contacts file (Optional)** | `AddressBook-v22.abcddb` or `AddressBook.sqlitedb`. Left empty, the Mac's own AddressBook is used. |

[Mac Messages](/docs/user/import-sources/mac-messages/) covers this source.

Both platforms have **Attachments**, described under [Attachments](#attachments).

### WhatsApp fields

**Platform** chooses **Android** or **iPhone**.
It starts on **Android**.

With **Android**:

| Field | What it takes |
|---|---|
| **Backup folder** | The folder that contains `msgstore.db`, or `msgstore.db.crypt12`, `.crypt14`, or `.crypt15`. Required. |
| **Decryption key** | A key file path, or a crypt15 key as hex. Required when the folder holds an encrypted backup and no `msgstore.db`, and **(Optional)** otherwise. |
| **WhatsApp phone number** | The number the WhatsApp account is registered to. Required, because an Android backup doesn't record it. Filled in from the first phone number on the account's profile. |
| **Contacts database (Optional)** | `wa.db`. Left empty, the one in the backup folder is used. |
| **Media folder (Optional)** | The WhatsApp media folder. Left empty, the one in the backup folder is used. |
| **Message database (Optional)** | `msgstore.db`. Left empty, the one in the backup folder is used. |

With **iPhone**:

| Field | What it takes |
|---|---|
| **Backup folder** | The root folder of a device backup. Required. |
| **Contacts database (Optional)** | `ContactsV2.sqlite`. Left empty, the one in the backup is used. |
| **WhatsApp Business** | A checkbox, off by default, for a backup of the WhatsApp Business app. |
| **WhatsApp phone number (Optional)** | Under **Processing Options (Advanced)**. A fallback, filled in from the profile. An iPhone backup records the number itself, and Import reads it from there. |

When an iPhone backup doesn't record the number and the fallback field is empty, the run stops with `the backup does not contain your WhatsApp phone number; enter it on the import form`.

[WhatsApp](/docs/user/import-sources/whatsapp/) covers both platforms.

Both platforms have **Attachments**.

### SMS Backup & Restore, GO SMS Pro, and SMS Backup+ fields

The three Android SMS sources share one form.

| Field | What it takes |
|---|---|
| **Backup Directory** | The folder that holds the backup files. For SMS Backup & Restore that is a folder of XML files, not a single ZIP, and an encrypted backup must be unlocked first. |
| **Attachments** | Described under [Attachments](#attachments). |
| **Backup Device Phone Numbers** | Every phone number the backup's phone had. Filled in from the account's profile. A number from another SIM can be added. |
| **Backup Device Email Addresses** | **SMS Backup+** only. The Gmail or IMAP account SMS Backup+ synced to, filled in from the profile. Commas separate several addresses. |

Every field here except **Attachments** carries an asterisk.
The **Import** button stays disabled until the folder is chosen and at least one phone number is entered.
SMS Backup+ also needs at least one email address.

The phone numbers are how Import tells sent messages from received ones, because these backups don't record whose phone they came from.
SMS Backup+ needs the email addresses for the same reason: its archive is a mail account, and the sender of a sent message is that account.

When the profile has no phone number, the form says so and links to **Settings → Profile**.

When none of the entered numbers is on the profile, the form shows a notice, and the **Import** button stays disabled until **Allow import from phone numbers not on my profile.** is ticked.
The notice says what ticking it accepts: the imported messages will not be linked to the account.

### iMazing and OpenExtract fields

Both sources have one field in **Import Messages**, **Backup path**, which takes the folder that holds the export.
It carries an asterisk, and the **Import** button stays disabled until the folder is chosen.
Neither has an **Attachments** setting.

**iMazing** adds **Time zone of the messages** under **Processing Options (Advanced)**.
iMazing writes each message time without a zone, so Import needs the zone the phone was in.
The field is filled in with the account's Time Zone.

### Attachments

**Attachments** has four choices: **Copy**, **Convert**, **Compress & Convert**, and **Skip**.
It starts on **Copy**.

**Compress & Convert** adds three fields: **Target resolution**, **Max FPS**, and **Minimum Video File Size (Megabytes)**.

[Attachments and media](/docs/user/features/messages/attachments-and-media/) describes what each choice produces.

### Processing Options (Advanced)

The section is closed when the form opens.
A source with none of the fields below has no such section.

| Field | Shown for | What it does |
|---|---|---|
| **Obfuscate** | iMessage with **iPhone backup**, and the three Android SMS sources | Replaces names, numbers, message text, and attachments with made-up substitutes. See [Obfuscate](/docs/user/features/messages/attachments-and-media/#obfuscate). |
| **Time zone of the messages** | iMazing | See above. |
| **WhatsApp phone number (Optional)** | WhatsApp with **iPhone** | See above. |

## The identity check for iMessage

For an iMessage source, **Import** first reads which addresses the backup's device sent messages from, and compares them with the phone numbers and email addresses on the account's profile.

When the backup records at least one address and none is on the profile, the screen stops before a run exists.
It reads `None of the addresses this backup sent from are on your profile.` and lists the addresses.
**Add to profile** beside an address adds that one address to the profile.
**Continue import** starts the run, whether or not an address was added.
**Cancel** returns to the form.

## Stages and approvals

An account has at most one Import Run at a time.

A run moves through its Stages in order, and stops at each Review until it is approved or cancelled.
The whole run is one screen: a list of the Stages, with each Review as a row in the list where the run stops.
Each row fills in with what that Stage made.

1. **Staging**
2. **Staging Review**
3. **Media**, only under **Convert** or **Compress & Convert**
4. **Media Review**, only under **Convert** or **Compress & Convert**
5. **Upload**

Under **Copy** and **Skip** the run goes from the Staging Review straight to Upload.

### Staging

Staging reads the backup and copies its messages and original attachments into the Staging Directory on the computer.
Nothing reaches the Message Crate during Staging.

The Staging row shows:

- **Staging directory**, a link that opens the folder.
- **Conversations**, with the count of **Messages** under it.
- **Attachments**: the **Action** chosen on the form, the **Count**, and the **Total size**.
- **Options**, only when **Obfuscate** is on.

The Staging Directory is a new folder for each run, named `staging-` followed by the source and the date and time.
It is made under `~/message-crate` unless **Settings → System** names another **Staging directory**.

### Staging Review

The run stops and the row reads **Awaiting approval**.
Everything it shows is read from the staged files, not estimated, except the group that is labelled as estimates.

- **Contacts**: the count of contacts in the backup, split into **Existing** and **New** to the account.
- **Attachments**: the **Size limit per file**, which is the server's attachment size limit as the run read it when it started, and **Files over the limit**, marked **Skip upload**. The row opens to each file and its size.
- **Conversion estimates** or **Compression estimates**, under **Convert** or **Compress & Convert**. It sorts the files that matter into **Likely within limit**, **May exceed limit**, and **Not audio or video**. Each opens to its files, with the staged size and the expected size. The group is marked **Media has not run yet**, because these are estimates.
- **Identities**, for an iMessage source: the addresses the backup's device used, how many messages each one **Sent** and **Received**, whether it is **On your profile**, and **Add to profile** for one that is not.

The approve button says what happens next: **Upload to Message Crate**, **Convert media**, or **Compress media**.
**Cancel this import** ends the run and deletes the Staging Directory.

When **Convert** or **Compress & Convert** is chosen and ffmpeg can't be found, the approve button is disabled and the row says so.
[Attachments and media](/docs/user/features/messages/attachments-and-media/#ffmpeg) says where the ffmpeg folder is set.

### Media

Media converts or compresses the staged attachments.

The Media row then shows the new **Total size** of the attachments, and **Could not be converted** or **Could not be compressed** with the count of files ffmpeg failed on.

### Media Review

The run stops again and reads **Awaiting approval**.
The row shows the **Size limit per file** and the **Files over the limit** as they are now, after Media, by name and size.

**Upload to Message Crate** continues.
**Cancel this import** ends the run and deletes the Staging Directory.

### Upload

Upload writes the staged messages and attachments into the Message Crate.
A file over the size limit is not uploaded, and its message is stored without it.

The Upload row shows, once the run finishes:

- **Messages**: the count sent, split into **New**, **Duplicate**, and **Failed**. A duplicate is a message the Message Crate already holds, which is skipped.
- **Attachments**: the count **Uploaded**.
- **Contacts**: **New** and **Modified**, and a **Contact list** that opens to each contact the run touched.
- **Import log**: a link to `message-crate-push.log` in the Staging Directory.

### Approved Reviews and errors

A Review that has been approved folds to one line, **Approved**.

When a finished run reports errors, an **Errors** table sits under the list, with the count beside the heading.
Identical errors are grouped.
Its columns are **Parse File**, **Stage**, and **Error Message**, and a row opens to the full message and the files it names.

## Leaving a run

A run keeps working while other screens are used.
A run at a Review keeps waiting on another screen and after the app is closed.

The **Import** entry in the sidebar carries a badge that reads **Waiting** while a run is at a Review, **Paused** while a run is paused at its Upload, and **Failed** when the run in the app has failed.

**Cancel** under Staging or Media stops that Stage.
The run stays open and the Staging Directory stays in place, so the run can be resumed.

**Pause** under Upload stops the Upload.
The run stays open at its Upload and the Staging Directory stays in place.
Resuming it sends only the conversations that are not in the Message Crate yet.

An Upload that fails is paused the same way, because a failure there is mostly the network or the server.
Every conversation it did not send stays staged for the resume.

**Log out** during an Upload first asks: "An Upload is running. Logging out pauses it; you can resume it after you log in."
**Log out** there pauses the Upload, waits until it has stopped, and then logs out; **Go back** leaves the Upload running.
The next time the same account logs in and opens Import, the run is offered with **Resume**.
An Upload is also paused, without asking, when the account is deleted or its session has ended.

## Resuming

When Import is opened and the account already has a run open, the screen shows that run in place of the form.
Every case offers **Discard this import**, which ends the run.
On the computer that staged the run, it also deletes the Staging Directory.

| The run stopped | The screen reads | The button |
|---|---|---|
| Before Staging began copying | `Pick up your last import` | **Start over** reads the backup from the beginning with the same settings. |
| During Staging | `Finish copying your backup` | **Pick up** reads the backup again and skips the conversations already copied. |
| During Staging, and the backup has changed since | `The backup has changed` | **Start over** reads it fresh, because copying more of it would mix two backups in one folder. |
| At a Review | `Pick up where you left off` | **Show me the summary** reads the staged files again and returns to the Review. |
| During Media | `Finish preparing your media` | **Carry on** processes the files Media had not reached. |
| During Upload, or paused | `Finish your last import` | **Resume** sends the conversations that are not uploaded yet, without staging again. |

Three cases offer only **Discard this import**, because there is nothing on this computer to carry on from:

- The Staging Directory is gone.
- The run was started on another computer, and its files are staged there.
- The run's settings could not be read.

A resumed run uses the settings it was started with.
The form is not shown, so no setting can be changed.

The one exception is a secret.
Message Crate does not store the **Encryption password** of an iPhone backup or the WhatsApp **Decryption key** with the run.
When the run was started with one and the resume reads the backup again, the screen asks for it, and the button stays disabled until the field is filled.
That applies to the first three rows of the table, where the run stopped before or during Staging.
A resume at a Review, during Media, or during Upload reads the staged files and asks for nothing.

### The upload journal

Upload keeps a journal named `.import-state.jsonl` in the Staging Directory.
It records each attachment, each batch of messages, and each conversation that reached the Message Crate, for one server address and one username.

A resumed Upload reads the journal and skips what is recorded there.
That is why resuming an interrupted Upload does not send everything again.

## After the run

The heading of a finished run says how it ended.

| Heading | Meaning |
|---|---|
| `Imported 1,234 messages` | The run succeeded. The number is the count of new messages. |
| `Imported 1,234 messages, with errors` | The run succeeded and some messages or files failed. The **Errors** table lists them. |
| `Import failed` | The run could not start, or Staging or Media failed. |
| `Import cancelled` | Staging or Media was cancelled. |
| `Import paused` | The Upload was paused, or it failed before every conversation was sent. The run can be resumed. |

A run that ends deletes its Staging Directory, and the import log and the journal with it.
After a success the Message Crate holds the messages, so the staged copy is no longer needed.
After a failed Staging or Media, nothing complete was staged, so there is nothing to upload.
A run that is cancelled or paused leaves the Staging Directory in place, because the staged files are what a resume reads.
The Errors of a paused run are kept with its staged files, so the finished run lists the Errors of every part.

A run that succeeded leads with where to go next:

- **View imported conversations** opens the conversation list narrowed to the run, with the search `import:#` followed by the run's number.
- **View modified contacts** opens the Contact Group the Message Crate made for the run. It is shown when the run created or modified at least one contact.

**Back** returns to the Import form.

The record of the run stays under **Import history** in [**Settings → Storage**](/docs/user/features/settings/storage/).

## Importing the same backup again

A second import of the same backup creates no duplicates.
The Message Crate recognises the messages it already holds and skips them, so a newer backup of the same phone adds only what is new.
