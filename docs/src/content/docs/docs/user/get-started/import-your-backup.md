---
title: Import your backup
description: Point the desktop app's Import form at the backup folder, approve the review, and open the imported conversations.
---

This step reads the backup folder from step 6 and stores its messages in the Message Crate.
It happens in the desktop app, logged in as the account from step 4.

## Open Import

In the left panel, under **Messages**, select **Import**.

![The Import form with iMessage and iPhone backup selected](../../../../../assets/user-guide/import-form.png)

The Owner's login doesn't show **Import**, because the Owner holds no messages.

## Fill in the form

### For an iPhone backup

1. The first list is the source. Choose **iMessage**. It covers SMS and MMS as well as iMessage, because an iPhone keeps all three together.
2. **Platform**: **iPhone backup**.
3. **iPhone Backup Directory**: select **Browse** and pick the device folder from step 6, the one that contains `Manifest.plist`.
4. **Encryption password**: the backup's password. The field is marked required when the app finds the backup is encrypted, and optional when it isn't.

### For an Android backup

1. The first list is the source. Choose **SMS Backup & Restore**.
2. **Backup Directory**: select **Browse** and pick the folder that holds the `.xml` files.
3. **Backup Device Phone Numbers**: every number that belonged to the phone. The numbers on the account's profile are filled in already.

### Attachments

**Attachments** stays on **Copy**, which uploads every photo, video, and file as it is.

**Convert** and **Compress & Convert** need the separate program ffmpeg, so they are left for a later import.
With **Copy**, a browser shows the formats it can show. Some iPhone photos and videos are in formats that not every browser displays, and those don't show until they are converted.
[Attachments and media](/docs/user/features/messages/attachments-and-media/) covers the other choices.

## Start the import

Select **Import**.

For an iPhone backup, the app first compares the addresses the phone sent from with the ones on the account's profile.
When none match, it stops and says so, because it would otherwise mark the person's own messages as received.
**Add to profile** adds the phone's addresses and carries on.

## Follow the run

The screen changes to a list of stages, in order.
The run works through them from the top.

1. **Staging** reads the backup and copies the messages and attachments into a working folder on this computer. Nothing has reached the Message Crate yet. A large backup takes a while here.
2. **Staging Review** stops the run and reads **Awaiting approval**. It shows what was found: the count of conversations and messages, the attachments and their total size, and the contacts, split into **Existing** and **New**.
3. **Upload** writes everything into the Message Crate.

Nothing is stored until the review is approved.
**Upload to Message Crate** approves it.
**Cancel this import** ends the run and deletes the working folder.

The app can be used for other things while a run works, and the run waits at the review for as long as it takes.

## Check that it worked

The screen's heading changes to **Imported**, followed by the number of messages.

**View imported conversations** opens the conversation list, narrowed to what this run brought in.

A heading that ends in **with errors** means some messages or files didn't import.
The errors are listed under the stages, grouped by reason, and the messages that did import are in the Message Crate.

The record of the run stays under **Settings → Storage → Import history**.

## Importing again later

Importing the same backup twice doesn't create duplicates.
The Message Crate recognises messages it already holds and skips them, so a newer backup of the same phone adds only what is new.

Next: [Where to go next](/docs/user/get-started/where-to-go-next/).
