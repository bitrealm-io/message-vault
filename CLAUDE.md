# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Message Crate pulls conversations out of chat apps (iMessage, WhatsApp, SMS backups) and makes them searchable on a self-hosted server. Three pieces:

- **Server** (`crates/server/server/`) — Axum HTTP API (`/v1/*`) over SQLite at `data/messagecrate.db` by default (`[paths] db`, or `--db`). Login is a local account: an Argon2 password hash, and an opaque `mc-user-` session token stored hashed with an expiry (not a JWT); named API tokens with import/export scopes also exist.
- **Desktop app** (`src-tauri/` + `web/`) — Tauri v2 shell around a Vite + React 19 + TypeScript SPA. It reads phone backups, writes JSONL, and imports into a running server. Browse/search work in the browser too; importing needs the desktop app. The installer carries `message-crate-server` and the built website, and the app starts that server at `127.0.0.1:8080` when nothing answers there, with its data in the operating system's app-data folder; it stops it on close. A Message Crate already answering at that address (Docker, `./scripts/run-dev.sh`) is used as it is. Rules: `src-tauri/src/local_server.rs`; why: `docs/adr/0018-the-desktop-app-starts-the-server-it-ships.md`.
- **Website** — the same `web/` SPA served from the server's `static/`.

**AGENTS.md is the canonical operations guide** (first-time setup, dev run instructions, release process, PR workflow) and the only place commands are written down. Claude Code loads this file on its own and AGENTS.md only when it is read, so the Commands section below says when to read it. This file covers the architecture and the rules that are easy to get wrong; see AGENTS.md for anything operational not covered here. Published docs live at messagecrate.app (Astro Starlight in `docs/`).

## Data flow (the big picture)

```
vendor backup (chat.db, SMS XML, WhatsApp crypt15, …)
  → exporter crate (crates/exporters/*) parses it into message-ir types
  → ConversationDocument (schema_version 4) written as JSONL
  → Tauri push command (message-crate-push library) → POST /v1/... → SQLite
  → web/ SPA reads threads back through the /v1/ API
```

- **`crates/libs/ir`** (`message-ir`) is the shared conversation model every exporter writes: `ConversationDocument` holds export metadata, participants, and messages. `schema_version` is `4` and independent of the product version. A version-3 file is refused by name, never upgraded.
- **`crates/libs/ir-format`** reads/writes the formats Message Crate emits itself (JSON, JSONL, CSV, EML, MBOX) to/from IR; **`crates/libs/staging`** (`message-staging`) is the resumable write path every exporter and the desktop app go through (`ExportWriter`, the write queue, the transcode pass, the staging summary); SBR XML belongs to `crates/exporters/sms-backup-restore-exporter` in both directions; **`crates/libs/reexport`** converts between existing export formats, which is how Export writes anything other than JSONL. One job per crate, and why: `docs/adr/0012-four-crates-in-the-export-pipeline.md`.
- **No command line except the server.** Every exporter, `message-reexport`, `message-crate-push`, and `message-crate-pull` are library crates with no binary; the desktop app calls them in process. Only `message-crate-server`, `demo-seed`, and `imessage-reader` build binaries, and the last is not a command line: it is the GPL helper the desktop app spawns to read Apple Messages (below). Why: `docs/adr/0001-no-command-line-except-the-server.md`.
- **GPL only behind a process boundary.** `imessage-database` and `crabapple` are GPL-3.0-or-later and the repository is under the Fair Core License, so `crates/helpers/imessage-reader` (GPL) is the only crate that links them. `crates/libs/ios-backup` starts it as a process (for the Apple Messages exporter, the WhatsApp import from an encrypted iPhone backup, and the desktop app's backup checks) and talks JSON lines over stdin/stdout through `crates/helpers/imessage-reader-protocol` (MIT OR Apache-2.0, so both sides can link it). `src-tauri/build.rs` builds the helper and Tauri ships it beside the app as an `externalBin`; `cargo tree --manifest-path src-tauri/Cargo.toml -i imessage-database` must match nothing. `cargo deny check licenses bans` in `audit.yml` enforces the rule. Why, and the rules: `docs/adr/0014-gpl-code-only-behind-a-process-boundary.md`.
- **One way to fetch data in `web/`** — TanStack Query over route functions in `web/src/lib/serverApi.ts`, with response types generated from `docs/src/assets/openapi.json`. Do not write a new cache, change-notification event, or fetching hook for a screen. This is **built** (PRs #290–#293): `useResource`, `usePagedList`, and `contactDetailCache` are gone, the `mv-*-changed` browser events with them; `nameCollection`, `savedSearches`, and `useAccountProfile` remain only as thin wrappers over TanStack Query, not as mechanisms of their own. Every cache entry is named with the logged-in account, so nothing has to be cleared when the account changes. Why, and what replaces what: `docs/adr/0002-one-way-to-fetch-data-in-the-web-app.md`.
- **`crates/core/message-crate-core`** — shared export pipeline, jobs, form model. The form's validation reports problems as a `Vec<String>` so the desktop app can show them as they are; the pipeline itself returns `anyhow` errors like every other crate.
- **`crates/server/server`** — each `*_api.rs` file is one Axum route group; `db/` modules mirror the table sources in the repo-root `schema/sql/*.sql`, which the server embeds at compile time (`db/schema.rs`) — change tables there, not in a live db file. Import path: `jsonl.rs` reads the records, then `imports_api/` runs `staging` (which calls `contact_name`) → `promote`, calling `dedupe.rs`; demo mode is a seed action, not a runtime mode — `reset_demo.rs` writes one account row, `demo` at the fixed `DEMO_ACCOUNT_ID` with a NULL password hash, which is why `demo` logs in with an empty password, and no owner. `serve` runs it with the medium set on a database that does not exist yet, before it listens, so every new Message Crate starts with the Demo Account; `reset-demo --size medium|large` rebuilds it; `create-database` makes an empty one. The generator's inputs are compiled into the server, so seeding needs no files beside the binary. Nothing at request time knows a Message Crate is a demo. The Demo Account itself is fixed by its id: `is_demo_account` reports `is_demo` on the profile, and the server refuses a password, a status, permission or identity change, and deleting its messages for good, from the account and from the owner alike. The seed grants it export and neither import nor delete. Why: `docs/adr/0016-the-demo-account-is-fixed-not-configured.md`. A test module over a few hundred lines lives beside its source as `<module>/tests.rs` (declared `mod tests;`), so the source file stays readable; small ones stay inline.
- **`src-tauri/`** is **not a workspace member** (own `Cargo.toml`, listed in the root workspace `exclude`). Its `commands/` wrap the exporter crates and push/pull for the desktop app. Format/build it with `--manifest-path`.
- **`web/src/lib/api.ts`** holds the server URL, the session token, and the `apiClient` fetch wrapper with its `ApiError`; the route functions built on it are in `serverApi.ts`; `web/src/lib/tauri.ts` wraps desktop-only commands; `desktopFeatures.ts` gates them. Tests sit next to sources as `*.test.ts(x)` (Vitest + Testing Library).
- **Not the product path**: `web-next/` (legacy Next.js browse UI). New features go in `web/` + `src-tauri/` + `crates/server/server/`. **It is kept on purpose — do not propose deleting it.** It stays until the functionality worth keeping has been ported into `web/`, and that porting work is not yet defined or scoped: nobody has named which screens or behaviours would come across. Being outside CI, unserved, and excluded from dependabot are all true and none of them are an argument for removing it. Its screens and a feature-by-feature comparison with `web/` are recorded in `docs/superpowers/reference/web-next.md`. The old Slint GUI is gone; its screens are recorded in `docs/superpowers/reference/legacy-slint-gui.md`.

## Commands

Every command is in AGENTS.md, and nothing is repeated here. Claude Code does not load AGENTS.md on its own, so read the section first:

- **Starting the server, the browser UI, or the desktop app** — "Run the server (development)": dev script flags, the demo login, and what must not run at the same time.
- **Checking work before a push** — "Build, format, and test". `./scripts/check-pr.sh` is the fast pre-flight and `./scripts/check-all.sh` is everything CI runs; the section has the single-crate, coverage, `web/` and `docs/` commands.
- **After a `web/` UI change** — "Tools": verify in the browser with the Playwright MCP.
- **A new machine** — "First time setup". **A version bump, changelog entry, or release** — "Releases and versions".

## Rules that are easy to get wrong

- **No backwards compatibility, anywhere.** Endpoint names, request and response
  shapes, config keys, on-disk layout: all of it changes whenever a better design
  is found, and breaking a client is an accepted, expected cost. There are no
  users to protect — only developers, who rebuild. Never add a compatibility
  alias, a deprecation window, a version handshake, or a migration path for an
  old client, and never argue against a change on the grounds that something
  already calls it. `docs/architecture/http-api.md` says this for the HTTP interface; it holds for every
  interface. Do not raise this as an open question. A version check that only
  refuses a mismatch, and never adapts to the older side, is allowed: it is not
  a handshake, because nothing is negotiated. The refusal of a version-3
  conversation file and the Apple Messages Reader's `PROTOCOL_VERSION`
  (`crates/helpers/imessage-reader-protocol`, checked in
  `crates/libs/ios-backup/src/helper.rs`) are the two in the code.
- **SQLite is the only database engine.** Write SQL for SQLite. Never make a
  query more awkward to keep it portable, and never add an engine abstraction
  or a dialect layer. Use a SQLite-only feature when the work at hand needs
  it; do not rewrite a working query only to use one. Postgres was removed,
  not forgotten: `docs/adr/0017-sqlite-is-the-only-database-engine.md` has
  the reason and the reference commit.
- **Version lockstep** (current `0.10.0`): `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `web/package.json`, `crates/server/server/Cargo.toml` all carry the product version. Leave other crates at `0.1.0`; never bump `web-next` (`0.3.0`).
- **Pushing a `v*` tag ships a release** — CI builds the Docker image and desktop installers, creates a GitHub Release, and publishes the docs site to messagecrate.app. A merge to `main` publishes nothing. Never create or push tags unless asked.
- **CI gates** (all in `ci.yml`, all required by the ruleset on `main`): rustfmt, Clippy at `-D warnings` (workspace and `src-tauri`), workspace build + test (with ffmpeg installed, so the media tests run rather than skip), `src-tauri` check/clippy/test, web Biome `ci` + generated-types check + build + Vitest, docs `astro check` + build, license, Docker context, a build of the release Dockerfile when it or a Cargo manifest changes, product version lockstep (and on a `v*` tag, that the tag matches). A `changes` job skips what a PR doesn't touch. Dependency audits run in `audit.yml` on lockfile changes and weekly, not on every PR. Test coverage (`./scripts/coverage.sh`, cargo-llvm-cov) is a report, not a gate: it points at functions no test calls, and `coverage.yml` runs it on each push to `main`. Mutation testing (`./scripts/mutants.sh`, cargo-mutants over the workspace less what `.cargo/mutants.toml` leaves out) is the measure of whether tests would catch a change: `mutants.yml` runs it only when started by hand, never on a schedule or per pull request. `nightly.yml` builds the release Dockerfile from `main` once a night when `main` changed since the previous night, without publishing it; it is not a gate either, and a failed night opens an issue labelled `bug`. Never write a test only to raise coverage; a test earns its place by failing for a bug that matters. Why: `docs/adr/0007-ci-is-the-only-gate.md`.
- **Git workflow**: never commit to `main`; use a branch or worktree. Verify PR state with `gh pr view` / `gh pr list` / `gh pr checks` before pushing — don't assume. Don't merge PRs unless explicitly asked. Write the PR description to the matching template in `.github/PULL_REQUEST_TEMPLATE/` (`feature.md` or `bugfix.md`) — those are for the author to fill in, not options offered to a reviewer. See AGENTS.md, "Submitting Work".
- **Biome**: prefer a real fix over `biome-ignore`; prefix unused bindings with `_`.
- **Tests** use committed fixtures in the `tests/fixtures/` folder of the crate that uses them, such as `crates/exporters/imazing-exporter/tests/fixtures/`. The repository-root `tests/fixtures/` holds only the few that are not one crate's own, such as `tests/fixtures/search/web-queries.txt`, which `web/src/lib/searchQuery.test.ts` writes and the server's search tests read. Never commit personal backups or real message data.

## Style

Write instructions and commit messages in plain, direct English. The full voice
guide — documents, product copy, commit messages, and the fixed product
vocabulary — is `docs/agents/writing-style.md`.

## Agent skills

### Issue tracker and triage labels

Issues live in this repo's GitHub Issues (`gh` CLI). See `docs/agents/issue-tracker.md`. The triage labels are the five canonical names, unchanged: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`.

### Domain docs

Single-context: one `CONTEXT.md` at the repo root plus `docs/adr/`. See `docs/agents/domain.md`.

### Architecture

`docs/architecture/` holds the model of the system itself, for people and AI alike: what the entities are, how they relate, and the rules between them, each with its reason. Read `docs/architecture/contacts-identities-and-messages.md` before touching contacts, handles, participants, or what an import creates, and `docs/architecture/http-api.md` before touching a `/v1` route. A rule is written there when it is decided; where the code does not follow it yet, an open issue says so.

`docs/architecture/http-api.md` holds every rule for `/v1` routes with its reason: route shape, status codes, lists, credentials, how the OpenAPI reference is built, and how the server code is named and layered. The HTTP interface has no ADRs (`docs/adr/0011`).
