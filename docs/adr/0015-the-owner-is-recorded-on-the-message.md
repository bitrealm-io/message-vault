# The owner is recorded on the message, not as a participant

An account's identities counted nothing after an import, because the counts
looked for the identity among a conversation's participants and no import ever
put the account holder there (#690). We decided the holder is never a
participant. The vault is read from the holder's side: every conversation in
an account is the holder's own, and the holder is logged in to it, so listing
them in it says nothing. Instead each message records which of the holder's
addresses it was sent from or received at, taken from the message where the
backup records it and from the backup's header where the backup has one owner.
An identity's counts are the messages held at it.

## Considered options

- **The holder as a participant**, which the `participants` table comment once
  anticipated. Rejected: every participant has a contact, so the holder would
  become a nameless contact holding every conversation, and the name loader,
  the conversation titles, the participant counts and the group bubble style
  would each have to leave the holder out again.
- **One owner identity per conversation.** Rejected: an iMessage conversation
  can use the holder's phone number for some messages and an Apple ID email for
  others, and one value per conversation cannot say which.

## Consequences

- A message's owner identity may be an address the account has not added. The
  import records it and never adds it to the account; linking it later shows
  its counts without a re-import.
- A backup that names no owner, such as a WhatsApp import today, leaves every
  message's owner identity empty, and its conversations count toward no
  identity.
- An owner identity is a `handles` row with no contact. It is the holder, not a
  person the import met.
