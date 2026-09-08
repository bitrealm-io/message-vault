# One way to name a route on the HTTP interface

Every route on the vault's HTTP interface is named by one convention, decided
once rather than per pull request.

**A collection is plural, and a member is `/{collection}/{id}`.** A singular
path is legal only for a singleton: one per vault (`/v1/vault`) or one per
session (`/v1/account`). `/v1/trash` is a singleton by the same rule.

**A path segment names a resource, never a caller's role.** Who may call a
route is enforced in its handler, so a change to permissions never renames a
URL. `/v1/owner/accounts` survives because accounts genuinely are the owner's
resource (ADR-0008); vault settings do not, and live at `/v1/vault/settings`.

**A verb is legal only as a sub-resource of a member, and only when the action
is not a field write** — it crosses a state machine, touches rows other than
the addressed one, or has no resource to write at all. `claim`, `stage`,
`complete`, `discard`, `trash`, `restore`, `login`, `logout`, `register` and
`check` pass that test. A verb that merely spells an HTTP method does not:
deleting an account is `DELETE /v1/account`, not `POST /v1/auth/delete-account`.

**The method says what kind of write it is.** PATCH updates part of a resource,
PUT replaces one, POST creates or runs an action, DELETE removes. A read is a
GET, except where its selector is a collection too large for a query string; a
POST that reads is named for what it returns, never for the verb that computes
it.

**A creation answers `201 Created` with a `Location` header** naming the new
resource. A create that takes a batch is the exception: it answers `200 OK`
with a summary of what was created, updated and skipped, because no single
resource was made and no URL can be named. A whole-batch failure is one
problem document; per-row outcomes are data, not errors. A write with nothing
to return answers `204 No Content`. A name collision answers `409 Conflict`.
A failed credential answers
`401 Unauthorized`, and a refused one `403 Forbidden`. A `Content-Type` that is
absent or unaccepted answers `415 Unsupported Media Type`.

**Sorting is a request parameter, in one spelling, on every list**:
`sort=-field,field`, comma-separated keys with a `-` prefix for descending.
Each list declares which columns it accepts, and an unlisted column is a
`400 Bad Request` naming the column. Filtering is the search language and
nothing else (ADR-0004); query parameters never filter, and there is no
`fields=` selection.

**Two levels of nesting, and `/health` is the one route outside `/v1`.** A
multipart upload is the single exception to the nesting rule, at three. Every
field on the wire is snake_case.

## Why

In September 2026 an audit of the interface found twenty-four conformance
problems across 58 paths, recorded with their evidence in
`docs/agents/http-api-audit.md`. Ten of them were path names.

ADR-0003 settled how a resource is identified and ADR-0005 settled what a
response looks like, but nothing said how a route is named. So each pull
request chose. `#403` shipped `/v1/admin/users*`; `#477` renamed it to
`/v1/owner/accounts*`; `#481` then added `/v1/owner/vault-settings`, whose
handlers live in `vault_api.rs` beside `/v1/vault` — the module grouping by
resource while the path grouped by who may call it. `POST /v1/import` grew as a
singular path appending batches to the plural collection beside it, carrying an
`account=` parameter the credential already supplied. `GET /v1/imports/active`
took a filter into the member-id namespace. Three paths spelled an HTTP method
as a word.

None of that was chosen either. It is the same failure ADR-0005 was written to
stop, one layer up: a convention per file, and an interface a caller has to
learn route by route.

## Considered and rejected

**No verbs at all.** Modelling every action as a field write turns trashing
into `PATCH /v1/conversations/{id} {"trashed": true}` and leaves login with no
home. Trash is the only door to permanent deletion, and `POST .../trash` says
that where a field write does not. A bounded exception, written down, beats a
rule everyone quietly breaks.

**A prefix that names the caller's role.** `/v1/owner/*` reads well until
permissions change and a URL has to be renamed to match. Authorization is a
property of a route, not of a path segment. The cost accepted is that
`/v1/vault` mixes an unauthenticated read with an owner-only write.

**`fields=` selection.** It turns one resource into many shapes. `web/`
generates its types from `docs/src/assets/openapi.json`, where a response whose
fields depend on a parameter can only be typed with every field optional, which
is the `?? []` and `as` casts ADR-0005 exists to have deleted. Pagination
already bounds the payload size it would save.

**Query-parameter filters beside the search language.** `?date_gte=2019`
alongside `q=date:>2019` is two ways to ask one question. ADR-0004 already says
a query only narrows while sort order is a request parameter.

## Consequences

- Ten paths are renamed, among them `POST /v1/imports/{id}/batches`,
  `DELETE /v1/account`, `PUT /v1/account/password`, `/v1/vault/settings` and
  `POST /v1/contacts`.
- An import session becomes mandatory: the sessionless one-shot import goes,
  and every Import Run is recorded as CONTEXT.md already says it is.
- Seven creating routes gain `201 Created` and a `Location` header.
- Four lists gain sorting, and the one that had it gains a direction.
- Breaking changes are accepted, as CLAUDE.md says for every interface.
