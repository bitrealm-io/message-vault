---
title: Storage
description: What the Storage tab of Settings shows, the account's usage, its Import Runs and Export Runs, and its largest attachments.
---

The **Storage** tab of **Settings** describes what one account holds and how it got there.
Nothing on it can be changed.
It has four sections, in this order: **Usage**, **Import history**, **Export history**, and **Largest attachments**.

The Owner's own Settings has no **Storage** tab, because the Owner holds no messages.

## Usage

**Usage** shows one large figure, the total size of the account's attachments.
The figure counts original file sizes, not the size of any converted or compressed copy.

Two lines under it give four counts: messages, attachments, conversations, and contacts.

## Import history

**Import history** lists the account's Import Runs, newest first.
A run is listed whether it completed, failed, or was cancelled, and a person can't delete one.

The list holds the 40 newest runs.

| Column | Shows |
|---|---|
| **Date** | When the run finished, or when it started if it has not finished |
| **Import type** | The source, such as `imessage`, `whatsapp`, or `sms-backup-restore` |
| **Messages** | Messages counted for the run |
| **Attachments** | Attachments counted for the run |
| **Uploaded size** | Bytes the run uploaded |

### What an Import Run's record shows

Selecting a row opens **Import details** under it.
Selecting the row again, or the **×** button, closes it.

Three labels sit at the top: **Type**, the source, **Mode**, `Append` or `Replace`, and **Status**.

Five figures follow: **Started**, **Finished**, **Messages**, **Attachments**, and **Bytes uploaded**.

A list of four steps comes next, each with the time it took: **Parse backup**, **Attachments**, **Preparing messages**, and **Upload to Message Crate**.

The **Messages** table accounts for every message the run read:

| Row | Counts |
|---|---|
| **Parsed** | Messages read from the backup |
| **Skipped** | Parsed messages the run did not try to upload |
| **Attempted** | Messages the run tried to upload |
| **New uploaded** | Attempted messages that were new to the account |
| **Duplicate** | Attempted messages the account already held |
| **Failed** | Attempted messages that could not be stored |

A count the run did not record shows as a dash.

**Import Errors** appears beside the table when the run recorded problems.
Identical errors are grouped, and selecting a row shows the full message and the files it applies to.

**Contacts** closes the record.
It states how many contacts the run made and how many it changed, then lists each one with what the run did to it:

| Label | Meaning |
|---|---|
| **New** | The run created the contact |
| **New, replaces a trashed contact** | The run created the contact in place of one in the Trash |
| **Named** | The run gave the contact a name |
| **Identity added** | The run added an Identity to the contact |

A contact with no name yet is listed as `(unknown)`.
A run that touched no contact reads "This import changed no contacts."

How a run proceeds is covered in [Import](/docs/user/features/messages/import/).

## Export history

**Export history** lists the account's Export Runs, newest first.
The record holds what was asked for and how much matched, never what the messages said.

The list holds the 40 newest runs.

| Column | Shows |
|---|---|
| **Date** | When the run finished, or when it started if it has not finished |
| **Scope** | What was asked for |
| **Status** | **Running**, **Completed**, **Failed**, or **Cancelled** |
| **Messages** | Messages that matched when the run started |
| **Delivered** | Messages the run handed over |
| **Attachments** | Distinct attachments among the matching messages |
| **Size** | Total size of those attachments |

**Scope** takes one of three forms:

- `Everything`, for all the account holds.
- `Search:` followed by the search text.
- `Picked:` followed by a count of conversations and messages chosen by hand.

A run that never finished shows how far it got in **Delivered**.

How an export is started is covered in [Export](/docs/user/features/messages/export/).

## Largest attachments

**Largest attachments** lists the account's 100 largest attachments by file size, 20 to a page.
**Back** and **Next** move between pages.

| Column | Shows |
|---|---|
| **Name** | The file's original name. Without one, its media type |
| **Conversation** | The conversation the attachment is in |
| **Size** | The file size |

Attachments themselves are covered in [Attachments and media](/docs/user/features/messages/attachments-and-media/).

## What the Owner reads

The Owner reads another account's **Storage** tab from [Owner Home](/docs/user/features/owner/owner-home/).
Two parts are held back, because they say who the account talks to.
The **Contacts** part of an Import Run's record gives the two counts without the list.
**Largest attachments** has no **Conversation** column.
