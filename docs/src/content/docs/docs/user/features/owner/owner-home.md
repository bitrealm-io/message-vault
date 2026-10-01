---
title: Owner Home
description: What the owner sees, and what the Dashboard's figures mean.
---

The owner logs in to **Owner Home** instead of a message list. The
owner administers Message Crate and reads no messages: nothing on these pages
names a conversation, a contact or a line of text. The side panel has
**Dashboard**, **Server Settings** and **User Accounts**.

## Dashboard

The Dashboard is a column of three sections about the whole Message Crate.

**Contents** shows how much attachment storage there is, and
how many messages, attachments, conversations and contacts there are across
every account.

**Database** shows three measured figures side by side: the size of the
database on disk, how much of it the messages take, and how much the
full-text search index adds. The database size excludes attachment files,
because they are stored as files beside the database and are counted under
Contents. The search index is one shared structure, so it is reported
once for the whole database rather than per account.

**Messages by account** has one row per account with its username, how many
messages it holds, how much text they are, and an estimated size on disk. The
estimate is the messages-on-disk figure split by each account's share of
text, so the rows add up to the whole-database figure in the totals row at the
bottom. An account with no messages reads zero.

## Server Settings

Whether anyone reaching the server may create their own account, or only the
owner creates accounts.

## User Accounts

Every account, the owner's own first. The gear on a row opens
that account's Settings, where the owner sets its profile, sees what it
holds under Storage, and manages its password, status and permissions under
Account. **Add account** opens the same Settings for an account that does
not exist yet.
