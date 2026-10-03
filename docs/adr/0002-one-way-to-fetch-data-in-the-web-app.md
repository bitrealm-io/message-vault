# One way to fetch data in the web app

The web app fetches server data through one mechanism: TanStack Query, calling
route functions that live in `web/src/lib/serverApi.ts`. Those functions do
nothing but talk to the server — no caching, no cross-component notification, no
React hooks inside them. Their response types are generated from
`docs/src/assets/openapi.json` rather than written by hand. Every cache entry is
named with the logged-in account, so one account cannot be served another
account's data.

Anyone who needs caching, request deduplication, or loading state on a new
screen uses TanStack Query. Writing a new one for a single screen is the thing
this decision exists to prevent.

The rule is that a screen fetches the account data it shows one way. Two
fetches are named exceptions, below, and they are the only ones. Any other fetch
outside TanStack Query is a violation of this decision.

## Two fetches that stay outside TanStack Query

Both were decided with the maintainer on 2 October 2026 (issue #1111).

**An attachment's bytes.** `web/src/hooks/useAssetObjectUrl.ts` downloads an
attachment, or its preview, through `fetchAssetObjectUrl` in `serverApi.ts` and
turns the bytes into an object URL. An object URL holds the bytes in memory
until something revokes it, and the component that shows the attachment is the
only thing that knows when the URL is no longer on screen. So the hook belongs
to that component: it revokes the URL when the component unmounts or shows a
different attachment. A cache entry has no such owner. TanStack Query drops an
entry on its own schedule and never revokes what the entry holds, so caching the
URL would either leak the bytes or revoke a URL still on screen. The cost is
that two components showing the same attachment each download it.

**The `/health` check.** `checkServerHealth` in `web/src/lib/serverHealth.ts`
calls `fetch` itself, and `web/src/lib/useServerHealth.ts` runs its own backoff
and polling. The check answers whether a server is listening at an address, and
on the login screen that address is the one the person is still typing, which
is not yet the server `apiClient` talks to. It carries no session and returns no
account data, so it has no place in `serverApi.ts`, whose functions all go
through `apiClient`, and no account to name a cache entry after.

## Why

Before this decision the web app had six separate mechanisms for fetching and
remembering server data, and each one solved the same four problems its own way:
remember an answer, avoid firing an identical request twice at once, tell other
components when the answer changed, and track loading and error state.

| Mechanism | Files that used it |
| --- | --- |
| `web/src/lib/useResource.ts` | 6 |
| `web/src/lib/usePagedList.ts` | 5 |
| `web/src/lib/nameCollection.ts`, through `contactGroups.ts` and `messageTags.ts` | 6 |
| `web/src/lib/savedSearches.ts` | 2 |
| `web/src/lib/contactDetailCache.ts` | 10 |
| `web/src/lib/useAccountProfile.ts` | 8 |

Nobody chose to have six. Each one appeared because a screen needed caching or
deduplication at the time it was built, and writing a small mechanism next to
that screen was faster than reshaping an existing one. The same pressure would
have produced a seventh.

The cost was not theoretical. Four of the six kept a copy of the account's data
in a module-level variable, so `web/src/lib/auth.tsx` had to clear them by hand
whenever the logged-in account changed. It did that in two places — lines
205–208 when someone logs in, lines 255–258 when someone logs out — and both
copies of the list named the same four mechanisms and omitted the fifth.
`savedSearches.ts` holds its list in a module-level `cached` variable and
returns it without asking the server whenever a caller passes no abort signal,
which `useSavedSearches` does. The result: log in as one account, view the
sidebar, log out, log in as a different account, and the sidebar shows the
first account's Saved Searches until someone adds, renames, or deletes one, or
reloads the page.

Two hand-maintained lists both missing the same entry is the argument for this
decision. Shortening the list would not have fixed the class of mistake, so
cache entries are named with the account instead: a second account asks for an
entry that has never been written, finds nothing, and fetches. Clearing the
cache on logout still happens, to release memory rather than for correctness.

The route functions are a separate matter from the caching, and they exist for a
different reason. Before this decision, 52 call sites across 25 files each wrote
a URL as a template literal and declared the response shape inline, so a field
renamed on the server compiled cleanly on both sides and failed when a person
opened the screen. The server already publishes an accurate description of all
46 routes: the test at `crates/server/server/src/openapi.rs:337` compares the
committed `docs/src/assets/openapi.json` against the document the live code
produces and fails when they differ. Generating the web app's response types
from that file turns a server-side rename into a web-side compile error.

## Considered and rejected: writing our own hook

One in-house hook — roughly 150 lines holding a `Map`, a guard against
duplicate in-flight requests, a subscriber list, and invalidation — would have
avoided a new dependency, and it is close to what `contactDetailCache.ts`
already does.

It was rejected because it does not remove the pressure that created the six.
An in-house hook is a thing this project maintains, so the first screen it does
not quite fit produces a seventh mechanism written beside that screen, and the
argument for writing it will be as reasonable as the arguments that produced the
first six. A dependency cannot be extended that way. When someone needs caching
on a new screen, the answer is an import.

The second reason is how much code the library's invalidation deletes. The four
`mv-*-changed` browser events — `mv-contact-groups-changed`,
`mv-message-tags-changed`, `mv-saved-searches-changed`,
`mv-contact-detail-changed` — exist to tell components that a cached list
changed, and every component that listens must also remove its listener when it
unmounts. All of that is replaced by marking cache entries stale.

## Considered and rejected: generating the route functions as well as the types

Tools exist that read an OpenAPI document and generate a whole client, not only
its types. That would have removed the 46 hand-written functions.

It was rejected because generated function names mirror the HTTP shape,
including where the HTTP shape reads badly. Renaming a contact is
`POST /v1/export/contacts/{id}`, which generates a name built from that path
rather than the name a reader wants, which is `renameContact`. Writing a
readable wrapper over the generated client means maintaining two layers where
one would do. The 46 functions are short, they are written once, and they are
where the good names live.

## Considered and rejected: replacing the URL strings in tests with a fake server

Eleven test files call `vi.mock` on `web/src/lib/api`, and thirteen name a
`/v1/` URL directly. Several decide what to return by comparing that URL text:
`useImportJob.test.tsx` writes a `/v1/` path in 15 places, including
`if (path === "/v1/imports")`. A route renamed on the server leaves those tests
passing, because the comparison stops matching rather than failing.

Running the real route functions against a fake HTTP server, such as MSW, would
have caught that. It was rejected because the fake server is a second
description of the API to keep in step with the server. Tests fake the named
route functions instead, and the URLs those functions build are asserted in
`serverApi.test.ts` — one file to keep honest rather than eleven.

Two tests keep naming URLs on purpose. `api.test.ts` and `assetUrl.test.ts`
have the URL as their subject. `AdminUsersPanel.test.tsx` stubs `fetch` and
throws on an address it does not recognise, so it exercises the real route
functions end to end and fails loudly on a rename rather than quietly matching
nothing — the opposite of the pattern this decision removes.

## Consequences

- `web/src/lib/serverApi.ts` holds one function per server route. Its generated
  companion, `web/src/lib/serverApi.types.ts`, is checked in.
  `scripts/check-generated-api-types.sh` regenerates the types and fails on any
  diff, mirroring what the `committed_openapi_matches_dump` test in
  `crates/server/server/src/openapi.rs` already does for the JSON document.
  Regenerate the JSON with
  `cargo run -p message-crate-server -- dump-openapi --output docs/src/assets/openapi.json`.
- `web/src/lib/api.ts` keeps `apiClient`, the base URL, and the Bearer header.
  It is the transport that `serverApi.ts` uses and is not called from screens.
- Response shapes are deleted from `web/src/lib/types.ts` and come from the
  generated file. Shapes that describe the interface rather than a server
  response stay.
- `useResource`, `usePagedList`, `contactDetailCache`, and the four
  `mv-*-changed` events are removed. `nameCollection.ts`, `savedSearches.ts`,
  `contactGroups.ts`, `messageTags.ts`, `useAccountProfile.ts`, and
  `importSession.ts` survive as
  the per-feature layer over TanStack Query, without caches of their own.
  `InfiniteOffsetList` and `VirtualList` are untouched: they render the items
  they are given and never fetch.
- The five browse and contact-edit routes move off the `/v1/export/` prefix onto
  prefixes naming the resource, and editing a contact becomes `PATCH` rather
  than `POST`. The prefix is only a naming problem: the server already requires
  a logged-in session for those five and accepts an export-scoped API token only
  on `GET /v1/export/messages` and `GET /v1/export/messages/count`, which keep
  the prefix (both later gave way to the Export Run routes under `/v1/exports`).
  The rename happens in the same pull request as the route
  functions, so those functions are written once against their final URLs.
- The work landed as four pull requests, split for reviewability rather than to
  avoid breaking anything. The first added the generated types, the route
  functions, and the drift check, renamed the routes, and converted every call
  site. The second added TanStack Query and converted the `useResource` and
  `usePagedList` screens. The third converted Contact Groups, Message Tags, and
  Saved Searches, added account-scoped cache keys, and closed the Saved Searches
  leak. The fourth converted the contact-detail and account-profile caches,
  which needed restructuring rather than a swap: one was read during render and
  written in place for optimistic group chips, and the other was loaded from
  outside React during login.
- `auth.tsx` clears nothing by hand. Its two lists are one `queryClient.clear()`
  in each path, which releases memory and nothing more.
- Every write marks the whole account's cache stale once it settles, through
  one call: `invalidateAccount` on `useRouteCache` in
  `web/src/lib/routeQuery.ts`. No write names the entries it changes. Each
  write once named its own list of entries, and the review of 2 October 2026
  found six that left out entries they changed, so the screens showing those
  kept the old state (issue #1239). A list per write is a list someone forgets
  to extend; the account prefix is the one key no write can leave out. The
  cost is small, because TanStack Query refetches only the entries on screen.
  A write that answers with the whole value still writes it into its entry,
  and the optimistic chips of Contact Groups and Message Tags still draw
  before the server answers, so neither waits for the refetch.
- `contact_id` on a conversation participant became an `i64`. It was the only
  contact id on the whole API sent as a string; every other shape — the contact
  list, one contact, a selection summary, an import's contacts, and the
  `/v1/contacts/{id}` path — already used the integer the database stores. The
  web app carries contact ids as strings for routes and DOM ids and converts at
  that edge.
- The generator runs through `npx`, pinned, rather than as a `web/` dependency:
  `openapi-typescript` declares a peer dependency on TypeScript 5 and this
  project is on TypeScript 7, so installing it into `web/` does not resolve.
  Only its text output reaches the repository. Biome is configured to skip the
  generated file, because formatting it would make it differ from what the
  generator produces and the drift check compares the two byte for byte.
- No part of this work keeps an existing interface for compatibility. Message
  Crate has no users, so routes, types, and module layouts change wherever a
  simpler result follows, and tests are rewritten to fit rather than preserved.
- `CONTEXT.md` is unchanged. It holds the product's language and no
  implementation detail, and nothing here introduces a product concept.
