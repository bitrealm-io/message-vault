---
title: Message Tags
description: A Message Tag is a name marked onto Conversations, listed in the left panel and usable in a search.
---

A Message Tag is a name marked onto Conversations.
**Message Tags** in the left panel lists every tag in alphabetical order, with **No tag** below them.

A tag marks whole Conversations, never single messages.
A Conversation can carry any number of tags, and a tag stays on a Conversation until it is taken off.
That separates a Message Tag from a [Saved Search](/docs/user/features/messages/saved-searches/), which holds a search and no Conversations, and from a Contact Group, which holds Contacts.

Message Tags belong to the Account and are stored on the server.
The same tags appear in every browser and desktop app logged in to that Account.

## Creating a tag

The **+** beside **Message Tags** opens **Create message tag**.
**Create** makes the tag and opens its list, which is empty until a Conversation carries the tag.

A tag name has three rules.

- It holds 80 characters at most.
- Two tags in one Account can't share a name, and letter case does not make a name different, so **Work** and **work** are one name.
- It can't be one of the names Message Crate keeps for itself: `home`, `contacts`, `threads`, `thread`, `all`, `excluded`, `unassigned`, `trash`, `tags`, `tag`, `no-tag`, `no tag`, `groups`, `group`, `labels`, `label`, or `none`. `none` is kept because `tag:none` searches for Conversations with no tag. Such a name is refused with a message like "trash" is a reserved Message Tag.

## Putting a tag on Conversations

The tag button is the button with a tag on it, at the top of the pane that shows the open Conversation.
It is there on **Messages**, on a tag's list, and on **No tag**.

The button acts on the Conversations whose checkboxes are ticked in the conversation list.
With no checkbox ticked, it acts on the open Conversation.
With neither, the button is disabled, because there is nothing to tag.

The button opens a list of every tag, each with a checkbox.

| Checkbox | Means |
|---|---|
| Ticked | Every chosen Conversation carries the tag |
| Dash | Some of the chosen Conversations carry it |
| Empty | None of them carries it |

Ticking an empty or dashed box puts the tag on every chosen Conversation.
Clearing a ticked box takes the tag off every one of them.

Three more controls sit in the same list.

- **Search tags…** narrows the list of tags by name.
- **Create tag** makes a new tag and puts it on the chosen Conversations in one step.
- **Clear all** takes every tag off the chosen Conversations.

A row of the conversation list does not show the tags its Conversation carries.
The ticked boxes behind the tag button are where the tags of the open Conversation show.

## Seeing the Conversations a tag marks

Selecting a tag in the left panel shows the conversation list narrowed to the Conversations that carry it.
**No tag** shows the Conversations that carry no tag at all.

The search box narrows that list further.
A search typed on a tag's list stays inside the tag.

A Conversation in the Trash keeps its tags, and stays off a tag's list until it is restored, because every list leaves the Trash out.

## Tags in a search

The search word for a Message Tag is `tag:`.
It works on Contacts, Conversations, and Messages.

| Search | Finds |
|---|---|
| `tag:Work` | Conversations that carry the tag **Work** |
| `tag:"Book Club"` | The same for a name with a space, which needs quotes |
| `tag:Hol*` | Conversations that carry a tag whose name starts with Hol |
| `tag:none` | Conversations that carry no tag |
| `-tag:Work` | Conversations that don't carry **Work** |
| `tag:Work tag:Urgent` | Conversations that carry both tags |

On Contacts, `tag:Work` finds the Contacts who are in a Conversation that carries the tag.
[Search](/docs/user/features/messages/search/) lists every search word.

## Renaming and deleting a tag

Pointing at a tag in the left panel shows a button with three dots.
It opens a menu with two entries.

**Rename…** opens **Rename tag**.
The new name follows the same three rules, and every Conversation that carried the old name carries the new one.
A Saved Search that names the tag is not updated, because a Saved Search stores the text of its search.

**Delete** asks for confirmation first, in a dialog that names the tag.
Confirming with **Delete** removes the tag and takes it off every Conversation that carried it.
The Conversations and their messages are not touched.
