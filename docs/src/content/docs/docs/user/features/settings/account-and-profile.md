---
title: Account and Profile
description: What the Account and Profile tabs of Settings show and change, the username, password, API Tokens, danger zone, display name, Time Zone, and the account's own identities.
---

**Settings** opens from the account menu, the round button at the top right of every screen.
The menu names the logged-in account and holds two entries, **Settings** and **Log out**.

Settings is divided into tabs.
An account sees **Account**, **Profile**, **Storage**, **Audit Trail**, **System**, and **Appearance**.
The desktop app adds **Convert** between **System** and **Appearance**.
This page covers the first two.

## What the Owner sees

The Owner's Settings has four tabs, **Account**, **Profile**, **Audit Trail**, and **Appearance**, under the heading **Settings for Owner**.
**Storage**, **System**, and **Convert** are absent, because they concern messages and the Owner holds none.

The Owner's **Account** tab holds **Username** and **Change Password** only.
The Owner's **Profile** tab holds **Display Name** and **Time Zone** only.

The Owner manages other accounts from [Owner Home](/docs/user/features/owner/owner-home/), not from this screen.

## Account

### Username

**Username** shows the name the account logs in with.
The field is read-only.

### Status and Message Permissions

**Status** reads **Active** or **Disabled**.
**Message Permissions** lists three checkboxes, **Import**, **Export**, and **Delete**, which cover messages and their attachments.

The account holder reads both and changes neither, because the Owner sets them.
The line under the checkboxes says so: "The owner sets your status and permissions."

### Change Password

The form takes **New password** and **Confirm new password**.
**Change password** stays disabled until both fields hold text.
The server compares the two and answers "New passwords do not match." when they differ.

A password has no minimum length.
The longest password the server accepts is 1,024 bytes.

**Reset password** clears the password, so the account then logs in with an empty one.
The screen confirms with "Password reset. This account now has no password, and its API Tokens were revoked."
A changed password is confirmed with "Password changed. This account's API Tokens were revoked."

Changing or resetting the password does two more things:

- It replaces the account's Session. The screen that made the change stays logged in, and any other browser or desktop app logged in to the account must log in again.
- It deletes every API Token the account holds, so each program using one needs a new token. The API Tokens table on the same tab empties at once.

The Owner's form differs in three ways, because the Owner's account reaches every other account.
It asks for **Current password** first.
It refuses a new password equal to the current one.
It has no **Reset password**, because the Owner must have a password.

On the `demo` account the form is disabled, because that account exists to be looked at and reset rather than changed.

### API Tokens

The **API Tokens** section holds the account's API Tokens.
An API Token is a named credential a program uses to act for the account through the server's [HTTP API](/docs/developer/reference/api/) without logging in.
It is not a Session: it can't browse messages, and it can't make, rename, or delete tokens.

**Add** opens a form with a name field and two checkboxes, **Import** and **Export**.
Both start checked.
A checkbox is disabled and reads "Your account cannot do this." when the account itself lacks that permission, because a token never exceeds its account.
A token never carries the **Delete** permission, because permanent deletion needs a logged-in person.

The name is required and holds at most 120 characters.

**Save** creates the token and shows its secret in the **API Token created** dialog, with a **Copy** button.
The secret is shown this once and can't be retrieved later, because the server stores only a hash of it.

The table lists each token:

| Column | Shows |
|---|---|
| **Name** | The name given at creation |
| **Token** | A masked hint, such as `mc-api-Sd..mE` |
| **Permissions** | `Import`, `Export`, `Import / Export`, or `None` |
| **Created** | The date the token was made |
| **Last Used** | The date a program last used it, or `Never` |
| **Expires** | The date the token stops working, or `Never` when it has no expiry |

The pencil button renames a token.
The secret does not change.

The trash button deletes a token after a confirmation.
A program using that token stops working at once.

A token made on this screen expires 365 days after it is made.
The **Expires** column shows that date.

The Owner sees this table in the account's **User Settings**, without the **Token** column, and can revoke a token there so a leaked one can be ended.
The Owner can't add or rename a token.

The Owner's own Settings have no **API Tokens** section, because a token reaches one account's messages and the Owner holds none.

### Danger zone

**Danger zone** is collapsed until its heading is selected.
It holds two actions.

**Delete all messages** permanently deletes the account's conversations, messages, and attachments, after one confirmation.
The account, its contacts, and its settings remain.
The server refuses the action when the account lacks the **Delete** permission.
While an import into the account is running, the attachment files stay on the server's disk, because the import may still need them.

**Delete account** permanently deletes the account with its messages, contacts, and attachments.
The dialog asks for the username, typed exactly.
It asks for **Current password** as well when the account has a password.
Deleting the account ends the Session and returns to the login screen.

Neither action can be undone.

Both buttons are disabled on the `demo` account and carry the tooltip "Unavailable on the demo account".
The server also refuses to delete the `demo` account.

The Owner has no **Danger zone**, because the Owner holds no messages and can't be deleted.

## Profile

### Display Name

**Display Name** is the account holder's name.
The account menu shows it under the username, and the Owner's list of accounts shows it beside the username.
**Save** stores it.
An empty field removes the name.

### Time Zone

**Time Zone** is the zone every message time is shown in.
It is the person's zone, never the server's.

Typing in the field narrows the list by city, country, abbreviation, offset, or IANA name.
With nothing typed, the list opens with **This browser**, the zone the browser reports, above **All time zones**.

Picking a row saves it.
There is no **Save** button, and every date on screen changes to the new zone.

On the Demo Account the display name and the time zone cannot be changed, by the account or by the Owner.
Every visitor shares that account, so a change one visitor made would be what the next one finds.

### My Identities

**My Identities** lists the phone numbers and email addresses that are the account holder's own.
Import uses them to decide which messages in a backup belong to the holder.
An import never adds one.

The table has one row per Identity:

| Column | Shows |
|---|---|
| **Service** | **Text message**, **Email**, or **WhatsApp** |
| **Identity** | The phone number or email address |
| **First sent** | The date of the earliest message the account holder sent from this Identity |
| **Last sent** | The date of the latest message the account holder sent from this Identity |
| **Conversations** | How many conversations it takes part in |
| **Direct messages** | Messages in one-to-one conversations |
| **Group messages** | Messages in group conversations |

A column heading sorts the table by that column.
A dash stands for a zero or a missing date.

**Add identity** opens a dialog with a **Service** and an **Identity** field.
A phone number must hold 7 to 15 digits, and spaces, dots, dashes, and parentheses are accepted.
An Identity already in the list on the same service is refused.
The same number on **Text message** and on **WhatsApp** counts as two Identities, so both can be listed.

The trash button on a row removes that Identity after a confirmation.
The confirmation states how many direct and group messages will no longer be associated with the account.

The Owner has no **My Identities** section, because the Owner holds no messages to match.

### Address book

The Address Book is a CSV file of contacts, written by **Export** on the Contacts screen and loaded back here after editing.

**How to load it** offers **Append**, which adds and renames and removes nothing, and **Edit**, which also makes each contact in the file match its rows.
**Choose a file** loads a `.csv` file of at most 8 MB.

The section then lists what the load changed: contacts created, updated and deleted, identities added, moved and removed, and Contact Groups created.
A file with a mistake in it is refused whole, and each row at fault is listed with its reason.

A spreadsheet can save a phone number such as `+6595550100` as `6595550100`, without its `+`.
A number written without `+` names the identity its contact already holds under `+` and those digits, so the contact keeps that identity.
When the contact holds the number both ways, for example `+6595550100` and `+16595550100`, the file is refused, because the row cannot say which it means.
Any other number without `+` is read as a US number when it has ten digits, and as its digits otherwise.
After the load, the section lists each number without `+` that it read with its `+` back, and each one that became a new identity.

Contacts the file does not mention stay as they are.

The Demo Account has no **Address book** section, and the server refuses a load into it.
An **Edit** load would delete its contacts for good.
An **Append** load would store real people's names and numbers in an account anyone can enter.

Contacts themselves are covered in [Contacts](/docs/user/features/contacts/contacts/).
