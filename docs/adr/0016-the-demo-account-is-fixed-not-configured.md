# The Demo Account is fixed, not configured

Every new Message Crate starts with the Demo Account, which has no password,
so that a person can look at conversations before bringing their own (#971).
Anyone who reaches the Message Crate can enter it. What it may and may not do
is therefore fixed in the server, keyed on its account id, and is not a set of
permissions the owner can change: it may export and use the trash; it may not
import or delete for good; it never has a password; its status, its
permissions and its own identities cannot be changed. The owner can delete it
or reset it and nothing else.

## Why

An account with no password is open to whoever is at the login card. Its
limits are what keep a person's real messages out of made-up ones, and what
keep one visitor from emptying it for the next. If those limits were ordinary
permissions, an owner could switch Import on and a visitor could then put a
real backup into an account the user guide tells people to delete.

A fixed account also behaves the same on every Message Crate, in Docker and
in one the desktop app starts. The user guide and the "Explore Demo Account"
button on the login card both rely on that: the button shows whenever the
account exists, with no second condition to check.

## Considered and rejected

**An ordinary account seeded with restrictive permissions.** It needs no
special cases in the server, and it was rejected because the permissions
model has no way to say "no password, ever" or "these identities cannot
change", and because anything an owner can set, an owner can unset.

**A read-only Demo Account.** It would always be pristine, and it was
rejected because trying the product means naming a contact, tagging a
conversation and saving a search. Those changes stay until the owner resets
the account.

## Consequences

The server checks for the Demo Account wherever one of these acts is
requested: starting an import, both permanent deletes, setting a password,
changing status or permissions, and changing its own identities. Before this
decision the only such check refused deleting the account from inside it. A
new act that could damage Demo Data, or open the account to real messages,
needs the same check.
