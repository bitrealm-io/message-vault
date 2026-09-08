# HTTP API conformance audit

Every known problem with the vault's `/v1` interface in one place, as of
2026-09-08 (`main` at 065f12b, product version 0.8.3, 58 paths in
`docs/src/assets/openapi.json`).

This file is a findings list, not a decision. The rules it grades against are
not yet written down anywhere in the repository, which is the root cause of
most of what follows. Writing them is the next step; the three open questions
at the end have to be answered first.

## What the interface is graded against

- **ADR-0003** — resources are addressed by integer id, and membership lives
  under the collection that owns it. Covers identifiers and sub-resource
  placement.
- **ADR-0005** — offset paging, `{items, total, limit, offset}`, `{error}`
  bodies, no `ok` envelope, Export is not a read path. Covers response shape.
- **Standard REST practice** — HTTP method semantics, status codes, resource
  oriented URIs, content negotiation, pagination, filtering, error format and
  rate limiting.

Nothing written down covers **path and verb naming**: singular against plural,
when a path segment may be a verb, which prefix owns what, which method
updates, or wire casing. Each pull request has been choosing. That gap is why
`#403` shipped `/v1/admin/users*`, `#477` renamed it to `/v1/owner/accounts*`,
and `#481` then added `/v1/owner/vault-settings` whose handlers live in
`vault_api.rs` next to `/v1/vault`.

## Findings

Twenty-five findings. Each names the evidence. A5 to A7 were found by
classifying the failures for the RFC 7807 work, not by reading the paths, and
H1 while planning the fix for B5.

### A. Status codes (7 findings)

**A1. No route returns `201 Created`, and no route sends a `Location` header.**
Seven creating POSTs answer `200 OK`: `/v1/contact-groups`, `/v1/message-tags`,
`/v1/saved-searches`, `/v1/account/api-tokens`, `/v1/owner/accounts`,
`/v1/imports`, `/v1/assets/{sha256}/uploads`. `StatusCode::CREATED` and the
`LOCATION` header appear nowhere in `crates/vault/server/src`. A client that
creates a resource cannot learn its URL from the response.

**A2. Two kinds of validation failure answer with two different statuses.**
A JSON body that fails to deserialize passes Axum's `422 Unprocessable Entity`
straight through, deliberately (`crates/vault/server/src/extract.rs:122`). The
vault's own validation never uses it: all 61 `ApiError::BadRequest` sites
answer `400 Bad Request` with a single message
(`crates/vault/server/src/server.rs:381`), so a form with four bad fields
reports one of them at a time.

**A3. The `429 Too Many Requests` carries no `Retry-After`.**
`ApiError::TooManyRequests` (`crates/vault/server/src/server.rs:386`) sends a
status and a message; a client is told to back off but not for how long. No
`X-RateLimit-Limit`, `X-RateLimit-Remaining` or `X-RateLimit-Reset` header
exists either.

**A4. The rate limiter is undocumented.** `check_auth_rate_limit`
(`crates/vault/server/src/auth.rs:40`) guards login, register and vault claim
over a 60-second window. Nothing in the published docs says the limit exists
or what it is.

**A5. Authentication failures answer `400 Bad Request`, not
`401 Unauthorized`.** A wrong password on login, and a wrong current password
on account delete, are both `ApiError::BadRequest`
(`crates/vault/server/src/auth.rs`). The status says the request was malformed
when the request was fine and the credential was not.

**A6. A duplicate username answers `400 Bad Request`, not `409 Conflict`.**
Registration refuses a taken username with `BadRequest`
(`crates/vault/server/src/auth.rs`), while a duplicate Contact Group, Message
Tag or Saved Search name already answers `409 Conflict`. The interface
disagrees with itself about what a name collision is.

**A7. A missing import `Content-Type` answers `400 Bad Request`, not
`415 Unsupported Media Type`.** `import/mod.rs` refuses a body with no
`Content-Type` using `BadRequest`, while the same module answers
`415 Unsupported Media Type` for a `Content-Type` it does not accept. Absent
and wrong are the same class of failure.

### B. Resource-oriented naming (9 findings)

**B1. `POST /v1/import` is a singular path beside the plural collection it
writes into.** It appends a JSONL batch to an Import Run created by
`POST /v1/imports`; `vault-push` calls both
(`crates/libs/vault-push/src/http.rs:290`).
Fix: `POST /v1/imports/{id}/batches`.

**B2. `POST /v1/import` takes `account=` as a query parameter.** The bearer
credential already names the account. The parameter is redundant and invites
a mismatch between the two.

**B3. `POST /v1/import` takes `source`, `mode` and `dedupe` per batch.** Two
batches of one Import Run can disagree about how to import. These belong on
the run's creation, where they can only be stated once.

**B4. `import_id` is optional, so a sessionless import exists.**
`post_import` takes `import_id: Option<i64>`
(`crates/libs/vault-push/src/http.rs:268`), and
`web/src/screens/import/ResumeImportPanel.test.tsx:164` describes the result
as "raw POST /v1/import — stores a null staging_dir". CONTEXT.md says an
Import Run is "recorded permanently whether it succeeded, failed, or was
cancelled"; a path that skips the record contradicts the domain model.

**B5. `GET /v1/imports/active` is a filter squatting in the member-id
namespace.** It returns the account's one running Import Run. The path collides
with `/v1/imports/{id}` and hides a filter in a path segment, and "active" is a
word the row never stores: `vault_imports.status` holds `running`, `completed`,
`completed_with_issues`, `failed` or `cancelled` (`schema/sql/accounts.sql:189`).
Fix: `GET /v1/imports?status=running`, accepting the five stored values.

**B6. Three paths spell an HTTP method as a verb.**
`POST /v1/auth/delete-account`, `POST /v1/account/delete-messages`,
`POST /v1/auth/change-password`. Fixes: `DELETE /v1/account`,
`DELETE /v1/account/messages`, `PUT /v1/account/password`. The last matches
`PUT /v1/owner/accounts/{id}/password`, which already gets it right.

**B7. `/v1/auth` and `/v1/account` split the same subject arbitrarily.**
`change-password` and `delete-account` sit under `auth`, while
`delete-messages`, `profile`, `storage` and `api-tokens` sit under `account`.
All six act on the signed-in account. `/v1/auth` should hold only what
establishes or ends a session: `login`, `logout`, `register`, `check`.

**B8. `/v1/owner/vault-settings` files a resource under a role prefix.** Its
handlers, `vault_settings_handler` and `patch_vault_settings_handler`, live in
`crates/vault/server/src/vault_api.rs` next to `/v1/vault`. The module groups
by resource; the path groups by who may call it. A path segment should not
encode authorization. Fix: `GET|PATCH /v1/vault/settings`.

**B9. Two paths are named for something other than what they return or do.**
`POST /v1/contacts/match` reports which identifiers have no vault contact, so
it returns unmatched handles, not matches. `GET /v1/search/fields` implies a
`search` collection that does not exist. Fixes:
`POST /v1/contacts/unmatched-handles` and `GET /v1/search-fields`.

### C. Method semantics (2 findings)

**C1. `POST /v1/account/profile` performs a partial update.** It changes the
display name and linked handles. Fix: `PATCH /v1/account/profile`. Every other
update on the interface already uses PATCH.

**C2. `POST /v1/contacts/address-book` is the only way to create a contact.**
There is no `POST /v1/contacts`. The one creation door is named after the file
the data came from rather than the resource it creates.

### D. Content negotiation (1 finding)

**D1. The `Accept` header is never read and `406 Not Acceptable` is never
returned.** `415 Unsupported Media Type` is handled on request bodies
(`crates/vault/server/src/extract.rs:156`,
`crates/vault/server/src/import/mod.rs:1531`), so half the rule is met. A
client asking for a format the vault cannot produce gets JSON anyway.

### E. Filtering and sorting (2 findings)

**E1. Only one of five list routes can be sorted.** `sort=` exists on
`GET /v1/conversations` alone
(`crates/vault/server/src/conversations_api.rs:571`, values `date` and
`messages`). `/v1/contacts`, `/v1/messages`,
`/v1/conversations/{id}/messages` and `/v1/export/messages` take `limit` and
`offset` with no ordering control.

**E2. The one `sort=` that exists spells direction as a second parameter.**
`sort=date|messages` takes its direction from a separate `order=asc|desc`
(`crates/vault/server/src/conversations_api.rs:52`), which the web sends
(`web/src/lib/vaultApi.ts:255`). Both are lenient by design: an unknown value
falls back to the default rather than being refused, so a typo sorts silently.
There is no second sort key.

### F. Error format (1 finding)

**F1. The error body is a bare string with no code and no request id.**
`ErrorBody { error: message }` (`crates/vault/server/src/server.rs:398`) means
the web app must match on human-readable text to branch on a failure. An
internal error is logged server-side with the full context chain
(`server.rs:390`) but nothing in the response ties the client's failure to
that log line.

### G. Documentation drift (2 findings)

**G1. The docs tell users to call an endpoint that does not exist.**
`docs/src/content/docs/vault/user/get-started/install-the-desktop-app.md:55`
says to run `curl http://127.0.0.1:8080/v1/auth/mode` when Connect fails.
There is no `/v1/auth/mode` anywhere in the server, and
`web/src/screens/LoginScreen.test.tsx:110` asserts the client never calls it.
The working equivalent is `GET /v1/auth/check`.

### H. Response shape (1 finding)

**H1. `GET /v1/imports` is not a page.** It answers `{items}` with no `total`,
`limit` or `offset` (`crates/vault/server/src/import/mod.rs:799`). ADR-0005
already forbids that shape; every other list answers a page.

**G2. CLAUDE.md describes the session token as a JWT.** It is not: there is no
`jsonwebtoken` dependency in any manifest, and `account_session_tokens`
(`schema/sql/accounts.sql:55`) stores a hash of an opaque `mv-user-` prefixed
secret with `created_at` and `expires_at`.

## Already conformant, so not to be re-litigated

- **Versioning.** `/v1` is a whole number in the path. No dotted versions.
- **Pagination.** Offset and limit, `{items, total, limit, offset}`, default
  40 for lists and 100 for export, maximum 500, offset capped at 50,000, and a
  `400 Bad Request` rather than a silent clamp
  (`crates/vault/server/src/paging.rs`).
- **Wire casing.** Zero camelCase fields and zero camelCase query parameters
  across all 58 paths. snake_case throughout, though nothing says so in
  writing.
- **Plural collections.** Every collection is plural and consistently so.
- **Sub-resources.** `/v1/conversations/{id}/messages`,
  `/v1/contact-groups/{id}/members` express relationships correctly per
  ADR-0003.
- **Nesting depth.** The deepest path,
  `/v1/assets/{sha256}/uploads/{upload_id}/parts/{part}`, is three levels,
  inside the usual two-to-three ceiling.
- **`204 No Content` on no-content writes.** Trash, restore, logout and account
  delete all answer `204 No Content` correctly.
- **`409 Conflict` on conflicts.** Duplicate named-collection names answer
  `409 Conflict`.
- **`401 Unauthorized` against `403 Forbidden`.** The two are distinguished
  rather than conflated.
- **Authentication.** Opaque bearer secrets hashed at rest, Argon2id with a
  per-hash salt, a timing-equalization dummy verify
  (`crates/vault/server/src/auth.rs:179`), `expires_at` on both session and
  API tokens, and internal error detail kept server-side.

## Decided

**The error body is RFC 7807 problem details.** Every failure answers with a
Content-Type of `application/problem+json` carrying `type`, `title`, `status`
and `detail`. A validation failure answers `422 Unprocessable Entity` with
`errors`, a list of human-readable strings, in place of `detail`. This replaces
ADR-0005's flat `{error}` shape, so ADR-0005 needs an amendment recorded
against it.

What the change touches:

- 118 `ApiError` construction sites across `crates/vault/server/src`, each of
  which carries a message today and needs a problem type.
- 61 of those are `ApiError::BadRequest` outside test modules. Classifying them
  is the whole cost of the change; everything else is mechanical. The first
  pass, a markdown registry since replaced by the declarations in
  `crates/vault/server/src/problem.rs`, collapsed them, and the other
  variants, into twenty problem types.
- The web client reads `message` off a `VaultApiError` parsed once in
  `web/src/lib/api.ts`; that parse moves to `title` and `detail`.
- Finding A2 closes with it: the vault's own validation moves to
  `422 Unprocessable Entity`, matching what Axum's deserializer already
  returns.
- The form model in `crates/core/message-vault-io-core` already reports
  validation problems as a `Vec<String>` (`pipeline.rs:87`), which is the shape
  `errors` wants.

All three sub-questions are settled:

1. **The `type` URL points at a real page.** Each problem type gets a page under
   `bitrealm.io/vault/developer/reference/errors/`, beside every other
   generated reference page, and `type` is that page's URL. Only
   `500 Internal Server Error` uses `about:blank`, because a page about it could
   say nothing a reader could act on.
2. **The taxonomy is per problem, not per status.** Twenty types cover the
   61 `BadRequest` sites and the other `ApiError` variants, declared once in
   `crates/vault/server/src/problem.rs`, which is the registry; the markdown
   draft that preceded it is gone.
3. **A request id travels in both the header and the body.** One middleware,
   `crates/vault/server/src/request_id.rs`, the outermost layer on the router,
   makes a UUID v4 per request, puts it on the request and the response, and
   holds it in a task local while the request is served; the id joins the
   `TraceLayer` span so every log line under a request carries it. Every response returns it as `x-request-id`, successes included,
   so a caller can quote an id for a request that answered `200 OK` and did the
   wrong thing. Every problem body repeats it as a `request_id` extension
   member, which RFC 7807 permits, so the person reading the failure can quote
   it without opening developer tools. It appears on every problem rather than
   only on `5xx`: ADR-0005's thesis is one shape for every response, and a body
   whose fields vary by status is the drift it exists to stop. RFC 7807's own
   `instance` member is left unused, because the specification defines it as a
   URI reference and a bare id is not one.

This closes finding F1: the error body carries a type, a title, a status, a
detail, and the id that joins a client's failure to the log line recording it.

**Sorting is a request parameter, in one spelling, on every list.**
`sort=-field,field`, comma-separated keys with a `-` prefix for descending, on
all five list routes. Each route declares which columns it accepts, and an
unlisted column is a `validation-failed` problem, `422 Unprocessable Entity`,
naming the column and the accepted set; the lenient fallback goes. The
separate `order=` parameter goes with it. This closes findings E1 and E2.

**The contacts list sorts by `name` only.** A "last heard from" key, the newest
message a contact's own handles sent, is a contacts feature with a field and a
column, tracked as #497.

**Filtering stays in the search language, and `fields=` is refused.** ADR-0004
already says a query only narrows while sort order is a request parameter, so
query parameters never filter: `q=date:>2019` is the vault's range operator and
there will not be a second one. The one exception is a list with no search
language, and imports is the only one: `GET /v1/imports?status=running` takes
the values the row stores. `GET /v1/imports` becomes a page at the same time,
closing H1. `fields=` selection is refused because it turns
one resource into many shapes, and `web/` generates its types from
`docs/src/assets/openapi.json`, where a response whose fields depend on a
parameter can only be typed with every field optional, which is the `?? []` and
`as`
casts ADR-0005 exists to have deleted. Pagination already bounds the payload
size that `fields=` exists to reduce.

**`406 Not Acceptable` is answered narrowly, on the `/v1` routes that produce
JSON.** A request is refused only when an `Accept` header is present and no
member of it matches `application/json`, `application/problem+json`, or `*/*`.
`GET /v1/assets/{sha256}` streams the asset's own bytes and is the one route
exempt; nothing outside `/v1` is checked, because the web app, `/health` and the
OpenAPI UI produce other things on purpose. A missing `Accept` is a request for
JSON: RFC 9110 says a request without one accepts any media type, and none of
the vault's own clients send one — `web/src/lib/api.ts:80` sets only
`Authorization`, so the browser's `fetch` defaults to `*/*`, and `vault-push`
and `vault-pull` send no `Accept` at all. A rule that required the header would
refuse the web app, the desktop app, and the push library on their first
request.

`application/problem+json` is named in that list on purpose. It does not
literally match `application/json`, so a matcher that compares strings refuses
the error body to a client that politely asked for JSON, exactly when something
has already gone wrong. RFC 7807 chose the `+json` suffix for this reason. The
condition is the whole design; the code is a few lines.

This closes finding D1.

**`POST /v1/contacts` takes the file.** The body is the address book itself,
with `Content-Type: text/vcard` or `text/csv` naming the format; the
`{filename, content}` envelope and the filename sniff go, and an unknown
`Content-Type` is `415 Unsupported Media Type`. How contacts are loaded is
still open in #270; the route is where that answer will land. This closes
finding C2.

**The request id is server-made.** A UUID v4 generated on every request; an
`x-request-id` a client sends is ignored, so an id in the log is one the
server made.

**Parse against validate has one line.** A request that cannot be read is
`malformed-body`, `400 Bad Request`; one that parsed and then broke a rule is
`validation-failed`, `422 Unprocessable Entity`, whether the rule was on a
query parameter, a path segment or a body field. Today `limit=ten` and a
non-numeric id answer `400 Bad Request` while a body missing a field answers
`422 Unprocessable Entity` (`crates/vault/server/src/extract.rs:100–135`); all
three move to the same side of the line.

**The word is Import Run.** `POST /v1/imports` creates an Import Run, the
term CONTEXT.md already has; "import session" is retired, since a session is
the signed-in account's token everywhere else on the interface.

## Open questions

None. Every question the audit raised is answered, and the rules are ready to
be written as an ADR.
