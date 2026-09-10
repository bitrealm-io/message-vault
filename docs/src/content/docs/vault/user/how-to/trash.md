---
title: Trash
description: Move a conversation or a contact to Trash, take it back out again, or delete it for good.
---

**Trash** in the sidebar holds conversations and contacts you have set aside. Nothing in it is deleted: a trashed item keeps all of its messages, handles and group membership, and it comes back exactly as it was. Trash is also the only door to deleting something for good — an item has to be in Trash before **Delete** or **Empty Trash** can remove it.

## Move something to Trash

Open a conversation and click **Move to trash** in its header. The conversation leaves the inbox, along with its messages, and no longer counts towards the message and conversation totals shown for the people in it.

For a contact, open the contact and click **Move to trash** in the drawer. The contact leaves the Contacts list. Their conversations stay where they are — trashing a person is not the same as trashing what you talked about.

A trashed contact stays set aside until an import meets one of their handles. A backup that still holds the person means you still talk to them, so the import discards the trashed contact — the name, the Contact Group memberships and every handle it had — and makes a new contact from the backup, the way a first import would. The new contact carries only the handles the backup mentions (the same number on another service counts as mentioned); a handle the trashed contact had that the backup does not mention belongs to nobody afterwards, and its conversations show under Unknown until you add it to a contact. To keep a person out of Contacts for good, delete them from Trash instead.

## Take it back

Click **Trash** in the sidebar. Trashed conversations are listed in the left column; click one and the pane shows it with a **Restore** button, next to a link to read the conversation before you decide. Trashed contacts are listed under **Contacts** in the pane itself, each row with its own **Restore**.

Restoring puts the item back where it came from. The row leaves Trash as soon as it does.

The search box narrows both lists at once while you are in Trash, so `ada` finds the trashed conversations and contacts that match it.

## Delete for good

Every trashed item also has a **Delete** button, and the pane has **Empty Trash** at the top. Each asks you to confirm first, and the dialog says what will happen, because Delete means two different things for the two kinds of item.

Deleting a conversation removes it and its messages from the vault. An attachment is stored once however many messages share it, so a photo that also appears in another conversation stays; a file only the deleted messages used goes with them.

Deleting a contact works the way Delete Contact works on a phone. The name and details you gave the person go, along with their Contact Group memberships, and the contact becomes Unknown again. The messages stay. Their conversations are untouched and now show the phone number or address instead of the name. A conversation is never deleted with a contact — to remove one, trash and delete it as a conversation.

**Empty Trash** does both at once: every conversation in Trash is deleted, and every contact in Trash becomes Unknown. It acts on all of Trash, not only on what a search is showing.

An Import Run's record under **Settings → Storage** does not change when things it brought in are later deleted. It describes the run as it happened.

The demo account can trash and restore but cannot delete, so its Delete and Empty Trash buttons stay disabled.

## Find trashed items from anywhere

Trash is a marker on the item, not a place its data was moved to, so search can ask about it directly. The `trashed:` operator works on Contacts, Conversations and Messages:

- `trashed:yes` — only trashed items
- `trashed:no` — only items that are not trashed, which is what every search does by default
- `trashed:any` — both

Searching `trashed:yes` on Contacts shows trashed contacts in the Contacts list, but they cannot be opened from there — restore or delete them from the Trash view.

See [Search](/vault/user/how-to/search/) for the rest of the operators.

## Remove everything at once

To remove all message content for an account without trashing each conversation first, use the danger-zone actions under **Settings → Account**. See [Settings](/vault/user/how-to/settings/).
