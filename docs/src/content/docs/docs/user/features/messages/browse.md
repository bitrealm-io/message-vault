---
title: Browse messages
description: The left panel, the conversation list, and the open conversation, which are the same in the browser and the desktop app.
---

**Messages** in the left panel lists the Account's Conversations.
Selecting a Conversation opens its messages on the right.
The screen is the same in the browser and in the desktop app.

## Left panel

| Label | Shows |
|-------|-------|
| **Messages** | The Conversations that are not in the Trash |
| **Contacts** | The Contacts ([Contacts](/docs/user/features/contacts/contacts/)) |
| **Trash** | The Conversations and Contacts set aside ([Trash](/docs/user/features/messages/trash/)) |
| **Import** | Brings a backup into Message Crate ([Import](/docs/user/features/messages/import/)) |
| **Export** | Writes messages to files on disk ([Export](/docs/user/features/messages/export/)) |
| **Contact Groups** | **Unknown**, each Contact Group by name, and **No group** ([Contact Groups](/docs/user/features/contacts/contact-groups/)) |
| **Saved Searches** | Each Saved Search by name ([Saved Searches](/docs/user/features/messages/saved-searches/)) |
| **Message Tags** | Each Message Tag by name, and **No tag** ([Message Tags](/docs/user/features/messages/message-tags/)) |

**Import** and **Export** appear only in the desktop app, because both work with files on the computer.
They sit under a second heading that is also named **Messages**.

A heading with an arrow beside it folds its rows away and back.
The browser remembers which headings are folded.
The **+** beside **Contact Groups**, **Saved Searches**, and **Message Tags** creates one.

The right edge of the left panel drags to make the panel wider or narrower.

**Settings** and **Log out** are in the account menu, the round button at the top right.
The username of the logged-in account is always shown beside that button, on every screen and for every account.

## The conversation list

Each row is one Conversation.
A group chat and a one-to-one Conversation appear in the same list.

A row shows four things:

- The title. A Conversation without a title shows the other person's name, or every participant's name for a group chat.
- The number of participants, for a group chat.
- How the messages travelled. iMessage, SMS, and MMS all read **Text Message**, and WhatsApp keeps its own name.
- The days of the first and the last message.

The list opens with the most recent Conversation first.
The sort button above the list offers **Date** or **Messages** under **Sort By**, and **Ascending** or **Descending** under **Order**.
**Messages** orders by how many messages a Conversation holds.
The browser remembers the choice.

The count at the bottom of the list reads like `1–12 of 382`: the rows on screen, then the number of Conversations in the list.
More rows load as the list scrolls.

The search box at the top narrows the list.
`kind:group` leaves only group chats, and `source:whatsapp` leaves only Conversations with messages from a WhatsApp backup.
[Search](/docs/user/features/messages/search/) lists every search word.

Each row has a checkbox, and the checkbox above the list ticks every row loaded so far.
The tag button acts on the ticked rows.
[Message Tags](/docs/user/features/messages/message-tags/) describes it.

## The open conversation

The header shows the title, or the number of participants for an untitled group chat.
Selecting the title folds the participant names away and back.
A participant's name opens that Contact.

Below the names, the header shows the kind of backup the messages came from, the months of the first and the last message, and the number of messages.
Up to three buttons follow:

- **Sources** lists each backup the Conversation's messages were imported from, with a message count for each.
- **Move to trash** moves the Conversation to the Trash and returns to the list.
- **Make a Contact Group** makes a Contact Group from the people in a group chat. It appears only when the group chat has two or more participants who are Contacts, because a Contact Group holds Contacts.

Messages appear 50 to a page.
**Previous** and **Next** turn the page, and the label between them reads like `Messages 1–50 of 1395`.

The year buttons in the header narrow the Conversation to one year.
**All** shows every year again, and so does selecting the active year a second time.

**Find in conversation…** searches the text of this one Conversation.
The page then shows only the matching messages, and the label beside the box reads like `1 of 14 in this conversation`.
The arrow buttons and the Enter key step through the matches.
With a year selected, the find covers that year only.
