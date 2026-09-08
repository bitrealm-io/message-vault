# HTTP interface rules

Every rule the vault's `/v1` interface follows, in one place. A pull request
that touches a route is graded against this file, and a change to a rule is
made here in the same pull request that changes the code. Each rule carries
its reason, and where an alternative was weighed and turned down, one line
says so, so the question is not reopened by accident.

This file states what the interface is. It never lists work in progress; an
issue does that. Why the rules live here and not in `docs/adr/`:
`docs/adr/0011-the-http-interface-has-one-rules-document.md`.

The generated reference, `docs/src/assets/openapi.json`, is produced from the
handlers by `message-vault-server dump-openapi` and checked in; a test fails
when the two differ, and CI checks the web app's generated types against it.
So the reference says what each route does today, and this file says what
every route must do.

## Identifiers

Every resource with a row is addressed by its integer id in the URL:
`/v1/contacts/{id}`, `/v1/accounts/{id}`, `/v1/accounts/{id}/api-tokens/{id}`.
Names are for people: they appear in the web app's routes, in the search
language, and in the text of a Saved Search, and the web app turns a name into
an id inside the module that owns the collection, never in a screen.
Why: a name is mutable, needs URL-encoding, and must be matched
case-insensitively on every request, and a rename would re-key the resource.

The one exception is an Asset, addressed by the SHA-256 of its contents:
`/v1/assets/{sha256}`. Why: the file exists before the vault does, the client
must know the hash before an upload can be deduplicated, and two uploads of
one file must be one asset.

Rejected: the name in the path for Contact Groups and Message Tags. It keeps
every reference to a group the same kind of thing, and it makes the rule
"id, except where the name is unique". One rule is worth more than the
symmetry.

Rejected: opaque string ids for accounts and API tokens. They were UUIDs from
the first draft with no reason recorded. An integer in a URL tells a stranger
nothing they can act on, because every route checks the caller against the
row.

## Naming a route

A collection is plural, and a member is `/{collection}/{id}`. A singular path
is legal only for a singleton: one per vault (`/v1/vault`), or one per signed-in
credential (`/v1/session`). `/v1/trash` is a singleton by the same rule.

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
`POST /v1/contacts/summaries`, `POST /v1/contacts/unmatched-handles`.

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
one per signed-in credential, which makes it a singleton, and login, logout and
check are its `POST`, `DELETE` and `GET`.

## Methods

`PATCH` updates part of a resource. `PUT` replaces one. `POST` creates, or
runs an action named by a verb sub-resource. `DELETE` removes. A read is a
`GET`, except the large-selector `POST` above.

Behaviour that differs by caller lives inside one handler, not in two routes.
`PUT /v1/accounts/{id}/password` is one route: the owner sets a temporary
password without the current one, the account itself must supply the current
one.

## Status codes

- A creation answers `201 Created` with a `Location` header naming the new
  resource. A create that takes a batch answers `200 OK` with a summary of what
  was created, updated and skipped, because no single resource was made.
- A write with nothing to return answers `204 No Content`.
- A name collision answers `409 Conflict`. So does an action on a resource in
  the wrong state: deleting before trashing, claiming a claimed vault, a batch
  on a finished run.
- A failed credential answers `401 Unauthorized`. A refused one, including a
  token without the needed scope and a disabled account, answers
  `403 Forbidden`.
- A `Content-Type` that is absent or unaccepted answers
  `415 Unsupported Media Type`.
- A request that cannot be read is `400 Bad Request` (`malformed-body`). One
  that parsed and then broke a rule is `422 Unprocessable Entity`
  (`validation-failed`), whether the rule was on a query parameter, a path
  segment or a body field. Axum's own rejections follow the same line.
- `429 Too Many Requests` carries `Retry-After`.
- An unknown `/v1` path answers `404` as a problem document, and a wrong method
  `405`, never Axum's plain text.

There is no `ok` flag on any success; the status carries the meaning.

## Lists

Every list route answers a page, `{items, total, limit, offset}`, and takes
`offset` and `limit`. No exceptions: a list the person curates by hand
(groups, tags, saved searches, API tokens), a fixed reference list
(`/v1/search-fields`), and a `POST` that reads all answer a page. The list
key is always `items`.
Why: the web app has one paged type and one hook, and a second shape is a
second convention.

A `POST` that reads the rows its body names — contact summaries, unmatched
handles — answers the whole of that body as one page and takes no `offset`
or `limit`: `total` is the row count, `limit` is the cap the body is held to,
`offset` is 0. Why: the body already says which rows to read and how many it
may name, so a second bound would only let a caller ask for rows it did not
name, or hide rows it did.

A count that describes the whole set rather than the page belongs on the
resource the set hangs off, not beside `items`. An Import Run's tally of
contacts created and changed is on the run's own record, and the contacts are
a page. Why: a page can only count its own rows, and a field beside `items`
that counts something else is a second shape.

`limit` is at least 1 and at most 500, default 40. `offset` is at most 50 000
on the browse lists. A value outside the range is `validation-failed`, never a
silent clamp.

Sorting is `sort=-field,field`: comma-separated keys, a leading `-` for
descending. Each list declares the keys it accepts, and an unlisted key is
`validation-failed` naming the accepted set. There is no separate `order=`.

Filtering is the search language in `q`, and nothing else. The one exception
is a list with no search language, which may take a filter parameter whose
values are the ones its rows store; imports is the only such list
(`GET /v1/imports?status=running`). There is no `fields=` selection.

Rejected: cursor paging. Stable under concurrent inserts, but nothing inserts
rows under a running read on a self-hosted vault, and every screen that shows
"51–100 of 4,213" needs `total`.

Rejected: a bare `{items}` for small lists. One justified exception is still
two conventions, and a group's member list has no bound the server enforces.

Rejected: `fields=`. It turns one resource into many shapes, and the web app's
generated types could only express that with every field optional.

Rejected: query-parameter filters beside the search language. `?date_gte=2019`
next to `q=date:>2019` is two ways to ask one question.

## Failures

Every failure answers an RFC 7807 problem document as
`application/problem+json`, carrying `type`, `title`, `status`, `detail` and
`request_id`. A validation failure replaces `detail` with `errors`, a list of
every rule that broke rather than the first.

`type` is the URL of a page under
`bitrealm.io/vault/developer/reference/errors/`, one page per problem type.
The code is the registry: each type is declared once in
`crates/vault/server/src/problem.rs`, the pages are generated from it, and a
test fails when the checked-in pages drift. Only `500 Internal Server Error`
uses `about:blank`, because a page about it could say nothing a reader could
act on. The taxonomy is per problem, not per status: the test for a new type
is that a client's remedy differs.

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
member of it matches `application/json`, `application/problem+json`, or `*/*`.
A missing `Accept` is a request for JSON. The check runs on every `/v1` route
but `GET /v1/assets/{sha256}`, which streams the asset's own bytes. Nothing
outside `/v1` is checked.

Rejected: requiring `Accept: application/json`. None of the vault's own clients
send one, and the rule would refuse the web app on its first request.

## Credentials and reach

Two credentials exist, and the OpenAPI document declares each as a security
scheme with its scopes, so every route says which it accepts.

- A **Session** is one per signed-in account or owner, made by
  `POST /v1/session` and ended by `DELETE /v1/session`. It carries the
  account's own permissions: `import`, `export`, `delete`. The owner's session
  carries none of those and reaches only the accounts collection and the vault
  settings, because the owner holds no messages.
- An **API token** is a named credential an account makes for a program, with
  the scopes the person chose, capped by the account's own. A token never signs
  in and never browses.

What each reaches:

- Browse routes (conversations, messages, contacts, groups, tags, saved
  searches, search fields) take a session only. A token is refused.
- `POST /v1/imports` and everything under a run, and every asset write, need
  the `import` scope on either credential.
- `POST /v1/exports` and everything under a run, `GET /v1/assets/{sha256}` and
  `HEAD /v1/assets/{sha256}`, need the `export` scope on either credential. A
  program with an export token reads messages only through an Export Run it
  started, so every read of message data by a program leaves a record.
- `HEAD /v1/assets/{sha256}` also accepts the `import` scope: a program that
  can only push may ask whether an asset exists, and may not read it.
- Permanent deletion (`DELETE /v1/conversations/{id}`,
  `DELETE /v1/contacts/{id}`, `DELETE /v1/trash`) needs a session with the
  `delete` permission. A token is refused.
- `/v1/accounts/{id}` and everything under it is read and written by the owner
  or by that account; a `Location` handed to a newly registered account names a
  row it may read.
- `GET /v1/vault` and `POST /v1/vault/claim` take no credential.
  `/v1/vault/settings` is the owner's.

The credential names the account. No route takes an `account=` parameter.

Rate limiting guards `POST /v1/session`, `POST /v1/accounts` and
`POST /v1/vault/claim` over a 60-second window; the limit is documented in the
developer reference.

## Runs

An Import Run and an Export Run are recorded permanently, whether they
completed, failed or were cancelled, and the client closes them: a run's
settings are stated once on creation, never per batch or per page, and
`complete`, `discard` (imports) and `cancel` (exports) are the only ways out.
There is no sessionless import and no unrecorded export.

An Export Run's scope is one of three forms, stored as given: everything the
account holds; a query in the search language; or picked `conversation_ids`
and `message_ids`. The run records the four counts the vault computed at
creation (messages, conversations, distinct attachments, bytes), and
`GET /v1/exports/{id}/messages` pages the rows the scope selects. The record
holds what was asked for and how much matched, never what the messages said.

## Versioning and change

`/v1` is a whole number in the path. There is no compatibility promise behind
it: endpoint names, request and response shapes, and stored formats change
whenever a better design is found, and breaking a client is an accepted cost.
No compatibility alias, deprecation window or version handshake is ever added.
