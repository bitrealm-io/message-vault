# An import names the Contact

When a backup knows a person's name and the Contact Message Crate makes for them
has no name yet, the import puts that name on the Contact and marks it as
supplied by an import. A name the person types, or loads from an address
book, replaces an imported name. A later backup with a different spelling
does not. This reverses part of #286, which made every imported Contact
nameless and kept the backup's name as residue only.

## Why

#286 (31 August 2026) moved name resolution out of the exporters and into the
server, and decided that "the display name the source supplied stays on the
identity as residue rather than being promoted to a preferred name, because
the same number arrives spelled differently across backups." The effect was
that importing a phone which knew everyone's name produced a Contacts list of
"(unknown)" entries, with the real names stored in two hint fields: once per
participant per conversation, and once per handle, first import wins, never
updated, and not editable by anyone. The conversation list and the message
pane then read those hints with different joins and different precedence, so
one person could show two names on one screen.

A phone backup is an address book the person already curated. Refusing its
names turns every import into a naming chore and fills Unknown with people
an import could have named. The spelling worry is real but small: first
import wins, and a person can correct a name once. Marking the name as
imported keeps the address-book refresh rule from #286 intact, because a
refresh replaces only what the address book owns and a typed name still wins.

## Consequences

- Display name for a participant is one rule in one loader used by the
  conversation list, the message pane, and Export: the Contact's name, else
  what that backup called them in that conversation, else the handle.
- A handle counts as a Contact's the moment it is on the Contact, so naming
  someone renames them in every conversation at once.
- `contact_handles.name_alias`, the per-handle copy of the backup's name, is
  deleted. `participants.name_alias` stays as the record of what one backup
  said.
- `contact_name_mode` on import (`fill_missing`, `overwrite`, `as_is`) goes;
  the rule above is the only mode.
- Unknown keeps its meaning from CONTEXT.md: a Contact with no name or no
  handle. It gets smaller after an import instead of larger.
