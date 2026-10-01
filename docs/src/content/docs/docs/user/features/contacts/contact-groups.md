---
title: Contact Groups
description: What a Contact Group is, how one is created and filled, the group an Import Run creates, and the group search word.
---

A Contact Group is a named collection of contacts.
A search can name the group, so a query can ask about a set of people without listing them.

A contact can be in any number of Contact Groups, and a group can hold any number of contacts.
A Contact Group collects people, not conversations: marking conversations is what a Message Tag does.

## Contact Groups in the left panel

The **Contact Groups** section of the left panel lists, from top to bottom:

1. **Unknown**, the group Message Crate computes. [Unknown](/docs/user/features/contacts/unknown/) describes it.
2. Every Contact Group the account has, by name.
3. **No group**.

Selecting a group opens the Contacts list narrowed to that group.
The search bar then narrows inside the group.
A group with no members shows **No contacts in this group**.

The section heading collapses and expands the section, and the browser remembers which.

## Create a group

The **+** button beside the **Contact Groups** heading opens the **Create contact group** dialog.
It takes a **Group name**, and **Create** makes the group and opens it.

A name follows three rules:

- It has at most 80 characters.
- It is not the name of another Contact Group. Letter case is ignored, so `Family` and `family` are one name.
- It is not a reserved name. `All`, `Contacts`, `Trash`, `Group`, `Unknown`, and `No group` are among them, because they name parts of the product. The dialog answers with a line such as **Trash is a reserved Contact Group**.

## Rename or delete a group

Pointing at a group in the left panel shows a **…** button with two commands.

**Rename…** opens the **Rename group** dialog.
The new name follows the same three rules.

**Delete** asks for confirmation first, in a dialog that names the group.
Confirming with **Delete** deletes the group.
The contacts that were in it are not deleted.
They lose that one membership.

**Unknown** and **No group** have no **…** button, because neither is a group a person made.

## Put contacts in a group

The **Contact Groups** button in the toolbar of the Contacts list sets membership.
It acts on the checked contacts, or on the one open contact when no row is checked.
With neither, the button is disabled.

The menu lists every Contact Group with a checkbox:

- A checked box means every one of those contacts is in the group.
- A box that is neither checked nor empty means some are and some are not.
- Selecting a box puts all of them in the group, or takes all of them out.

**Search groups…** at the top narrows a long list of groups.
**Create group** makes a new group and puts the contacts in it in one step.
**Clear all** takes the contacts out of every group they are in.

An open contact lists its groups under **Contact groups**, or **No groups**.

## Make a group from a group conversation

A group conversation is a set of people the account holder already gathered.
Its header offers **Make a Contact Group** when the conversation has at least two other people who are contacts.

The dialog, **Make a Contact Group from these people**, starts with the conversation's title as the group name.
**Create** makes the group and adds every other person in the conversation.
When the name matches an existing group, the people are added to that group.
The header then reads, for example, **Added 4 people to Book Club.**

The account holder is left out, because the account holder is never a participant.

## The group an Import Run creates

When an Import Run completes, Message Crate creates a Contact Group for the contacts the run changed.
The group is named for the source and the day the run finished, in UTC: `whatsapp import 2026-10-01`.
Each run gets a group of its own.
When a Contact Group already has that name, the new group takes the next free one: `whatsapp import 2026-10-01 2`, then `whatsapp import 2026-10-01 3`.
An import never adds contacts to a group that already exists, whether an earlier run or a person made it.

Its members are the contacts the run:

- created,
- gave a name to,
- added an identity to, or
- made in place of a trashed contact.

A run that changed no contact creates no group.

The membership is fixed when the run completes.
It records what the run brought in, so it does not change as contacts are renamed later.

The group is a shortcut to the run and behaves like any other Contact Group.
It can be renamed, filled, and deleted, and deleting it leaves the Import Run's record in place.

## No group

**No group** lists the contacts that are in no Contact Group.
When every contact is in one, it shows **Every contact has a group**.

Unknown counts as a group.
A contact in Unknown is therefore not in **No group**, even when it is in no group a person made.

## The `group:` search word

`group:` narrows a list to a Contact Group. It works on every list:

- On **Contacts**, it matches the contacts in the group.
- On **Messages**, it matches conversations and messages where someone in the group is a participant.

| Search | Meaning |
|---|---|
| `group:Family` | In the Contact Group named Family |
| `group:"Book Club"` | A name with a space needs quotes |
| `group:none` | In no Contact Group, the same contacts as **No group** |
| `group:unknown` | In Unknown |
| `groups:>5` | On **Contacts** only: in more than 5 Contact Groups |

[Search](/docs/user/features/messages/search/) covers the full language, including how words combine.
