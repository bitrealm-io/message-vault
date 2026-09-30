# Rename Message Vault to Message Crate

Date: 2026-09-30. Status: approved design, not yet built.

## Why

The product is now called Message Crate. The GitHub repository already moved
to `messagecrate/message-crate`, the docs site already answers at
`messagecrate.app`, the hosted product already answers at
`my.messagecrate.app`, and the Docker Hub repository `bitrealm/message-crate`
already exists. The tree has not followed: nothing in the source, docs, or
workflows says `messagecrate`, the live docs site still calls itself Message
Vault, and its CNAME file still says `bitrealm.io`.

The word "vault" leaves with the name. It was both the brand and the
glossary's word for one installation, and it goes from both. Nothing replaces
it as a noun: the installation is "a Message Crate", the person who claims it
is "the Owner", and code that used `Vault` as a qualifier drops it.

The no-compatibility rule applies in full. Routes, table names, token
prefixes, environment variables, the database file name, crate names, and
the desktop bundle identifier all change. No alias, no redirect inside the
product, no migration.

## Decisions

| Question | Decision |
|---|---|
| Product name in prose | "Message Crate", two words. One lowercase word only in domains and identifiers. |
| Word for one installation | None. "A Message Crate", "your Message Crate". The glossary keeps a **Message Crate** entry defining that countable use. |
| Vault Owner | "Owner". "Create Vault Owner" becomes "Create Owner". |
| Landing page and docs host | `messagecrate.app`, one Astro site on GitHub Pages. Docs move from `/vault/user/` and `/vault/developer/` to `/docs/user/` and `/docs/developer/`. |
| Hosted product host | `my.messagecrate.app`. |
| Docker image | `bitrealm/message-crate`. |
| Token, env, and CSS prefixes | `mv-` becomes `mc-`, `MV_` becomes `MC_`. |
| Cargo, npm, and directory names | `message-crate-` prefix on every product crate; `-io` dropped. |
| Desktop bundle identifier | `app.messagecrate.desktop`. |
| Code identifiers that used `Vault` as a qualifier | Drop the qualifier. Where the bare word collides, name the thing: `Server`. |
| History | Links are rewritten everywhere. Past changelog entries keep "Message Vault". ADRs are updated to describe the current state. A new changelog entry records the rename. |
| Landing page overhaul | Separate work, after PR (a). |
| Version | `0.10.0`, tagged after PR (c) merges. |
| Sequencing | Three PRs: (a) org, domains, docs URLs; (b) the name in words; (c) identifiers. |

## What does not change

- `web-next/` is untouched. It is not the product path and is kept on
  purpose (CLAUDE.md).
- `imessage-reader`, `imessage-reader-protocol`, every `*-exporter` crate,
  and every library crate whose name does not contain `vault` keep their
  names.
- `schema_version` of the shared conversation model stays `4`.
- The `/v1` route shapes, status codes, and the rules in
  `docs/architecture/http-api.md` stay as they are; only the four routes
  named below move.
- The Docker Hub account stays `bitrealm`, so the release job's credentials
  stay the same.

## PR (a): org, domains, and docs URLs

Ships first and alone. After it merges and `docs.yml` runs, the live site at
`messagecrate.app` stops pointing at bitrealm.io.

| Item | From | To |
|---|---|---|
| GitHub links | `github.com/bitrealm-io/message-vault` | `github.com/messagecrate/message-crate` |
| Docs host | `https://bitrealm.io` | `https://messagecrate.app` |
| Docs path | `/vault/user/…`, `/vault/developer/…` | `/docs/user/…`, `/docs/developer/…` |
| Content directory | `docs/src/content/docs/vault/` | `docs/src/content/docs/docs/` |
| `docs/public/CNAME` | `bitrealm.io` | `messagecrate.app` |
| Astro `site` | `https://bitrealm.io` | `https://messagecrate.app` |
| Astro `redirects` | `/vault/developer/docker-compose/` | `/docs/developer/docker-compose/`, target `/docs/developer/docker/` |
| Astro `editLink.baseUrl` and `social` | old repo | new repo |
| Rustdoc publish path in `docs.yml` | `docs/public/vault/developer/rustdoc` | `docs/public/docs/developer/rustdoc` |
| `ERRORS_URL` in `crates/vault/server/src/problem.rs` | `https://bitrealm.io/vault/developer/reference/errors/` | `https://messagecrate.app/docs/developer/reference/errors/` |
| Error pages and every generated link in `docs/src/assets/openapi.json` | old host and path | new host and path (regenerate from `openapi.rs`, then run the generated-types check) |
| Doc comments in `config.rs`, `lib.rs` files, and READMEs | `bitrealm.io/vault/…` | `messagecrate.app/docs/…` |
| Test hosts in `vault-http`, `vault-push`, `vault-pull` | `app.bitrealm.io` | `my.messagecrate.app` |
| Docker image in `docker/compose.yml` and the release job in `ci.yml` | `bitrealm/message-vault` | `bitrealm/message-crate` |
| Release notes template in `ci.yml` | old docs URLs and image | new |
| Sidebar entries in `docs/astro.config.mjs` | `vault/developer/…` | `docs/developer/…` |
| Repo description and homepage on GitHub | "self-hosted vault", `http://bitrealm.io` | one sentence without the word, `https://messagecrate.app` |

The GitHub Pages build type is `workflow`, and the custom domain was set by
hand in the repository settings. A deploy that ships a CNAME file saying
`bitrealm.io` can reset that domain, so the CNAME file must change in this
PR and not later.

## PR (b): the name in words

Every reader-facing and agent-facing sentence. No identifiers.

- **Product name.** "Message Vault" becomes "Message Crate" in product copy,
  the docs title and description, `productName` and the window `title` in
  `src-tauri/tauri.conf.json`, CLAUDE.md, AGENTS.md, CONTRIBUTING.md, every
  README, `docs/agents/writing-style.md`, and the intro of CONTEXT.md.
- **The installation.** "the vault" and "your vault" become "Message Crate"
  and "your Message Crate" where the sentence is about the installation.
  Where the sentence is about the server process, "the server". Where it is
  about the database, "the database".
- **The owner.** "Vault Owner" becomes "Owner"; "Create Vault Owner" becomes
  "Create Owner"; "claim the vault" becomes "claim it" or "claim this
  Message Crate".
- **Glossary.** CONTEXT.md loses the **Vault** entry and gains a **Message
  Crate** entry: one installation of the product, the thing a person claims,
  owns, and logs into, holding many accounts. **Owner**, **Claiming**,
  **Account**, and **Product Version** are reworded to point at it.
- **ADRs.** Each ADR body is updated to the current vocabulary, including
  the file names `0001-no-command-line-except-the-vault-server.md` and
  `0008-the-vault-owner-holds-no-messages.md`, which become
  `0001-no-command-line-except-the-server.md` and
  `0008-the-owner-holds-no-messages.md`. Links to them follow.
- **Architecture docs.** `docs/architecture/http-api.md` and
  `contacts-identities-and-messages.md` are reworded; the route names in
  them change in PR (c).
- **Changelog.** Past entries keep "Message Vault". A new entry under the
  unreleased heading records the rename, the new hosts, the new image, and
  that every token, route, table, and file name changes.
- **Style guide.** The fixed product vocabulary in
  `docs/agents/writing-style.md` replaces "vault" with the words above and
  adds "vault" to the words not to use.
- **Docs pages.** `docs/src/content/docs/docs/developer/vault-design.md`
  becomes `design.md`; the user glossary page follows CONTEXT.md.

## PR (c): identifiers

Mechanical, large, and green on its own. Every developer needs a fresh
`data/` directory afterwards because the database file name changes and the
schema fingerprint changes with the table names.

### Routes

| From | To |
|---|---|
| `GET /v1/vault` | `GET /v1/server` |
| `POST /v1/vault/claim` | `POST /v1/server/claim` |
| `GET`/`PUT /v1/vault/settings` | `GET`/`PUT /v1/server/settings` |
| `GET /v1/vault/storage` | `GET /v1/server/storage` |

`Server` is the collision tiebreaker from the decisions table: bare
`/v1/settings` would read as the logged-in account's settings, and the
http-api rules already describe this group as the one singleton per
installation. The route group file `vault_api.rs` becomes `server_api.rs`.
`docs/architecture/http-api.md` is updated to match, and the OpenAPI
document and generated web types are regenerated.

### Tables and indexes

| From | To |
|---|---|
| `vault_settings` | `settings` |
| `vault_imports` | `imports` |
| `vault_exports` | `exports` |
| `vault_import_issues` | `import_issues` |
| `vault_import_contacts` | `import_contacts` |
| `vault_export_messages` | `export_messages` |
| `ix_vault_*`, `ux_vault_*` | same names without `vault_` |

Only `schema/sql/*.sql` and the Rust that names these tables change. The
schema fingerprint changes on its own.

### Prefixes and environment

| From | To |
|---|---|
| `mv-user-`, `mv-api-`, `mv-tok-` token prefixes | `mc-user-`, `mc-api-`, `mc-tok-` |
| `mv-app-`, `mv-theme-`, `mv-remember-`, and the other browser-storage keys | `mc-…` |
| `mv-staging-`, `mv-importer-`, `mv-ffmpeg-`, `mv-device-`, `mv-contact-`, `mv-contacts-`, `mv-left-` internal prefixes | `mc-…` |
| `--mv-*` CSS variables and `mv-*` class names | `--mc-*`, `mc-*` |
| `MV_TEST_POSTGRES_URL`, `MV_ZONE_CHECK` | `MC_TEST_POSTGRES_URL`, `MC_ZONE_CHECK` |
| `VAULT_DB`, `VAULT_DATA_DIR`, `VAULT_API_URL` | `MC_DB`, `MC_DATA_DIR`, `MC_API_URL` |
| `VAULT_SCHEMA_META_KEY`, `VAULT_SCHEMA_LOCK_ID`, `VAULT_EXPORT_COLUMNS`, `VAULT_IMPORT_COLUMNS` (Rust constants) | same without `VAULT_` |
| Postgres user, password, and database `vault` in `ci.yml` and the Postgres dev script | `messagecrate` |

### Files and defaults

| From | To |
|---|---|
| `data/vault.db` | `data/messagecrate.db` |
| `scripts/run-vault-dev.sh`, `scripts/run-vault-pg-dev.sh`, `scripts/sync-vault-schema.mjs` | `scripts/run-dev.sh`, `scripts/run-pg-dev.sh`, `scripts/sync-schema.mjs` |
| `message_vault_*` release asset names in `ci.yml` | `message_crate_*` |
| `io.bitrealm.message-vault` in `tauri.conf.json` | `app.messagecrate.desktop` |

### Crates, packages, and directories

| From | To | Directory |
|---|---|---|
| `message-vault-server` | `message-crate-server` | `crates/vault/server/` → `crates/server/server/` |
| `demo-seed` (unchanged name) | `demo-seed` | `crates/vault/demo-seed/` → `crates/server/demo-seed/` |
| `message-vault-io-core` | `message-crate-core` | `crates/core/message-vault-io-core/` → `crates/core/message-crate-core/` |
| `vault-api-types` | `message-crate-api-types` | `crates/libs/vault-api-types/` → `crates/libs/api-types/` |
| `vault-http` | `message-crate-http` | `crates/libs/vault-http/` → `crates/libs/http/` |
| `vault-push` | `message-crate-push` | `crates/libs/vault-push/` → `crates/libs/push/` |
| `vault-pull` | `message-crate-pull` | `crates/libs/vault-pull/` → `crates/libs/pull/` |
| `message-vault-io-tauri`, lib `message_vault_io_tauri_lib` | `message-crate-desktop`, lib `message_crate_desktop_lib` | `src-tauri/` stays |
| `message-vault-io-web` | `message-crate-web` | `web/` stays |

`crates/vault/` becomes `crates/server/` so the top-level layout reads
`core`, `exporters`, `helpers`, `libs`, `server`. Every `use`, `Cargo.toml`
dependency, `--manifest-path`, `-p` flag in scripts and workflows, the
rustdoc landing page in `docs.yml`, and the `cargo tree` GPL check in
`audit.yml` follow.

### Rust identifiers

Drop `Vault` or `vault_` and keep the rest, except where the bare name
would be meaningless or collide. The full list with the resolved names:

| From | To |
|---|---|
| `VaultSettings`, `VaultState`, `VaultStorage` (server) | `ServerSettings`, `ServerState`, `ServerStorage` |
| `UpdateVaultSettingsRequest`, `ClaimVaultRequest` | `UpdateServerSettingsRequest`, `ClaimRequest` |
| `get_vault`, `claim_vault`, `get_vault_settings`, `update_vault_settings`, `get_vault_storage` | `get_server`, `claim`, `get_server_settings`, `update_server_settings`, `get_server_storage` |
| `vault_is_claimed`, `is_vault_owner`, `claim_vault_as_owner` | `is_claimed`, `is_owner`, `claim_as_owner` |
| `OpenVault`, `open_vault`, `ensure_vault_schema`, `migrate_vault_schema`, `rebuild_vault_schema`, `with_vault_pragmas`, `apply_vault_ddl`, `apply_postgres_vault_ddl`, `pg_vault_*` | `OpenDb`, `open_db`, `ensure_schema`, `migrate_schema`, `rebuild_schema`, `with_pragmas`, `apply_ddl`, `apply_postgres_ddl`, `pg_*` |
| `VaultOperationLock` | `OperationLock` |
| `VaultImportRow`, `vault_import_from_row`, `vault_imports`, `vault_exports`, `vault_export` | `ImportRow`, `import_from_row`, `imports`, `exports`, `export` |
| `VaultPushConfig`, `VaultPullConfig`, `VaultHttpError` | `PushConfig`, `PullConfig`, `HttpError` |
| `vault_request`, `vault_config` | `request`, `config` |
| `TestVault`, `test_vault`, `vault_with_account`, `vault_with_two_conversations`, `vault_with_png`, `vault_with_export_and_book`, `vault_with_a_run`, `seeded_export_vault`, `seeded_schema_vault`, `setup_vault`, `generate_vault` (test helpers) | `TestServer`, `test_server`, `server_with_account`, and so on |
| `message_vault_io_core`, `message_vault_server`, `vault_api_types`, `vault_http`, `vault_push`, `vault_pull` (module paths) | follow the crate names above |

### Web identifiers and files

| From | To |
|---|---|
| `web/src/lib/vaultApi.ts`, `.test.ts`, `.types.ts`, `vaultApiOpenapi.test.ts` | `routes.ts`, `routes.test.ts`, `routes.types.ts`, `routesOpenapi.test.ts` |
| `vaultApi` (the object) | `routes` |
| `VaultApiError` | `ApiError` |
| `VaultRequestOptions` | `RequestOptions` |
| `vaultQuery.ts`, `useVaultQuery`, `VaultQueryKey`, `vaultQueryKey.ts`, `vaultKeys.ts`, `vaultKeys` | `query.ts`, `useQuery` collides with TanStack, so `useRouteQuery`; `QueryKey` collides, so `RouteQueryKey`; `queryKey.ts`; `keys.ts`, `keys` |
| `useVaultCache`, `VaultCache`, `VaultCacheEntries`, `resetVaultCache`, `createVaultQueryClient` | `useCache`, `Cache`, `CacheEntries`, `resetCache`, `createQueryClient` |
| `useVaultPagedList` | `useRoutePagedList` (`usePagedList` was the name of the removed mechanism, so it is not reused) |
| `VaultProviders`, `vaultProviders.tsx`, `renderWithVault`, `stubVault` | `Providers`, `providers.tsx`, `renderWithProviders`, `stubServer` |
| `useVaultState.ts`, `useVaultState`, `getVaultState`, `vaultState`, `VaultState`, `VaultConnection` | `useServerState.ts`, `useServerState`, `getServerState`, `serverState`, `ServerState`, `ServerConnection` |
| `useVaultHealth.ts`, `vaultHealth.ts`, `useVaultHealth`, `checkVaultHealth`, `vaultHealth`, `VaultHealthStatus` | `useServerHealth.ts`, `serverHealth.ts`, `useServerHealth`, `checkServerHealth`, `serverHealth`, `ServerHealthStatus` |
| `useVaultInfo.ts`, `useVaultInfo`, `vaultInfo`, `vaultVersion` | `useServerInfo.ts`, `useServerInfo`, `serverInfo`, `serverVersion` |
| `useIsVaultOwner.ts`, `useIsVaultOwner` | `useIsOwner.ts`, `useIsOwner` |
| `vaultSource.ts`, `vaultSource`, `vaultSourceForMethod` | `source.ts`, `source`, `sourceForMethod` |
| `vaultLogin`, `vaultLogout` | `login`, `logout` |
| `getVaultSettings`, `updateVaultSettings`, `getVaultStorage`, `vaultSettings`, `vaultStorage`, `VaultSettings`, `VaultStorage`, `UpdateVaultSettingsRequest`, `ClaimVaultRequest`, `claimVault` | `getServerSettings`, `updateServerSettings`, `getServerStorage`, `serverSettings`, `serverStorage`, `ServerSettings`, `ServerStorage`, `UpdateServerSettingsRequest`, `ClaimRequest`, `claim` |
| `trashVaultConversation`, `restoreVaultConversation`, `deleteVaultConversation`, `trashVaultContact`, `restoreVaultContact`, `deleteVaultContact`, `emptyVaultTrash`, `deleteAllVaultMessages`, `createVaultSavedSearch`, `updateVaultSavedSearch`, `deleteVaultSavedSearch` | same without `Vault` |
| `VaultStatus.tsx`, `VaultStatus`, `VaultStatusProps` | `ServerStatus.tsx`, `ServerStatus`, `ServerStatusProps` |
| `VaultSettingsScreen.tsx`, `VaultSettingsPanel.tsx`, `VaultContentsSection.tsx`, `ClaimVaultForm.tsx` and their components | `ServerSettingsScreen.tsx`, `ServerSettingsPanel.tsx`, `ContentsSection.tsx`, `ClaimForm.tsx` |
| Tauri command names `get_vault`, `claim_vault`, `get_vault_settings`, `update_vault_settings`, `get_vault_storage` in `tauri.ts` and `src-tauri/src/commands/` | `get_server`, `claim`, `get_server_settings`, `update_server_settings`, `get_server_storage` |
| `vault_imports` in web strings | `imports` |

Every web rename is a Biome-clean rename; no `biome-ignore`.

## Release

After PR (c) merges: bump the four lockstep files to `0.10.0`, move the
changelog entry under that version, and tag `v0.10.0`. The release job pushes
`bitrealm/message-crate:0.10.0`, publishes the docs to `messagecrate.app`,
and attaches `message_crate_*` installers. The next desktop build installs
beside any existing Message Vault build rather than over it, because the
bundle identifier changed.

## Verification

- Each PR passes `./scripts/check-all.sh`, which is what CI runs.
- After PR (a): the live site's links resolve, the OpenAPI document's
  `type` URLs match `ERRORS_URL`, and `grep -rI bitrealm.io` over the tree
  (excluding `web-next/` and CHANGELOG.md) finds nothing.
- After PR (b): `grep -rIiw vault` over prose files finds only CHANGELOG.md
  entries dated before the rename.
- After PR (c): `grep -rIi vault` over the tree, excluding `web-next/` and
  CHANGELOG.md, finds nothing, and a fresh clone runs the dev script, claims
  an owner, imports a fixture, and logs in from the desktop app against
  `my.messagecrate.app`.

## What was built differently

Names chosen while building PR (c), where the name above collided with
something that already exists or a better fit turned up. These are the
names in the tree.

| This document says | The tree has | Why |
|---|---|---|
| `routes.ts`, `routes`, `routes.types.ts` | `serverApi.ts`, `serverApi`, `serverApi.types.ts` | `routes` is already used about a hundred times in `web/src` |
| `query.ts`, `queryKey.ts`, `keys.ts` | `routeQuery.ts`, `routeQueryKey.ts`, `queryKeys.ts` | the hooks in them are `useRouteQuery` and `RouteQueryKey`; `keys` stays the exported object |
| `useCache`, `Cache`, `resetCache` | `useRouteCache`, `RouteCache`, `resetRouteCache` | matches `useRouteQuery` |
| `login`, `logout`, `claim` (web) | `serverLogin`, `serverLogout`, `claimServer` | the bare names already exist in `web/src` |
| `source.ts`, `sourceForMethod` file | `importSource.ts`; the function is `sourceForMethod` | `source` alone says nothing |
| `TestServer`, `test_server`, `server_with_*` | `TestFixture`, `test_fixture`, `fixture_with_*` | a `TestServer` test type already exists |
| table `settings` | `server_settings` | a bare `settings` table reads as an account's settings |
| `is_owner`, `claim` (Rust) | `is_server_owner`, `claim_server` | `is_owner` and a `claim` function already exist |
| `request`, `config` (Rust) | `server_request`, `server_config` | too generic to be found again |
| `Vault` response type, not listed | `ServerInfo` | the `GET /v1/server` response |
| not listed | `storage::Scope::AllAccounts`, schema marker key `schema_fingerprint` | were `Scope::Vault` and `vault_schema` |
| not listed | `~/message-crate` staging default, `.import-state.jsonl`, `server.ready`, `x-message-crate-app` and `x-message-crate-version` headers, `message-crate-auth` storage key, `MESSAGE_CRATE_*` build env vars | every on-disk and on-the-wire name that carried the old product name |
| not listed | pulled documents carry `server:{id}`, `server_message_id`, `server_source` | were `vault:{id}`, `vault_message_id`, `vault_source` |
| not listed | compose service `server`, volume `message-crate-data`, Postgres user and database `messagecrate` | |
| diagram files `vault_N_*_diagram` | `system_diagram`, `container_diagram`, `deployment_diagram` | |

The word survives in four places on purpose: the two "avoid" lines in
`CONTEXT.md`, the vocabulary row in `docs/agents/writing-style.md`, paths
into `web-next/`, and the address `vault@bitrealm.io`, which has no
replacement yet.

## Out of scope

- The landing page at `messagecrate.app/` (its own brainstorm).
- Any change to what the product does.
- `web-next/`.
