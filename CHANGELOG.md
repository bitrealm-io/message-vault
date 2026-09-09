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

## [0.9.0] — in development

The release that gives a vault an owner, a trash you can take things back out
of, and a search that reads the same everywhere.

### Features

- 2026-09-05: **A vault has an owner.** A fresh vault now asks you to create
  its owner before anything else, and that owner is the one account that
  manages the vault: create accounts, disable them, reset a password, delete
  someone's messages, and decide whether strangers may sign themselves up. The
  owner has no messages of their own and cannot read anyone else's — the
  account list shows a name, a message count and a storage total, and nothing
  of what those messages say. There is exactly one owner and it cannot be
  deleted. If you forget its password, `create-owner` and
  `reset-owner-password` on the server put you back in.
- 2026-09-05: **New accounts are closed by default.** A vault admits nobody the
  owner has not admitted, until the owner turns on public sign-up. An account
  the owner creates has to replace the owner's password the first time it
  signs in, so the owner never keeps knowing it.
- 2026-09-05: **Permanent delete, from the trash only.** Deleting a trashed
  conversation removes it, its messages, and any attachment no other message
  still uses. Deleting a trashed contact does what a phone does: the name and
  edits go, the contact becomes Unknown, and its conversations stay, showing
  the number. **Empty Trash** does both for everything in it.
- 2026-09-08: **Export history.** An export is recorded like an import: what it
  covered — everything, a search, or conversations you picked — and how much it
  handed over. Settings → Storage lists Export history beside Import history.
- 2026-09-05: **Convert**, a desktop tool under Settings that rewrites a folder
  of already-exported files into another format without touching a backup or
  the vault.
- 2026-09-05: **Times read in your zone.** Your account carries a time zone,
  chosen at setup and changeable afterwards, and every message time, day and
  year is shown in it.
- 2026-09-05: The vault server writes a proper log — one line per request, the
  full reason behind any internal failure, and a warning for work it could not
  finish.
- 2026-09-05: **Find in conversation**, and years that page like every other
  list.
- 2026-09-05: Make a Contact Group straight from a group conversation.
- 2026-09-05: Import accepts owner email addresses for SMS Backup+, and the two
  Android SMS sources share one form.
- 2026-09-04: **A trash you can undo.** Conversations and contacts can be set
  aside and taken back. Nothing in the trash is deleted, a trashed conversation
  can still be opened and read, and lists leave the trash out unless asked.
  Trash has its own advanced search.
- 2026-09-03: **An import names the contact.** When a backup knows someone's
  name and the vault does not, the import puts that name on the contact. A name
  you type, or load from an address book, replaces one an import supplied; a
  later backup spelling it differently does not.

### Fixes

- 2026-09-05: Long-running vaults no longer grow in memory for every file ever
  uploaded and every username ever tried.
- 2026-09-05: A failure now says what actually went wrong — the step that
  failed and the file or database error under it — rather than the outermost
  message alone.
- 2026-09-05: The published Docker image is built with the compiler the project
  tests against, not whatever the base image happened to carry.
- 2026-09-05: An obfuscated export no longer carries the original vendor data
  alongside the substituted text.
- 2026-09-05: A trashed contact is set aside rather than gone: an import that
  meets one of its numbers attaches to it and leaves it in the trash, and
  contact counts leave the trash out.
- 2026-09-04: Only the newest connection check may speak for the sign-in card,
  so a slow answer for an old address cannot overwrite a newer one.
- 2026-09-03: Importing a file the vault cannot read explains what is wrong —
  which version the file is and which the vault reads, or which line is bad —
  instead of "internal server error".
- 2026-08-30: Message Vault Settings no longer shows an address green when it
  has not tried it. An address typed but not tested reads **Not tested**.
- 2026-08-30: The sign-in and profile-setup pages no longer show a scrollbar on
  a screen tall enough to hold the card, so opening a dropdown stops shifting
  the card sideways.
- 2026-08-30: Profile setup refuses a phone number or address already in the
  list, marks the row that repeated it, and compares numbers regardless of how
  they are written. The same number on Text Message and on WhatsApp still
  counts as two.
- 2026-08-27: Desktop import no longer fails partway through with a source
  mismatch.
- 2026-08-27: Import errors group identical problems into one row with a file
  count, instead of one row per file.

### Design

- 2026-09-09: **An import brings back a contact you had trashed.** Until now
  an import that met the handle of a trashed contact attached to it and left
  it in Trash, so someone you set aside once never appeared in Contacts again
  however many newer backups you imported. A backup that still holds the
  person means you still talk to them: the import now discards the trashed
  contact, name, group memberships and handles included, and makes a new
  contact from the backup, the way a first import would. The forecast of new
  contacts shown before an import counts them. To keep someone out for good,
  delete them from Trash. Why: `docs/adr/0013-an-import-replaces-a-trashed-contact.md`.
- 2026-09-09: **An Import Run says what it did to each contact.** The run
  records, as it goes, whether it created a contact, created one in place of
  a trashed contact, named one that had no name, or added a handle to one,
  and the run's record under Settings → Storage lists the contacts with that
  reason. The new and changed counts come from the same record instead of
  being guessed from timestamps afterwards.
- 2026-09-09: **Importing SMS Backup+ mail reads one message per file, and
  nothing else.** Message Vault briefly also read a second kind of `.eml` — a
  whole conversation written out as a dated transcript in one mail. That shape
  is not something SMS Backup+ produces, and every message in the only known
  collection of them was already present as ordinary SMS Backup+ mail, so
  reading it added a second copy of messages the vault already had. Support for
  it is gone. Importing a folder of SMS Backup+ mail is unchanged.
- 2026-09-08: **The HTTP interface was rebuilt on one set of conventions.**
  Every list pages and sorts the same way — the browse lists and the ones you
  curate alike, with no list left answering a bare array — every failure comes
  back in the same shape with a link to a page explaining that kind of failure,
  every created thing answers with its address, and every response carries an
  id you can quote when reporting a problem. Signing in, and everything to do
  with accounts, each moved to one address serving everyone, with the vault
  deciding what a given caller may see rather than the address saying it. A
  single message can now be read by its id, so a search result links to the
  message rather than to a position in a list. Parameters that no longer did
  anything are gone — nothing asks you to name your account when your key
  already says it — and import history sorts the same way export history does.
  This matters if you wrote something against the interface yourself; nothing
  in the app or the desktop app changes.
- 2026-09-05: The vault and the desktop app now convert media with the same
  code, so a video converted on import and a preview generated later can no
  longer differ. Previews are better quality than before.
- 2026-09-05: Import progress is reported by the exporters directly rather than
  read back out of their log text, so the progress bar can no longer be broken
  by a wording change. An encrypted iPhone backup now narrates its setup steps
  instead of sitting on "Reading backup…".
- 2026-09-05: The desktop app reads Apple Messages through a small separate
  program shipped beside it, because the library it uses carries a licence that
  cannot be combined with the app's. Nothing changes on screen.
- 2026-09-05: Release builds of the server and the desktop app are optimised
  and stripped, so they are smaller and faster.
- 2026-09-03: **One search language.** The search box, saved searches and the
  export filter are compiled by the same code, so a query means the same thing
  everywhere it is typed.
- 2026-08-30: The vault accepts the packaged desktop app without being
  configured to. A vault built from source used to refuse it in a way that
  looked like an unreachable server.
- 2026-08-27: Importing is substantially faster — messages, attachments and
  reactions are written in batches rather than one database call at a time.
- 2026-08-26: Import lists one **iMessage** source with a choice of Mac
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

- A grey, green or red light beside the server address on the sign-in screen,
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

- Sign-in leads with Connect or Sign in. Extracting and converting files moved
  off that screen; importing a backup after signing in is unchanged.
- Closing the desktop window signs out, so the next launch asks you to sign in.
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

The release where Message Vault became one product: a vault you sign in to and
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
- **Accounts and sign-in**, with profile setup, appearance themes, and a danger
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
- Sign-in explains an insecure-to-secure address mismatch instead of failing
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
