---
title: Contacts
description: What a Contact is, what the Contacts list shows, where a contact's name comes from, and how a contact is renamed, given identities, and moved to the Trash.
---

A Contact is one person Message Crate knows: a name, and the identities that reach them.
An Identity is one address a person is reached at: a phone number, an email address, or a username on a service.
An identity belongs to at most one contact.

Contacts are not typed in.
An import makes a contact for every person it meets in a backup, so the Contacts list fills as messages arrive.

## The Contacts list

**Contacts** in the left panel opens the list of every contact the account holds, except those in the [Trash](/docs/user/features/messages/trash/).

Each row shows the contact's name.
A contact with no name shows its first identity in italics, so an address does not read as a name someone gave the contact.
The date at the right of a row is **Last heard from**: the day the contact last sent a message.
A contact that never sent a message has no date.
A message the account holder sent to the contact does not move the date, because that is not hearing from the contact.

The sort button at the top of the list opens a menu with two parts.
**Sort By** offers **First Name**, **Last Name**, and **Last Heard From**.
**Order** offers **Ascending** and **Descending**.
A new browser starts on **Last Name**, **Ascending**, and the browser remembers the last choice.

The first name is the first word of the name and the last name is the last word.
A name written with a comma, such as `Lovelace, Ada`, is read as last name first.
Under either name sort the list is divided by letter, and a `#` section holds names that start with a digit or a symbol.
**Last Heard From** starts with the newest date first, and a contact with no date sorts last in both orders.

## Search the list

The search bar at the top narrows the list as text is typed.
Plain text matches a contact's name or any of its identities.
When the match is an identity, that identity appears under the name in the row.

The list also takes search words such as `name:`, `identity:`, `group:`, `messages:`, and `last-message:`.
[Search](/docs/user/features/messages/search/) lists every word and the lists it works on.

## One contact

Selecting a row opens the contact in the right pane. The pane shows:

- The contact's name, with the **Edit name** pencil beside it.
- **Contact groups**: the [Contact Groups](/docs/user/features/contacts/contact-groups/) the contact is in, or **No groups**.
- A table with one row per identity.
- **Add identity**, **Move to trash**, and a close button.

The identity table has these columns:

| Column | What it shows |
|---|---|
| **Service** | **Text message**, **Email**, or **WhatsApp** |
| **Identity** | The phone number, email address, or username |
| **First heard from** | The date of the first message the contact sent from this identity |
| **Last heard from** | The date of the last message the contact sent from this identity |
| **Conversations** | How many conversations the identity takes part in |
| **Direct messages** | How many messages the contact sent from it in one-to-one conversations |
| **Group messages** | How many messages the contact sent from it in group conversations |

The message counts and the two dates cover only messages the contact sent.
The account holder's own replies, and other people's messages in a shared group, are not counted.
A count of zero shows as a dash.
The **Summary** row at the bottom gives the earliest date, the latest date, and the sum of each count.

A number under **Conversations** is a link.
It opens **Messages** narrowed to the conversations that identity takes part in.

## Rename a contact

**Edit name** turns the name into a text field.
Enter saves the name.
Escape, or a click anywhere else, cancels the edit.

An empty name is refused, so a name can be changed but not removed this way.
A typed name is the contact's name from then on: no later import replaces it.
An Address Book load replaces it only when the file gives the contact a different name.

## Add or remove an identity

**Add identity** opens a dialog with two fields.
**Service** is **Text message**, **Email**, or **WhatsApp**.
**Identity** is the phone number or the email address.

- A phone number must have 7 to 15 digits. Spaces, dots, dashes, and parentheses are accepted.
- An email address must have the form `name@example.com`.
- The same number on **Text message** and on **WhatsApp** is two identities, so both can be added.
- An identity the contact already has is refused with **This identity is already in the list.**
- An identity that belongs to another contact with a name is refused, because an identity belongs to at most one contact. It must be removed from the other contact first. An identity on a contact with no name moves to this one, and a contact with no name left with no identity goes.

The trash icon at the end of an identity's row removes it, after the **Remove identity from contact?** dialog is confirmed with **Remove identity**.
Removing an identity takes it off the contact and deletes no messages.
An identity that is in a conversation goes to a new contact with no name, which is [Unknown](/docs/user/features/contacts/unknown/), so the person can still be found and named.
An identity in no conversation is deleted.
A contact left with no identity is Unknown too.

Message Crate has no command that merges two contacts.
Two contacts that are one person are joined by removing the identities from one and adding them to the other.

## Several contacts at once

Pointing at the circle at the left of a row turns it into a checkbox.
Shift and a click checks or unchecks every row between the last one clicked and this one.
The checkbox in the list header checks or unchecks every contact the list shows.

While any row is checked, a click on a row checks or unchecks it and does not open the contact.
The right pane shows a table headed with the count, such as **3 contacts selected**, and **Clear contacts** unchecks every row.

| Column | What it shows |
|---|---|
| **Contact** | The name, or the first identity in italics |
| **First heard from** | The date of the first message the contact sent |
| **Last heard from** | The date of the last message the contact sent |
| **Conversations** | How many conversations the contact takes part in |
| **Direct Messages** | How many messages the contact sent in one-to-one conversations |
| **Group Messages** | How many messages the contact sent in group conversations |

Changing the search or opening a Contact Group unchecks every row, because the checked contacts might no longer be on the screen.

The **Contact Groups** button in the toolbar puts the checked contacts, or the one open contact, in and out of groups.
[Contact Groups](/docs/user/features/contacts/contact-groups/) describes it.

## What an import does to contacts

An import makes a contact for every person it meets.
A phone number the backup has no name for becomes a contact with that identity and no name.
A person the backup names without an address becomes a contact with a name and no identity.
Both are [Unknown](/docs/user/features/contacts/unknown/) until the missing half is supplied.

A group conversation is not a person, so the id a backup gives a group never becomes a contact.
Only the people in the group do.

One phone number is one person on every service.
When a number arrives as an iMessage address and again as an SMS address, both land on the same contact.

## Where a contact's name comes from

Three things can name a contact:

1. **A name typed in Edit name.** It replaces whatever name the contact had.
2. **An Address Book load.** The file is the same person typing in a spreadsheet, so a name in the file replaces whatever name the contact had. A blank name in the file leaves the name alone.
3. **An import.** It names only a contact that has no name.

Because an import names only a nameless contact, the first backup that knows a name wins.
A later backup that spells the name differently does not change it.

## Move a contact to the Trash

**Move to trash** takes the contact out of the Contacts list at once, with no confirmation, because nothing is deleted.
The contact's conversations and messages stay where they are.

**Trash** in the left panel lists trashed contacts under **Contacts**, each with two buttons:

- **Restore** puts the contact back in the Contacts list.
- **Delete** removes the contact's name, details, and Contact Group memberships. The identities stay in their conversations, the messages stay, and the contact becomes Unknown.

**Delete** is disabled for an account without the **Delete** permission.

A trashed contact stays in the Trash until an import meets one of its identities.
The import then discards the trashed contact with every identity it had and makes a new contact from the backup, as a first import would.

[Trash](/docs/user/features/messages/trash/) covers the Trash as a whole, including **Empty Trash**.

## The Address Book

The Address Book is a CSV file of contacts and their identities, for editing many contacts at once in a spreadsheet.
Contacts themselves arrive with message imports.
The file is how fifty Unknown contacts are named in one sitting, how a wrongly linked number is moved, and how Contact Groups are filled in bulk.

The work has three steps: export the file, edit it, load it back.

### Export the file

**Export** sits above the Contacts list, beside the sort menu.
It writes the contacts the list is showing:

- With rows checked, the file holds the checked contacts.
- With a search or a Contact Group open, the file holds the contacts that match. **Unknown** in the left panel followed by **Export** gives a file of every contact that still needs a name.
- With neither, the file holds every contact.

The file is named `address-book.csv`.
A browser saves it to its downloads, and the desktop app asks where to save it.

### What the file holds

The file has one row for each identity, and six columns.

| Column | What it holds |
|---|---|
| `contact_id` | The number Message Crate knows the contact by. Rows with the same value are one contact. |
| `display_name` | The contact's name. Blank for a contact with no name. |
| `groups` | The contact's Contact Groups, separated by `;`. A Contact Group's name can't hold `;`, so the cell never needs escaping. |
| `service` | `phone` for a text message identity, `whatsapp` for a WhatsApp one. |
| `identity_type` | `phone`, `email`, `username`, or `other`. |
| `identity` | The phone number, email address, or username. |

A contact with three identities is three rows, and its name and Contact Groups repeat on each.
A contact with no identity is one row with the last three columns blank.

```csv title="address-book.csv"
contact_id,display_name,groups,service,identity_type,identity
12,Ada Lovelace,Family;Work,phone,phone,'+15555550100
12,Ada Lovelace,Family;Work,whatsapp,phone,'+15555550100
12,Ada Lovelace,Family;Work,phone,email,ada@example.com
31,,,phone,phone,'+15555550142
```

Contact 31 above is Unknown: it has an identity and no name.

A cell that starts with `=`, `+`, `-`, `@`, a tab or a carriage return is written with a `'` in front, in every column.
A spreadsheet runs such a cell as a formula otherwise, and a name can come from a backup or, on WhatsApp, from the other person.
The `'` keeps the cell as text, so a phone number keeps its `+`.
Loading the file takes that `'` off again, whether the spreadsheet kept it or dropped it when it saved.
LibreOffice Calc shows the `'` in the cell, and it can stay there: the load reads the cell the same with it or without it.

### Edit the file

Any spreadsheet opens the file.
The `identity` column should be kept as text, because a spreadsheet that reads `+6591234567` as a number drops the `+`, and the number is then read as a US one.

- **Name a contact.** Fill in `display_name` on its rows.
- **Put a contact in a Contact Group.** Add the group's name to `groups`. A name that matches no Contact Group creates one.
- **Give a contact another identity.** Add a row with the same `contact_id`.
- **Move an identity to another contact.** Change the row's `contact_id` to the other contact's.
- **Make a new contact.** Leave `contact_id` blank. To give a new contact several identities, put the same made-up word, such as `new-1`, in `contact_id` on each of its rows.
- **Leave a contact alone.** Delete its rows from the file. A contact the file does not mention is never changed.

The rows of one contact must agree on `display_name` and on `groups`.
A blank cell agrees with anything, so the name needs filling in only once.

### Load the file

The load is in **Settings**, on the **Profile** tab, under **Address book**.
**How to load it** has two choices:

- **Append** creates the contacts the file adds, renames the ones it holds, and adds the identities and Contact Groups it lists. It removes nothing.
- **Edit** does the same, then makes each contact in the file hold exactly the identities and Contact Groups its rows list. A row taken out of the file takes that identity off the contact. The identity stays in its conversations, which show the number again in place of the name.

**Choose a file** takes a `.csv` file of at most 8 MB.
A larger file is refused with **That file is larger than 8 MB.**

When the load finishes, the section lists what it changed: contacts created, updated and deleted, identities added, moved and removed, and Contact Groups created.
A file exported and loaded straight back changes nothing, and every count is zero.

No load deletes a contact, with one exception: a contact left with neither a name nor an identity is deleted, because nothing could reach it.

### When a load is refused

A file with a mistake in it is refused whole, and nothing is loaded.
The section lists each row at fault with its row number and the reason, so the fix is made in the spreadsheet and the file loaded again.
Row 1 is the header.

A load refuses:

- A row with more cells than the header, most often a name with a comma that is not in double quotes. A row with fewer cells reads the missing ones at its end as blank.
- A phone number that is not 4 to 15 digits, or holds anything but digits, spaces, and `+ - ( ) .`
- An email address without exactly one `@` and text on both sides of it.
- A `service` or `identity_type` that is not one of the values in the table above.
- Two rows of one contact that give different names or different Contact Groups.
- One identity listed under two contacts.
- A Contact Group name the product reserves, such as `Unknown`.
- An identity that belongs to a named contact the file does not mention.

The last rule protects a named contact from losing an identity unseen.
An identity moves freely from a contact with no name, which is what naming the Unknown contacts needs.
To move an identity between two named contacts, both must be in the file.

Message Crate does not read a phone's vCard (`.vcf`) file.
