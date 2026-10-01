---
title: Saved Searches
description: A Saved Search keeps a search under a name in the left panel, so it runs again with one selection.
---

A Saved Search is a search kept under a name.
**Saved Searches** in the left panel lists them in alphabetical order.

A Saved Search holds no Conversations.
It stores the text of the search, and the search runs afresh each time, so the same Saved Search shows different Conversations as messages arrive.
That separates it from a [Message Tag](/docs/user/features/messages/message-tags/), which marks the Conversations picked for it, and from a Contact Group, which holds Contacts.

Saved Searches belong to the Account and are stored on the server.
The same list appears in every browser and desktop app logged in to that Account.

## What selecting one does

Selecting a Saved Search opens **Messages**, puts the stored search in the search box, and narrows the conversation list to the Conversations that match.

A Saved Search always runs on the conversation list.
A search that uses a word the conversation list does not have, such as `from:`, is refused with a message when it runs.
[Search](/docs/user/features/messages/search/) marks the words that work on Conversations.

## Creating one

The **+** beside **Saved Searches** opens **New saved search**, which has two fields.

| Field | Holds |
|---|---|
| **Name** | The name shown in the left panel, 80 characters at most |
| **Query** | The search, typed as it would be in the search box |

**Query** starts empty.
The search in the search box is not copied in, so it has to be typed or pasted.

**Save** stays disabled until both fields hold text.
The search is stored as typed and is not checked, so a mistake in it shows only when the Saved Search runs.

Two Saved Searches in one Account can't share a name, and letter case does not make a name different.
A Saved Search with a name already in use is not created.

## Changing and deleting one

Pointing at a Saved Search shows a button with three dots.
It opens a menu with two entries.

**Rename…** opens **Edit saved search**, where both the name and the search can be changed.

**Delete** removes the Saved Search at once, without asking first.
It removes only the name and the stored search.
The Conversations it showed are not touched.

## What a Saved Search does not follow

A Saved Search stores text, so it does not follow a later rename.
`group:Family` matches nothing once the Contact Group **Family** is renamed or deleted, and `tag:Work` matches nothing once the Message Tag **Work** is.
The Saved Search still runs.
It shows an empty list until its search is edited.

## The Saved Search an import adds

An Import Run that brings in at least one message adds a Saved Search of its own.
It shows the messages that Import Run added.
Its name is the word Import, the kind of backup, and the day, such as **Import whatsapp 2026-10-01**.
A second import of the same kind on the same day gets the same name with a 2 after it.

Deleting that Saved Search does not delete the Import Run or its messages.
The run stays listed under **Settings**, on the **Storage** tab.
