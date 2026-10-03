---
title: Unknown
description: The Unknown entry under Contact Groups, which contacts land in it, and how a contact leaves it.
---

**Unknown** is the Contact Group Message Crate computes.
It holds every contact that is missing a name or missing an identity.

Nothing is added to Unknown by hand, and it has no members of its own.
Message Crate works it out from the contacts each time, so it empties as people are named.

## Which contacts are Unknown

A contact is Unknown when either of these is true:

- It has one or more identities and no name.
- It has no phone number, email address or username, whether or not it has a name. A name the backup gave with no address does not count as one.

Unknown is not a list of strangers.
A contact named `Ada` with no identity is Unknown, because nothing ties the name to a message.

## How a contact becomes Unknown

Four things put a contact in Unknown:

- **An import meets a phone number the backup has no name for.** The import makes a contact with that identity and no name, because an identity on no contact could never be found or named.
- **An import meets a person the backup names without an address.** The import keeps the name as the person's identity and makes a contact with that name. Nothing ties the name to an address, so the contact is Unknown until one is added.
- **An identity is removed from a contact.** An identity that is in a conversation goes to a new contact with no name. A contact whose last identity is removed keeps its name and has nothing left to reach it.
- **A contact is deleted from the Trash.** Its name and details go, its identities stay in their conversations, and the contact is Unknown again.

## What Unknown looks like

**Unknown** is the first entry under **Contact Groups** in the left panel.
It opens the Contacts list narrowed to the Unknown contacts.
The search `group:unknown` gives the same contacts on **Contacts**.

A contact with no name shows its first identity in italics where the name would be.
The italics keep an address from reading as a name someone gave the contact.

An open contact that is Unknown shows **Unknown** first under **Contact groups**.
The contact can be in other Contact Groups at the same time.

Unknown counts as a group, so an Unknown contact is not listed under **No group**.

Unknown has no **…** button and does not appear in the **Contact Groups** menu.
It cannot be renamed, deleted, or filled, because it is computed and not stored.

## Take a contact out of Unknown

A contact leaves Unknown the moment it has both a name and an identity.
[Contacts](/docs/user/features/contacts/contacts/) describes each of these actions.

- **Name it.** **Edit name** on the open contact gives a nameless contact a name.
- **Add an identity.** **Add identity** gives a named contact with no identity a way to be reached.
- **Name many at once with the Address Book.** **Export** on the Contacts screen, with **Unknown** open, writes the Unknown contacts to a CSV file. Filling in its `display_name` column and loading the file under **Settings** names them all, which suits a long Unknown list.
- **Move it to the Trash.** A number that is nobody worth naming, such as a short code, can be set aside with **Move to trash**.

A later import can also name an Unknown contact.
An import names a contact that has no name when the backup knows one.

When no contact is left in it, Unknown shows **Every contact has a name and a way to reach them**.
The entry stays in the left panel.
