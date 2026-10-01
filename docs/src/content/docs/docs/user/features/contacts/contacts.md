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

The list also takes search words such as `name:`, `handle:`, `group:`, `messages:`, and `last-message:`.
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
A typed name is the contact's name from then on: no later import and no address book load replaces it.

## Add or remove an identity

**Add identity** opens a dialog with two fields.
**Service** is **Text message**, **Email**, or **WhatsApp**.
**Identity** is the phone number or the email address.

- A phone number must have 7 to 15 digits. Spaces, dots, dashes, and parentheses are accepted.
- An email address must have the form `name@example.com`.
- The same number on **Text message** and on **WhatsApp** is two identities, so both can be added.
- An identity the contact already has is refused with **This identity is already in the list.**
- An identity that belongs to another contact is refused, because an identity belongs to at most one contact. It must be removed from the other contact first.

The trash icon at the end of an identity's row removes it, after the **Remove identity from contact?** dialog is confirmed with **Remove identity**.
Removing an identity unlinks it from the contact and deletes no messages.
A contact left with no identity is [Unknown](/docs/user/features/contacts/unknown/).

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

Three things can name a contact, and they rank in this order:

1. **A name typed in Edit name.** Nothing replaces it.
2. **An address book load.** It replaces a name an import supplied and leaves a typed name alone.
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

An Address Book load puts names to the phone numbers already in the account.
It lives in **Settings**, on the **Profile** tab, under **Address book**.

**Choose a file** takes a `.vcf` file or a vCard `.csv` file of at most 8 MB.
A larger file is refused with **That file is larger than 8 MB.**
Any other file type is refused with **Choose a .vcf or .csv file.**

The load reads names and phone numbers only.
A card with no phone number is skipped, because a name with no number matches no message.

For each card, the load looks for a contact that already has one of the card's phone numbers:

- A contact with no name, or with a name an import supplied, takes the card's name.
- A contact with a typed name keeps it.
- A card that matches no contact makes a new contact.

When the load finishes, the section reports what it read, such as **Loaded 120 contacts and 134 phone numbers.**
A number the load could not read with certainty is counted in the same line as a number that needs a look.

Loading a file again updates the contacts earlier loads made.
A contact an earlier load made that the new file no longer lists is deleted.
Contacts found in messages, typed names, and Contact Groups are left as they are, and a load never creates a Contact Group.
