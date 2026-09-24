# Changelog

What changed in Message Vault, written for the people who use it.

Each release is grouped three ways:

- **Features** — something you can now do that you could not before.
- **Fixes** — something that was wrong and now behaves correctly.
- **Design** — a change in how the product works that is worth knowing about,
  including work under the surface that changes nothing on screen.

Internal rework is mentioned only as far as it matters to you. The reasoning
behind a decision lives in the architecture notes under `docs/adr/`, and the
detail of any single change lives in its pull request.

Version numbers follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Bullets under the version still in development carry the date they landed;
released versions carry their date on the heading.

## [0.10.0] — in development

### Features

- 2026-09-24 **A WhatsApp import knows which number is yours.** Every
  imported WhatsApp message now records the phone number your WhatsApp
  account is registered to, so its conversations count toward that identity
  in Settings. An iPhone backup carries the number, and Import reads it from
  there. An Android backup does not, so the Import form asks for it in a
  **WhatsApp phone number** field, pre-filled from your profile's phone; on
  iPhone the same field sits under Processing Options as a fallback for a
  backup without the number. The number is recorded on the messages and is
  not added to your profile.
- 2026-09-23 **Search contacts by when you last heard from them.**
  `first-heard:` and `last-heard:` on Contacts find the first and last
  message a contact sent you, in a direct or a group conversation:
  `-last-heard:>=2022` lists everyone you have not heard from since 2022,
  including people who never messaged you. They replace `first-message:` and
  `last-message:` on Contacts, which counted your own messages and everyone
  else's in a shared group chat. On Conversations and Messages,
  `first-message:` and `last-message:` still mean the conversation's first
  and last message. The Advanced Search contacts form's date fields are now
  First Heard and Last Heard, and Trash's form leaves them out.
- 2026-09-22 **One identity table, on the contact drawer and on an account's
  Profile.** An account's identities now show what a contact's do: the
  service, the address, when it was first and last seen, and how many
  conversations, direct messages and group messages it takes part in. The
  columns line up under their headers, the sort arrow sits next to the
  label, and every row ends with a visible Remove. Adding an identity opens
  a small dialog instead of a permanent row under the table, and the dialog
  offers Email everywhere, so a contact can be given an email address by
  hand.
- 2026-09-22 **The Dashboard shows where the vault's disk space goes.**
  Owner Home's Dashboard is now three sections. Vault contents is the card it
  had. Database shows the size of the database on disk, how much of it the
  messages take and how much the full-text search index adds, all measured
  by the vault. Messages by account lists every account with its message
  count, its text and an estimated size on disk, split from the messages
  figure by each account's share of text, with a totals row so the split
  visibly adds up. Attachment files are counted under Vault contents, not in
  the database size.

### Design

- 2026-09-22 **An account identity means ownership.** The Profile tab now
  says what the identities are for: your phone numbers and emails, which
  Import uses to determine which messages belong to you. The glossary and
  the architecture notes record the same distinction: a contact's identity
  means the person took part, an account's means the messages are theirs.
- 2026-09-22 **Profile Setup shows the identities already on your account
  in their own fields.** Phone numbers and emails the vault owner added
  now fill the rows, where you can change or remove them before going on,
  instead of sitting in a line of text above them.
- 2026-09-22 **Shorter wording on two screens.** Create Vault Owner now
  opens with "A vault owner is required to create and manage users.", and
  the Display Name button in Settings reads Save without changing to Saved.
- 2026-09-23 **A contact's identities read the same as an account's.** The
  contact drawer now shows each identity in the form the vault stores it,
  a phone number in international form, and names an email address as
  Email, just as the Profile tab does. The vault counts both tables the
  same way. Under the surface, the vault's interface and code were renamed
  to use the words the product uses, with nothing else to see.

### Fixes

- 2026-09-24 **GO SMS Pro picture messages import whole, and the ones you
  sent import at all.** An import read only the picture messages you received
  and, for most of them, mistook bytes inside the picture for phone numbers,
  so a photo from one friend could land in a group chat with hundreds of made-up
  members. Every picture message now lands with the people who were actually
  on it, the ones you sent are included, and a voicemail notice from Google
  Voice stays in the Google Voice conversation instead of being moved under
  the caller.
- 2026-09-24 **iMazing message times are read in your account's time zone.**
  An iMazing export writes each message time without a zone, and the desktop
  app used to read them in whatever zone the computer running the import was
  set to, so the same folder gave different times on different machines. The
  import now reads them in the time zone on your profile. If the phone lived
  in another zone at the time, Processing Options on the Import screen has a
  Time zone of the messages picker for the iMazing source.
- 2026-09-24 **An iMessage you sent is always yours, however the phone
  recorded your number.** Some iPhone databases store the sending number on
  an outgoing message as `tel:+1…`, and the import kept that prefix, so those
  messages carried a sender that did not match the number on your profile.
  The prefix is now removed the same way for every message and for the list
  of addresses the backup sent from.
- 2026-09-23 **Excluding something from a search no longer hides the rows
  that have nothing to compare.** A search with `-` in front of a word left
  out every row with no value for that word, so those rows appeared under
  neither the word nor its negation. `-import:last` found no messages at all
  before the first import, and a negated date word on Contacts left out
  every contact with no messages. A search and its negation now always
  divide the list between them.
- 2026-09-23 **A group text from an SMS Backup & Restore backup is no longer
  credited to the wrong person when the backup names no sender.** A group
  MMS without a sender address was shown as sent by whichever member the
  backup happened to list first. Such a message now shows no sender, as a
  message with no recorded sender does from any other source. A message that
  does name its sender was already credited correctly, whichever position the
  sender holds in the group.
- 2026-09-23 **An iMazing message sent in the hour the clocks spring forward
  is kept.** iMazing writes each message's date as a wall-clock time with no
  zone. A time that never showed on the clock, such as 02:30 on the March
  morning when 02:00 became 03:00, was dropped as an invalid date; it is
  now read with the offset in force just before the change, so it lands at
  the instant the new clock called 03:30. A time that showed twice on the
  November morning the clocks fall back is the earlier of the two. The
  zone an iMazing export is read in can now be given by name, such as
  `America/New_York`, as well as by offset.

- 2026-09-23 **Searching for a word with punctuation in it works on every
  vault.** A search such as `a&b`, `o'bri*`, or text pasted with a hidden
  NUL character failed with an error, or on a Postgres vault found messages
  that had the words in any order. Punctuation inside a word now always means
  the words next to each other in that order, and a NUL is read as a space.

- 2026-09-22 **Changing a password checks things in a sensible order and
  says so in full sentences.** The vault now checks the current password
  first, then that the new password was typed the same way twice, then that
  it differs from the current one, and tells you only the first thing that
  went wrong. The messages read as sentences ("Current password is
  incorrect.") and the Change password button no longer sits tight against
  the last field.

- 2026-09-22 **International phone numbers keep their country.** A number
  written with a country code, such as `+65 9123 4567` in an address book or
  `+44 7700 900123` as your own number, is now matched as that number. Before,
  some were read as a US number with the same digits and named the wrong
  person, and some matched nobody.

## [0.9.0] - 2026-09-22

The release that gives a vault an owner, a trash you can take things back out
of, and a search that reads the same everywhere.

### Features

- **A command-line import that was killed can be cleared from
  the command line.** An import stopped mid-run leaves its session open, and
  no later import for that account can start until it is discarded. The
  vault server now has `imports discard --account <account>`, which discards
  the account's open session and says which one it was, or that there was
  none. The refusal names the command, so you no longer need the desktop
  app's Import screen to get unstuck.
- **The vault owner can see how much the vault holds.** Owner
  Home's Dashboard now shows the vault's totals across every account: its
  storage total and how many messages, attachments, conversations and
  contacts it holds. An account's Storage tab shows its conversation and
  contact counts beside its message and attachment counts. These are counts
  only: no conversation, contact or message is named to the owner.
- **The contact list shows when you last heard from each
  contact, and can sort on it.** Every contact row now carries the date of
  the newest message that contact sent you, and the sort menu gains Last
  Heard From beside First Name and Last Name, newest first. A message you
  sent them, or one someone else sent in a group chat, does not count: the
  date is when they last wrote. Contacts you have never heard from sit at the
  end whichever way the list runs. The vault's contact list takes the same
  key: `GET /v1/contacts?sort=-last_heard`, and each contact carries
  `last_heard_at`.
- **The vault owner sees an account's Storage as its holder
  does.** Open an account from User Accounts and its Storage tab now shows its
  message count and storage total, every import and export it has run, and
  its largest attachments by name and size. Opening an import shows its
  counts, timings and issues, and how many contacts it created and changed.
  Who those contacts are, and which conversation a file is in, stay with the
  account. The account's Profile tab
  shows when it last logged in and which app it connects with.
- **Every screen can tell you which version it is.** Settings →
  System shows the version of the app you are using, in the browser and in the
  desktop app. The vault owner's Settings shows the vault's version and
  its schema fingerprint, and an account's Profile tab shows the owner which app
  the account connects with, the desktop app or the website, and its version.
  A build between two releases carries the commit it came from, such as
  `0.9.0+343fe0d8`, so two dev builds can be told apart. When an app and its
  vault come from different releases, the app says so in a line under the
  header, "This vault is 0.10.0. This app is 0.9.0.", and the owner sees it
  marked on that account's Profile. Nothing is blocked: the vault serves every app
  whatever its version.
- **User Accounts shows when each account last logged in.** A
  Last login column next to Status, in your own time zone, or "Never" for
  an account nobody has logged in to yet. Logging in, claiming the vault and
  registering all count; a password change does not.
- **Export can write just the conversations a search finds.**
  The Export screen opens with a scope: Everything, as before, or Search,
  which shows a box for a search in the same language as the search bar and
  exports only what it finds. `in:#19,#22` exports those two conversations
  and nothing else. Opening Export while browsing a list of conversations
  starts in Search with that list's search already filled in.
- **The stop in an import run is a review, and it shows how many
  messages each identity sent.** "Staging Approval" and "Media Approval" are
  now "Staging Review" and "Media Review", and a waiting one reads "Awaiting
  approval". Identities is a table: each address the backup sent from, how
  many of the staged messages it sent, whether it is on your profile, and
  "Add to profile" at the end of the row for one that is not. "Files over the
  limit" says "Skip vault upload" beside it instead of a sentence above the
  list. Each stage's title has a rule under it, "Cancel this import" looks
  like a button before the pointer reaches it, and the attachments'
  "Operation" is "Action".
- **An import run and its approvals are one list.** The run
  screen is the list of stages, and each approval is a row in that list where
  the run stops, with the decision inside it. There is no separate approval
  screen to open and come back from. Each stage's row holds what that stage
  made, one fact per line: Staging has the staging directory, conversations
  and messages, and the attachments' operation, count and total size. The
  Staging Approval shows contacts as Existing and New, the size limit per
  file (50 MB, which no screen showed before), and the files over it, which
  open to each file and its size. With Convert or Compress it adds estimates
  in three groups: Likely within limit, May exceed limit, and Not audio or
  video. The Media Approval shows what is true after Media, not how it
  compares with the estimate. The import log link sits in Upload's row and
  appears once Upload starts, so it no longer opens onto a file that does
  not exist yet. A finished run offers "View imported conversations", "View
  modified contacts" and Back; errors are in one table under the list.
- **Import is one screen that fills in as the run goes.** The
  form collapses into "what you asked for" once the run starts, each of the
  three stages (Staging, Media, Upload) adds its result underneath, and the
  finished run leads with where to go next: the conversations it added, the
  contacts it touched, or another import. Both approvals are the same
  screen; it opens on its own when a stage finishes and has a link back to
  the run. A run keeps working, and keeps waiting at an approval, while you
  are on another screen, and the Import entry in the sidebar carries a badge
  while a run needs you. The two approval screens no longer say "gate".
- **A vault has an owner.** A fresh vault now asks you to create
  its owner before anything else, and that owner is the one account that
  manages the vault: create accounts, disable them, reset a password, delete
  someone's messages, and decide whether strangers may sign themselves up. The
  owner has no messages of their own and cannot read anyone else's — the
  account list shows a name, a message count and a storage total, and nothing
  of what those messages say. There is exactly one owner and it cannot be
  deleted. If you forget its password, `create-owner` and
  `reset-owner-password` on the server put you back in.
- **New accounts are closed by default.** A vault admits nobody the
  owner has not admitted, until the owner turns on public registration. An account
  the owner creates has to replace the owner's password the first time it
  logs in, so the owner never keeps knowing it.
- **Permanent delete, from the trash only.** Deleting a trashed
  conversation removes it, its messages, and any attachment no other message
  still uses. Deleting a trashed contact does what a phone does: the name and
  edits go, the contact becomes Unknown, and its conversations stay, showing
  the number. **Empty Trash** does both for everything in it.
- **Export history.** An export is recorded like an import: what it
  covered — everything, a search, or conversations you picked — and how much it
  handed over. Settings → Storage lists Export history beside Import history.
- **Convert**, a desktop tool under Settings that rewrites a folder
  of already-exported files into another format without touching a backup or
  the vault.
- **Times read in your zone.** Your account carries a time zone,
  chosen at setup and changeable afterwards, and every message time, day and
  year is shown in it.
- The vault server writes a proper log — one line per request, the
  full reason behind any internal failure, and a warning for work it could not
  finish.
- **Find in conversation**, and years that page like every other
  list.
- Make a Contact Group straight from a group conversation.
- Import accepts owner email addresses for SMS Backup+, and the two
  Android SMS sources share one form.
- **A trash you can undo.** Conversations and contacts can be set
  aside and taken back. Nothing in the trash is deleted, a trashed conversation
  can still be opened and read, and lists leave the trash out unless asked.
  Trash has its own advanced search.
- **An import names the contact.** When a backup knows someone's
  name and the vault does not, the import puts that name on the contact. A name
  you type, or load from an address book, replaces one an import supplied; a
  later backup spelling it differently does not.

### Fixes

- **An unread Apple Messages message no longer claims it was
  read on 2001-01-01.** Messages stores no read time for a message nobody
  has read, and the reader turned that empty value into the earliest date
  Apple's clock can express, so every unread message imported from a Mac or
  an iPhone backup carried a read receipt from the start of 2001. An unread
  message now carries no read receipt; a read one keeps its real time.
- **A finished import no longer leaves its staging folder
  behind.** Every import wrote a copy of the backup's messages and
  attachments into the staging directory and left it there after the upload,
  so each import added gigabytes to the folder. An import that succeeds now
  deletes its staging folder, the import log with it; the run's record under
  Settings → Storage → Import history keeps its counts, timings and errors. A
  failed import still leaves the folder in place.
- **An attachment gets the same filename on every computer.** The
  date at the front of an attachment's filename is now the message's time in
  UTC. It used to be the time zone of the computer running the export, so the
  same backup exported on two computers named its attachments differently.
- **Searching every conversation for a word answers at once.** A
  word or phrase typed into Messages without an `in:` scope, and
  `messages:0` or `first-message:` on Contacts, took ten seconds to several
  minutes on a vault of 600,000 messages. Both now answer in well under a
  second with the same results.
- **Loading an address book again keeps your Contact Groups.** A
  contact the book created and that is still in the file keeps its group
  memberships, its conversations and its place in Import History; only its
  name and phone numbers change to what the file now says. A contact the file
  dropped is removed as before. A conversation with a number the book had
  supplied also survives a reload, where it used to be deleted with the number.
- **Reloading the website while the vault is down no longer logs
  you out.** The Login screen shows Disconnected as before, and when the vault
  answers again you go straight back in without typing your password. A login
  the vault itself rejects still asks for the password.
- **A contact with no name shows who it is.** The contact list,
  the contact's own panel and the Trash show its first identity, in italics,
  where they used to read "(unknown)" on every row. Its Contact Groups now
  say Unknown, and it no longer appears under No group as well.
- Long-running vaults no longer grow in memory for every file ever
  uploaded and every username ever tried.
- A failure now says what actually went wrong — the step that
  failed and the file or database error under it — rather than the outermost
  message alone.
- The published Docker image is built with the compiler the project
  tests against, not whatever the base image happened to carry.
- An obfuscated export no longer carries the original vendor data
  alongside the substituted text.
- A trashed contact is set aside rather than gone: an import that
  meets one of its numbers attaches to it and leaves it in the trash, and
  contact counts leave the trash out.
- Only the newest connection check may speak for the login card,
  so a slow answer for an old address cannot overwrite a newer one.
- Importing a file the vault cannot read explains what is wrong —
  which version the file is and which the vault reads, or which line is bad —
  instead of "internal server error".
- Message Vault Settings no longer shows an address green when it
  has not tried it. An address typed but not tested reads **Not tested**.
- The login and profile-setup pages no longer show a scrollbar on
  a screen tall enough to hold the card, so opening a dropdown stops shifting
  the card sideways.
- Profile setup refuses a phone number or address already in the
  list, marks the row that repeated it, and compares numbers regardless of how
  they are written. The same number on Text Message and on WhatsApp still
  counts as two.
- Desktop import no longer fails partway through with a source
  mismatch.
- Import errors group identical problems into one row with a file
  count, instead of one row per file.

### Design

- **The desktop app says what it ships from others.** Apple
  Messages are read by a separate program, the Apple Messages reader, which is
  free software under the GNU General Public License. Settings → About now has
  a Third-party software note naming it, with links to its source and license
  for the exact version you are running, and every installer carries the
  license text beside the program.
- **Owner Home has room to grow.** The side panel reads
  Dashboard, Settings, User Accounts, Activity and Logs. Settings is what was
  Vault Settings; Dashboard, Activity and Logs are named and empty for now.
  User Accounts is down to who, their status and their last login: the app, the
  message count and the storage total moved into the account's own Profile and
  Storage tabs.
- **Settings, Account shows an account's status and
  permissions.** Permissions lists Import messages, Export messages, and
  Delete messages and attachments. You can see what your account may do; the
  vault owner sets it. The owner changes an account's status and permissions
  from that account's Settings, and User Accounts now shows each status
  without the Import, Export and Delete columns.
- **Changing the vault owner's password asks for the current
  one.** The owner's account reaches every other account, so Settings,
  Account has a Current password field for the owner, and the vault checks
  it before it stores the new password. Every other account changes its
  password as before. The owner's Settings are now Account, Profile and
  Appearance: System and Convert work on messages, which the owner does not
  hold, and Profile no longer asks the owner for handles.
- **A gear in each User Accounts row opens that account's
  Settings.** It appears at the left of the row while the pointer is in it.
  The column headings are bold with a line between them, and every other row
  is a shade lighter.
  The screen is the one the account holder sees, with the Account, Profile and
  Storage tabs. Reset password, Delete messages and Delete account moved
  there from the Actions column, which is gone. You can read an account's
  profile and how much it stores; you cannot change the profile or see what
  is stored. Each account shows its preferred name under its username, and
  the search bar matches either. Your own account now leads the list, and its
  gear opens your own Settings.
- **Owner Home looks like the screen every account sees.** The
  product name, a search bar and the account button now run across the top.
  The search bar narrows User Accounts by username. The side panel lists
  Vault Settings, then User Accounts. The owner's password and appearance
  moved to Settings, under the account button, which is also where Log out
  now is.
- **The vault owner's screen is Owner Home, with a side panel.**
  The owner lands on it at login. Its side panel lists User Accounts first,
  then Vault, Password and Appearance, in place of the tabs across the top.
  In User Accounts, an account's status is a dropdown (Active or Disabled)
  instead of an Enable/Disable button, and both Add account and Reset
  password ask for the password twice and save only when the two match.
  Resetting a password now does just that: the person's current session
  carries on, and they keep the new password until they change it themselves.
  The vault no longer makes anyone replace a password the owner chose at
  their next login.
- **An import brings back a contact you had trashed.** Until now
  an import that met the handle of a trashed contact attached to it and left
  it in Trash, so someone you set aside once never appeared in Contacts again
  however many newer backups you imported. A backup that still holds the
  person means you still talk to them: the import now discards the trashed
  contact, name, group memberships and handles included, and makes a new
  contact from the backup, the way a first import would. The forecast of new
  contacts shown before an import counts them. To keep someone out for good,
  delete them from Trash. Why: `docs/adr/0013-an-import-replaces-a-trashed-contact.md`.
- **An Import Run says what it did to each contact.** The run
  records, as it goes, whether it created a contact, created one in place of
  a trashed contact, named one that had no name, or added a handle to one,
  and the run's record under Settings → Storage lists the contacts with that
  reason. The new and changed counts come from the same record instead of
  being guessed from timestamps afterwards.
- **Importing SMS Backup+ mail reads one message per file, and
  nothing else.** Message Vault briefly also read a second kind of `.eml` — a
  whole conversation written out as a dated transcript in one mail. That shape
  is not something SMS Backup+ produces, and every message in the only known
  collection of them was already present as ordinary SMS Backup+ mail, so
  reading it added a second copy of messages the vault already had. Support for
  it is gone. Importing a folder of SMS Backup+ mail is unchanged.
- **The HTTP interface was rebuilt on one set of conventions.**
  Every list pages and sorts the same way — the browse lists and the ones you
  curate alike, with no list left answering a bare array — every failure comes
  back in the same shape with a link to a page explaining that kind of failure,
  every created thing answers with its address, and every response carries an
  id you can quote when reporting a problem. Logging in, and everything to do
  with accounts, each moved to one address serving everyone, with the vault
  deciding what a given caller may see rather than the address saying it. A
  single message can now be read by its id, so a search result links to the
  message rather than to a position in a list. Parameters that no longer did
  anything are gone — nothing asks you to name your account when your key
  already says it — and import history sorts the same way export history does.
  This matters if you wrote something against the interface yourself; nothing
  in the app or the desktop app changes.
- The vault and the desktop app now convert media with the same
  code, so a video converted on import and a preview generated later can no
  longer differ. Previews are better quality than before.
- Import progress is reported by the exporters directly rather than
  read back out of their log text, so the progress bar can no longer be broken
  by a wording change. An encrypted iPhone backup now narrates its setup steps
  instead of sitting on "Reading backup…".
- The desktop app reads Apple Messages through a small separate
  program shipped beside it, because the library it uses carries a licence that
  cannot be combined with the app's. Nothing changes on screen.
- Release builds of the server and the desktop app are optimised
  and stripped, so they are smaller and faster.
- **One search language.** The search box, saved searches and the
  export filter are compiled by the same code, so a query means the same thing
  everywhere it is typed.
- The vault accepts the packaged desktop app without being
  configured to. A vault built from source used to refuse it in a way that
  looked like an unreachable server.
- Importing is substantially faster — messages, attachments and
  reactions are written in batches rather than one database call at a time.
- Import lists one **iMessage** source with a choice of Mac
  Messages, iPhone backup, or jailbroken iPhone, and one **WhatsApp** source
  with a choice of Android or iPhone. Encrypted iPhone backups ask for the
  password in the form. Required fields are marked; optional ones say so.

### Upgrading

- The database format changed several times during this release. **An existing
  vault is rebuilt empty on first start and its messages must be imported
  again.** There is no migration before the first stable release.
- If your configuration file sets `asset_hash_threshold_bytes`, delete the
  line. The setting did nothing and the vault now refuses to start with it.
- An obfuscation seed is exactly 64 characters — the length the exporter prints
  when it generates one. Shorter seeds are no longer accepted.
- The old desktop interface built with Slint has been removed. The desktop app
  is the one built with Tauri.

## [0.8.3] - 2026-08-25

### Fixes

- The published Docker image can finish building its sample inbox on systems
  where it previously failed partway through.

## [0.8.2] - 2026-08-25

### Fixes

- The published Docker image includes the files it needs to generate its sample
  inbox.

## [0.8.1] - 2026-08-25

### Fixes

- The published Docker image builds again. The 0.8.0 image failed to build.

## [0.8.0] - 2026-08-25

### Features

- A grey, green or red light beside the server address on the login screen,
  so you can see whether the vault is reachable before trying to connect.
- Settings → System applies changes to the import staging folder and the ffmpeg
  folder immediately, with no Save button, and reports whether ffmpeg was found.
- The Contacts list shows which range of contacts you are looking at in a pill
  at the bottom of the panel, always visible.

### Fixes

- Opening a contact from a message thread no longer flashes an empty Loading
  row before showing the real details.
- The panel divider between the navigation and the list can be grabbed again,
  and drags up to a wider maximum.
- Packaged desktop builds can connect to a vault out of the box. Release builds
  were previously blocked unless the vault was configured for them by hand.
- Editing a contact's name discards the draft on click-away, Tab or blur, and
  saves on Enter.
- The import staging folder no longer nests an extra folder inside the one you
  chose.

### Design

- Login leads with Connect or Log in. Extracting and converting files moved
  off that screen; importing a backup after logging in is unchanged.
- Closing the desktop window logs out, so the next launch asks you to log in.
- Pushing an import to the vault is considerably faster — larger batches, less
  repeated checking of files the vault already has, and more work overlapped.
- The navigation panel is width-draggable, and the conversation list can shrink
  away entirely on a narrow window so the thread stays readable.
- The sidebar section previously called Thread Tags is now **Message Tags**.
- Internal rework across the server, the libraries, the exporters and the
  command-line tools, with no change to what any of them produce. One
  behavioural difference: a KnugiHK binary placed in a custom tools folder is
  no longer found by WhatsApp Android export.

## [0.7.3] - 2026-08-13

### Fixes

- Packaging corrections for the 0.7 release.

## [0.7.2] - 2026-08-13

### Fixes

- The Docker image builds again after a removed folder was still being copied.

## [0.7.1] - 2026-08-13

### Fixes

- The Windows application icon.

## [0.7.0] - 2026-08-13

The release where Message Vault became one product: a vault you log in to and
browse, rather than a set of tools that write files.

### Features

- **Browse your messages in the app.** Conversations, contacts, threads,
  attachments with thumbnails, a lightbox and inline video, date jump links,
  and search.
- **Contacts.** A contact drawer with an editable table of the numbers and
  addresses that reach a person, name aliases, advanced search with date
  operators, and the ability to browse a contact's conversations from the
  drawer.
- **Guided import** with a live progress summary, per-stage timings, contact
  name review, and an import that continues past a conversation it cannot read
  instead of stopping. A finished import is saved as a group you can go back to.
- **Accounts and login**, with profile setup, appearance themes, and a danger
  zone for deleting your messages or your account.
- **API tokens** with scopes, for scripting against the vault.
- A sample inbox you can try the vault with, seeded on first start.

### Fixes

- Hardening across the exporters and the vault: attachment paths that tried to
  escape their folder, digest and date handling, authentication, tokens and
  cross-origin rules.
- Long lists no longer slow the app down; the sidebar, contacts and
  conversations are all paged and virtualised.

### Design

- The desktop app and the website are one React application, built on React
  Aria and Tailwind, so dialogs, drawers, progress bars and form fields behave
  consistently and work with a keyboard and a screen reader.
- The separate `message-vault-rs` repository was merged in, so the vault server
  and the tools that feed it live together.
- A message's transport (iMessage, SMS) is recorded separately from the
  platform a number belongs to.

## [0.6.0] - 2026-08-04

### Features

- **Guided vault import** in the desktop app: a form that walks through the
  credentials and the backup, with readable progress as it uploads.
- Attachments upload in parts, so a large file no longer has to succeed in one
  go.

### Fixes

- Import logs stay responsive under heavy output.
- Login explains an insecure-to-secure address mismatch instead of failing
  obscurely.

## [0.5.0] - 2026-08-02

### Features

- Push exported messages straight into a vault from the desktop app.
- Read contacts from a vCard file or a contacts CSV.

### Fixes

- The saved settings file is written with restricted permissions, and vault
  fields are kept when the app closes.

## [0.4.1] - 2026-07-30

### Fixes

- The Windows app no longer opens a console window behind it.

## [0.4.0] - 2026-07-30

### Features

- **A desktop application**, replacing the command-line-only workflow, with a
  form-based screen per task and a log you can watch.
- Errors are shown against the tab that produced them and can be dismissed.

### Design

- Uploading to a vault is much faster: files the vault already holds are
  detected before being sent, imports are batched, and the work overlaps.

## [0.3.0] - 2026-07-29

### Features

- Releases ship as self-contained archives per platform, so nothing has to be
  built to try the product.

## [0.2.0] - 2026-07-29

### Features

- **One common message format** underneath every backup type, so an iPhone
  export and an Android export produce the same thing.
- **Export to the format you want**: JSON Lines, JSON, CSV, EML, MBOX, or the
  Android restore XML, with JSON the default.
- **Convert an existing export** into another format without the original
  backup.
- Media handling and obfuscation apply to every format rather than to some of
  them.

### Fixes

- Staged attachments are cleaned up after being embedded in mail or XML output.

### Design

- The documentation site was rebuilt as end-user guides organised by the kind
  of message you are exporting, rather than by internal structure.

## [0.1.0] - 2026-07-18

The first release: command-line tools that read a phone backup and write CSV.

### Features

- Exporters for iMessage, SMS Backup & Restore, and related Android backups.

### Fixes

- Conversations with unknown participants are kept rather than dropped.
- MMS media is kept when a message's layout only partially matches.
- An owner phone number is required, so messages can be attributed correctly.

---

Installable builds also appear on
[GitHub Releases](https://github.com/bitrealm-io/message-vault/releases), and a
summary is published at <https://bitrealm.io/changelog/>.
