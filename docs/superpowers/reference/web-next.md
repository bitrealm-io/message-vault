# web-next screens and features

A record of `web-next/`, the Next.js browse UI kept in the tree for evaluation,
captured on 2026-09-05 from the branch that made it an HTTP client of the vault
(pull request #415, commit `c01560e4`). The product UI is `web/`; this document
exists so the two can be compared feature by feature. The comparison at the end
records what each app offers today. A feature present in one and absent from
the other is a fact about the two trees, not a verdict on either.

The screenshots are of the running app against the demo vault
(`./scripts/run-vault-dev.sh --reset-demo`: 442 contacts, 382 conversations,
609,436 messages, no Contact Groups), in Chromium at 1360 × 860 with the
default dark theme. They live in [`web-next/`](web-next/).

## Running it

web-next reads the vault through the `/v1` HTTP API, the same way `web/`
does. It needs a running vault and Node 22.

```bash
./scripts/run-vault-dev.sh --reset-demo   # vault API on http://127.0.0.1:8080

cd web-next
npm ci
npm run dev                               # http://127.0.0.1:3000
```

Log in as `demo` with an empty password. The vault host comes from
`VAULT_API_URL`, default `http://127.0.0.1:8080`. `npm run gen:api`
regenerates the response types from `docs/src/assets/openapi.json`.

Two costs are worth knowing before browsing. The Group Messages list splits
every conversation by calendar year, which costs one count request per
conversation per year: about four seconds for the demo's 185 groups. A search
with body text runs through the message list, and on the demo vault an
unscoped text search takes about fifty seconds (issue #413).

## What works and what does not

Every read works: login, Home, the contact list and tree, direct and group
threads with paging, the source picker, find in conversation, the Group
Messages list, Trash, the three search modes, and the Settings pages.

Writes are not mapped. Every route handler that would change data answers 501
with "Not available: web-next reads the vault through /v1 and has not mapped
writes yet", so renaming, labelling, merging, trashing, importing a vCard,
changing the profile or password, and creating an account all fail visibly.
Undo and redo are not features of web-next: the history engine is unwired and
its controls are gone from the list menus.

Some reads have no `/v1` route and are approximated or absent. Display
preferences live in a browser cookie. Transcoded media is not served; HEIC and
MOV attachments show a notice with a link to the raw file. Trash timestamps,
per-message trash, "delete messages only", surrounding-context search,
relevance ordering, the import-source filter on lists, the duplicate-copy
count, contact CSV export, unassigned handles, demo reset, and Hanko login
are all listed with their nearest routes in issue #412. Two behaviours were
observed on this build: right-clicking a message search result opens no menu,
and clicking a message hit opens the conversation and then walks it back one
page of 80 at a time until it reaches the message, so a hit from 2014 in a
long thread takes a while to appear.

## Screens

### Login and account creation

`src/components/LoginScreen.tsx`. User ID and password with a show/hide
toggle. "Create a new account" unfolds a second form below: User ID, password
and confirmation, a "No password" option, display name, and a phone number
entered as a USA number or in international form with the E.164 value shown
beside it. Account creation posts to a route that answers 501 on this build.

![Login](web-next/login.png)

![Create a new account](web-next/create-account.png)

### Home

`src/components/HomePageClient.tsx`. Overview tiles for messages, contacts,
group chats and attachments; recent contacts with last-message date, group
count and direct-message count; a vault history panel with date range,
sent/received proportion, import sources and duplicate copies; and Explore
tiles for all contacts and contacts with no messages, the second of which is a
prefilled search.

![Home](web-next/home.png)

### Navigation

`src/components/AppSidebar.tsx`, `LabelsNav.tsx`, `VaultTitleMenu.tsx`. The
title menu holds Logout. Labels have a "+" popover for creating one, and each
label row carries a rename and delete menu. The navigation collapses to an
icon rail and its state persists.

![Vault title menu](web-next/vault-title-menu.png)

![Create label popover](web-next/create-label.png)

![Navigation collapsed to an icon rail](web-next/nav-collapsed.png)

### Contacts

`src/components/BrowseShell.tsx` and the `Browse*` components. Three
resizable panes: the contact tree, the thread reader, and a details inspector.
Rows show initials, name, formatted handle and a handle-type chip, with
optional message-count, group and date-range badges from Settings. Letter
headers group the list. A filter field narrows it. The toolbar has a contacts
sort menu (first name, last name, phone number, message count, group count),
a year filter for the group rows, a group-row sort menu (date, message count,
people) and a "⋯" actions menu.

![All contacts](web-next/contacts-all.png)

Expanding a contact shows a "Direct messages" row and the group conversations
the contact takes part in, collapsed to one row per conversation. Opening the
direct row fills the reader and the inspector: handles, message and
attachment counts, first and last message, and activity by year.

![Contact tree, direct thread and inspector](web-next/contact-direct-thread.png)

Checkboxes appear on hover; shift and ctrl extend the selection. The inspector
shows the selected contacts and the tree adds the group conversations they
share.

![Three contacts selected](web-next/contacts-selection.png)

Right-clicking a contact opens New contact, Edit, Labels and Delete contact.
"Merge into…" appears for a contact with no name. The list's "⋯" menu adds
Import VCF, Export contacts CSV and Delete group messages. Delete and merge
are disabled by a capability flag that predates this build.

![Contact context menu](web-next/contact-context-menu.png)

![List actions menu](web-next/list-actions-menu.png)

![Sort contacts menu](web-next/sort-contacts-menu.png)

![Year filter for group rows](web-next/year-filter-menu.png)

![Sort group rows menu](web-next/sort-chats-menu.png)

Clicking a name in the inspector opens the edit form: display name, a labels
picker, and handle rows with a type selector (Phone, Email, Username, Other)
that grow as they are filled.

![Edit contact form](web-next/contact-edit-form.png)

### Thread reader

`src/components/BrowseThreadPane.tsx`, `ThreadFindBar.tsx`,
`MessageAttachments.tsx`. A year strip jumps between years and follows the
scroll. The source picker shows Combined plus each import source that carries
the conversation. "Photos & files" narrows the thread to messages with
attachments. Messages page newest first; scrolling up loads older pages and a
floating "Newest messages" button jumps back. Find in conversation (Ctrl+F)
counts matches, steps through them and highlights the term. Day separators sit
between calendar days. Images render inline, other files as links, and missing
or unplayable media as labelled placeholders.

![Find in conversation](web-next/thread-find.png)

![Photos and files filter](web-next/thread-photos-files.png)

### Search

`src/components/VaultSearchField.tsx`, `AdvancedSearchForm.tsx`,
`Search*ResultsList.tsx`. The search field accepts web-next's own query words
and autocompletes people and labels for `with:`, `from:`, `to:`, `within:`
and `in:`. The chevron opens an advanced form with two tabs. Contacts: within
a label, handle, first name, last name, phone, first and last message dates,
group and direct message counts. Messages: within, from, to, with person, has
the words, doesn't have, subject, date, message type, source, attachment,
file type, filename, larger and smaller than, results mode, sort and context.

![Advanced search, Contacts tab](web-next/advanced-search.png)

![Advanced search, Messages tab](web-next/advanced-search-messages.png)

Results come in three modes. The default is one row per conversation with a
match count, snippet and date range. `group:none` gives one row per message
with sender, timestamp, text and attachment names. `search:contacts` gives
contact rows that expand to their matching conversations. Each list has a
select-all, a sort menu and a "Show more" row.

![Search results by conversation](web-next/search-conversations.png)

![Search results by message](web-next/search-messages.png)

![Search results by contact](web-next/search-contacts.png)

### Group Messages

`src/components/GroupMessagesShell.tsx`, `BrowseGroupChatsPane.tsx`. Every
group conversation, one row per conversation with its participants and message
count, with a year filter and a sort menu (date, message count, people).
Checkboxes select rows for trashing. Opening a row fills the reader and the
inspector with participants, counts, dates and activity by year. Selecting
several rows shows a selection summary.

![Group Messages list](web-next/group-messages.png)

![Year filter on the group list](web-next/group-year-filter.png)

![Sort menu on the group list](web-next/group-sort-menu.png)

![A group thread with its inspector](web-next/group-thread.png)

![Two groups selected](web-next/group-selection.png)

### Trash

`src/components/TrashShell.tsx`, `TrashUnifiedList.tsx`. Two tabs, Contacts
and Group Messages, each with a count; a search field; select-all; Restore and
Delete forever; and a read-only preview of the trashed item on the right. The
demo vault has nothing in Trash, and the vault reports no trashed-at time, so
the list is empty here.

![Trash](web-next/trash.png)

![Trash tab picker](web-next/trash-tabs.png)

### Settings

`src/components/Settings*Form.tsx`, `ThemeSettings.tsx`,
`DateTimeSettings.tsx`. Four tabs. Account: user ID, change password or sign
in without one, display name, phone numbers, and a danger zone for deleting
all messages or the account. Access: a view-only mode that blocks edits while
browsing, and one API token to generate or delete. Storage: attachment usage,
import history, and the largest attachments. Appearance: list badge toggles,
contact initials, light and dark theme with four colour seeds, a share string,
preset swatches, and date and time formats with custom patterns.

![Settings, Account](web-next/settings-account.png)

![Settings, Account, lower half](web-next/settings-account-bottom.png)

![Settings, Access](web-next/settings-access.png)

![Settings, Storage](web-next/settings-storage.png)

![Settings, Appearance](web-next/settings-appearance.png)

![Settings, Appearance, theme colours](web-next/settings-appearance-theme.png)

![Settings, Appearance, date and time](web-next/settings-appearance-datetime.png)

### Label sections

`src/app/no-label/page.tsx`, `label/[slug]/page.tsx`, `no-messages/page.tsx`.
The same browse screen scoped to one label, to contacts with no label, or to
contacts with no messages. The demo vault has no labels, so only "No label"
is reachable.

![No label](web-next/no-label.png)

## Feature inventory: web and web-next

What each app offers today, by area. The web column comes from `web/src/`
(the Vite SPA, with desktop-only items marked); the web-next column from
`web-next/src/`. A web-next entry describes what the screen offers; on the
current build, every write answers 501 as described above. "—" means the app
has no such feature.

### Login and account

| Feature | web | web-next |
|---|---|---|
| Log in with username and password | yes | yes |
| Create an account from the login screen | username, password, confirmation | user ID, password, confirmation, display name, phone, "No password" option |
| Passwordless login | — | yes |
| Onboarding profile setup | display name and up to five handles (phone, email, WhatsApp) | display name and phone number |
| Vault address and connection status | address field, test, connected/disconnected line | — (`VAULT_API_URL` at start) |
| Passkey (Hanko) login | — | present, unwired on this build |
| Log out | yes | yes |
| Change password | yes | yes |
| Delete all messages, delete account | yes, with confirmation | yes, with confirmation |
| API tokens | named tokens with import, export and delete permissions; rename, revoke, one-time reveal | one token: generate, delete, one-time reveal |
| Administer other users | yes (admins) | — |
| View-only mode | — | yes |
| Profile handles | phone, email, WhatsApp; add and remove | phone numbers; add and remove |
| Address-book file load | `.vcf`, `.vcard`, `.csv` from Settings | `.vcf` with a preview dialog mapping vCard categories to labels |
| Demo account | password change and deletion disabled | reset-demo entry pointing at the CLI |

### Navigation and layout

| Feature | web | web-next |
|---|---|---|
| Three-pane layout | navigation, list column, right pane; two resizable | tree, reader, inspector; all resizable, inspector collapsible |
| Collapsible navigation | sections collapse individually | whole navigation collapses to an icon rail |
| Home dashboard | — | tiles, recent contacts, vault history, explore |
| Deep links in the URL | query, filter, trash selection | contact, conversation, group, year, query |
| Recent searches | per scope, ten most recent | — |
| Search autocomplete | words, values and contact names from the vault | people and labels for person and label words |
| Mouse back and forward buttons | desktop | — |
| Keyboard shortcuts | search popdown, lightbox arrows, Enter in find | Ctrl+F find, Enter and Shift+Enter, Escape, Delete and Backspace |

### Contacts

| Feature | web | web-next |
|---|---|---|
| Contact list with A–Z sections | yes | yes |
| Filter the list as typed | yes | yes |
| Sort menu | first name, last name; ascending, descending | first name, last name, phone, message count, group count; ascending, descending |
| Row badges | initials; matching handles while filtering | initials, handle chip, message count, group icon, date range (each switchable) |
| Multi-select with select-all | yes | yes, with shift-range and ctrl toggle |
| Selected-contacts summary | sortable table of dates and counts | names and handles; shared group conversations in the tree |
| Contact tree with conversations under each contact | — | direct row plus group rows, collapsed per conversation |
| Unknown contacts page | server-computed group | — |
| Contacts with no messages | advanced search "Never messaged" | dedicated section and Home tile |
| Contact detail | drawer with sortable identity table, first and last seen, thread and message counts, group chips | inspector with handles, labels, direct and group counts, first and last message, activity by year |
| Rename a contact | inline in the drawer | edit form |
| Add or remove a handle | dialog with service select | form rows with Phone, Email, Username, Other |
| Create a contact by hand | — | yes, including from a group participant |
| Merge contacts | — | "Merge into…" for a nameless contact |
| Move a contact to Trash | yes | delete contact action, disabled by capability flag |
| Contacts CSV export | — | yes |
| Right-click context menu on a contact | — | yes |
| "Needs review" flag on an ambiguous handle | — | yes |

### Contact Groups, Message Tags, Saved Searches

| Feature | web | web-next |
|---|---|---|
| Contact Groups: create, rename, delete | yes | yes (called labels) |
| Membership menu with tri-state checkboxes | yes | yes |
| Per-group and no-group pages | yes | yes |
| Reserved names refused | yes | yes |
| Message Tags on conversations | create, rename, delete, assign, tag pages | — |
| Saved Searches | create, rename, delete, run | — |

### Conversations and threads

| Feature | web | web-next |
|---|---|---|
| Conversation list across direct and group | yes, sortable by date or messages | — (contact tree and Group Messages instead) |
| Thread header | title, participant chips, source, date range, count | name, year strip, source picker, attachments filter |
| Paging | Previous and Next over 50-message pages | newest first, older pages on scroll, "Newest messages" jump |
| Year filter | chip bar, loads the year in full | year strip in the thread; year filter on lists |
| Find in conversation | yes | yes |
| Sources | drawer with per-source counts and shares | picker that filters the thread by source |
| Attachments-only view | — | "Photos & files" |
| Message bubbles | per service: iMessage, SMS/MMS, WhatsApp, Instagram, Discord | one style with grouped runs and sender names |
| Tapbacks | badges with who reacted | — |
| Attachments | thumbnails, video player, lightbox, file chips, missing labels | inline images, file links, missing placeholders |
| Day separators | — (timestamp under every bubble) | yes |
| Move a conversation to Trash | yes | group conversations, from the list |
| Open a contact from a participant | chip opens the drawer | chip opens edit or create contact |

### Search

| Feature | web | web-next |
|---|---|---|
| Query language | the vault's words, with autocomplete | its own parser, re-spelled into the vault's words |
| Advanced search, messages | name or title, identity, type, participant count | within, from, to, with, words, doesn't have, subject, date, type, source, attachment, file type, filename, size, results mode, sort, context |
| Advanced search, contacts | name, no name, identity, no identity, service, first seen, last seen, activity | within, handle, first name, last name, phone, first and last message, group and message counts |
| Result modes | conversations or contacts, by route | conversations, one row per message, contacts with expandable matches |
| Search across every message | — (issue #313) | `group:none` |
| Open a hit at the message | find bar highlights on the page | opens the conversation and pages back to the hit; seeds the find bar |
| Load more results | infinite list | "Show more" row |
| Match highlighting | yes | yes |

### Group conversations

| Feature | web | web-next |
|---|---|---|
| Dedicated group list | — (in the conversation list) | yes, with year filter and sort |
| Group detail | header chips and sources drawer | inspector with participants, counts, dates, activity by year |
| Bulk trash from the list | select rows, then per-conversation | select rows, delete group messages |

### Trash

| Feature | web | web-next |
|---|---|---|
| Trashed conversations | list with restore | tab with restore, delete forever, preview |
| Trashed contacts | up to 100 with restore | tab with restore, delete forever |
| Search inside Trash | yes | yes |
| Delete forever | — | yes |

### Settings

| Feature | web | web-next |
|---|---|---|
| Storage: usage, import history, largest attachments | yes; import rows expand to a summary and the contacts created | yes |
| System: staging directory, ffmpeg | desktop | — |
| Appearance: light and dark, four seeds, share string, presets | yes | yes |
| Date and time format | — (browser locale) | modes and custom patterns with preview |
| List badge preferences | — | yes |

### Import and export

| Feature | web | web-next |
|---|---|---|
| Import from a backup | desktop: sources, gates, resume, progress, summary | — |
| Export to a folder | desktop | — |

### Cross-cutting

| Feature | web | web-next |
|---|---|---|
| Undo and redo | — | present in the tree, unwired |
| Read-only mode | — | yes |
| Confirmation dialogs | yes | yes |
| Theme applied before first paint | yes | yes |
