# Message Crate

Message Crate pulls conversations out of chat apps and makes them searchable on
a server a person runs themselves. This file is the glossary: the words we use
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
participants. It is the unit the product acts on: tagging and trashing
resolve to whole conversations, and searching resolves to whole
conversations unless the person asks for messages. A Conversation the source app keeps as a group is a
**group conversation**; one with a single other person is a **one-to-one
conversation**. The search words for the two are `kind:group` and
`kind:direct`, short because they are typed; everything a person reads says
"group conversation" and "one-to-one conversation", never "group chat",
because "group" alone could also mean a Contact Group.
_Avoid_: Thread, Chat, Group chat, Direct conversation

**Message**:
One thing sent or received inside a Conversation: who sent it, when, what
it said, and what was attached. A message can be read and pointed at on its
own, and picked by hand for an Export Run, but it is never acted on alone:
tagging, trashing and deleting happen to its Conversation. Two records a
backup cannot tell apart, alike in conversation, sender, time, text and
attachments, are one message.
_Avoid_: Text, Post, Item, Row

**Deleted in the source app**:
A mark on a Message the person had deleted in the app it came from before
the backup was made, while the backup still held it. The message is kept,
shown muted with its text where the backup has it, and labelled with the
source, for example "Deleted in WhatsApp"; search finds it, and
`deleted:yes` or `deleted:no` narrows to or away from it. It is a different
thing from the Trash, which is what a person removes inside Message Crate.
_Avoid_: Trashed, Removed, Deleted on the phone

**Unsent**:
A mark on a Message its sender pulled back after sending it. The backup
usually holds no text for it, so it is shown as an empty muted bubble that
reads "Unsent". It is kept apart from Deleted in the source app, because the
sender took the message back for everyone rather than a person deleting
their own copy.
_Avoid_: Retracted, Recalled, Deleted

**Orphaned message**:
A Message the backup holds without recording which Conversation it was said
in. Orphaned messages one person sent sit in a Conversation of their own with
that person as its only participant, apart from the one-to-one Conversation
with them; the ones the account holder sent, whose recipient is not recorded,
sit together in one with no participants. Such a Conversation is neither
one-to-one nor a group.
_Avoid_: Stray, Unfiled, Lost message

**Asset**:
The bytes of one attachment, stored once and named by the hash of its
contents, so the same file sent in ten messages is one asset. An asset is
the only thing in the database addressed by a hash rather than a row number,
because the file exists before the database does and its contents are its
identity.
_Avoid_: Attachment file, Blob, Media, Upload

**Preview**:
A copy of an Asset in a format every browser can show, made by the server and
kept beside the original, which is never changed. A Conversation shows the
Preview of an attachment that has one; opening the attachment gives the
original. Not every Asset has a Preview.
_Avoid_: Derived asset, Converted file

**Import Run**:
One attempt to bring messages from a backup into Message Crate, recorded
permanently whether it succeeded, failed, or was cancelled. An account has at
most one running at a time. A run moves through its Stages and stops at each
Review until the person approves or cancels it. The record belongs to the
account and cannot be deleted by the person; anything in the interface that
merely points at a run is a shortcut and can be. The HTTP interface creates
one with `POST /v1/imports`; it is not a session, which is the logged-in
account's token.
_Avoid_: Import Job, Import Session, Push

**Message Crate**:
One installation of the product: the thing a person claims, owns, and logs
into. It holds many accounts and their messages, and each account's data is
isolated from the others. The product carries the same name. "A Message
Crate" or "this Message Crate" is one installation; "Message Crate" with no
article is the product.
_Avoid_: Vault, Crate on its own, Instance. The server is the running process
and the database is the store; neither is a name for the installation.

**Demo Data**:
The made-up conversations, contacts, and attachments every new Message Crate
starts with, so a person can look around before bringing their own messages.
It belongs to the Demo Account and to no one else. It sits beside real
accounts on the same Message Crate, and goes when the owner deletes the Demo
Account.
_Avoid_: Test data, Sample data, Demo mode

**Demo Account**:
The account that holds Demo Data, with the username `demo`. It has no
password and can never be given one, so anyone who reaches the Message Crate
can enter it. It may export, and move things to the trash and restore them; it
may not import or delete for good, so a person's own messages never land in it
and one visitor cannot empty it for the next. Its status, its permissions and
its own identities are fixed; the names, groups, tags and searches a visitor
makes in it stay until it is reset. The owner can delete it, or reset it to
how it started, and change nothing else about it.
_Avoid_: Demo user, Guest, Sample account

**Time Zone**:
The zone an account shows every message time in, chosen when the account is
set up and changeable afterwards. A message records the instant it arrived
and nothing about where the phone was, so the account's zone is what turns
that instant into a clock reading, a day, and a year, in search and in the
thread alike. It is the person's zone, never the server's.
_Avoid_: Server time, Local time, Offset

### People

**Contact**:
One person Message Crate knows: a name, and the identities that reach them. A
contact is made for every person an import meets, and named from the backup
when the backup knew the name; a name the person types or loads from an
address book replaces one an import supplied. Deleting a contact removes the
name and details, not the conversations: the person's identities stay in their
conversations and the contact becomes Unknown again, the way deleting a card
from a phone's address book leaves its text threads in place.
_Avoid_: Card, Identity, Person record

**Address Book**:
Message Crate's own CSV of contacts and their identities, one row per identity,
written by Export on the Contacts screen and read by Load under Settings. It
is for taking contacts out to a spreadsheet, correcting them, and putting them
back; contacts themselves arrive with message imports. A load is Append,
which adds and renames and removes nothing, or Edit, which makes each contact
in the file match its rows. A phone's vCard is not an address book Message
Crate reads.
_Avoid_: Contacts file, VCF, vCard

**Identity**:
One address a person can be reached at: a phone number, an email address,
or a username on a service. An identity belongs to at most one contact. The
id a source gives a group conversation (`chat1000000005`) reaches no person:
Message Crate keeps it to key the conversation, and it never belongs to a contact.
Only the people in the group do.

An identity means one of two things depending on whose it is. A contact's
identity means participation: this person took part in these messages. An
account identity means ownership: the messages sent from or received at this
address belong to the account holder, and Import uses the account's identities
to decide which messages are the holder's own rather than someone else's. A
backup's owner address becomes an account identity only when the person adds it;
an import never adds one.

A person a backup names with no address has an identity of type `other`
whose value is the name, one for each service. It is a sign the import was
incomplete, and a contact whose only identities are of that type is Unknown.
Every identity a conversation or a message uses is on a contact; one taken
off its contact goes to a new contact with no name.

Handle is the word in the code and the database for the same thing.
_Avoid_: Handle, Address, Number

**Participant**:
Another person in a Conversation, as the account holder sees it. The account
holder is never a participant: every conversation in an account is the holder's
own, so Message Crate knows they are in it without listing them. Which of the
holder's identities a message used is recorded on the message. Every
participant has exactly one identity, an identity of type `other` holding the
name when the backup named the person with no address, and the participant's
contact is the one that identity is on. A conversation
the holder has with themselves, notes sent to their own address, therefore has
no participants and makes no contact; it goes by the account's display name.
_Avoid_: Member, Recipient

**Last heard from**:
When a contact last sent a message: the newest message any of the contact's
identities was the sender of, shown on the contact list and one of the two ways
the list can be ordered. It is not the contact's last activity. A message the
account owner sent to the contact, or one another member of a group conversation
sent, does not move it, because neither is hearing from the contact. A
message in a conversation in the Trash does not move it either: the column
is not asked for the Trash, so it leaves the Trash out, and `last-message:`
on Contacts asks the same question with the same answer. A contact none of
whose identities ever sent a message has no date and sorts last whichever way
the list runs.
_Avoid_: Last seen, Last active, Last message

**First heard from**:
When a contact, or one of a contact's identities, first sent a message. It is
the other end of Last heard from and follows the same rule: only a message the
contact sent counts.
_Avoid_: First seen, First active, First message

**Unknown**:
The Contact Group Message Crate computes from contacts that have no name or no
address. An identity of type `other` holds a name, not an address, so a
contact whose only identities are of that type is Unknown. It has no members of its own and empties as a person names people.
_Avoid_: Unnamed, Unresolved, Uncategorised

**Trash**:
Where a person sets aside conversations and contacts they do not want to
see. Membership is explicit, nothing in it is deleted, and a trashed
conversation can still be opened and read. Lists leave the trash out unless
asked to show it, and so does everything a search looks at on the way to its
answer: a trashed conversation is not one of a contact's conversations and a
trashed contact is not one of a conversation's contacts, until the search
asks for the trash with `trashed:`, and then the trash counts everywhere that
search looks. A trashed contact stays set aside until an import meets
one of its identities: the import then discards the trashed contact together
with every identity it had and makes a new contact from the backup, as a first
import would. The trash is the only door to permanent
deletion; something must be trashed before it can be deleted, one item at a
time or all at once with Empty Trash.
_Avoid_: Deleted, Archive, Hidden, Bin

### Logging in

**Account**:
One person's store inside a Message Crate: the login they log in with, and the
conversations, contacts, and identities that store holds. A Message Crate holds many
accounts and keeps each one's data isolated from the others, so nothing an
account holds is visible to another. An account is not finished until
profile setup finishes.
_Avoid_: Login, Profile, Tenant, Workspace

**Owner**:
The administrator of a Message Crate: the one account that manages its other
accounts and its server settings, and the only account that holds no messages
of its own. The owner creates, disables and deletes accounts, resets their
passwords, deletes their message data, and decides whether strangers may create
an account. The owner monitors it through metadata: counts and totals of
messages, contacts and attachments, each account's imports and exports, when
it last logged in, and attachment file names and sizes. The owner never reads
content: a message's text, an attachment's bytes, or a contact's name and
identities. The full line is in `docs/adr/0008`. There is exactly one, it cannot
be deleted, and no other account can be given its powers.
_Avoid_ as its name: Vault Owner, Admin, Administrator, Superuser, Root. The
role is an administrator's; the account is called the owner, because exactly
one exists and it is whoever claimed the Message Crate.

**Owner Home**:
The screen the owner lands on at login and works from, the way any
other account lands in Messages. It has the frame every account sees: the
product name, a search bar, the username and the account button across the top, over a
side panel and a content pane. The side panel lists Dashboard, Server Settings,
User Accounts, Audit Trail and Logs; Dashboard shows what the whole Message Crate
holds, and Audit Trail and Logs are named and hold nothing yet. The search bar narrows User Accounts by username or
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

**Audit Trail**:
The permanent record of what each user did on a Message Crate, and when:
logging in, sessions ending, logins refused, Import Runs, Export Runs, and the
owner's changes to accounts. It records that something happened and how much,
never what the messages said. Nobody edits or deletes it, the owner included,
and deleting an account does not remove its entries. The owner reads it for
every account on Owner Home. It is not the server's log output, which is Logs.
Why it outlives the account: `docs/adr/0020`.
_Avoid_: Activity, Audit log, Event log, History

**Claiming**:
Making a Message Crate's owner. One with no owner is unclaimed and offers only
the Create Owner screen; claiming it is what that screen does. A claimed
Message Crate is closed or open, depending on whether it lets strangers
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
both. A program holding one can bring messages in, or take them out
through an Export Run it starts, but it can never browse: reading messages
outside a run needs a Session. The secret is shown once when the token is
made; afterwards the account sees only its name, a masked hint, and when it
was last used. The owner sees the same without the hint, and may revoke it.
_Avoid_: App password, Key, Credential, Session

**User**:
The person operating Message Crate, in the browser or in the desktop app. A
user has an account in a Message Crate, and "user" is the colloquial word for that
account: User names the person at the keyboard, Account names the record
they log in to and the data it holds.
_Avoid_: Member, Operator, End user

### Moving messages in and out

**Export**:
Moving messages out of Message Crate into files on disk, in a format the
person chooses. It reads the database, never a phone backup. Every export is
recorded as an Export Run.
_Avoid_: Extract, Pull, Download

**Export Run**:
One attempt to move messages out of Message Crate, recorded permanently whether
it completed, failed, or was cancelled. The record holds what was asked for
and how much matched, never what the messages said. A person asks in one of
three ways: everything the account holds, whatever a search currently shows,
or conversations and messages they have picked by hand. The run hands over
the messages that matched when it started, whatever is imported or trashed
while it is read. Exporting everything
is still an Export Run; it is not called a backup, because a backup is the
phone's file that Import reads.
_Avoid_: Export Job, Backup, Download

**Convert**:
Rewriting a folder of already-exported files into a different format,
reading neither the original backup nor the database. Export uses it for any
format other than JSON Lines. As an operation a person starts on a folder of
their own it is an advanced tool most people never need, so it lives under
Settings rather than beside Import and Export.
_Avoid_: Reexport, Transcode, Reformat

**Stage**:
One of the three parts of an Import Run, in order: **Staging** reads the
backup and copies its messages and original attachments into the Staging
Directory; **Media** converts or compresses the staged attachments, and
exists only when the person asked for it; **Upload** writes the staged
messages and attachments into Message Crate. A run shows its stages as one list
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
Message Crate. There are at most two: the Staging Review after Staging, and
the Media Review after Media. A waiting review reads "Awaiting approval".
Approving continues the run; cancelling ends it and deletes what was staged.
A run left at a review keeps waiting, on another screen or after the app is
closed, until the person decides. The stop is named for what the person does
there; "approve" stays the word for the decision that continues the run.
_Avoid_: Gate, Approval (for the stop), Checkpoint, Confirmation, Deny

**Pause**:
Halting the running Stage of an Import Run while keeping everything it has
staged, so the run can resume later from where it halted. The Upload's own
button pauses it, logging out during an Upload pauses it after asking, and
an Upload that fails is paused rather than failed, since a failure there is
mostly the network or the server. A paused run is offered again, with
Resume or Discard, the next time its account opens Import.
_Avoid_: Stop, Cancel (for this), Suspend

**Resume**:
Continuing a paused Import Run, or one interrupted by the app closing, from
where it halted; an Upload sends only what is still missing.
_Avoid_: Retry, Restart

**Cancel**:
Ending an Import Run at a Review, or before Staging starts, and deleting
what it has staged. A cancelled run is recorded as cancelled.
_Avoid_: Stop, Abort, Pause (for this)

**Discard**:
Ending a paused Import Run instead of resuming it, and deleting what it has
staged. A failed Staging or Media Stage is discarded at once, since nothing
complete exists to upload.
_Avoid_: Delete run, Clear

**Message Crate Directory**:
The one directory the desktop app keeps on a computer, made the first time
the app starts. It holds the Data Directory of the Message Crate the app
starts, the Staging Directory, and the Tools Directory. It exists on every
computer the app runs on, including one whose app connects to a Message
Crate elsewhere. Decided, not built yet: #1053 tracks the work, and until it
lands the app keeps its Message Crate's data in the operating system's
app-data directory.
_Avoid_: Folder, App Data, Home Directory

**Data Directory**:
The directory a Message Crate keeps everything it stores in: its database
and each account's attachments. One Message Crate has one Data Directory,
and a copy of it is a complete backup. The desktop app's is inside the
Message Crate Directory; a Docker Message Crate's is the volume given to it.
_Avoid_: Data Folder, DB Directory, Database Directory

**Staging Directory**:
The directory where Message Crate writes intermediate files that neither the
person nor Message Crate keeps — a backup being prepared for import, or JSON
Lines waiting to be converted into the format an export asked for. It is
deleted when the job ends, whether it succeeded, failed or was cancelled,
the import log, resume journal and run record with it. An Import Run that
can still be resumed, paused or waiting at a Review, keeps it, since the
staged files are what Resume reads.
_Avoid_: Import Staging Directory, Temp Folder, Working Directory

**Tools Directory**:
The directory inside the Message Crate Directory where the desktop app keeps
the programs it downloads for itself: ffmpeg, ffprobe and wtsexporter. The
app owns its contents and replaces them when a release needs a newer
version. Decided, not built yet: #1053 tracks the work, and until it lands
the app downloads nothing.
_Avoid_: Tools Folder, Bin Directory, ffmpeg directory

**Apple Messages Reader**:
The separate program the desktop app runs to read Apple Messages from a Mac
or an iPhone backup during Import. It is its own program, under the GNU GPL,
because the library that understands Apple's message database is GPL and the
rest of Message Crate is not; the app starts it, sends it one request, and
reads its answers back over a pipe. It is also the only program that can
decrypt an iPhone backup, so a WhatsApp import from an encrypted iPhone
backup has it decrypt WhatsApp's files before wtsexporter reads them. Where
a person sees it, it is named with its file name once: "the Apple Messages
reader (imessage-reader)".
_Avoid_: Helper, Sidecar, GPL helper, iMessage reader

Extract is not a word for something a person does. It survives only as the
internal name of the desktop command that reads a backup during Import.

### Versions

**Product Version**:
The number a release of Message Crate is named by, such as `0.9.0`. The
server, the desktop app and the website carry the same one. An app whose
Product Version differs from its server's is flagged to the person and to the
owner, and is served all the same.
_Avoid_: Release, Release number, App version

**Build**:
A Product Version together with the commit it was built from, such as
`0.9.0+343fe0d8`, with `.dirty` added when the source held uncommitted
changes. A build made from a release tag is the Product Version alone. A
screen shows the Build under the plain label "Version".
_Avoid_: Revision, Build number, Full version

**Schema Fingerprint**:
The number derived from the database's table definitions, which the
server stamps into its database. It is not the `schema_version` of the shared
chat file format, which is a separate number kept by hand.
_Avoid_: Schema version, Schema hash, Database version
