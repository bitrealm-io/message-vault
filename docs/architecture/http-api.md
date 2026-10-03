# The HTTP interface

Every rule the server's `/v1` interface follows, in one place. The interface is
part of the architecture, so its rules are written down when they are decided,
not when the code catches up: a design is argued against this file, a pull
request that touches a route is graded against it, and a route that breaks a
rule here is a bug. Each rule carries its reason, and where an alternative was
weighed and turned down, one line says so, so the question is not reopened by
accident.

This file states what every route must do. The generated reference (see
[The reference](#the-reference)) states what each route does today. Where the
two differ, an open issue names the routes still to change; this file never
lists that work itself. A change to a rule is made here, in the same pull
request as the code when the code changes with it. Why the rules live here and
not in `docs/adr/`: `docs/adr/0011-the-http-interface-has-one-rules-document.md`.

## Identifiers

Every resource with a row is addressed by its integer id in the URL:
`/v1/contacts/{id}`, `/v1/accounts/{id}`, `/v1/accounts/{id}/api-tokens/{id}`.
Names are for people: they appear in the web app's routes, in the search
language, and in the text of a Saved Search, and the web app turns a name into
an id inside the module that owns the collection, never in a screen.
Why: a name is mutable, needs URL-encoding, and must be matched
case-insensitively on every request, and a rename would re-key the resource.

The one exception is an Asset, addressed by the SHA-256 of its contents:
`/v1/assets/{sha256}`. Why: the file exists before the database does, the client
must know the hash before an upload can be deduplicated, and two uploads of
one file must be one asset.

An asset's Preview has no address of its own. It is
`/v1/assets/{sha256}/preview`, under the hash of the original, and the
attachment says whether there is one in `preview_mime_type`.
Why: the client holds the original's hash and has no use for the preview's, and
a second hash on the attachment would be a second thing to address by.

Rejected: the name in the path for Contact Groups and Message Tags. It keeps
every reference to a group the same kind of thing, and it makes the rule
"id, except where the name is unique". One rule is worth more than the
symmetry.

Rejected: opaque string ids for accounts and API tokens. They were UUIDs from
the first draft with no reason recorded. An integer in a URL tells a stranger
nothing they can act on, because every route checks the caller against the
row.

## Words on the wire

Paths, fields, parameters, summaries and problem details use the words
`CONTEXT.md` defines, and never the words it lists under _Avoid_: an
Identity, never a handle (`/v1/contacts/unmatched-identities`); a
Conversation, never a thread; an Import Run, never an import session. "Handle"
stays the name of a table and nothing a client sees.
Why: the web app, the docs and the interface name one thing one way, and the
generated types carry the interface's words into the web app's code.

## Naming a route

A collection is plural, and a member is `/{collection}/{id}`. A singular path
is legal only for a singleton: one per installation (`/v1/server`), or one per logged-in
credential (`/v1/session`). `/v1/trash` is a singleton by the same rule.

The Demo Account is managed as a singleton of the server,
`/v1/server/demo-account`: `GET` says whether it exists, is being built, or
failed to build, and `PUT {size}` adds or resets it. It is deleted as any
account is, `DELETE /v1/accounts/{id}`. Why: adding it cannot be a verb on
`/v1/accounts/{id}`, because there is no member to address until it exists,
and there is at most one per Message Crate.

A path segment names a resource, never a caller's role. Who may call a route is
decided in its handler, so a change to permissions never renames a URL.
`/v1/accounts` is one collection for the owner and for the account itself;
there is no `/v1/owner/` and no `/v1/account`.

A verb is legal only as a sub-resource of a member, and only when the action is
not a field write: it crosses a state machine, touches rows other than the
addressed one, or has no field to write. `claim`, `complete`, `cancel`,
`discard`, `trash` and `restore` pass. Setting a run's progress marker does
not; that is `PATCH /v1/imports/{id} {stage}`. A verb that merely spells an
HTTP method never passes: deleting an account is `DELETE /v1/accounts/{id}`.

Membership lives under the collection that owns it:
`GET` and `PATCH /v1/contact-groups/{id}/members`, with `{add, remove}` id
lists.

A read whose selector is too large for a query string is a `POST` named for
what it returns, never for the verb that computes it:
`POST /v1/contacts/summaries`, `POST /v1/contacts/unmatched-identities`,
`POST /v1/contacts/address-book`.

A choice between two different lists is a path segment, never a parameter:
`/v1/search-fields/contacts`, `/v1/search-fields/conversations` and
`/v1/search-fields/messages` are three fixed lists, not one list read with
`?list=`. Why: a parameter narrows a list;
choosing which list to read is choosing a resource, and the path does that.

Two levels of nesting. The multipart upload,
`/v1/assets/{sha256}/uploads/{upload_id}/parts/{part}`, is the one exception at
three. `/health` is the only route outside `/v1`. Path segments are
kebab-case; every field and query parameter on the wire is snake_case.

Rejected: no verbs at all. Modelling every action as a field write turns
trashing into `PATCH {"trashed": true}`, and trash is the only door to
permanent deletion, which `POST .../trash` says and a field write does not.

Rejected: a role prefix such as `/v1/owner/`. It reads well until permissions
change and a URL has to be renamed to match.

Rejected: `/v1/auth/login` and its siblings. `/v1/auth` was neither a
collection nor a singleton, so the verb rule could not reach it. A Session is
one per logged-in credential, which makes it a singleton, and login, logout and
check are its `POST`, `DELETE` and `GET`.

## Methods

`PATCH` updates part of a resource. `PUT` replaces one. `POST` creates, or
runs an action named by a verb sub-resource. `DELETE` removes. A read is a
`GET`, except the large-selector `POST` above.

A read by id takes no filter. `GET /v1/conversations/{id}/messages` opens a
conversation; searching inside one is `GET /v1/messages?q=in:#{id} …`.
Why: opening and searching answer different questions. The read by id answers
`404` for an id the caller does not hold and shows a conversation in the
trash; a search answers an empty page and leaves the trash out. A filter on the
read by id is a second search that can drift from the first, as `?year=` beside
`date:` did.

A file the server reads is the request body, with `Content-Type` naming its
format, and anything about how to apply it is a declared query parameter.
`POST /v1/contacts` takes the address book as a `text/csv` body and
`mode=append|edit`; any other `Content-Type` is `415 Unsupported Media Type`.
The file the server writes is answered by a `POST` named for it,
`POST /v1/contacts/address-book`, whose body `{q, ids}` selects the contacts
and whose answer is `text/csv` with a `Content-Disposition` filename.
Why: a body that is the file cannot carry a mode field, and a JSON envelope
around a file base64-encodes it for nothing. Rejected: `?format=csv` on
`GET /v1/contacts`, which cannot carry the checked ids and is the `fields=`
idea in another coat; `Accept: text/csv` on the list, which makes one list
negotiate where every other answers JSON.

Behaviour that differs by caller lives inside one handler, not in two routes.
`PUT /v1/accounts/{id}/password` is one route: the owner sets another
account's password without the current one, a user account changes its own on
its session alone, and the owner changing its own must supply the
current one, because that account reaches every other.

## Status codes

- A creation answers `201 Created` with a `Location` header naming the new
  resource, whatever the method that made it: a `PUT` that stores an asset the
  database did not hold, and a claim that makes the owner's Session
  (`Location: /v1/session`), both answer `201`. A create that takes a batch
  answers `200 OK` with a summary of what was created, updated and skipped,
  because no single resource was made.
- A write with nothing to return answers `204 No Content`.
- A write the server finishes after it answers is `202 Accepted`, with the
  resource in its body and a `status` that says the work is under way. The
  client reads the resource with `GET` until `status` changes; a second write
  while the first is under way answers `409`. `PUT /v1/server/demo-account`
  is the one such route: it removes the Demo Account and builds it again,
  which takes from seconds to a minute, and answers with `status` `building`.
  Why: an answer held for a minute is lost to a closed tab or a proxy's
  timeout while the work carries on unseen, and `200` would say the Demo
  Account is there when it is not yet.
- A name collision answers `409 Conflict`. So does an action on a resource in
  the wrong state: deleting before trashing, claiming a claimed Message Crate, a batch
  or a `complete` on a finished run.
- A failed credential answers `401 Unauthorized`. A refused one, including a
  token without the needed scope and a disabled account, answers
  `403 Forbidden`.
- A `Content-Type` that is absent or unaccepted answers
  `415 Unsupported Media Type`, on every route that takes a body. On a route
  whose body is optional, a request with no body is read as no body, and a
  body sent without a `Content-Type` is still `415`.
- A request that cannot be read is `400 Bad Request` (`malformed-body`): JSON
  that does not parse, or a body of the wrong type. Nothing else is `400`.
- A request that was read and broke a rule is `422 Unprocessable Entity`,
  whether the rule was on a query parameter, a path segment or a body field: a
  field or parameter missing or blank, a value out of range, an id that names
  no row the caller holds in a list of ids to add, a body that does not match
  the hash it is addressed by, and a search query that uses a word its list
  does not have. Axum's own rejections follow the same line. A missing
  parameter is one entry in `validation-failed`'s `errors`, not a problem type
  of its own. A search that does not parse keeps its own type,
  `search-query-invalid`, because the client's remedy differs (rewrite the
  query), and answers `422` like every other.
- A request that was read, broke no rule, and completed answers a `2xx` even
  when it changed nothing. The status code says whether the action ran or
  failed; what the action found or changed is the body's job. An id in the
  `remove` list of a `{add, remove}` membership patch that names no member is
  ignored: the set is already in the state the caller asked for, so the route
  answers `200 OK` with `{added, removed}`, `removed` counts only the rows
  deleted, and a retried request succeeds. An id in `add` that names no row
  the caller holds stays `422 Unprocessable Entity`, because adding a row the
  caller does not hold breaks a rule rather than completing with an empty
  result.
- `429 Too Many Requests` carries `Retry-After`.
- An unknown `/v1` path answers `404` as a problem document, and a wrong method
  `405`, never Axum's plain text.

There is no `ok` flag on any success, and none in any request: the status
carries the meaning, and a run's outcome is its `status`.

Rejected: a search query that does not parse as `400`, "the query could not be
read". The request was read; the query is a value that broke the rules of the
search language, which is what `422` means. One line with no exception is
easier to hold than a line with one.

## Lists

Every list route answers a page, `{items, total, limit, offset}`, and takes
`offset` and `limit`. No exceptions: a list the person curates by hand
(groups, tags, saved searches, API tokens), a fixed reference list
(`/v1/search-fields/contacts`), and a `POST` that reads all answer a page. The
list key is always `items`.
Why: the web app has one paged type and one hook, and a second shape is a
second convention.

A `POST` that reads the rows its body names — contact summaries, unmatched
identities — answers the whole of that body as one page and takes no `offset`
or `limit`: `total` is the row count, `limit` is the cap the body is held to,
`offset` is 0. Why: the body already says which rows to read and how many it
may name, so a second bound would only let a caller ask for rows it did not
name, or hide rows it did.

A count that describes the whole set rather than the page belongs on the
resource the set hangs off, not beside `items`. An Import Run's tally of
contacts created and changed is on the run's own record, and the contacts are
a page. Why: a page can only count its own rows, and a field beside `items`
that counts something else is a second shape.

`limit` is at least 1 and at most 500, default 40, on every list including an
Export Run's messages. `offset` is at most 50 000 on the browse lists. A value
outside the range is `validation-failed`, never a silent clamp. One
conversation's messages, `GET /v1/conversations/{id}/messages`, is not a browse
list and has no `offset` cap. Why: every message of a long conversation must
be reachable, and a cap would leave the rest of a long thread out of reach.

One conversation's messages can also be read beside one message, in place of
`offset`: `around={message_id}` answers the page with that message in the
middle, and `before={message_id}` and `after={message_id}` the page just
before or just after it in the page's order, without it. The answer is the
same page, and its `offset` says where the page sits, so `total` and the
position stay known. A request sends at most one of `offset`, `around`,
`before` and `after`, and a message the conversation does not show (another
conversation's, a duplicate, or none) is `validation-failed`. Why: a jump to a
message (a search result, a Find match, the first message of a year) does not
know the message's offset, and a screen that scrolls from there reads the next
page from the message at its edge rather than from a number that an import
or a deletion in between would shift. This is not the cursor paging rejected
below: the page keeps `total` and `offset`, and the parameters name a message,
not an opaque token.

Sorting is `sort=-field,field`: comma-separated keys, a leading `-` for
descending. Each list declares the keys it accepts, and an unlisted key is
`validation-failed` naming the accepted set. There is no separate `order=`.

Filtering is the search language in `q`, and nothing else. The one exception
is a list with no search language, which may take a filter parameter whose
values are the ones its rows store. The Import Run and Export Run lists are the
only such lists: `GET /v1/imports?status=running`,
`GET /v1/exports?status=completed`, and their twins under an account. There is
no `fields=` selection.

A query parameter a route does not declare is `validation-failed`, naming the
parameters the route accepts. Why: a typo (`limt=10`) or a guess at a
convention this file rejects (`order=`, `fields=`, `year=`) would otherwise be
answered as though it had been obeyed.

Rejected: cursor paging. Stable under concurrent inserts, but nothing inserts
rows under a running read on a self-hosted server, and every screen that shows
"51–100 of 4,213" needs `total`.

Rejected: a bare `{items}` for small lists. One justified exception is still
two conventions, and a group's member list has no bound the server enforces.

Rejected: `fields=`. It turns one resource into many shapes, and the web app's
generated types could only express that with every field optional.

Rejected: query-parameter filters beside the search language. `?date_gte=2019`
next to `q=date:>2019` is two ways to ask one question.

Rejected: ignoring a query parameter the route does not know, the forgiving
default of most web servers. The server's clients are its own apps and programs
written against the reference, and a silent wrong answer costs them more than
a refusal.

## Failures

Every failure answers an RFC 7807 problem document as
`application/problem+json`, carrying `type`, `title`, `status`, `detail` and
`request_id`. A validation failure replaces `detail` with `errors`, a list of
every rule that broke rather than the first.

`type` is the URL of a page under
`messagecrate.app/docs/developer/reference/errors/`, one page per problem type.
The code is the registry: each type is declared once in
`crates/server/server/src/problem.rs`, the pages are generated from it, and a
test fails when the checked-in pages drift. Only `500 Internal Server Error`
uses `about:blank`, because a page about it could say nothing a reader could
act on. The taxonomy is per problem, not per status: the test for a new type
is that a client's remedy differs. A closed registration is its own type,
because the remedy (ask the owner for an account) is not the remedy for a
caller who is not the owner.

Every response, success or failure, carries an `x-request-id` header, a UUID v4
the server makes; a request id a client sends is ignored. Problem bodies repeat
it as `request_id`. The id joins the request's tracing span so every log line
under a request carries it. RFC 7807's `instance` stays unused, because it is
defined as a URI reference and a bare id is not one.

Rejected: a flat `{error}` body. It gives a client nothing to branch on except
the sentence.

Rejected: a request id on `5xx` only. A body whose members vary by status is
the drift the one-shape rule exists to stop.

## Content negotiation

`406 Not Acceptable` is answered only when an `Accept` header is present and no
member of it matches `application/json`, `application/problem+json`,
`application/*`, or `*/*`. `application/*` is a media range that matches
`application/json` (RFC 9110), so refusing it would refuse a client that asks
for JSON. A missing `Accept` is a request for JSON. The check runs on every `/v1` route
but the three that answer bytes: `GET /v1/assets/{sha256}`, which streams the
asset's own contents, `GET /v1/assets/{sha256}/preview`, which streams its
Preview, and `POST /v1/contacts/address-book`, which answers the address book
as `text/csv`. Nothing outside `/v1` is checked.

Rejected: requiring `Accept: application/json`. None of the server's own clients
send one, and the rule would refuse the web app on its first request.

## Credentials and reach

Two credentials exist, and the OpenAPI document declares each as a security
scheme with its scopes, so every route says which it accepts.

- A **Session** is one per logged-in account or owner, made by
  `POST /v1/session` and ended by `DELETE /v1/session`. It carries the
  account's own permissions: `import`, `export`, `delete`. The owner's session
  carries none of those and reaches only the accounts collection, the server
  settings and the installation's storage totals, because the owner holds no
  messages.
- An **API token** is a named credential an account makes for a program, with
  the scopes the person chose from `import` and `export`, capped by the
  account's own. The cap holds on every request, when the token is made, and
  when it is listed, so the list shows what the token may do now: a scope the
  owner turns off later shows as off. A token never carries `delete`: permanent deletion is a
  person's act, and a leaked or faulty program must not be able to empty an
  archive. A token never signs in and never browses. It is ended by the person
  or the owner revoking it (`DELETE /v1/accounts/{id}/api-tokens/{token_id}`,
  with a session) or by its expiry, never by the program holding it.
- `GET /v1/session` answers whose credential the caller holds — the account's
  id and username — for a session or a token. Why: a program holding a token
  needs to know which account it writes to before it starts, and push and pull
  label their work with it. `DELETE /v1/session` refuses a token with `403`,
  because a token is not a Session and a `204` would say something ended when
  nothing did.

What each reaches:

- Browse routes (conversations, messages, contacts, groups, tags, saved
  searches, search fields) take a session only. A token is refused.
- `POST /v1/imports` and everything under a run, and every asset write, need
  the `import` scope on either credential.
- `POST /v1/exports` and everything under a run, and
  `HEAD /v1/assets/{sha256}`, need the `export` scope on either credential. A
  program with an export token reads messages only through an Export Run it
  started, so every read of message data by a program leaves a record.
- `GET /v1/assets/{sha256}` takes any session, or a token with the `export`
  scope. Why: a person looking at a photo in their own conversation is not
  exporting it, so an account whose `export` permission is off still sees its
  attachments, as it still reads its messages. A token has no screen to show
  bytes on; fetching them with one is taking them out, which is what the
  `export` scope decides.
- `GET /v1/assets/{sha256}/preview` is read under the same rule as the asset
  it was made from, by the same account and nobody else. Why: a Preview is the
  attachment's content as much as the original is, so a caller who may not
  read one may not read the other, and the owner reads neither.
- `HEAD /v1/assets/{sha256}` also accepts the `import` scope: a program that
  can only push may ask whether an asset exists, and may not read it.
- Permanent deletion (`DELETE /v1/conversations/{id}`,
  `DELETE /v1/contacts/{id}`, `DELETE /v1/trash`,
  `DELETE /v1/accounts/{id}/messages`, `DELETE /v1/accounts/{id}`) needs a
  session: the account's own with the `delete` permission, or, for an
  account's messages and for the account itself, the owner's. A token is
  refused whatever its scopes.
- An account may do everything with its own messages, deleting them and
  itself included, unless the owner limits it. Deleting an account deletes
  every message it owns, so an account whose `delete` permission is off
  cannot delete itself either: it is refused with `403 Forbidden` and asks the
  owner, who can. No separate permission for closing an account exists,
  because one that allowed it without `delete` would undo the owner's limit.
- `/v1/accounts/{id}` and everything under it is read and written by the owner
  or by that account; a `Location` handed to a newly registered account names a
  row it may read.
- An account's API tokens are listed and revoked by the account and by the
  owner, and made and renamed by the account alone. The owner's list holds
  each token's id, label, permissions, creation, last use, expiry and state,
  and leaves out `token_hint`. Why: the owner must be able to end a
  credential that has leaked, and a token's label and permissions are not
  message content (`docs/adr/0008-the-owner-holds-no-messages.md`). The
  masked hint is part of the secret, and the owner never reads a secret.
  Making a token answers with its secret, so the owner makes none, and a
  token is the account's own name for its program, so the owner renames none.
- An account's history is read under the account:
  `GET /v1/accounts/{id}/imports`, `GET /v1/accounts/{id}/imports/{import_id}`
  and `GET /v1/accounts/{id}/exports`, beside `GET /v1/accounts/{id}/storage`.
  They ask who is calling and no permission, so the owner reads them, and so
  does an account whose `import` or `export` permission is off. `/v1/imports`
  and `/v1/exports` are the pipelines' routes: they ask for the permission,
  which the owner's session never carries, and a program's token reaches only
  them. Each pair answers from one function, so the two lists cannot differ.
  Which contacts a run created is content, so `/v1/imports/{id}/contacts` has
  no twin under the account.
- The account reads its own runs in full. The owner reads each run as an
  `OwnerImportRun` or `OwnerExportRun`: the source, mode, tool, times,
  outcome and counts, with the counts an import's summary reported and how
  many issues it recorded, and for an export only which form its scope took.
  Why: a staging summary lists the addresses of everyone in the backup, an
  issue names its conversation's file, and an export's query is a search over
  the account's messages, all content under
  `docs/adr/0008-the-owner-holds-no-messages.md`. The owner's view is a type
  of its own rather than the account's with fields removed, so a field added
  to a run reaches the owner only when someone adds it to that type.
- `GET /v1/server` and `POST /v1/server/claim` take no credential.
  `/v1/server/settings` and `GET /v1/server/storage` are the owner's: the
  storage totals sum every account, and no account holds more than its own.
- The attachment size limit is a server setting, `asset_max_bytes` in bytes,
  and the settings row is the only place it lives. The owner reads and
  changes it at `/v1/server/settings`; `GET /v1/server` reports it to any
  client. Why: a program that uploads has to know the limit before it
  prepares attachments, and it holds an account's credential, never the
  owner's. A change is `422 Unprocessable Entity` only when it is zero or
  more than the database can hold, and an accepted one holds from the next
  upload, because every upload reads the setting. The part size a multipart
  upload is told is the configured one or the limit, whichever is smaller,
  worked out when the upload starts. Why: the limit is the owner's to set,
  and no value of it, and no edit to the config file, may leave a server
  that refuses to start or an upload that cannot complete. Rejected:
  refusing a limit below the configured part size, and stopping `serve`
  when the config file's part size is above a stored limit.
- The attachment size limit holds the attachment uploads only:
  `PUT /v1/assets/{sha256}` and the start of a multipart upload. Every
  other route has a body cap fixed in the code. Why: the owner may set any
  limit of 1 byte or more, and a limit that held every body would refuse
  the login and the settings change that raise it again.
- Each part of a multipart upload is held to the part size its upload was
  told when it started, which the upload's manifest records, and not to the
  limit as it is now. Why: an upload in progress keeps the limit it started
  with. Lowering the limit mid-upload otherwise refused every remaining part
  with `413 Payload Too Large`, and the push failed the conversation (#1179).

The credential names the account. No route takes an `account=` parameter.

Rate limiting guards the three routes that take no credential and make one,
over a 60-second window; the limit is documented in the developer reference.
`POST /v1/session` counts per account, because it guards one account's
password: by the account's id when the username names one, and otherwise by
the username folded the way the lookup folds it, so no spelling of a username
gets a count of its own. A wrong `current_password` sent to
`PUT /v1/accounts/{id}/password` or `DELETE /v1/accounts/{id}` counts in the
same per-account count, and past the limit those routes answer `429` without
checking the guess: an open session must not guess faster than the login.
`POST /v1/accounts` and `POST /v1/server/claim` count once for the
whole server, because they guard against a flood of new accounts, and a count
per name lets a script that tries a new name each time straight through.

Rejected: counting registrations by the caller's address. Behind a reverse
proxy every visitor shares one address, and believing a forwarded address
needs a list of trusted proxies that is easy to get wrong. A self-hosted server
takes a handful of registrations, so a server-wide count never stops a person.

## Runs

An Import Run and an Export Run are recorded permanently, whether they
completed, failed or were cancelled, and the client closes them: a run's
settings are stated once on creation, never per batch or per page, and
`complete`, `discard` (imports) and `cancel` (exports) are the only ways out.
There is no sessionless import and no unrecorded export.

A run that has finished answers `409` to `complete`, `discard`, `cancel`, a
batch, and a change of stage; its record is never rewritten. Its outcome is
stated once, as `status`. Why: the finished record is the history the person
reads, and a cancelled run marked completed afterwards would lie. A program
unsure how a run ended reads it with `GET` rather than repeating the call.

Rejected: a repeatable `complete`, answering `200` when the run already ended
the same way. It makes one call safe to retry at the cost of a second rule,
and `GET` already answers the question a retry is asking.

An Export Run's scope is one of three forms, stored as given: everything the
account holds; a query in the search language; or picked `conversation_ids`
and `message_ids`. The record holds what was asked for and how much matched,
never what the messages said.

A `query` scope names the list its query is for, in `list`, and has no
default: `{"kind": "query", "list": "conversations", "q": "messages:>100"}`.
`messages` compiles `q` on the Messages list and hands over the messages it
matches. `conversations` compiles `q` on the Conversations list and hands over
every message of each conversation that list shows for it, as opening each
one would: the list's own treatment of the Trash decides which conversations,
and a duplicate message is left out.
Why: each list has words the other does not (`messages:` on Conversations,
`from:` on Messages), and for a word both have, the Conversations list shows
whole conversations where the Messages list shows single messages. A person
who exports the conversation list they are looking at expects those
conversations, so the scope must say which list was meant.
Why a field, when [Naming a route](#naming-a-route) makes a choice between two
lists a path segment: that rule is about which resource a route reads. Here
the resource is one, the Export Run, and the list is part of what the run was
asked for, stored with it and read back.

Rejected: guessing the list from the words in the query. A query made of
words both lists have compiles on either and means something different on
each.

An Export Run is a snapshot taken when it is created. In the transaction that
records the run, the server lists the ids of the messages the scope matches,
each at a numbered place (oldest first), and computes the four counts
(messages, conversations, distinct attachments, bytes) from that list.
`GET /v1/exports/{id}/messages` pages the list, never the scope again, and
`complete` or `cancel` deletes it.
Why: re-running the scope for every page let an import, a trash or a new day
between pages move the offsets, so pages skipped or repeated messages and
`import:last` or a relative date could mean something else by the last page.

- A page's `total` is always the run's `message_count`. `offset` and `limit`
  address places in the list, and `sort=-date` counts them from the end.
- A message that matched at creation is handed over even if its conversation
  is trashed afterwards, because the run asked for it when it could still be
  read. A message imported afterwards is not handed over.
- A message deleted permanently afterwards leaves its place empty: its page
  holds fewer than `limit` items, and the places after it do not move. A
  client steps `offset` by `limit` until it reaches `total`, never by the
  items it got, and never stops on a short or empty page.

## The reference

The generated reference, `docs/src/assets/openapi.json`, is produced from the
handlers by `message-crate-server dump-openapi` and checked in; a test fails
when the two differ, and CI checks the web app's generated types against it.

An operation's error responses are built from shared parts, never written out
by hand. The credential a route accepts brings its `401` and `403`; a request
body brings `400`, `413`, `415` and `422`; an id in the path brings `404` and
`422`; and every `/v1` route brings `422` for a query parameter it does not
declare. The handler adds only what is its own, such as `409` for a run in the
wrong state, by naming the problem type (`crate::problem::openapi`).
Every error response is declared as `application/problem+json`, names the
problem types it can carry (in its description and in `x-problem-types`), and
has a description. The first sentence of a
handler's doc comment is the operation's summary, and the rest is its
description.
Why: every mismatch between the reference and the handlers that the September
2026 review found was in a hand-written list.

A rule that can be checked by walking every operation in the document is
checked that way, by one test, as `openapi/credential_matrix.rs` checks every
route's reach: the page shape and paging parameters on every list, a
`Location` on every `201`, a problem document on every failure, `401` without
a credential, a refused unknown query parameter, `415` for a body without an
accepted `Content-Type`, `400` for a JSON body that is not JSON, kebab-case
paths and the nesting depth. Why: a rule checked one route at a time is
checked on the routes someone remembered.

The shared failures are checked by calling each operation into them, and the
status and problem type the server answers must be ones the document lists.
Reading them back from the document proves nothing, because the shared parts
wrote them there from the same inputs the reading would use.

## Code

A route group is one module named for the route's first path segment, with
`_api`: `contacts_api`, `conversations_api`, `imports_api`, `exports_api`,
`assets_api`, `search_fields_api`, `session_api`, `server_api`, `trash_api`.
Contact Groups and Message Tags, one shape served twice, share
`named_set_api`. Why: a route's code is found from its URL without searching.

A handler is named `verb_noun`, with no `_handler` suffix. The verb is `list`,
`get`, `create`, `update` (`PATCH`), `replace` (`PUT`) or `delete`, or the
action's own verb: `list_contacts`, `get_contact`, `update_contact`,
`claim_server`, `complete_import`.

A type on the wire is named one of two ways, and a reader can tell which from
the name:

- A thing the interface hands out is named for what it is, with no suffix:
  `Message`, `Account`, `ApiToken`, `Contact`, `ContactSummary`, `Identity`,
  `ImportRun`. It keeps that name wherever it appears.
- An action's input and output are named for the action:
  `VerbNounRequest` for a body sent in, `VerbNounResponse` for an answer that
  is not a thing (`CreateApiTokenRequest`, `DeleteMessagesResponse`), and
  `VerbNounQuery` for a query string (`ListContactsQuery`).

No other endings: no `Body`, `Payload`, `Input`, `Patch`, `Item`, `Info`,
`Detail` or `Row`. Why: these names become the web app's type names, and
`Message` next to `MessageResponse` would leave a reader asking whether they
are one thing or two.

A handler reads the request, checks the caller and shapes the answer; it holds
no SQL. Every query lives in `db/`, in the module for the table it is chiefly
about, and it is written for SQLite, the only database engine
(`docs/adr/0017-sqlite-is-the-only-database-engine.md` has the rule and its
reason). Why no SQL in a handler: a query written into a handler gets written
twice (the identity counts for contacts and for accounts were).

Does the rule stop at handlers? No. An import stage (`imports_api/staging.rs`,
`imports_api/promote.rs`) is not a handler, but it holds no SQL either: it
sequences its statements, logs them and keeps the counts, and the statements
are in `db/staging.rs`, the module for the staging tables. Why: the point of
the rule is that the SQL for a table is found in one place, and a stage that
carried its own statements would be a second place for `staging_messages`.

A promotion statement reads a staging table and writes a production one in
the same `INSERT ... SELECT`, and it belongs with the staging tables rather
than the production one, because the staging tables have no reader but the
import while the production tables have many. The messages watermark and
count a promotion takes before it inserts live there too, for the same
reason: nothing but a promotion reads them.

A test of a route's answer goes through the router, checks a failure with
`expect_problem` (status, `type` and `request_id`, not only the sentence), and
takes its database and account from the shared fixtures in `test_support.rs`.
A rule every route follows is tested once over the whole document, as above,
not again per route.

## Versioning and change

`/v1` is a whole number in the path. There is no compatibility promise behind
it: endpoint names, request and response shapes, and stored formats change
whenever a better design is found, and breaking a client is an accepted cost.
No compatibility alias, deprecation window or version handshake is ever added.

The server says which code it runs, and an app says which code it is, and
neither decides anything. `GET /v1/server` carries the server's Build in
`version` and the Schema Fingerprint in `schema_fingerprint`. The desktop app
and the website send `x-message-crate-app` (`desktop` or `website`) and
`x-message-crate-version` (their Build) on every request; the server records
the pair on the account's session, rewrites it only when it changes, and shows
it to the owner. A request that sends neither header, or sends them
malformed, is served and nothing is recorded, which covers curl, Swagger UI
and any program holding an API token. No route refuses, redirects or changes
its answer on account of either header: this is a record of what connected,
not a handshake. The Product Version is the only value an app or Owner Home
compares, and the HTTP interface has no version number of its own beyond the
`1` in the path.
