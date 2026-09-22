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

**Message**:
One thing sent or received inside a Conversation: who sent it, when, what
it said, and what was attached. A message can be read and pointed at on its
own, and picked by hand for an Export Run, but it is never acted on alone:
tagging, trashing and deleting happen to its Conversation.
_Avoid_: Text, Post, Item, Row

**Asset**:
The bytes of one attachment, stored once and named by the hash of its
contents, so the same file sent in ten messages is one asset. An asset is
the only thing in the vault addressed by a hash rather than a row number,
because the file exists before the vault does and its contents are its
identity.
_Avoid_: Attachment file, Blob, Media, Upload

**Import Run**:
One attempt to bring messages from a backup into the vault, recorded
permanently whether it succeeded, failed, or was cancelled. An account has at
most one running at a time. A run moves through its Stages and stops at each
Review until the person approves or cancels it. The record belongs to the
account and cannot be deleted by the person; anything in the interface that
merely points at a run is a shortcut and can be. The HTTP interface creates
one with `POST /v1/imports`; it is not a session, which is the logged-in
account's token.
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
or a username on a service. A handle belongs to at most one contact. The id
a source gives a group conversation (`chat1000000005`) reaches no person: the
vault keeps it to key the conversation, and it never belongs to a contact.
Only the people in the group do. Handle is the word in the code and the
database; the app and the published documentation call the same thing an
**Identity**.
_Avoid_: Address, Number

**Unknown**:
The Contact Group the vault computes from contacts that have no name or no
handle. It has no members of its own and empties as a person names people.
_Avoid_: Unnamed, Unresolved, Uncategorised

**Trash**:
Where a person sets aside conversations and contacts they do not want to
see. Membership is explicit, nothing in it is deleted, and a trashed
conversation can still be opened and read. Lists leave the trash out unless
asked to show it. A trashed contact stays set aside until an import meets
one of its handles: the import then discards the trashed contact together
with every handle it had and makes a new contact from the backup, as a first
import would. The trash is the only door to permanent
deletion; something must be trashed before it can be deleted, one item at a
time or all at once with Empty Trash.
_Avoid_: Deleted, Archive, Hidden, Bin

### Logging in

**Account**:
One person's store inside a vault: the login they log in with, and the
conversations, contacts, and handles that store holds. A vault holds many
accounts and keeps each one's data isolated from the others, so nothing an
account holds is visible to another. An account is not finished until
profile setup finishes.
_Avoid_: Login, Profile, Tenant, Workspace

**Vault Owner**:
The vault's administrator: the one account that manages a vault's other
accounts and its global settings, and the only account that has no vault of
its own. The vault owner creates, disables and deletes accounts, resets their
passwords, deletes their message data, and decides whether strangers may create
an account. The owner monitors the vault through metadata: counts and totals of
messages, contacts and attachments, each account's imports and exports, when
it last logged in, and attachment file names and sizes. The owner never reads
content: a message's text, an attachment's bytes, or a contact's name and
handles. The full line is in `docs/adr/0008`. There is exactly one, it cannot
be deleted, and no other account can be given its powers.
_Avoid_ as its name: Admin, Administrator, Superuser, Root. The role is an
administrator's; the account is called the vault owner, because exactly one
exists and it is whoever claimed the vault.

**Owner Home**:
The screen the vault owner lands on at login and works from, the way any
other account lands in Messages. It has the frame every account sees: the
product name, a search bar and the account button across the top, over a
side panel and a content pane. The side panel lists Dashboard, Settings, User
Accounts, Activity and Logs; Dashboard, Activity and Logs are named and hold
nothing yet. The search bar narrows User Accounts by username or
preferred name. User Accounts lists every account, the owner's own first,
each by username with its preferred name, its status and its last login. There the owner adds
accounts. An account's name opens that account's Settings, the screen its
holder sees, where the owner sets its password, its status (active or
disabled) and its permissions (import, export, delete), and deletes its
messages or the account. The account holder reads the same status and
permissions under Settings, Account, and changes none. The owner sets the
account's display name, time zone and identities on Profile as the holder does,
which is not the holder's own profile setup, and reads there its last login and
the app it connects with. The owner reads the Storage tab as the holder sees
it, without seeing inside it. The owner's own row opens the owner's own Settings,
which is also where the account button's Settings goes. An owner's password
reset sets the password and nothing more: it does not end the person's
session and does not make them choose a new one.
_Avoid_: Console, Dashboard, Admin, Admin panel

**Claiming**:
Making a vault's owner. A vault with no owner is unclaimed and offers only
the Create Vault Owner screen; claiming it is what that screen does. A
claimed vault is closed or open, depending on whether it lets strangers
create their own accounts.
_Avoid_: Setup, First run, Provisioning

**Session**:
One account's logged-in state, made by logging in with the account's
password and ended by logging out or by expiry. There is one per logged-in
account, and it is what the browser and the desktop app hold between
requests. It is not an API token: a token is a named, scoped credential the
person makes for a program, and it never logs in. Nothing else on the
product is a session; the record of an import attempt is an Import Run.
_Avoid_: Login, Auth, Import session, Token

**API Token**:
A named credential an account makes so a program can act for it without
logging in, limited to the scopes the person chose: importing, exporting, or
deleting. A program holding one can bring messages in, or take them out
through an Export Run it starts, but it can never browse: reading messages
outside a run needs a Session. The secret is shown once when the token is
made; afterwards the account sees only its name, a masked hint, and when it
was last used.
_Avoid_: App password, Key, Credential, Session

**User**:
The person operating Message Vault, in the browser or in the desktop app. A
user has an account in the vault, and "user" is the colloquial word for that
account: User names the person at the keyboard, Account names the record
they log in to and the data it holds.
_Avoid_: Member, Operator, End user

### Moving messages in and out

**Export**:
Moving messages out of the vault into files on disk, in a format the
person chooses. It reads the vault, never a phone backup. Every export is
recorded as an Export Run.
_Avoid_: Extract, Pull, Download

**Export Run**:
One attempt to move messages out of the vault, recorded permanently whether
it completed, failed, or was cancelled. The record holds what was asked for
and how much matched, never what the messages said. A person asks in one of
three ways: everything the account holds, whatever a search currently shows,
or conversations and messages they have picked by hand. Exporting everything
is still an Export Run; it is not called a backup, because a backup is the
phone's file that Import reads.
_Avoid_: Export Job, Backup, Download

**Convert**:
Rewriting a folder of already-exported files into a different format,
reading neither the original backup nor the vault. Export uses it for any
format other than JSON Lines. As an operation a person starts on a folder of
their own it is an advanced tool most people never need, so it lives under
Settings rather than beside Import and Export.
_Avoid_: Reexport, Transcode, Reformat

**Stage**:
One of the three parts of an Import Run, in order: **Staging** reads the
backup and copies its messages and original attachments into the Staging
Directory; **Media** converts or compresses the staged attachments, and
exists only when the person asked for it; **Upload** writes the staged
messages and attachments into the vault. A run shows its stages as one list
that fills in as it goes.
_Avoid_: Step, Phase, Pass, Gate

**Staging**:
Writing messages and their attachments into the Staging Directory so a
later step can read them back. It is one operation wherever it runs: the
first Stage of an Import Run, and the write that gives Convert its input.
A conversation is complete on disk or absent, never half written, so an
interrupted Staging resumes by skipping what it already wrote.
_Avoid_: Transcode, Write path, Write queue

**Review**:
A stop inside an Import Run where the run shows what it has staged and waits
for the person to approve or cancel it before spending more time or touching
the vault. There are at most two: the Staging Review after Staging, and
the Media Review after Media. A waiting review reads "Awaiting approval".
Approving continues the run; cancelling ends it and deletes what was staged.
A run left at a review keeps waiting, on another screen or after the app is
closed, until the person decides. The stop is named for what the person does
there; "approve" stays the word for the decision that continues the run.
_Avoid_: Gate, Approval (for the stop), Checkpoint, Confirmation, Deny

**Staging Directory**:
The folder where Message Vault writes intermediate files that neither the
person nor the vault keeps — a backup being prepared for import, or JSON
Lines waiting to be converted into the format an export asked for. It is
deleted when the job succeeds or is cancelled, the import log and resume
journal with it; a failed import leaves it in place, since the staged files
are what a retry reads.
_Avoid_: Import Staging Directory, Temp Folder, Working Directory

Extract is not a word for something a person does. It survives only as the
internal name of the desktop command that reads a backup during Import.

### Versions

**Product Version**:
The number a release of Message Vault is named by, such as `0.9.0`. The
vault, the desktop app and the website carry the same one. An app whose
Product Version differs from its vault's is flagged to the person and to the
vault owner, and is served all the same.
_Avoid_: Release, Release number, App version

**Build**:
A Product Version together with the commit it was built from, such as
`0.9.0+343fe0d8`, with `.dirty` added when the source held uncommitted
changes. A build made from a release tag is the Product Version alone. A
screen shows the Build under the plain label "Version".
_Avoid_: Revision, Build number, Full version

**Schema Fingerprint**:
The number derived from the vault database's table definitions, which the
vault stamps into its database. It is not the `schema_version` of the shared
chat file format, which is a separate number kept by hand.
_Avoid_: Schema version, Schema hash, Database version
