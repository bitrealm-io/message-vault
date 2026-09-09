# An import replaces a trashed Contact

A Contact in the Trash is set aside, not deleted, and until now an import
that met one of its handles attached the handle to it and left it in the
Trash, so a person who trashed someone and imported a newer backup months
later never saw that person in Contacts again. We decided the opposite: when
an import meets a handle that belongs to a trashed Contact, the import
discards that Contact together with every handle it had, then makes a new
Contact from the backup, as a first import would. A backup
that still holds the person is the person telling the vault they still talk
to them, and the surprise of a Contact that never comes back is worse than
the surprise of one that does.

## Considered options

- **Restore the trashed Contact**, name, Contact Group memberships and all.
  Rejected: it brings back edits the person made before setting the Contact
  aside, and it differs from what already happens after a permanent delete,
  where the next import names a fresh Contact.
- **Leave it in the Trash and report it** on the finished run. Rejected: the
  report tells the person about a thing they then have to go and fix.

## Consequences

- The new Contact carries only the handles the backup knew, where the same
  number on another service counts as known. A handle the trashed Contact
  had that the backup did not mention belongs to no Contact afterwards, so
  its conversations show under Unknown until the person merges them.
- A backup that holds the person under two different handles makes two
  Contacts, as a first import does; the trashed Contact that joined them is
  not consulted.
- The forecast of Contacts new to the vault, shown before an import runs,
  counts a trashed Contact's handle as new, because that is what the person
  is about to see.
- The old name and Contact Group memberships are gone for good.
- The Trash entry in CONTEXT.md no longer describes an import attaching to
  a trashed Contact. Deleting a Contact from the Trash is still different:
  the deleted Contact keeps its handles and becomes Unknown, so a later
  import names that one Contact instead of making a new one.
