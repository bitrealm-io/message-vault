---
title: Audit Trail
description: What the Audit Trail tab of Settings shows, every login, Import Run, Export Run and change made to one account, and who made it.
---

The **Audit Trail** tab of **Settings** lists what was done with one account, and when, newest first.
It holds the account's logins and how each session ended, the logins refused for its username, its Import Runs and Export Runs, and every change made to the account, whether the account holder or the Owner made it.

Every account has the tab, the Owner's included.
The Owner reads every account's entries together under **Audit Trail** in [Owner Home](/docs/user/features/owner/owner-home/).

## What an entry holds

Each row has three columns:

| Column | Meaning |
|---|---|
| **Date** | When it happened. |
| **What happened** | The act, with its counts. |
| **By** | Who acted: the **Account holder**, the **Owner**, a **Server command** such as `reset-owner-password`, the **Server** on its own, or the **Login screen** for a refused login. |

An entry says that something happened and how much.
It never says what a message said or which conversation it was in.
An Export Run shows whether it covered everything, a search or picked conversations, and how many messages matched, never the search itself.

## What is recorded

- Logging in, from which app and Build, and how each session ended: logged out, replaced by a newer login, ended by the server, or expired.
- A refused login: an unknown username, a wrong password, or a disabled account.
- Each Import Run and Export Run, and what started it: a login, or an API token named by its label and hint.
- The Owner creating the account, disabling or re-enabling it, setting its password, changing its permissions, deleting its messages for good, or deleting it.
- The account holder changing their password, deleting their messages for good, deleting a conversation from the trash, emptying the trash, making or deleting an API token, and loading or exporting an address book.

Moving conversations to the trash and back, tags, Saved Searches, contact names, the display name, the Time Zone, and identities are not recorded.

## Nothing is removed

Nobody can change or delete an entry, the Owner included.
Deleting an account keeps its entries and runs under its old username, adds an **Account deleted** entry, and drops each Export Run's search text.
Refused logins for a username no account has are kept for 90 days.
