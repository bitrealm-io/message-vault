# The Audit Trail outlives the account, and nobody edits it

The Audit Trail records what each user did on a Message Crate and when:
logging in, sessions ending, logins refused, Import Runs, Export Runs,
Address Book loads and exports, and the owner's changes to accounts (#619).
Its entries are never edited or deleted, by the owner or anyone else. When an
account is deleted, its entries stay: they are unlinked from the account, keep
the username they had, and gain an "account deleted" entry. An account holder
reads every entry about their own account, including what the owner did to
it and the logins refused for their username.

## Why

The record is there so that the owner, and each person hosted, can answer
"who did this, and when" afterwards. A record that goes with its account
fails exactly when it is needed: an account that exports everything and is
then deleted would leave no trace of the export. Today `imports` and `exports`
are "recorded permanently" yet cascade from `accounts`, so deleting an
account already loses them; the Audit Trail ends that.

A record the owner can clear is no check on the owner. The owner manages
other people's accounts (ADR 0008): sets their passwords, changes their
permissions, deletes their messages. The person hosted can already read their
status and permissions; the Audit Trail lets them read when those changed and
who changed them. "Delete messages" remains the way to clear an account's
data while its record stays.

## Considered and rejected

**Entries deleted with their account**, as runs were. It is the simpler
schema, and it was rejected because deleting the account is the owner's act,
and it would erase the record of what that account did.

**Owner's actions visible only to the owner.** It was rejected because an
entry about an account is about the person who holds it, whoever acted.

## Consequences

When an account is deleted, an entry keeps what was asked for and how much
matched, and loses what describes the person's messages.
An Export Run loses its search text, its hand-picked conversation and message
ids, and its list of messages.
An Import Run loses its issues, its form, its staging folder, its source
details and the addresses the backup sent from.
A run still open is closed as cancelled, and the account's live session ends
as revoked by whoever deleted it.
Refused logins for a
username that matches no account belong to no one and are deleted after 90
days, so that anyone who can reach the server cannot grow the record without
limit. The Demo Account is recorded like any other (ADR 0016), so
`reset-demo` leaves the old Demo Account's entries in place.
