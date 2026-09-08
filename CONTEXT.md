# Message Vault

Message Vault pulls conversations out of chat apps and stores them in a
self-hosted, searchable vault. This file is the glossary: the words we use
for things in the product, and the words we have decided not to use. It
holds no implementation details.

## Language

### Collections in the sidebar

Three concepts sit next to each other in the sidebar and are easy to
confuse. They are distinguished by what they collect and by whether
membership is explicit or computed.

**Contact Group**:
A named collection of contacts, referenced from a search so a query can
name a set of people without listing them.
_Avoid_: Group, Label, Contact List

**Saved Search**:
A named query, stored so it can be run again. It collects nothing and
holds no members; the same saved search returns different results as
messages arrive.
_Avoid_: Saved Group, Smart Group, Filter

**Message Tag**:
A name marked onto conversations. Membership is explicit, which is what
separates it from a Saved Search; it marks conversations rather than
people, which is what separates it from a Contact Group.
_Avoid_: Thread Tag, Conversation Tag, Label

### The archive

**Conversation**:
One exchange with one person or group, holding its messages and
participants. It is the unit the product acts on: tagging, trashing, and
searching all resolve to whole conversations even where the interface
speaks of messages.
_Avoid_: Thread, Chat

**Import Run**:
One attempt to bring messages from a backup into the vault, recorded
permanently whether it succeeded, failed, or was cancelled. The record
belongs to the account and cannot be deleted by the person; anything in
the interface that merely points at a run is a shortcut and can be. The HTTP
interface creates one with `POST /v1/imports`; it is not a session, which is
the signed-in account's token.
_Avoid_: Import Job, Import Session, Push

**Vault**:
One installation's store of accounts and their messages. A vault holds
many accounts, and each account's data is isolated from the others.
_Avoid_: Database, Instance, Server

**Time Zone**:
The zone an account shows every message time in, chosen when the account is
set up and changeable afterwards. A message records the instant it arrived
and nothing about where the phone was, so the account's zone is what turns
that instant into a clock reading, a day, and a year, in search and in the
thread alike. It is the person's zone, never the server's.
_Avoid_: Server time, Local time, Offset

### People

**Contact**:
One person the vault knows: a name, and the handles that reach them. A
contact is made for every person an import meets, and named from the backup
when the backup knew the name; a name the person types or loads from an
address book replaces one an import supplied. Deleting a contact removes the
name and details, not the conversations: the person's handles stay in their
conversations and the contact becomes Unknown again, the way deleting a card
from a phone's address book leaves its text threads in place.
_Avoid_: Card, Identity, Person record

**Handle**:
One address a person can be reached at: a phone number, an email address,
or a username on a service. A handle belongs to at most one contact.
_Avoid_: Identity, Address, Number

**Unknown**:
The Contact Group the vault computes from contacts that have no name or no
handle. It has no members of its own and empties as a person names people.
_Avoid_: Unnamed, Unresolved, Uncategorised

**Trash**:
Where a person sets aside conversations and contacts they do not want to
see. Membership is explicit, nothing in it is deleted, and a trashed
conversation can still be opened and read. Lists leave the trash out unless
asked to show it. A trashed contact is set aside, not gone: an import that
meets one of its handles attaches to it and leaves it in the trash. The trash
is the only door to permanent deletion; something must be trashed before it
can be deleted, one item at a time or all at once with Empty Trash.
_Avoid_: Deleted, Archive, Hidden, Bin

### Signing in

**Account**:
One person's store inside a vault: the login they sign in with, and the
conversations, contacts, and handles that store holds. A vault holds many
accounts and keeps each one's data isolated from the others, so nothing an
account holds is visible to another. An account is not finished until
profile setup finishes.
_Avoid_: Login, Profile, Tenant, Workspace

**Vault Owner**:
The one account that manages a vault's other accounts and settings, and the
only account that has no vault of its own. The vault owner creates, disables
and deletes accounts, resets their passwords, deletes their message data, and
decides whether strangers may sign up. The owner never reads a message: the
account list shows a name, a message count and a storage total, and nothing
of what those messages say. There is exactly one, it cannot be deleted, and
no other account can be given its powers.
_Avoid_: Admin, Administrator, Superuser, Root

**Claiming**:
Making a vault's owner. A vault with no owner is unclaimed and offers only
the Create Vault Owner screen; claiming it is what that screen does. A
claimed vault is closed or open, depending on whether it lets strangers
create their own accounts.
_Avoid_: Setup, First run, Provisioning

**User**:
The person operating Message Vault, in the browser or in the desktop app. A
user has an account in the vault, and "user" is the colloquial word for that
account: User names the person at the keyboard, Account names the record
they sign in to and the data it holds.
_Avoid_: Member, Operator, End user

### Moving messages in and out

**Export**:
Moving messages out of the vault into files on disk, in a format the
person chooses. It reads the vault, never a phone backup.
_Avoid_: Extract, Pull, Download

**Convert**:
Rewriting a folder of already-exported files into a different format,
reading neither the original backup nor the vault. Export uses it for any
format other than JSON Lines. As an operation a person starts on a folder of
their own it is an advanced tool most people never need, so it lives under
Settings rather than beside Import and Export.
_Avoid_: Reexport, Transcode, Reformat

**Staging Directory**:
The folder where Message Vault writes intermediate files that neither the
person nor the vault keeps — a backup being prepared for import, or JSON
Lines waiting to be converted into the format an export asked for. Its
contents are deleted when the job finishes.
_Avoid_: Import Staging Directory, Temp Folder, Working Directory

Extract is not a word for something a person does. It survives only as the
internal name of the desktop command that reads a backup during Import.
