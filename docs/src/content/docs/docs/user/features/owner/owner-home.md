---
title: Owner Home
description: The five sections of Owner Home, and how the Owner adds, changes, disables, and deletes accounts.
---

The Owner logs in to **Owner Home** in place of a message list.
The Owner runs the Message Crate and holds no messages, so Owner Home has no **Messages**, **Contacts**, **Import**, **Export**, or **Trash**.

Nothing in Owner Home shows a message's text, an attachment's contents, or a contact's name.
The Owner sees counts, sizes, dates, and attachment file names.

The side panel lists five sections: **Dashboard**, **Server Settings**, **User Accounts**, **Activity**, and **Logs**.
Login opens **User Accounts**.

## Dashboard

**Dashboard** shows what the Message Crate holds across every account, in three parts.

**Contents** shows the total size of all attachments.
Under it are the number of messages, attachments, conversations, and contacts.

**Database** shows three sizes:

| Figure | Meaning |
|---|---|
| **Database size** | The size of the database on disk. |
| **Messages on disk** | The part of the database the messages take. |
| **Full-text search index** | The part the search index takes. |

**Database size** leaves out attachment files, because they are stored as files beside the database and are counted under **Contents**.
The search index is one structure shared by every account, so it has one figure and no split by account.

**Messages by account** has one row per account, the Owner first.

| Column | Meaning |
|---|---|
| **Account** | The account's username. |
| **Messages** | How many messages the account holds. |
| **Text** | The size of the text of those messages. |
| **Estimated size on disk** | The account's share of **Messages on disk**. |

The estimate is **Messages on disk** split by each account's share of the text.
The last row, **All accounts**, carries the totals.
The Owner's row reads zero, because the Owner holds no messages.

## Server Settings

**Server Settings** holds one switch, the attachment size limit, the Demo Account, and two facts about the server.

The switch is **Let anyone who can reach this server create their own account**.

- Off, the login screen offers **Login** only, and the Owner creates every account.
- On, the login screen also offers **Create Account**, and anyone who can reach the server can make an account for themselves.

It is off on a new Message Crate.
The switch saves when it is selected. There is no save button.

### Attachment size limit

**Attachment size limit** is the largest file an import can upload as an attachment, in MB.
It is 512 on a new Message Crate.
The Owner types a new number and selects **Save**.
The server refuses a number below the part size it uses for large uploads, 64 MB unless the config file changes it, and says so under the field.

A new limit holds for every upload after it is saved, with no restart.
An Import Run already under way keeps the limit it started with, so its Staging Review still describes what it will upload.

### Demo Account

The Demo Account holds made-up conversations.
Anyone who reaches the server opens it with **Explore Demo Account** on the login screen, and it has no password.
Every new Message Crate starts with it.

- **Add Demo Account** shows when there is none. It builds the account with the size chosen beside it.
- **Reset Demo Account** shows when there is one. It removes the account, with everything visitors changed in it, and builds it again. It asks first. No other account is touched.

The sizes are **Medium**, about 54,000 messages, and **Large**, about 613,000 messages.
Medium takes a few seconds. Large takes about a minute.
The card shows **Building the Demo Account** until the build ends, and the server keeps working meanwhile.

The Demo Account is removed under **User Accounts**, as any account is.
Its page there shows its status, permissions, and identities, and none of them can be changed.

Below the Demo Account:

- **Version** is the version of the server that is running.
- **Schema fingerprint** is a number the server derives from its database layout. [Update Message Crate](/docs/user/features/owner/update/#when-the-database-layout-changes) says what a changed number means.

## User Accounts

**User Accounts** lists every account, the Owner first.

| Column | Meaning |
|---|---|
| **User** | The username, with the account's display name under it when it has one. |
| **Status** | **Owner**, **Active**, or **Disabled**. |
| **Last login** | The date and time of the last login, or **Never**. |

The search bar at the top reads **Search accounts**.
It narrows the list to the accounts whose username or display name contains the typed text.

A gear appears at the left of a row when the pointer is over it.
The gear opens that account's settings.

### Add an account

1. Select **Add account**. The **New account** form opens. Its **Profile** and **Storage** tabs can't be opened yet, because they belong to an account that exists.
2. Enter a **Username**.
3. Enter a **Password** and repeat it in **Confirm password**. The heading reads **Password (optional)**, because the form allows an account with no password. An account that holds real messages should have one.
4. Select **Create**.

The form reports **Invalid username.** when the username is already taken.

The screen changes to **User Settings: _username_** for the new account.
The Owner stays logged in.
The new account is **Active**, with **Import**, **Export**, and **Delete** all on.

### User Settings for one account

The gear on a row opens **User Settings: _username_**, with the display name in brackets when the account has one.
**← User Accounts** at the top goes back to the list.

The screen has three tabs: **Account**, **Profile**, and **Storage**.

#### Account

**Username** shows the username. It can't be changed.

**Status** is **Active** or **Disabled**.
The server refuses a disabled account's login, and refuses every request from a Session the account already has open.
The account's messages stay where they are.

**Message Permissions** has three checkboxes: **Import**, **Export**, and **Delete**.
Each one covers messages and their attachments.

**Status** and **Message Permissions** save when they are changed. There is no save button.
The account's holder sees the same status and permissions in their own **Settings** and can't change them.

**Change Password** has **New password** and **Confirm new password**.

- **Change password** sets the new password.
- **Reset password** removes the password, so the account logs in with an empty one.

Neither one ends a Session the account has open, and neither makes the person choose a new password at the next login.
The Owner doesn't see the old password and isn't asked for it.

**Danger zone** is closed until it is selected. It holds two actions:

| Action | Button | What it deletes |
|---|---|---|
| **Delete all messages & attachments** | **Delete all messages** | The account's messages and attachments. The account, its contacts, and its settings remain. |
| **Delete this account** | **Delete account** | The account with all its contacts, messages, and attachments. |

Each button asks for confirmation first, and the confirmation states how many messages will be deleted.
Neither deletion can be undone.

The Owner doesn't see an account's API tokens. Only the account's holder does.

#### Profile

The Owner sets the same three things the account's holder sets:

- **Display Name**, saved with **Save**.
- **Time Zone**, the zone the account's message times are shown in. It saves when a zone is chosen.
- **Identities**, the account holder's own phone numbers and email addresses.

Two more parts are for reading only:

- **Last Login** is the date and time of the account's last login, or **Never**.
- **App** is the app the account last connected with, **Desktop app** or **Website**, and that app's version. It reads **Never connected.** for an account that hasn't connected. When the app's version differs from the server's, a line under it gives the server's version. The server serves that app all the same.

#### Storage

**Storage** shows what the account holds, as its holder sees it:

- **Usage**: the size of the account's attachments, with its counts of messages, attachments, conversations, and contacts.
- **Import history**: each import, with its date, type, message count, attachment count, and size.
- **Export history**: each export, with its date, scope, status, and counts.
- **Largest attachments**: file names and sizes.

Two things the holder sees are left out for the Owner, because they say who the account talks to.
The Owner doesn't see which contacts an import created, and doesn't see which conversation a large attachment is in.

### The Owner's own settings

The Owner's row opens **Settings for Owner**.
**Settings** in the account menu, the round button at the top right, opens the same settings.

The tabs are **Account**, **Profile**, and **Appearance**.
There is no **Storage** tab, because the Owner holds no messages.

Changing the Owner's password needs **Current password** as well as the new one, because the Owner's login reaches every account.
There is no **Reset password**, because the Owner must have a password.
There is no **Danger zone**, because the Owner can't be deleted.

## Activity and Logs

**Activity** and **Logs** show only their name.
Nothing is built behind them yet.
