# A failure is a problem document

Every failure on the vault's HTTP interface answers with an RFC 7807 problem
document, served as `application/problem+json`:

```json
{
  "type": "https://bitrealm.io/vault/developer/errors/username-taken",
  "title": "Username taken",
  "status": 409,
  "detail": "The username 'alice' already belongs to an account.",
  "request_id": "01K6Q2J8ZC3M4N5P6R7S8T9V0W"
}
```

`type` is the URL of a page describing the problem, one page per type. A
validation failure answers `422 Unprocessable Entity` and replaces `detail`
with `errors`, a list of every field that failed rather than the first:

```json
{
  "type": "https://bitrealm.io/vault/developer/errors/validation-failed",
  "title": "Validation failed",
  "status": 422,
  "errors": [
    "limit must be at least 1",
    "offset exceeds maximum of 50000"
  ],
  "request_id": "01K6Q2J8ZC3M4N5P6R7S8T9V0W"
}
```

**Every response carries a request id**, as an `x-request-id` header on success
and failure alike, repeated in the problem body as `request_id`. The id joins
the request's `TraceLayer` span, so every log line the request produces carries
it too. RFC 7807's `instance` member stays unused: the specification defines it
as a URI reference, and a bare id is not one.

**`406 Not Acceptable` is answered narrowly**: only when an `Accept` header is
present and no member of it matches `application/json`,
`application/problem+json`, or `*/*`. A missing `Accept` is a request for JSON.

The problem types, and which failure becomes which, are registered in
`docs/agents/http-api-problem-types.md`.

## Why

ADR-0005 gave every failure the body `{"error": "one sentence"}`, which was a
large improvement on the five shapes it replaced. It leaves two things
undone.

A client cannot branch on a failure without matching the sentence. The web app
reads `message` off a `VaultApiError` and can do nothing with it but show it,
so a screen that should react differently to a taken username than to a weak
password cannot tell them apart without string comparison against text a
person is meant to read.

And a `500 Internal Server Error` deliberately answers one stable sentence
while the whole error context chain goes to the log. That asymmetry is right —
internals should not leak — but nothing joined the two halves, so a person
reporting a failure and the log line recording it could only be matched by
timestamp and guesswork.

Sixty-one `ApiError::BadRequest` sites carried a message and no classification;
sixteen of them passed an inner error's `Display` straight through. Grouping
them by what actually went wrong found three that answered with the wrong
status: a wrong password as `400 Bad Request` rather than `401 Unauthorized`, a
duplicate username as `400 Bad Request` while a duplicate collection name
already answered `409 Conflict`, and a missing import `Content-Type` as
`400 Bad Request` where a wrong one answered `415 Unsupported Media Type`.

## Considered and rejected

**Keeping the flat `{error}` shape.** It costs nothing to keep and gives a
client nothing to act on. The sentence stays, as `detail`.

**A type per status code.** Cheap, and it adds nothing a client could not
already read from the status line. The value of RFC 7807 is the taxonomy, and
the taxonomy is the work.

**`about:blank` for every type.** Legal, and it means the `type` member never
tells a reader anything. Only `500 Internal Server Error` uses it, because a
page about it could say nothing a reader could act on.

**A request id on `5xx` only.** A `400 Bad Request` explains itself and a
`500 Internal Server Error` does not, so the id looks like it belongs on one
and not the other. Rejected because ADR-0005's thesis is one shape for every
response, and a body whose members vary by status is the drift that ADR exists
to stop.

**Requiring `Accept: application/json`.** RFC 9110 says a request without an
`Accept` accepts any media type, and none of the vault's own clients send one:
`web/src/lib/api.ts` sets only `Authorization`, so `fetch` defaults to `*/*`,
and `vault-push` and `vault-pull` send no `Accept` at all. The rule would
refuse the web app, the desktop app, and the push library on their first
request.

## Consequences

- ADR-0005's `{"error": "..."}` body is replaced. Its paging shape, its "no
  `ok` flag", and its "the status carries the meaning" all stand.
- 118 `ApiError` construction sites gain a problem type; 61 of them are
  `BadRequest` and are the work.
- The vault's own validation moves to `422 Unprocessable Entity`, matching what
  Axum's deserializer already returns for a body that fails to deserialize.
- A page per problem type is published under
  `bitrealm.io/vault/developer/errors/`, and an unpublished type is a broken
  `type` URL, so the registry and the pages ship together.
- `tower-http` gains its `request-id` feature.
- `web/`'s one error parse moves from `message` to `title` and `detail`, and
  gains the ability to branch on `type`.
