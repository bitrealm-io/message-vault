# AGENTS.md

The operations guide for any agent working in this repository: git and pull request workflow, first-time setup, running the server, the checks, and the release process. Architecture and the rules that are easy to get wrong are in `CLAUDE.md`.

## Writing

`docs/agents/writing-style.md` governs documents, product copy, commit
messages, and pull request descriptions, and holds the fixed product
vocabulary. Read it before writing any of those.

## Git Workflow

- Every change goes on a branch or worktree. `main` takes pull requests only:
  its ruleset requires one, plus the ten `ci.yml` checks.
- Before pushing, run `git fetch`, then read the pull request's real state
  with `gh pr list`, `gh pr view <number>`, and `gh pr checks <number>`.
- Merge a pull request as "Merging" below says.

### Submitting Work

Open the pull request as a draft, with `gh pr create --draft`. CI runs
nothing on a draft, and marking it ready starts the run; `pr-review` marks it
ready after the review's last push. A draft's checks are skipped, and a
skipped check reads as passed, so they say nothing. Why:
`docs/adr/0007-ci-is-the-only-gate.md`, "Consequences".
**Write the description to one of the templates in `.github/PULL_REQUEST_TEMPLATE/`.**
They exist for whoever opens the pull request to fill in — an agent included —
not as options offered to a reviewer:

- `feature.md` for new behaviour: what it does and for whom, the key files
  changed, HTTP API and schema changes, how to test it.
- `bugfix.md` for a fix: expected against actual, the root cause stated
  separately from the fix, steps to reproduce before and verify after,
  impact, and regression risk.
- `.github/pull_request_template.md` is the generic default applied
  automatically. Use it for changes that are neither, such as documentation.

Fill the sections in rather than deleting them. Root Cause and Regression
Risk on a fix are the two that make it reviewable, so answer them plainly
instead of dropping them.

#### Review on the pull request

The ruleset on `main` blocks a merge while any review conversation is open,
and requires no approval: the reviewing agent and the author share one GitHub
account, and GitHub does not let an author approve their own pull request.
Why: `docs/adr/0007-ci-is-the-only-gate.md`.

Review a pull request with the `pr-review` skill (`.claude/skills/pr-review/`).
It runs the steps below, fixes what it finds, and queues the pull request.

##### The marker

Every comment `pr-review` posts starts with the line `<!-- pr-review -->`. The user and the agents post from one GitHub account,
so the marker is how their threads are told apart. A thread whose first
comment carries it is an agent thread. Any other thread is a user thread, and
only the user resolves it.

GitHub limits how fast one account creates content (reviews, comments,
replies, pull requests), apart from its hourly limit, and every session posts
from the same account. So make those calls one at a time, at least a second
apart. When GitHub refuses one ("submitted too quickly", or a 403 or 422 that
names a secondary rate limit), check that it did not land, wait a minute (or
the `retry-after` it gives), and send the same call again.

1. **Read the pull request**: its head, base, draft state, the issues it
   closes, and the diff.

   ```bash
   gh pr view <N> --json headRefName,headRefOid,baseRefName,isDraft,body
   gh api graphql -f query='{ repository(owner: "messagecrate", name: "message-crate") {
     pullRequest(number: <N>) { closingIssuesReferences(first: 20) { nodes { number } } } } }'
   gh pr diff <N>
   gh pr ready <N> --undo   # a pull request that is not a draft becomes one
   ```

2. **Post each finding on its line**, all in one review, pinned to the head
   commit that was reviewed so a later push cannot move the lines:

   ```bash
   gh api repos/messagecrate/message-crate/pulls/<N>/reviews \
     -f commit_id=<headRefOid> -f event=COMMENT \
     -f body=$'<!-- pr-review -->\n<summary>' \
     -f 'comments[][path]=<file>' -F 'comments[][line]=<line>' \
     -f comments[][body]=$'<!-- pr-review -->\n<the finding and why it matters>'
   ```

   Repeat the three `comments[]` fields for each finding. The marker goes in
   each `comments[][body]`, because that comment opens the thread. A finding
   with no line in the diff goes in a top-level comment instead
   (`gh pr comment <N>`), with the marker on its first line. It has no thread,
   so it is answered by a new marked `gh pr comment <N>` that quotes it.
3. **Work on a detached worktree** made at the pull request's head, before
   the review. The branch may be checked out in another worktree, and a
   detached one works either way. Before every push, run the **local
   checks**: `./scripts/check-pr.sh`, then the tests for each area the
   unpushed commits change, as "Build, format, and test" gives them
   (`cargo test -p <crate>` for a workspace crate, the `src-tauri` tests,
   Vitest for `web/`). CI runs everything else. Push without force, because
   the branch may carry another session's commits:

   ```bash
   git fetch origin <headRefName>
   git worktree add --detach .worktrees/review-<N> <headRefOid>
   git diff --name-only <last pushed SHA>..HEAD   # the areas to test
   git push origin HEAD:<headRefName>
   ```

   If the push is rejected because the branch moved, step 5 says how to
   bring it in.
4. **Answer every thread**, with the commit that fixes it or the reason it
   stays as it is. Then resolve it if it is an agent thread:

   ```bash
   gh api repos/messagecrate/message-crate/pulls/<N>/comments/<comment-id>/replies \
       -f body=$'<!-- pr-review -->\nFixed in <sha>: <what changed>.'
   gh api graphql -f query='query { repository(owner: "messagecrate", name: "message-crate") {
     pullRequest(number: <N>) { reviewThreads(first: 100) { nodes { id isResolved
       comments(first: 1) { nodes { databaseId path body } } } } } } }'
   gh api graphql -f query='mutation { resolveReviewThread(input: {threadId: "<thread-id>"}) { thread { isResolved } } }'
   ```

   Never resolve a thread without a reply in it.
5. **Merge the base into the pull request** before the review, whenever the
   base has commits the pull request lacks, so the review and the pull
   request's checks see the code as it would land. Before queueing, merge it
   again only on `CONFLICTING`: the merge queue drops a pull request it
   cannot merge onto the base, and tests one that is only behind on the
   latest base itself. Merge rather than rebase: a rebase needs a force-push,
   and the queue squashes the merge commit away. GitHub reports `UNKNOWN`
   for a few seconds after a push, so wait for a settled answer:

   ```bash
   git fetch origin <baseRefName>
   git merge-base --is-ancestor origin/<baseRefName> HEAD || echo behind
   until m=$(gh pr view <N> --json mergeable -q .mergeable) && [ "$m" != UNKNOWN ]; do sleep 10; done
   echo "$m"                      # before queueing, CONFLICTING means merge the base
   git merge origin/<baseRefName> # stops at each conflict, with nothing committed
   # resolve every conflict, git add the files, git commit, run the local checks
   git show --remerge-diff HEAD   # the conflict resolution alone, for review
   git push origin HEAD:<headRefName>
   ```

   If a push is rejected because the branch moved, fetch it and merge
   `origin/<headRefName>` in (`git merge origin/<headRefName>`). A rebase
   would drop the merge commit and replay the base's commits one by one,
   which brings the conflict back. If that merge conflicts too, resolve it,
   commit, run the local checks, and review its remerge diff like the
   first one. Then push again.

6. **Push, mark the pull request ready, then watch its run.** Push to the
   draft first, and wait for that push's own run of `ci.yml`, whose jobs all
   skip. Then mark the pull request ready, which starts the real run, and
   watch that run by its id: `gh pr checks` can still show the draft's
   skipped checks on the same commit, which read as passed. With nothing left
   to push, mark it ready and watch the run that starts. A later push to a
   pull request that is already ready, such as a fix for a failed job, starts
   its run itself and is watched the same way, from `last=0`.

   If the push is rejected, stop: the pull request stays a draft, and step 5
   says how to bring the moved branch in.

   Watch only the head you mean to queue: a new push to the pull request
   cancels the run on the head before it (`ci.yml`'s concurrency group), so
   push a fix as soon as a job fails because of the pull request, rather
   than waiting for the rest. When the first failure is outside the pull
   request, let the run finish, because GitHub reruns the failed jobs of a
   finished run only. Then sort every failed job: any that failed because of
   the pull request is fixed and pushed, which replaces the rerun; only when
   every failure is outside does the run get its rerun. The run is green only
   when its conclusion is `success`; a `cancelled` run means something pushed
   over it, so check the head. Green counts only while the pull request's
   head is still the commit you pushed: a push from another session moves it,
   and its commits have not been reviewed.

   ```bash
   before=$(gh pr view <N> --json headRefOid -q .headRefOid)
   git push origin HEAD:<headRefName> || exit 1   # rejected: stop, see step 5
   sha=$(git rev-parse HEAD)
   until h=$(gh pr view <N> --json headRefOid -q .headRefOid) && [ "$h" != "$before" ] || [ "$sha" = "$before" ]
   do sleep 10; done
   [ "$h" = "$sha" ] || echo moved      # another session pushed on top
   until last=$(gh run list --commit "$sha" --workflow ci.yml --json databaseId -q 'map(.databaseId) | max // empty') &&
         [ -n "$last" ]
   do sleep 10; done                    # the draft's own run, all skipped
   gh pr ready <N>
   until run=$(gh run list --commit "$sha" --workflow ci.yml --json databaseId \
                 -q "map(select(.databaseId > $last)) | .[0].databaseId // empty") && [ -n "$run" ]
   do sleep 10; done
   until s=$(gh run view "$run" --json status,jobs \
               -q 'if any(.jobs[]; .conclusion == "failure") then "failed" else .status end') &&
         { [ "$s" = failed ] || [ "$s" = completed ]; }
   do sleep 30; done                    # stops at the first failed job
   gh run view "$run" --json conclusion,jobs -q '.conclusion, (.jobs[] | select(.conclusion == "failure") | .name)'
   gh run watch "$run"                  # an outside failure: wait for the run to finish
   gh run rerun "$run" --failed
   [ "$(gh pr view <N> --json headRefOid -q .headRefOid)" = "$sha" ] || echo moved
   ```

#### Merging

`main` requires the merge queue. `gh pr merge <N> --match-head-commit <sha>`
adds a green pull request to the queue only while its head is still `<sha>`,
the commit that was reviewed and checked, or turns on auto-merge when its
checks are still running. It takes no `--squash`, because the queue's merge
method is fixed. The queue runs `ci.yml` again on the pull request merged onto
the latest `main`, and lands it only when that run is green. Never pass
`--admin`: it merges past the queue.

A pull request that `pr-review` has reviewed is queued without asking, once
every thread on it is resolved, its required checks are green, and it is not a
draft. Any other merge waits for the user to ask for it.

## Tools

- **GitHub MCP** (`plugin-github-github`) for issues, PR read/search, reviews, and GitHub code search when the server is authenticated; fall back to `gh` when it is not. See [`.cursor/rules/github-mcp.mdc`](.cursor/rules/github-mcp.mdc).
- **Playwright MCP** (`plugin-playwright-playwright`) to verify browser UI after `web/` changes: navigate to the Vite app (`http://127.0.0.1:5173` with the server on `:8080`), take a snapshot, then click/type as needed. See [`.cursor/rules/playwright-mcp.mdc`](.cursor/rules/playwright-mcp.mdc). Desktop-only screens gated by `isTauri()` still need the Tauri window or unit tests — Playwright against Vite alone cannot exercise them.

## Message Crate Repository

This repository is **messagecrate/message-crate**. The Cargo packages carry the same name (`message-crate-server`, `message-crate-core`). Public docs live at messagecrate.app.

The product has two pieces:

- **The server** — `message-crate-server`. Stores messages in SQLite (`data/messagecrate.db`), serves `/v1/*`, and can host the website from `static/`. Run it with `./scripts/run-dev.sh` (http://127.0.0.1:8080) or Docker. Login is a local account, not a cloud account.
- **The desktop app** — Tauri v2 around the Vite SPA in `web/`. Reads phone backups, writes JSONL, and imports into a running server. Browse and search also work in the browser against the server; importing a backup needs the desktop app. The installer carries `message-crate-server` and the built website, and the app starts that server at `127.0.0.1:8080` when nothing answers there, with its data in the operating system's app-data folder; it stops it on close. A Message Crate already answering at that address (Docker, `./scripts/run-dev.sh`) is used as it is. Rules: `src-tauri/src/local_server.rs`; why: `docs/adr/0018-the-desktop-app-starts-the-server-it-ships.md`.

### Technology stack

| Piece                  | Stack                                                                                                                             |
|------------------------|-----------------------------------------------------------------------------------------------------------------------------------|
| Language (Rust crates) | Rust, edition 2024. `rust-toolchain.toml` pins the version (`1.98.1`) for every checkout, CI, and the release image.              |
| Server                 | Tokio + Axum 0.8 HTTP API. sqlx over SQLite (bundled), the only database engine (`docs/adr/0017-sqlite-is-the-only-database-engine.md`). TOML config. Argon2 passwords, opaque hashed session tokens. |
| Database               | SQLite file at `data/messagecrate.db`. Table SQL lives in `schema/sql/`. The server fingerprints those files at compile time (`SCHEMA_FINGERPRINT` in `db/schema.rs`) and rebuilds a database stamped with any other fingerprint empty, so a schema change is only a change to the SQL: nothing to bump. The rebuilt database needs a fresh import. |
| Desktop app            | Tauri 2 native window. Vite 8 + React 19 + TypeScript SPA in `web/`. React Router 7, React Aria, Tailwind CSS 4. Vitest + Biome.  |
| Website                | Same `web/` SPA. Dev server on port 5173. Production copy in `static/`, served by the server on port 8080.                        |
| Node                   | Node.js 22+ for `web/`, `docs/`, and Docker frontend builds.                                                                      |
| Docs site              | Astro 7 + Starlight, published to GitHub Pages at messagecrate.app on each `v*` release tag.                                            |
| Packaging              | Docker (Node 22 + Rust image). GitHub Actions on `v*` tags builds the image and Tauri installers.                                 |
| Helpers on PATH        | `ffmpeg` / `ffprobe` for media. `wtsexporter` (Python) for WhatsApp. `gh` for GitHub. `imessage-reader` and `message-crate-server` are bundled beside the app, not on PATH (`src-tauri/build.rs` builds both). |
| Not the product path   | Restored Next.js 16 browse app (`web-next/`), an HTTP client of the server's `/v1` API for evaluating its screens. Kept on purpose; see CLAUDE.md before proposing its removal. |

### Directory map (`tree -L 2 message-crate`)

```text
message-crate
├── config/                 # server config templates (copy example → config.toml)
├── crates/                 # Rust workspace (src-tauri is excluded)
│   ├── core/               # shared form model, export pipeline, jobs
│   ├── exporters/          # backup parsers (iMessage, WhatsApp, SMS, experimental)
│   ├── helpers/            # imessage-reader (GPL helper process the app spawns) and its protocol
│   ├── libs/               # shared libraries (ir, ir-format, reexport, contacts, media,
│   │                       #   message-crate-push, message-crate-pull, …)
│   └── server/             # message-crate-server (HTTP API + SQLite) and demo-seed
├── docker/                 # Dockerfile and Compose for a release-shaped server image
├── docs/                   # Astro Starlight site (messagecrate.app)
│   ├── img/                # images used in README / docs
│   ├── public/             # CNAME and other files copied as-is
│   └── src/                # landing page + User Guide + Developer guidebook
│       └── assets/architecture/  # C4 PlantUML sources and exported SVGs
├── schema/                 # SQLite schema for the database
│   └── sql/                # CREATE TABLE sources embedded by the server
├── scripts/                # host helpers (run-dev, build-static, schema sync)
│   └── deprecated/         # retired helper scripts
├── src-tauri/              # Tauri v2 native shell (not a workspace member)
│   ├── capabilities/       # Tauri permission manifests
│   ├── icons/              # desktop app icons
│   └── src/                # Tauri commands wrapping exporters / push / pull
├── staging/                # empty; the release Compose file mounts it for JSONL imports
├── tests/
│   └── fixtures/           # committed schema and search fixtures (no personal backups)
├── vendor/                 # sqlx-sqlite with libsqlite3-sys bumped (why: VENDORING.md)
├── web/                    # Vite + React SPA: website and desktop UI
│   └── src/                # screens, components, API client, Tauri wrappers
└── web-next/               # restored historical Next.js browse UI (not the product GUI)
    └── src/                # App Router pages; reads go through src/lib/vault/ to the /v1 API
```

```text
# ❌ BAD — web-next IS NOT the product; it exists to evaluate what is worth porting into web/
# ✅ GOOD — product UI is web/ + src-tauri/; the server API is crates/server/server/
```

### First time setup

Do this once on a new machine. Then follow **Run the server (development)**.

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

**2. Rust through rustup.** Do not use the distro `apt` package: rustup reads `rust-toolchain.toml` and installs the pinned version on the first `cargo` command.

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
cargo install cargo-llvm-cov --locked                             # ./scripts/coverage.sh
cargo install cargo-mutants cargo-nextest --locked                # ./scripts/mutants.sh
```

**5. Clone and install the frontend**

```bash
git clone https://github.com/messagecrate/message-crate.git
cd message-crate
cd web && npm ci && cd ..
```

First `cargo build --workspace` and first `cargo tauri dev` each take several minutes. `config/config.toml` is created from `config/config.toml.example` on the first `./scripts/run-dev.sh` if it is missing.

### Run the server (development)

Work from the repository root. The server process must be running before the website or desktop app can log in. First compile of the server and of Tauri each take several minutes.

**Terminal 1 — server API** (leave this running)

```bash
./scripts/run-dev.sh                 # keep data/ if present; the server adds the Demo Account if none
./scripts/run-dev.sh --reset-demo    # wipe data/, seed the sample inbox, about 54,000 messages (needs ffmpeg)
./scripts/run-dev.sh --reset-demo --large  # the same with about 613,000 messages
./scripts/run-dev.sh --reset         # wipe data/, start empty and unclaimed (UI opens on Create Owner)
./scripts/run-dev.sh --reset --owner # wipe data/, claim it as admin / admin
./scripts/run-dev.sh --sqlweb        # also SQLite browser at http://127.0.0.1:8081
./scripts/run-dev.sh --release       # optimized build; combines with any flag above
```

`serve` adds the Demo Account to a database that does not exist yet, so a plain first start has it too; `--reset` creates the database empty first (`create-database`). `--reset` and `--reset-demo` cannot be combined. Neither claims the Message Crate; add `--owner` to either for that. `./scripts/run-dev.sh --help` lists every flag with examples. The script writes `config/config.toml` from the example (CORS for Vite `:5173` enabled) only when the file is missing. Later sessions omit `--reset-demo` so the existing database stays.

API: **http://127.0.0.1:8080**. After `--reset-demo`, press **Explore Demo Account** on the login card; the Demo Account has no password. After `--owner`, log in as `admin` / `admin`. Otherwise create the owner in the UI.

Restart terminal 1 after edits under `crates/server/server/` (debug `cargo run`; no hot reload).

**Terminal 2 — UI** (pick one)

```bash
cd web && npm ci && cd ..    # first time, or after web/package-lock.json changes
cargo tauri dev              # desktop window; starts Vite itself
```

Or, browser only (no Tauri):

```bash
cd web && npm run dev        # http://localhost:5173, proxies /v1 to :8080
```

`cargo tauri dev` uses the server on **127.0.0.1:8080** when one is running, so start `./scripts/run-dev.sh` first to work against the repository's `data/`. With nothing on that port the app starts its own server, built by `src-tauri/build.rs`, with its data in `data-dev` in the app-data folder (`~/.local/share/app.messagecrate.desktop/data-dev` on Linux), and stops it when the window closes. The installed app keeps its data in `data` beside it, and a dev build never opens that folder, because a branch with another Schema Fingerprint would rebuild the installed app's database empty. **Open data folder** in a dev build opens `data-dev`.

Do not run `npm run dev` and `cargo tauri dev` at the same time. Point the app at **http://127.0.0.1:8080** (not `localhost` — that can resolve to IPv6, which the server does not listen on). `web/` and `src-tauri/` usually reload; restart `cargo tauri dev` if they do not.

Optional: `./scripts/build-static.sh` copies `web/dist` to `static/` so the server serves the UI at http://127.0.0.1:8080 without Vite. Do not run `docker compose -f docker/compose.release.yml` and the host script at once; they both use port 8080.

### Build, format, and test

#### Backend

Run these from the repository root unless a `cd` is shown.

Rust formatter is `rustfmt`. CI gates Clippy at `-D warnings` on the workspace and on `src-tauri`. `src-tauri/` is not a workspace member, so format it with `--manifest-path`. `rust-toolchain.toml` pins the toolchain for every checkout, for CI, and for the release image (`docker/Dockerfile` copies the file and installs that toolchain; `scripts/check-docker-context.sh` fails when its base image drifts to another minor); bump it deliberately, in its own pull request, fixing any new Clippy lints there — a floating stable can redden `main` with no code change.

```bash
# Check format (what CI runs)
cargo fmt --all -- --check
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check

# Rewrite Rust (workspace + src-tauri) and web/ (Biome)
./scripts/format-all.sh

# Fast pre-flight: fmt --check, Clippy -D warnings, Biome ci, tsc.
./scripts/check-pr.sh

# Everything CI runs, serially (build, tests, audits, docs, the lot).
./scripts/check-all.sh

cargo build --workspace
cargo test --workspace
cargo test -p sms-backup-restore-exporter   # one crate
cargo build --manifest-path src-tauri/Cargo.toml

# Test coverage for the workspace (cargo-llvm-cov). Ends with the count of
# functions no test calls and the files with the most; every one is named
# in target/llvm-cov/uncovered-functions.txt. HTML report at
# target/llvm-cov/html/index.html; --open shows it.
./scripts/coverage.sh

# Mutation testing (cargo-mutants) over the workspace, less what
# .cargo/mutants.toml leaves out. Ends with a per-file table of caught and
# missed mutants; every missed one is named in target/mutants/summary.md.
# A full run is well over a day on one machine, so point it at the file
# you changed.
./scripts/mutants.sh --file crates/libs/phone/src/lib.rs
```

Coverage is a report, not a gate, and it points at gaps rather than measuring test quality: a function no test calls is worth a look, while uncovered lines inside a called function are not a target. Coverage cannot say whether the tests that do call a function would notice it breaking; mutation testing (below) measures that. Never write a test to raise the coverage number. Write one only when it would fail for a bug that matters, and name that bug. `scripts/coverage.sh` needs `cargo-llvm-cov`, the `llvm-tools` component that `rust-toolchain.toml` installs, and `python3`; it leaves test code out of the numbers and does not measure `src-tauri`. The `Coverage` workflow (`coverage.yml`) runs the same script on every push to `main`, puts the function headline on the run's summary page, and keeps the reports and the uncovered-functions list as a workflow artifact for 30 days.

Mutation testing is a report too, and it answers what coverage cannot: whether a test that calls a function would fail if the function were wrong. cargo-mutants changes the code one small way at a time (`<` to `<=`, `&&` to `||`, a function returning `Default::default()`) and runs that package's tests. A mutant every test still passes is "missed", and that list is the finding. Every workspace crate is mutated except the few `.cargo/mutants.toml` leaves out (test-data generators, the build stamp, serde-only types), along with `Display` and `Debug` text, a handful of functions that only build a log or hint line, arithmetic in top-level constants, and retry jitter; it says why for each. `scripts/mutants.sh` needs `cargo-mutants`, `cargo-nextest` (it runs each mutant's tests and stops at the first failure, which halves the time of a caught mutant) and `python3`; other arguments go to `cargo mutants`, and `--file` mutates only that file. The `Mutants` workflow (`mutants.yml`) runs only when started by hand from the Actions tab, never on a schedule or a pull request. It splits the run across 40 shards of about 2.5 hours each (about 100 runner-hours in all, most of it the server crate, whose whole test suite runs for every mutant), puts the joined table and every missed mutant on the run's summary page, and keeps each shard's logs and diffs as a workflow artifact for 30 days.

The `Nightly` workflow (`nightly.yml`) builds the release Dockerfile from `main` every day at 10:37 UTC, with the same steps as "Docker image builds" in `ci.yml`, because that job runs only on a pull request that changes the Dockerfile, a Cargo manifest or the lockfile, and a change anywhere else can break the image. It builds and never publishes; publishing stays with a `v*` tag. A scheduled run ends at once when `main` is still at the commit the previous scheduled run saw. A run started by hand from the Actions tab (`gh workflow run nightly.yml`) builds regardless. It is a report, not a gate: it does not run on pull requests and the ruleset does not require it. A failed night opens an issue titled "Nightly run failed" with the `bug` label, or adds a comment to that issue while it is open, naming the failed job and linking the run. A night that fails is not built again until `main` moves, so the fix is a commit or a run started by hand.

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
npm run dev               # Vite on http://localhost:5173 (proxies /v1 to :8080)
```

From the repository root, `./scripts/format-all.sh` rewrites Rust and web sources to the formatters. `./scripts/check-pr.sh` is the fast pre-flight — `cargo fmt --check` and Clippy at `-D warnings` on both manifests, `biome ci`, and the web type-check; it checks and never rewrites. `./scripts/check-all.sh` runs everything CI runs, serially, starting with `check-pr.sh`. Why the split: `docs/adr/0007-ci-is-the-only-gate.md`.

Do not start a separate `npm run dev` while `cargo tauri dev` is running. Tauri starts Vite itself.

#### Docs

Docs (`docs/`) not the product UI, but CI-adjacent when that tree changes:

```bash
cd docs && npm ci && npm run check && npm run build
```

#### Not gated by CI

Not gated by CI `web-next/` (`npm run lint` / `npm test` there if that tree is edited).

Clippy is a CI job (`-D warnings`, workspace and `src-tauri`). `./scripts/check-pr.sh` runs it locally (`rust-analyzer.check.command` is `clippy` in `.vscode/settings.json`).

### Releases and versions

The product follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html) (`MAJOR.MINOR.PATCH`). Record user-visible changes in `CHANGELOG.md` under the heading for the version in development (`## [0.9.0] — in development`), which becomes `## [0.9.0] - 2026-09-08` when the tag ships.

**`CHANGELOG.md` is written for the people who use Message Crate, not for developers.** Group every entry as **Features** (something a person can now do), **Fixes** (something that was wrong and now behaves correctly), or **Design** (a change in how the product works that is worth knowing about). Say what changed for the reader in plain language: no route paths, status codes, schema versions, type or crate names, or file paths. Internal rework earns a sentence about what it changes for the reader, or one line saying the internals were reworked with nothing visible — never a list of what was moved where. The developer-facing detail already lives in the pull request, the ADRs under `docs/adr/`, and the commit message.

Bullets under the in-development heading start with an ISO date (`YYYY-MM-DD`), the day the change landed. Released sections carry their date on the heading alone. Where a change forces someone to do something — a config key that must be deleted, a database that is rebuilt empty — put it under an **Upgrading** heading in that release, in a sentence they can act on.

Three version numbers are easy to mix up:

| What            | Example             | Meaning                                                                                |
|-----------------|---------------------|----------------------------------------------------------------------------------------|
| Product version | `0.9.0`             | Desktop app + server image. Git tag is `v0.9.0`.                                       |
| Docker Hub tag  | `0.9.0` (no `v`)    | `bitrealm/message-crate:0.9.0`. Also `0.9`, `latest`, and `sha-…`.                     |
| JSONL schema    | `schema_version: 4` | Shared chat file format. Independent of the product version. Version 3 is refused, never upgraded. |
| Build           | `0.9.0+343fe0d8`    | The product version plus the commit, which is what a screen shows as "Version". `.dirty` follows the commit when tracked files held uncommitted changes; a build from a `v*` tag is `0.9.0` alone; `0.9.0+unknown` when nothing is known. Nobody writes it: `crates/libs/build-version` works it out for the server and the desktop app, and `web/vite.config.ts` for the SPA, under the same rules. |
| Schema fingerprint | `1176793189`     | Derived from `schema/sql/*.sql` and stamped into the database. Shown in Owner Home → Server Settings. Never bumped by hand. |

The Build asks git for the commit. Where there is no `.git`, which is the case inside `docker/Dockerfile`, set `MESSAGE_CRATE_BUILD_METADATA` to the part after the `+` (the Dockerfile takes it as the `BUILD_METADATA` build argument). Set and empty means a release, and is what the tag job passes.

**Product version files** (keep these in lockstep; current value is `0.10.0`; CI's `version` job fails when they disagree, and on a `v*` tag when the tag disagrees with them):

- `src-tauri/Cargo.toml` — the value the other three are compared against
- `src-tauri/tauri.conf.json` — installer version
- `web/package.json` — Vite SPA
- `crates/server/server/Cargo.toml` — server crate

Leave most other `Cargo.toml` files at `0.1.0`. Do not bump `web-next/` (`0.3.0`) for a product release.

**Ship a release**

1. Merge the work to `main`.
2. In `CHANGELOG.md`, change the in-development heading to the dated form (`## [0.9.0] - 2026-09-08`) and drop the per-bullet dates, which the heading now carries. Add the next in-development heading above it when work resumes.
3. Set the four product version files to the new number (for example `0.8.0`).
4. Push a git tag `v0.8.0` on that commit. Pushing the tag is what ships. Push/PR to `main` does not. The `version` job fails the tag run if the four files, their lockfiles, or the changelog heading disagree with the tag, and nothing is built or published.

`.github/workflows/ci.yml` then: runs fmt/test, pushes `bitrealm/message-crate`, builds Tauri installers (Linux `.deb` + AppImage, Windows `.msi`, macOS `.dmg`), and creates a GitHub Release named `Message Crate v0.8.0`. `.github/workflows/docs.yml` publishes the documentation site to messagecrate.app on the same tag; a merge to `main` does not publish it.

The docs deploy runs in the `github-pages` environment, whose deployment branch policy in the repository settings must allow the `v*` tag rule as well as `main` (for `workflow_dispatch`). The workflow trigger and that policy have to agree: a tag push against an environment that only allows `main` builds the site and then refuses the deploy, which is what happened to `v0.9.0` (#654). Check and set it with:

```bash
gh api repos/messagecrate/message-crate/environments/github-pages/deployment-branch-policies -q '.branch_policies[] | "\(.type) \(.name)"'
gh api --method POST repos/messagecrate/message-crate/environments/github-pages/deployment-branch-policies -f name='v*' -f type=tag
```

**Build a release-shaped binary locally (does not publish)**

```bash
./scripts/build-app.sh                 # desktop installers, renamed to message_crate_<version>_<arch>, under src-tauri/target/release/bundle/
docker compose -f docker/compose.release.yml up --build   # server image from this checkout
cargo build --workspace --release          # workspace crates only; not the Tauri installer
```

Do not create or push tags unless asked.
