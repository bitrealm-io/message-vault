# Operations Guide for This Project

## Communication Style

Prose in this repo — explanations, design documents, reviews, issues, commit
messages — is written for an experienced software engineer who has never seen
this project before.

- Every recommendation says what changes, why it changes, and what problem
  that solves. Never stop after naming an idea.
- Describe the actual work rather than compressing it into an engineering
  noun. "Parity", "hardening", "normalization" and their kind are labels, not
  explanations; write the sentence they stand in for.
- Prefer verbs over nouns, one idea per sentence, and plain English over
  jargon. Define a technical term the first time it appears.
- Clarity beats brevity. If expanding a sentence makes the intent clearer,
  expand it.
- Write as the tool — "the parser reads…", not "we read…".

## Git Workflow

Work on a branch or a worktree; never commit to `main`. Give the branch a
descriptive name (`feature/add-auth`, `fix/parsing-bug`).

Read PR state from `gh` before acting on it — `gh pr list`, `gh pr view
<number>`, `gh pr checks <number>` — rather than from memory of an earlier
turn, because CI finishes and reviewers land between turns. Open work with
`gh pr create`. Do not merge a PR unless asked.

Run `./scripts/check-pr.sh` before pushing; it is the same set of gates CI
runs, and it stops on the first failure.

## GitHub

Read and write GitHub through whatever browsing tool is connected, or `gh`.
The repository is `bitrealm-io/message-vault`; `gh` infers it from the remote.
Two limits hold whichever tool is in use: do not merge a pull request unless
asked, and do not create or push a `v*` tag unless asked, because pushing one
ships a release.

## Verifying UI changes

Changes to `web/` screens, components, or user-visible copy are verified in a
real browser before they are called done, using whatever browser-automation
tool is connected.

- The vault must already be running (`./scripts/run-vault-dev.sh` on `:8080`).
  Point the browser at Vite on **http://127.0.0.1:5173** — not `localhost`,
  which can resolve to IPv6, which the vault does not listen on.
- Screens gated by `isTauri()` — Import, System settings, path openers — are
  not reachable this way. Against Vite alone they render the "available in the
  desktop app" stub. Cover those in the Tauri window by hand, or in unit tests
  of the path helpers, and say which was done.
- Do not start a second Vite server while `cargo tauri dev` is running; they
  share the port.

## Message Vault Repository

This repository is **message-vault**. Cargo package names may still say `message-vault-io`; that is a package namespace, not the repo name. Public docs and GitHub live under `bitrealm-io`.

The product has two pieces:

- **The vault** — `message-vault-server`. Stores messages in SQLite (`data/vault.db`), serves `/v1/*`, and can host the website from `static/`. Run it with `./scripts/run-vault-dev.sh` (http://127.0.0.1:8080) or Docker. Login is a local vault account, not a cloud account.
- **The desktop app** — Tauri v2 around the Vite SPA in `web/`. Reads phone backups, writes JSONL, and imports into a running vault. Browse and search also work in the browser against the vault; importing a backup needs the desktop app.

### Technology stack

| Piece                  | Stack                                                                                                                             |
|------------------------|-----------------------------------------------------------------------------------------------------------------------------------|
| Language (Rust crates) | Rust 1.85+ (edition 2024). CI uses latest stable.                                                                                 |
| Vault server           | Tokio + Axum 0.8 HTTP API. sqlx Any: SQLite (bundled) by default, Postgres via `[database] url`. TOML config. Argon2 passwords, JWT sessions. |
| Database               | SQLite file at `data/vault.db`. Table SQL lives in `schema/sql/`. Schema changes bump `SCHEMA_VERSION` in `db/schema.rs`; old vaults are rebuilt empty and need a fresh import. |
| Desktop app            | Tauri 2 native window. Vite 6 + React 19 + TypeScript SPA in `web/`. React Router 7, React Aria, Tailwind CSS 4. Vitest + Biome. |
| Website                | Same `web/` SPA. Dev server on port 5173. Production copy in `static/`, served by the vault on port 8080.                         |
| Node                   | Node.js 22+ for `web/`, `docs/`, and Docker frontend builds.                                                                      |
| Docs site              | Astro 7 + Starlight, published to GitHub Pages at bitrealm.io.                                                                    |
| Packaging              | Docker (Node 22 + Rust image). GitHub Actions on `v*` tags builds the image and Tauri installers.                                 |
| Helpers on PATH        | `ffmpeg` / `ffprobe` for media. `wtsexporter` (Python) for WhatsApp. `gh` for GitHub.                                             |
| Not the product path   | Restored Next.js 16 browse app (`web-next/`, better-sqlite3).                                                                     |

### Directory map (`tree -L 2 message-vault`)

```text
message-vault
├── config/                 # vault server config templates (copy example → config.toml)
├── crates/                 # Rust workspace (src-tauri is excluded)
│   ├── core/               # shared form model, jobs, export.ini
│   ├── exporters/          # backup parsers (iMessage, WhatsApp, SMS, experimental)
│   ├── libs/               # shared libraries (ir, ir-format, reexport, contacts, media,
│   │                       #   vault-push, vault-pull, …)
│   └── vault/              # message-vault-server (HTTP API + SQLite) and demo-seed
├── docker/                 # Dockerfile and Compose for a release-shaped vault image
├── docs/                   # Astro Starlight site (bitrealm.io)
│   ├── img/                # images used in README / docs
│   ├── public/             # CNAME and other files copied as-is
│   └── src/                # landing page + User Guide + Developer guidebook
│       └── assets/architecture/  # C4 PlantUML sources and exported SVGs
├── schema/                 # SQLite schema for the vault
│   └── sql/                # CREATE TABLE sources embedded by the server
├── scripts/                # host helpers (run-vault-dev, build-static, schema sync)
│   ├── deprecated/         # retired helper scripts
│   └── test/               # scripted test helpers
├── src-tauri/              # Tauri v2 native shell (not a workspace member)
│   ├── capabilities/       # Tauri permission manifests
│   ├── icons/              # desktop app icons
│   └── src/                # Tauri commands wrapping exporters / push / pull
├── tests/                  # workspace-level tests
│   └── fixtures/           # committed schema/API fixtures (no personal backups)
├── web/                    # Vite + React SPA: website and desktop UI
│   └── src/                # screens, components, vault API client, Tauri wrappers
└── web-next/               # restored historical Next.js browse UI (not the product GUI)
    └── src/                # App Router pages that read vault.db via better-sqlite3
```

The product path is `web/` + `src-tauri/`, with the vault API in
`crates/vault/server/`. `web-next/` is a restored historical browse UI and is
not the product. Two schema numbers are in play and neither is 3: the JSONL
chat format is `schema_version: 4` (`crates/libs/ir/src/lib.rs`) and the vault
database is `SCHEMA_VERSION = 7` (`crates/vault/server/src/db/schema.rs`).

### First time setup

Do this once on a new machine. Then follow **Run the vault (development)**.

**1. OS toolchain**

| OS             | What to install                                                                                                                          |
|----------------|------------------------------------------------------------------------------------------------------------------------------------------|
| Linux (Ubuntu) | C compiler, OpenSSL, test libs, WebKit/GTK for Tauri, ffmpeg (commands below)                                                            |
| macOS          | Xcode Command Line Tools: `xcode-select --install`                                                                                       |
| Windows        | [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with the "Desktop development with C++" workload |
| WSL2           | Keep the clone under `~/…`, not `/mnt/c`. Install Rust and Node inside WSL. Prefer WSLg (Windows 11).                                    |

Ubuntu packages:

```bash
sudo apt update
sudo apt install -y curl git build-essential pkg-config libssl-dev
sudo apt install -y libfontconfig1-dev libxkbcommon-dev   # cargo test --workspace
sudo apt install -y \
  libwebkit2gtk-4.1-dev libgtk-3-dev \
  libappindicator3-dev librsvg2-dev patchelf \
  libjavascriptcoregtk-4.1-dev libsoup-3.0-dev
sudo apt install -y ffmpeg
```

**2. Rust 1.85+** (edition 2024). Do not use the distro `apt` package.

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cargo install tauri-cli --version "^2"
```

**3. Node.js 22+**. Distro Node is usually too old. nvm example:

```bash
curl -fsSL https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.3/install.sh | bash
# new shell, then:
nvm install 22
nvm use 22
```

**4. Optional helpers**

```bash
sudo apt install -y pipx && pipx ensurepath
pipx install 'whatsapp-chat-exporter[android_backup,crypt15]'   # wtsexporter
pipx install sqlite-web                                          # --sqlweb on port 8081
```

**5. Clone and install the frontend**

```bash
git clone https://github.com/bitrealm-io/message-vault.git
cd message-vault
cd web && npm ci && cd ..
```

First `cargo build --workspace` and first `cargo tauri dev` each take several minutes. `config/config.toml` is created from `config/config.toml.example` on the first `./scripts/run-vault-dev.sh` if it is missing.

### Run the vault (development)

Work from the repository root. The vault process must be running before the website or desktop app can sign in. First compile of the server and of Tauri each take several minutes.

**Terminal 1 — vault API** (leave this running)

```bash
./scripts/run-vault-dev.sh                 # keep data/ if present; empty vault if none
./scripts/run-vault-dev.sh --reset-demo    # wipe data/, seed the sample inbox (needs ffmpeg)
./scripts/run-vault-dev.sh --reset         # wipe data/, start empty
./scripts/run-vault-dev.sh --sqlweb        # also SQLite browser at http://127.0.0.1:8081
```

`--reset` and `--reset-demo` cannot be combined. `--reset-demo` also rewrites `config/config.toml` from the example (CORS for Vite `:5173` enabled). Later sessions omit `--reset-demo` so the existing database stays.

API: **http://127.0.0.1:8080**. After `--reset-demo`, sign in as username `demo` with an empty password. Otherwise create an account in the UI.

Restart terminal 1 after edits under `crates/vault/server/` (debug `cargo run`; no hot reload).

**Run on Postgres (optional)** — `./scripts/run-vault-pg-dev.sh` starts
compose Postgres, runs this checkout's vault with `--db-url
postgres://vault:vault@127.0.0.1:5432/vault`, and stops the container
on exit. `--reset` / `--reset-demo` wipe the `vault_pg_data` volume and
host `data/`. After `--reset-demo`, sign in as `demo` with an empty
password. Pass `--release` to seed and serve with the optimized binary
(first compile can take several minutes). Do not run this and
`./scripts/run-vault-dev.sh` at once (both serve on 127.0.0.1:8080).

**Terminal 2 — UI** (pick one)

```bash
cd web && npm ci && cd ..    # first time, or after web/package-lock.json changes
cargo tauri dev              # desktop window; starts Vite itself
```

Or, browser only (no Tauri):

```bash
cd web && npm run dev        # http://127.0.0.1:5173, proxies /v1 to :8080
```

Do not run `npm run dev` and `cargo tauri dev` at the same time. Point the app at **http://127.0.0.1:8080** (not `localhost` — that can resolve to IPv6, which the vault does not listen on). `web/` and `src-tauri/` usually reload; restart `cargo tauri dev` if they do not.

Optional: `./scripts/build-static.sh` copies `web/dist` to `static/` so the vault serves the UI at http://127.0.0.1:8080 without Vite. Do not run `docker compose -f docker/compose.release.yml` and the host script at once; they both use port 8080.

### Build, format, and test

#### Backend

Run these from the repository root unless a `cd` is shown.

Rust formatter is `rustfmt`. `src-tauri/` is not a workspace member, so format it with `--manifest-path`.

```bash
# Check format (what CI runs)
cargo fmt --all -- --check
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check

# Rewrite Rust (workspace + src-tauri) and web/ (Biome)
./scripts/format-all.sh

# Clippy (workspace + src-tauri) and web lint (Biome). Warnings do not fail.
./scripts/lint-all.sh

cargo build --workspace
cargo test --workspace
cargo test -p sms-backup-restore-exporter   # one crate
cargo build --manifest-path src-tauri/Cargo.toml

# Postgres engine tests (skip unless a dev Postgres is reachable)
docker compose -f docker-compose.pg.yml up -d
MV_TEST_POSTGRES_URL=postgres://vault:vault@127.0.0.1:5432/vault cargo test -p message-vault-server
```

#### Frontend

Frontend (`web/`) — Biome (`web/biome.json`) lints and formats TypeScript, JavaScript, CSS, JSON, and HTML. TypeScript (`npm run build` runs `tsc` then Vite). CI runs `biome ci .` (lint and format drift fail). Prefer a real fix over `biome-ignore`. Prefix unused bindings with `_`.

```bash
cd web
npm ci                    # first time, or after package-lock.json changes
npm run lint              # biome lint .
npm run format            # rewrite format + import order
npm run format:check      # format + import order, no write
npm test                  # vitest run (src/**/*.{test,spec}.{ts,tsx})
npm run test:watch
npm run build             # tsc && vite build
npm run dev               # Vite on http://127.0.0.1:5173 (proxies /v1 to :8080)
```

From the repository root, `./scripts/format-all.sh` runs rustfmt then the web formatter. `./scripts/lint-all.sh` runs Clippy (workspace plus `src-tauri`) then the web linter. `./scripts/check-pr.sh` calls `format-all.sh`, then build/test/lint.

Do not start a separate `npm run dev` while `cargo tauri dev` is running. Tauri starts Vite itself.

#### Docs

Docs (`docs/`) not the product UI, but CI-adjacent when that tree changes:

```bash
cd docs && npm ci && npm run check && npm run build
```

#### Not gated by CI

Clippy is not a CI job — run it locally with `./scripts/lint-all.sh`
(`rust-analyzer.check.command` is `clippy` in `.vscode/settings.json`).
`web-next/` is not gated either; run `npm run lint` / `npm test` there if that
tree is edited.

### Releases and versions

The product follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html) (`MAJOR.MINOR.PATCH`). Record user-visible changes in `CHANGELOG.md` ([Keep a Changelog](https://keepachangelog.com/en/1.1.0/)) under `[Unreleased]` until a tag ships. Every changelog bullet must start with an ISO date (`YYYY-MM-DD`); released version headings use `## [0.8.0] - 2026-08-24`.

Three version numbers are easy to mix up:

| What            | Example             | Meaning                                                                                |
|-----------------|---------------------|----------------------------------------------------------------------------------------|
| Product version | `<X.Y.Z>`           | Desktop app + vault image. Git tag is `v<X.Y.Z>`. Read the current value from `src-tauri/Cargo.toml`. |
| Docker Hub tag  | `<X.Y.Z>` (no `v`)  | `bitrealm/message-vault:<X.Y.Z>`. Also `<X.Y>`, `latest`, and `sha-…`.                 |
| JSONL schema    | `schema_version: 4` | Shared chat file format. Independent of the product version. Version 3 is refused, never upgraded. |

**Product version files** (keep these in lockstep; `src-tauri/Cargo.toml` is the value to read):

- `src-tauri/Cargo.toml` — bump this before tagging (this is the one CI docs call out)
- `src-tauri/tauri.conf.json` — installer version
- `web/package.json` — Vite SPA
- `crates/vault/server/Cargo.toml` — vault server crate

Leave most other `Cargo.toml` files at `0.1.0`. Do not bump `web-next/` (`0.3.0`) for a product release.

**Ship a release**

1. Merge the work to `main`.
2. Move `[Unreleased]` notes in `CHANGELOG.md` under the new version heading.
3. Set the four product version files to the new number (for example `0.8.0`).
4. Push a git tag `v0.8.0` on that commit. Pushing the tag is what ships. Push/PR to `main` does not.

`.github/workflows/ci.yml` then: runs fmt/test, pushes `bitrealm/message-vault`, builds Tauri installers (Linux `.deb` + AppImage, Windows `.msi`, macOS `.dmg`), and creates a GitHub Release named `Message Vault v0.8.0`.

**Build a release-shaped binary locally (does not publish)**

```bash
cargo tauri build                          # desktop installers under src-tauri/target/release/bundle/
docker compose -f docker/compose.release.yml up --build   # vault image from this checkout
cargo build --workspace --release          # workspace crates only; not the Tauri installer
```

Do not create or push tags unless asked.
