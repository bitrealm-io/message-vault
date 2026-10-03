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

A switch above the list reads **Conversations** and **Messages**.
**Conversations** lists Conversations, as this section describes.
**Messages** lists single messages, as [the Messages list](#the-messages-list) describes.
The search box drives both.

Each row is one Conversation.
A group Conversation and a one-to-one Conversation appear in the same list.

A row shows four things:

- The title. A Conversation without a title shows the other person's name, or every participant's name for a group Conversation.
- The number of participants, for a group Conversation.
- How the messages travelled. iMessage, SMS, and MMS all read **Text Message**, and WhatsApp keeps its own name.
- The days of the first and the last message.

The list opens with the most recent Conversation first.
The sort button above the list offers **Date** or **Messages** under **Sort By**, and **Ascending** or **Descending** under **Order**.
**Messages** orders by how many messages a Conversation holds.
The browser remembers the choice.

The count at the bottom of the list reads like `1–12 of 382`: the rows on screen, then the number of Conversations in the list.
More rows load as the list scrolls.

The search box at the top narrows the list.
`kind:group` leaves only group Conversations, and `source:whatsapp` leaves only Conversations with messages from a WhatsApp backup.
[Search](/docs/user/features/messages/search/) lists every search word.

Each row has a checkbox, and the checkbox above the list ticks every row loaded so far.
The tag button acts on the ticked rows.
[Message Tags](/docs/user/features/messages/message-tags/) describes it.

## The Messages list

With **Messages** picked above the list, the search in the box lists every message it matches, from every Conversation, one row per message.
`from:Alice photo` lists the messages Alice sent with the word "photo" in them.
An empty search lists no messages.

A row shows:

- The Conversation's name, as the conversation list shows it, and the day the message was sent, in the Account's Time Zone.
- Who sent it, or **You** for a message the Account sent.
- The message's text, from shortly before the first matching word, with the words of the search in bold. A message with no text shows the names of its attachments.
- 📎 and a count, for a message with attachments.

The count above the list reads like `12,408 messages`.
More rows load as the list scrolls, up to the first 50,000.

The sort button above the list offers **Relevance** and **Date**.
**Relevance** puts the best match first, and is offered only when the search has plain words, which are what it ranks by; it is then where the list starts.
**Date** orders by when the message was sent, **Newest first** or **Oldest first**, and is where a search with no plain words starts, newest first.

Selecting a row opens its Conversation on the right at that message, highlighted, with the messages before and after it.
The list stays as it was, so another row opens the same way.

## The open conversation

The header is one line: the title, or the number of participants for an untitled group Conversation; how many people are in a group Conversation; the kind of messages, such as "Text Message", as the list names it; the months of the first and the last message; and the number of messages.
Three buttons follow: **Find**, **Jump to**, and **⋯**.

**⋯** holds:

- The people in the Conversation. A person who is a Contact opens that Contact.
- **Sources** lists each backup the Conversation's messages were imported from, with a message count for each.
- **Move to trash** moves the Conversation to the Trash and returns to the list.
- **Make a Contact Group** makes a Contact Group from the people in a group Conversation. It appears only when the group Conversation has two or more participants who are Contacts, because a Contact Group holds Contacts.

A Conversation opens at its newest message, at the bottom, the way a phone shows it.
Scrolling up loads older messages, and scrolling down loads newer ones, so every message of a long Conversation can be reached by scrolling.
A line with the day, such as `Thu, Jul 2`, separates the days; outside the current year it carries the year too, such as `Mon, Nov 29, 2021`.
Every message has its time under it.
In a group Conversation, the sender's name is above the first message of each run: a run ends at a new day, a new sender, or a gap of an hour or more.
A photo is shown at most 280 pixels on its long side, and selecting it opens it full size.

**Jump to** lists **Newest** and every year of the Conversation.
A year jumps to its first message, and scrolling either way keeps loading from there.

**Find** opens a box under the header that searches the text of this one Conversation.
Typing jumps to the newest match, highlighted, with the messages around it, and the label beside the box reads like `1 of 14`.
▲ moves to the older match and ▼ to the newer one; Enter does the same as ▲, and Shift+Enter as ▼.
Nothing is hidden: the whole Conversation stays in view around each match.
✕ closes Find and leaves the Conversation where it is.
