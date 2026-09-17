---
title: Contributing
description: Set up a development environment, run tests, and open pull requests for Message Vault.
---

Thanks for helping out. This page covers the development environment, running the code, and how pull requests work. It assumes basic Git. For how the code fits together, start with [Vault Design](/vault/developer/vault-design/); the [User Guide](/vault/user/) explains the product itself.

## Report bugs or request features

- Found a bug? Open a [bug report](https://github.com/bitrealm-io/message-vault/issues/new?template=bug_report.md) with steps to reproduce it.
- Have a feature idea? Open a [feature request](https://github.com/bitrealm-io/message-vault/issues/new?template=feature_request.md).

Use the issue forms. They ask for the details that let a maintainer reproduce the problem.

## Set up your environment

These steps are for Ubuntu and other Debian-based distributions. On macOS or Windows, install the same tools with their usual installers.

### Install system packages

```bash title="Install Ubuntu packages"
sudo apt update

# Rust native crates need a C compiler. libssl-dev is for OpenSSL
sudo apt install -y curl git build-essential pkg-config libssl-dev

# Needed for `cargo test --workspace`
sudo apt install -y libfontconfig1-dev libxkbcommon-dev

# Desktop app (cargo tauri dev / cargo tauri build) — WebKit/GTK
sudo apt install -y \
  libwebkit2gtk-4.1-dev libgtk-3-dev \
  libappindicator3-dev librsvg2-dev patchelf \
  libjavascriptcoregtk-4.1-dev libsoup-3.0-dev

# Media conversion / compression
sudo apt install -y ffmpeg
```

### Install Rust

Ubuntu's `rust` package is usually too old. Install Rust with rustup instead; the minimum supported version is 1.85.

```bash title="Install Rust with rustup"
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

```bash title="Install tauri-cli"
cargo install tauri-cli --version "^2"
```

### Install Node

Ubuntu's `nodejs` package is usually too old. Node 22 or newer is required; nvm is the easy way to get it.

```bash title="Install Node.js 22"
curl -fsSL https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.3/install.sh | bash
source ~/.bashrc   # or open a new terminal
nvm install 22
nvm use 22
node -v            # should print v22.x
npm -v
```

### Install helper tools

`wtsexporter` extracts WhatsApp messages. `sqlite-web` is a small UI for looking at the database.

```bash title="Install pipx helpers"
sudo apt install -y pipx
pipx ensurepath

# reopen the shell, or: source ~/.bashrc
pipx install 'whatsapp-chat-exporter[android_backup,crypt15]'
pipx install sqlite-web
```

### Fork and clone

Fork the [Message Vault repo](https://github.com/bitrealm-io/message-vault) on GitHub and clone your fork. If you have never forked a repo before, GitHub has [a guide](https://docs.github.com/en/pull-requests/how-tos/work-with-forks/fork-a-repo).

When forking, you only need the default `main` branch.

## Build and run

You run two processes at the same time: the vault, and a UI that talks to it. The vault is the HTTP API and the SQLite database; it has to be running before anyone can sign in.

Work from the repository root in two terminals. The first server compile takes several minutes.

### Start the vault (terminal 1)

`--reset-demo` deletes `data/` and loads a sample inbox. Use it on the first run, or whenever you want a fresh sample inbox.

```bash title="Start the vault"
./scripts/run-vault-dev.sh --reset-demo
```

Leave this terminal running. The API listens at **http://127.0.0.1:8080**.

To browse the tables while developing, add `--sqlweb` (needs `sqlite-web` from the previous step). That UI is **http://127.0.0.1:8081**.

### Vault flags

The first run uses `--reset-demo`. Later sessions, start with no flags so `data/` stays:

```bash title="Start the vault, keep data"
./scripts/run-vault-dev.sh
```

`--reset` wipes `data/` and starts empty (no sample inbox). Don't combine `--reset` and `--reset-demo`. `--sqlweb` works with any of these.

`--reset` alone leaves the vault **unclaimed**, so the first screen is Create Vault Owner — which is the only way to reach that screen in dev. Add `--owner` to claim it as `admin`/`admin` instead and land on the login. `--reset-demo` claims the vault itself, so it rejects `--owner`.

### Start the vault on Postgres (optional)

Same flags as the SQLite script, against the compose Postgres on
`127.0.0.1:5432`. Needs Docker. There is no `--sqlweb`.

```bash title="Start the vault on Postgres"
./scripts/run-vault-pg-dev.sh --reset-demo
```

Sign in as username `demo` with an empty password, or as `admin` with the
password `admin` to manage accounts. `--reset` wipes the
Postgres volume and `data/` and starts empty. A run with no flags keeps
the volume. Stopping the script (Ctrl+C) stops the Postgres container
and keeps the volume. Do not run this at the same time as
`./scripts/run-vault-dev.sh` — both use port 8080.

### Open the website (terminal 2)

Install the frontend packages once, then start the Vite UI. Vite is the local web server for `web/`.

```bash title="Start the website"
cd web && npm ci && npm run dev
```

Open **http://localhost:5173**. Sign in as username `demo` with an empty password. That account holds invented messages and can do everything a real account can, so import and other writes are testable on it; `./scripts/run-vault-dev.sh --reset-demo` puts it back. Signing in as `admin` with the password `admin` reaches the same vault as its owner, which manages accounts and reads no messages.

Later sessions, skip `npm ci` unless `web/package-lock.json` changed.

### Desktop app

The desktop app is the same `web/` UI in a native window (Tauri). Use it instead of the Vite website when changing `src-tauri/` or testing import from a backup. Don't run `npm run dev` at the same time — Tauri starts Vite itself.

```bash title="Start the desktop app"
cd web && npm ci && cd ..
cargo tauri dev
```

When the window opens, point it at **http://127.0.0.1:8080**. The first compile of the desktop app also takes several minutes.

For a release-shaped desktop binary (faster on real backups, or when packaging installers):

```bash title="Build a release-shaped desktop app"
./scripts/build-desktop.sh
```

The script runs `cargo tauri build` and then renames the installers under `src-tauri/target/release/bundle/` from `Message Vault_…` to `message_vault_<version>_<arch>`, the same names a release carries. It is not for day-to-day UI work — it doesn't reload. Use `cargo tauri dev` for that.

### Serve the website from the vault (optional)

Vite is the usual UI. To have the vault itself serve the website at **http://127.0.0.1:8080**:

```bash title="Build the website into static/"
./scripts/build-static.sh
```

That copies `web/dist` into `static/`. Don't run the host vault and the [Docker](/vault/developer/docker/) Compose stack at the same time; both use port 8080.

### Stopping and restarting

Ctrl+C in terminal 1 stops the vault (and the SQLite UI if it was started). Ctrl+C in terminal 2 stops the website or the desktop app.

After edits under `crates/vault/server/`, restart terminal 1. After edits under `web/` or `src-tauri/`, the UI usually reloads on its own. Restart `cargo tauri dev` if it doesn't.

## Make code changes

Rust doc comments and utoipa annotations follow the [Rust doc style](/vault/developer/rustdoc-style/) guide. If you change the vault server's command line, regenerate its reference page:

```bash title="regenerate the server CLI page"
cargo run -p message-vault-server -- dump-cli-docs --output docs/src/content/docs/vault/developer/reference/server-cli.md
```

Open a GitHub issue before starting the work, so the later pull request can link to it. Use the bug report or feature request form. You don't need to wait for a reply before coding. If there's no reply after 5 business days, comment on the issue.

### Branch

Start from the latest `main`. Don't commit on `main`. Name the branch with a prefix:

- `feat/short-name` — new behavior
- `fix/short-name` — a bug
- `docs/short-name` — documentation only

Keep the branch current with `main` while working (merge or rebase). One pull request should do one job.

### Commits

Each commit should be one idea. Don't mix a bug fix with a rename, or a feature with formatting of unrelated files.

Prefer `feat:`, `fix:`, or `docs:` at the start of the subject when it fits; other prefixes are fine too. The subject should say what changed. Add a short body when the reason isn't obvious, and mention the issue (`Ref: #123`).

Never commit passwords, vault keys, certificates, credential `.env` files, or real message backups. Tests use committed fixtures under `crates/*/tests/fixtures/`.

### Example

After the fork is cloned, from the repository root:

```bash title="Create a feature branch"
git remote add upstream https://github.com/bitrealm-io/message-vault.git
git fetch upstream
git checkout -b feat/short-name upstream/main
git commit -m "feat: add support for x

Ref: #123"
git push -u origin feat/short-name
```

Add `upstream` once. For later branches: `git fetch upstream`, then `git checkout -b … upstream/main`.

Most first PRs touch one of these:

- **Vault API or database** — `crates/vault/server/` and `schema/sql/`
- **Website or desktop screens** — `web/`
- **Import from a phone backup** — `crates/exporters/` and, for the native file dialogs, `src-tauri/`
- **This guidebook** — `docs/src/content/docs/`

Don't start in `web-next/`; that's an old UI still in the tree.

The full folder list is on [Vault Design → Directory map](/vault/developer/vault-design/#directory-map).

Once the vault is running, [Vault Design](/vault/developer/vault-design/) also lists the programs a build creates and shows how the website and the vault talk to each other.

Phone backups do not go into the vault as raw files. A converter reads the backup and writes a folder of chat files (one file per conversation, one message per line). Import loads that folder into the vault. [Message Transfer](/vault/developer/message-transfer/) explains that path and which converters are ready to use.

### Preview the guidebook

Guidebook pages live under `docs/src/content/docs/vault/` and show up at paths like `/vault/user/` and `/vault/developer/`. The home page at `/` comes from `docs/src/pages/index.astro`. The published site is **https://bitrealm.io/**.

To preview locally:

```bash title="Preview the guidebook"
cd docs
npm ci
npm run dev
```

Open **http://localhost:4321/** for the home page, or **http://localhost:4321/vault/developer/** for the Developer docs. `./scripts/check-all.sh` checks and builds `docs/` along with everything else, and CI builds the site on every pull request that touches it.

## Open a pull request

Run the checks, then open the pull request against `main`. The first compile and the first `npm ci` each take several minutes.

### Run the checks

From the repository root:

```bash title="Run pull-request checks"
./scripts/check-pr.sh
```

That script is quick: it checks formatting on the workspace and `src-tauri/`, runs Clippy on both at `-D warnings`, and lints and type-checks `web/`. It stops on the first failure and never rewrites files — if formatting fails, run `./scripts/format-all.sh` and commit the result.

CI runs the complete gate on every pull request: build, tests (including the Postgres-backed server suites), the web bundle and its tests, and the docs build. To run all of that locally in one command, use `./scripts/check-all.sh`; expect it to take a while, since it does serially what CI does in parallel.

While iterating on one crate, `cargo test -p go-sms-pro-exporter` is enough. Exporter smoke tests use committed fixtures; personal phone backups are not required.

### Before you submit

- [ ] Started from the latest `main` on a prefixed branch (`feat/`, `fix/`, `docs/`)
- [ ] The pull request does one job
- [ ] `./scripts/check-pr.sh` passes, and formatter rewrites are committed
- [ ] An issue exists and the pull request links it (`Ref: #123`)
- [ ] The style matches the surrounding code

### Keep the branch current

If `main` has moved, update the branch before asking for review:

```bash title="Update the branch from main"
git fetch upstream
git merge upstream/main
git push
```

Rebase is allowed; merge is enough. Don't force-push unless the branch is only used by that one contributor.

### Open the pull request

A pull request asks to merge the branch into `main`. Open it against `main` on [bitrealm-io/message-vault](https://github.com/bitrealm-io/message-vault). Use the GitHub pull request form; the default template is enough for most changes. Two fuller templates also exist, and they aren't required: `feature.md` asks what the change does and how to test it, and `bugfix.md` asks for the root cause and the regression risk alongside the fix. Pick one by adding `?template=feature.md` or `?template=bugfix.md` to the compare URL. Open it as a draft if you want early feedback.

Link the issue (`Ref: #123`). Write `Fixes #123` in the description if this change should close that issue.

Prefer `feat:`, `fix:`, or `docs:` at the start of the title when it fits.

From the repository root, this also works:

```bash title="Open a pull request"
gh pr create --base main --title "feat: add support for x" --body "Ref: #123"
```

### After you submit

- [ ] Watch the status checks and fix failures
- [ ] Reply to review comments on the pull request
- [ ] Keep the branch current with `main` until it is merged

## License

Distributed under the Fair Core License. Contributions are made under that same license. See [LICENSE.md](https://github.com/bitrealm-io/message-vault/blob/main/LICENSE.md) for the full text.

## Release

Maintainers ship versions as described on [Release](/vault/developer/release/).
