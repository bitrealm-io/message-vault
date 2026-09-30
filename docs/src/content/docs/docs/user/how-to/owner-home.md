---
title: Owner Home
description: What the vault owner sees, and what the Dashboard's figures mean.
---

The vault owner logs in to **Owner Home** instead of a message list. The
owner administers the vault and reads no messages: nothing on these pages
names a conversation, a contact or a line of text. The side panel has
**Dashboard**, **Vault Settings** and **User Accounts**.

## Dashboard

The Dashboard is a column of three sections about the whole vault.

**Vault contents** shows how much attachment storage the vault holds, and
how many messages, attachments, conversations and contacts there are across
every account.

**Database** shows three measured figures side by side: the size of the
database on disk, how much of it the messages take, and how much the
full-text search index adds. The database size excludes attachment files,
because they are stored as files beside the database and are counted under
Vault contents. The search index is one shared structure, so it is reported
once for the vault rather than per account.

**Messages by account** has one row per account with its username, how many
messages it holds, how much text they are, and an estimated size on disk. The
estimate is the messages-on-disk figure split by each account's share of
text, so the rows add up to the whole-vault figure in the totals row at the
bottom. An account with no messages reads zero.

## Vault Settings

Whether anyone reaching the vault may create their own account, or only the
owner creates accounts.

## User Accounts

Every account in the vault, the owner's own first. The gear on a row opens
that account's Settings, where the owner sets its profile, sees what it
holds under Storage, and manages its password, status and permissions under
Account. **Add account** opens the same Settings for an account that does
not exist yet.
