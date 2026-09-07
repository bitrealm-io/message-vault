# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Message Vault pulls conversations out of chat apps (iMessage, WhatsApp, SMS backups) and stores them in a self-hosted, searchable vault. Three pieces:

- **Vault server** (`crates/vault/server/`) — Axum HTTP API (`/v1/*`) over SQLite at `data/vault.db` by default; set the `[database] url` config (or `serve --db-url`) to run on Postgres instead. Login is a local vault account (Argon2 + JWT session tokens); named API tokens with import/export scopes also exist.
- **Desktop app** (`src-tauri/` + `web/`) — Tauri v2 shell around a Vite + React 19 + TypeScript SPA. It reads phone backups, writes JSONL, and imports into a running vault. Browse/search work in the browser too; importing needs the desktop app.
- **Website** — the same `web/` SPA served from the vault's `static/`.

**AGENTS.md is the canonical operations guide** (first-time setup, dev run instructions, release process, PR workflow) and is read by Claude Code automatically. This file covers the architecture and the rules that are easy to get wrong; see AGENTS.md for anything operational not covered here. Published docs live at bitrealm.io (Astro Starlight in `docs/`).

## Data flow (the big picture)

```
vendor backup (chat.db, SMS XML, WhatsApp crypt15, …)
  → exporter crate (crates/exporters/*) parses it into message-ir types
  → ConversationDocument (schema_version 4) written as JSONL
  → Tauri push command (vault-push library) → POST /v1/... → SQLite
  → web/ SPA reads threads back through the /v1/ API
```

- **`crates/libs/ir`** (`message-ir`) is the shared conversation model every exporter writes: `ConversationDocument` holds export metadata, participants, and messages. `schema_version` is `4` and independent of the product version. A version-3 file is refused by name, never upgraded.
- **`crates/libs/ir-format`** reads/writes on-disk formats (JSON, CSV, EML, SBR XML) to/from IR; **`crates/libs/reexport`** converts between existing export formats, which is how Export writes anything other than JSONL.
- **No command line except the vault server.** Every exporter, `message-reexport`, `vault-push`, and `vault-pull` are library crates with no binary; the desktop app calls them in process. Only `message-vault-server`, `demo-seed`, and `imessage-reader` build binaries, and the last is not a command line: it is the GPL helper the desktop app spawns to read Apple Messages (below). Why: `docs/adr/0001-no-command-line-except-the-vault-server.md`.
- **GPL only behind a process boundary.** `imessage-database` and `crabapple` are GPL-3.0-or-later and the repository is under the Fair Core License, so `crates/helpers/imessage-reader` (GPL) is the only crate that links them. `imessage-ir-exporter` starts it as a process and talks JSON lines over stdin/stdout through `crates/helpers/imessage-reader-protocol` (MIT OR Apache-2.0, so both sides can link it). `src-tauri/build.rs` builds the helper and Tauri ships it beside the app as an `externalBin`; `cargo tree --manifest-path src-tauri/Cargo.toml -i imessage-database` must match nothing. `cargo deny check licenses bans` in `audit.yml` enforces the rule. Policy: `docs/agents/licences.md`.
- **One way to fetch data in `web/`** — TanStack Query over route functions in `web/src/lib/vaultApi.ts`, with response types generated from `docs/src/assets/openapi.json`. Do not write a new cache, change-notification event, or fetching hook for a screen. This is **built** (PRs #290–#293): `useResource`, `usePagedList`, and `contactDetailCache` are gone, the `mv-*-changed` browser events with them; `nameCollection`, `savedSearches`, and `useAccountProfile` remain only as thin wrappers over TanStack Query, not as mechanisms of their own. Every cache entry is named with the signed-in account, so nothing has to be cleared when the account changes. Why, and what replaces what: `docs/adr/0002-one-way-to-fetch-data-in-the-web-app.md`.
- **`crates/core/message-vault-io-core`** — shared export pipeline, jobs, form model. The form's validation reports problems as a `Vec<String>` so the desktop app can show them as they are; the pipeline itself returns `anyhow` errors like every other crate.
- **`crates/vault/server`** — each `*_api.rs` file is one Axum route group; `db/` modules mirror the table sources in `schema/sql/*.sql`, which the server embeds at compile time (`db/schema.rs`) — change tables there, not in a live db file. Import path: `jsonl.rs` → `import.rs` → `dedupe.rs`; demo mode is a seed action, not a runtime mode — `reset-demo` (`reset_demo.rs`) writes one account row at the fixed `DEMO_ACCOUNT_ID` with a NULL password hash, which is why `demo` signs in with an empty password, and nothing at request time knows a vault is a demo. A test module over a few hundred lines lives beside its source as `<module>/tests.rs` (declared `mod tests;`), so the source file stays readable; small ones stay inline.
- **`src-tauri/`** is **not a workspace member** (own `Cargo.toml`, listed in the root workspace `exclude`). Its `commands/` wrap the exporter crates and push/pull for the desktop app. Format/build it with `--manifest-path`.
- **`web/src/lib/api.ts`** is the vault API client; `web/src/lib/tauri.ts` wraps desktop-only commands; `desktopFeatures.ts` gates them. Tests sit next to sources as `*.test.ts(x)` (Vitest + Testing Library).
- **Not the product path**: `web-next/` (legacy Next.js browse UI). New features go in `web/` + `src-tauri/` + `crates/vault/server/`. **It is kept on purpose — do not propose deleting it.** It stays until the functionality worth keeping has been ported into `web/`, and that porting work is not yet defined or scoped: nobody has named which screens or behaviours would come across. Being outside CI, unserved, and excluded from dependabot are all true and none of them are an argument for removing it. Its screens and a feature-by-feature comparison with `web/` are recorded in `docs/superpowers/reference/web-next.md`. The old Slint GUI is gone; its screens are recorded in `docs/superpowers/reference/legacy-slint-gui.md`.

## Commands

Run from the repo root unless noted. Full setup instructions: AGENTS.md.

### Dev loop

```bash
./scripts/run-vault-dev.sh                # vault API on http://127.0.0.1:8080 (keeps data/)
./scripts/run-vault-dev.sh --reset-demo   # wipe data/, seed sample inbox (sign in: user `demo`, empty password)
cd web && npm run dev                     # browser UI on :5173, proxies /v1 — OR:
cargo tauri dev                           # desktop app (starts Vite itself; never run both at once)
```

Use **127.0.0.1**, not `localhost` (the latter can resolve to IPv6, which the vault does not listen on). Restart the vault script after edits under `crates/vault/server/` (debug `cargo run`; no hot reload).

### Verify

```bash
cargo fmt --all -- --check
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo build --workspace && cargo test --workspace
cargo test -p sms-backup-restore-exporter     # one crate
cargo build --manifest-path src-tauri/Cargo.toml
cd web && npm run lint && npm test            # Biome + Vitest (CI runs `biome ci`)
cd docs && npm run check && npm run build     # docs tree only
./scripts/format-all.sh                       # rewrite: rustfmt (workspace + src-tauri) + Biome
./scripts/check-pr.sh                         # fast pre-flight: fmt --check, Clippy -D warnings, Biome ci, tsc
./scripts/check-all.sh                        # everything CI runs, serially; stops on first failure
```

After `web/` UI changes, verify in the browser with the Playwright MCP (`plugin-playwright-playwright`) against Vite on `http://127.0.0.1:5173` (vault on `:8080`). Details and Tauri-only limits: [`.cursor/rules/playwright-mcp.mdc`](.cursor/rules/playwright-mcp.mdc).

## Rules that are easy to get wrong

- **Version lockstep** (current `0.8.3`): `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `web/package.json`, `crates/vault/server/Cargo.toml` all carry the product version. Leave other crates at `0.1.0`; never bump `web-next` (`0.3.0`).
- **Pushing a `v*` tag ships a release** — CI builds the Docker image and desktop installers, creates a GitHub Release, and publishes the docs site to bitrealm.io. A merge to `main` publishes nothing. Never create or push tags unless asked.
- **CI gates** (all in `ci.yml`, all required by the ruleset on `main`): rustfmt, Clippy at `-D warnings` (workspace and `src-tauri`), workspace build + test with the Postgres suites live, `src-tauri` check/clippy/test, web Biome `ci` + generated-types check + build + Vitest, docs `astro check` + build, license, Docker context, a build of the release Dockerfile when it or a Cargo manifest changes, product version lockstep (and on a `v*` tag, that the tag matches). A `changes` job skips what a PR doesn't touch. Dependency audits run in `audit.yml` on lockfile changes and weekly, not on every PR. Test coverage (`./scripts/coverage.sh`, cargo-llvm-cov) is a report, not a gate, and function coverage is the number to chase: `coverage.yml` runs it on each push to `main`. Why: `docs/adr/0007-ci-is-the-only-gate.md`.
- **Git workflow**: never commit to `main`; use a branch or worktree. Verify PR state with `gh pr view` / `gh pr list` / `gh pr checks` before pushing — don't assume. Don't merge PRs unless explicitly asked. Write the PR description to the matching template in `.github/PULL_REQUEST_TEMPLATE/` (`feature.md` or `bugfix.md`) — those are for the author to fill in, not options offered to a reviewer. See AGENTS.md, "Submitting Work".
- **Biome**: prefer a real fix over `biome-ignore`; prefix unused bindings with `_`.
- **Tests** use committed fixtures in `tests/fixtures/`; never commit personal backups or real message data.

## Style

Write instructions and commit messages in plain, direct English. The full voice
guide — documents, product copy, commit messages, and the fixed product
vocabulary — is `docs/agents/writing-style.md`.

## Agent skills

### Issue tracker

Issues live in this repo's GitHub Issues (`gh` CLI). See `docs/agents/issue-tracker.md`.

### Triage labels

The five canonical triage labels, unchanged: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `CONTEXT.md` at the repo root plus `docs/adr/`. See `docs/agents/domain.md`.
