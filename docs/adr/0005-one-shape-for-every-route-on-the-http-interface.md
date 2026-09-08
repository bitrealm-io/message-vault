# One shape for every route on the HTTP interface

Every route on the vault's HTTP interface follows one convention, decided
once rather than per route file. A thing is read by its id and returned as
itself. A list takes `offset` and `limit` and returns `{items, total, limit,
offset}`. A failure returns `{error}` with the HTTP status carrying the
meaning, including failures Axum raises on its own. **Amended by ADR-0010**: a
failure is now an RFC 7807 problem document served as
`application/problem+json`. Everything else here stands. There is no `ok` flag
on success. Every id is an integer. Reading is never done through Export:
`GET /v1/conversations/{id}` and `GET /v1/conversations/{id}/messages` are
the read path for one Conversation, and `GET /v1/export/messages` is for
downloading only.

## Why

In September 2026 the interface had grown one convention per file. Export
paged by cursor and also by offset, clamped `limit` silently and refused a bad
`offset` with a 400; conversations and contacts paged by offset and returned
`total`. Export, auth, profile, tokens, assets and import wrapped success in
`ok: true`; conversations, contacts, named sets, saved searches and search
fields did not. Lists were keyed `messages`, `conversations`, `contacts`,
`items`, and `savedSearches`, the last the only camelCase field on the API.
A conversation's id was a string in the list and an `i64` in the path that
consumed it. A bad query parameter came back as plain text from Axum, not as
the JSON error body every other failure used. `source=` meant an on-disk
directory slug on import and assets and a closed three-word set on export.

None of that was chosen. Each file picked what was convenient when it was
written, and the web app learned each shape separately, with `?? []` and
`as` casts covering the gaps. The interface a caller had to know was close to
the sum of the implementations, which is the definition of shallow.

The vault had no route for reading one Conversation. The web read messages by
calling Export with a search string `in:#id`, and found a conversation by
paging the whole list until the id appeared. Export's token could read every
message but could not list the conversations they belonged to. Export is the
download path and the opposite of Import; a display path wired through it is
a defect, not a naming quirk.

## Considered and rejected

**Cursor paging everywhere.** Stable under concurrent inserts, but this is a
self-hosted vault for one person and nothing inserts rows underneath a
running export. Offset with a stable sort is correct here, and every screen
that shows "51–100 of 4,213" needs `total` anyway.

**Offset for screens, cursor for Export.** One justified exception is still
two conventions. Rejected for the same reason ADR-0003 rejected "id, except
where the name is unique".

**An `ok: true` envelope on every success.** A second copy of what the status
code already says. Rejected.

**Keeping `GET /v1/messages?q=in:#id` as the read path.** Opening a
conversation is a lookup, not a search, and the screen should not have to
spell a query to do it. A message-list search route is a separate need
(#313) and is not Export either.

## Consequences

- New read routes need a signed-in session, as the conversation list does.
  API tokens keep their import and export scopes and gain nothing.
- Export loses its cursor and its `source=` parameter; `source:` in the
  query does that job. The desktop pull library pages by offset.
- `ConversationSummary.id` becomes an integer, the last string id on the API.
- The web has one paged type and one paged hook; resource-specific list keys
  and `?? []` fallbacks on required fields go.
- Breaking changes are accepted. Message Vault has no users to protect.
