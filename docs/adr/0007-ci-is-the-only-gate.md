# CI is the only gate

Every check that decides whether work may reach `main` runs in GitHub Actions.
A ruleset on `main` requires those checks to pass and requires a pull request to
carry them. The two scripts under `scripts/` decide nothing: `check-pr.sh` is a
fast pre-flight that catches the mistakes not worth a round trip, and
`check-all.sh` runs the whole set locally for anyone who wants it before
pushing.

`ci.yml` holds eleven jobs on a pull request. A `changes` job diffs the branch
against its base and publishes four booleans — `rust`, `web`, `docs`, `docker`
— and the heavy jobs read them:

| Job | Runs when | What it does |
| --- | --- | --- |
| `changes` | always | Diffs against the base and outputs `rust`, `web`, `docs`, `docker` |
| `fmt` | `rust` | `cargo fmt --check` on the workspace and on `src-tauri` |
| `clippy` | `rust` | `cargo clippy --workspace --all-targets -- -D warnings` |
| `test` | `rust` | `cargo build --workspace` and `cargo test --workspace` against a `postgres:16-alpine` service |
| `check-tauri` | `rust` | `cargo check`, Clippy at `-D warnings`, and `cargo test` on `src-tauri` |
| `web` | `web` | `biome ci`, `check-generated-api-types.sh`, `npm run build`, `npm test` |
| `docs` | `docs` | `npm ci`, `astro check`, `astro build` — the site without rustdoc or the HTTP API catalog |
| `license` | always | `check-license.sh` |
| `docker-context` | always | `check-docker-context.sh` |
| `docker-build` | `docker`, pull requests only | `docker/build-push-action` with `push: false`: the release Dockerfile builds |
| `version` | always | `check-version-lockstep.sh`: the four product version files and their lockfiles agree, and on a `v*` tag agree with the tag |

Two classifier arms are not what the directory alone would suggest.
`scripts/check-generated-api-types.sh` sets `web`, not `rust`, because the
`web` job is the one that runs it; every other path under `scripts/` falls to
`rust`. `docker` is true for `docker/`, the root and per-crate Cargo manifests,
`Cargo.lock`, `rust-toolchain.toml` and `config/config.docker.toml`: the
files the Dockerfile copies or reads, and so the only files that can stop the
image building while the Rust jobs stay green. Before `docker-build` existed
the image was built for the first time on the release tag, so a pull request
that broke `docker/Dockerfile` merged green and the failure appeared where the
fix is a new tag. The job runs on pull requests only: the tag job builds and
pushes the same image, and a push to `main` was checked by the pull request
that produced it.

`src-tauri`'s Clippy stays inside `check-tauri` rather than the `clippy` job,
because its build needs the webkit and gtk system packages that job already
installs; a second install would double the cost for no coverage.

The dependency audits live in `audit.yml`, not `ci.yml`. That workflow runs
`cargo deny check advisories` and `npm audit --audit-level=high` for `web/` and
`docs/`, triggered by a pull request that touches `Cargo.lock`,
`web/package-lock.json` or `docs/package-lock.json`, and by a weekly schedule.

Test coverage lives in `coverage.yml` for the same reason seen from the other
side: it is a report that never fails a pull request, so it has no place in a
workflow whose every job is required. It runs `scripts/coverage.sh`
(cargo-llvm-cov over the workspace, with the Postgres suites live) on each push
to `main` and on demand, and keeps the reports as a workflow artifact.

The docs build on a pull request is the `docs` job in `ci.yml`, not a trigger
on `docs.yml` — a required check that lives in a path-filtered workflow
deadlocks every pull request that misses the filter, exactly like
`paths-ignore`. `docs.yml` keeps the full build with rustdoc and the Pages
deploy, and runs on the `v*` tag that ships a release, or by hand.

The site publishes on a release, not on a merge. bitrealm.io describes the
product people can install, and between releases that is the tagged version,
not the tip of `main`; a page that documents a screen nobody can download yet
is wrong for every reader who arrives from the download link. Publishing on
merge also did real work for nothing: a burst of twelve squash merges on
2026-09-05 produced twelve Pages runs, eleven of them cancelled by the next,
each after a full `cargo doc`. `workflow_dispatch` stays for a docs-only
correction that should not wait for the next tag.

On a `v*` tag the `docker` and `release` jobs depend on every job in `ci.yml`,
and `github-release` depends on both of them, because its notes promise the
Docker image as well as the installers. Nothing ships unless everything is
green.

The `release` matrix installs `tauri-cli` with `cargo binstall`, not
`cargo install`. `tauri-cli` publishes cargo-binstall metadata pointing at
prebuilt `cargo-tauri-{target}` archives on its GitHub releases, so the
install is a download. Compiling it from source took 5.6 minutes on Linux, 8.5
on macOS and 10.6 on Windows in the v0.8.3 release, about as long as building
the app itself. The `^2` version range stays, so the release remains on the
major the desktop app targets. The step passes `--strategies crate-meta-data`,
which drops binstall's compile fallback, so a missing archive fails the step
rather than quietly costing the ten minutes again. The macOS leg adds
`--pkg-fmt zip`: `tauri-cli`'s metadata declares `tgz` and overrides only
`x86_64-apple-darwin` to `zip`, while its release ships only a `zip` for
`aarch64-apple-darwin`, the runner's target. A dry run of binstall 1.23.0
against that target fails without the flag and resolves 2.11.4 with it. The
step also passes the workflow's `GITHUB_TOKEN`: binstall resolves the asset
through api.github.com, and unauthenticated requests share GitHub's
sixty-per-hour limit with every other runner on the same address, which is a
403 in practice. A dry run on the runners without the token failed on Linux
and macOS and passed on Windows by luck of the address.

## Why

`scripts/check-pr.sh` began as a stopgap: a way to see whether a branch was
ready for a pull request by checking formatting and the other cheap things.
It grew by accretion. Every time a check appeared in CI, someone added it
locally as well, and every time a check seemed useful locally, it stayed out of
CI because CI already took long enough. By the time this decision was written
the script ran a full workspace build and test, `cargo deny`, two `npm audit`
invocations, the web bundle build, and the documentation site check and build —
serially, in one shell, on one machine.

That is the same work GitHub Actions performs across ten parallel runners. The
script therefore cost roughly ten times the wall-clock of a `git push` and
returned the same answer. A pre-flight that takes as long as the real flight is
not a pre-flight.

The accretion also ran the other way, and left four holes.

**Clippy gated nothing.** `AGENTS.md` said "CI does not run Clippy" in three
separate places. The only Clippy that failed anything was the `src-tauri`
invocation inside the `check-tauri` job, which has run at `-D warnings` for
some time and is clean. The workspace carried seventeen warnings: two unused
`empty_contacts` imports in the exporter smoke tests, an unused `tmp` binding,
two needless `mut`, two needless borrows in
`crates/exporters/imessage-ir-exporter/src/attachments.rs`, an empty line after
a doc comment, items declared after a test module, an `if let` that is
`.unwrap_or_default()`, three `too_many_arguments` — including a nine-argument
function at `crates/libs/vault-push/src/run.rs:2055` — three `type_complexity`,
and a `contains` method at
`crates/exporters/imessage-ir-exporter/src/contacts.rs:71` that nothing calls.

**A documentation pull request was checked nowhere.** `ci.yml` carried
`paths-ignore: docs/**`, and `docs.yml` had no `pull_request` trigger. A
pull request touching only `docs/` triggered no workflow at all, so a broken
`astro check` merged green and then failed on `main`, where the same workflow
both builds and deploys. The site stopped updating until someone fixed it
forward.

**Two checks existed only on a developer's machine.**
`scripts/check-generated-api-types.sh` regenerates
`web/src/lib/vaultApi.types.ts` from `docs/src/assets/openapi.json` and diffs
the result; it is the only thing tying the generated TypeScript to the
specification, and no CI job ran it. `vite build` ran only in the tag-only
`release` job, so a bundle that TypeScript accepted and Vite rejected surfaced
during a release build, where the fix has to be a new tag. A third gap sat
between them: CI type-checked with a bare `npx tsc --noEmit`, which reads
`tsconfig.json` alone, while `web/package.json`'s `typecheck` script also reads
`tsconfig.test.json`. Test files were type-checked locally and not in CI.

**A release did not depend on its own checks.** The `docker` and `release` jobs
declared `needs: [fmt, test, test-postgres, check-tauri]` — four of the ten jobs
that run on a tag. A failing `web-test` did not stop the Docker image, the
desktop installers, or the GitHub Release, and the desktop installers bundle
`web/`. The web app could ship with red tests.

Behind all four sat a fifth fact: `main` had neither a ruleset nor classic
branch protection. Nothing in CI was required, so a red pull request merged and
a direct push to `main` succeeded.

## Considered and rejected: making `check-pr.sh` mirror CI exactly

The tidiest-sounding resolution is to define `check-pr.sh` as precisely what CI
runs, so that a green script guarantees a green pull request. It fails on
arithmetic. CI's value comes from running ten jobs at once on ten machines; a
shell script runs them one after another on one. Mirroring converts a
five-minute parallel result into a twenty-minute serial one, and the reward is
information that a push produces for free. The mirror is also fragile in the
one direction that matters: it has to be updated every time CI changes, and
nothing enforces that it was.

## Considered and rejected: deleting the heavy local script

Cutting `check-pr.sh` down to format and lint suggests deleting the expensive
work outright and letting CI own it. That removes a real convenience. Running
the full set by hand means assembling `cargo test --workspace`, the `src-tauri`
invocation with its `--manifest-path`, two `npm` trees, and four helper
scripts. Keeping one command that runs everything costs a file; making somebody
retype nine commands costs them every time. Hence `check-all.sh`, which calls
`check-pr.sh` first and then does the rest.

`scripts/lint-all.sh` was deleted instead. Its contents — Clippy on both
manifests plus the web linter — became a strict subset of the new
`check-pr.sh`, and a third script that is a subset of the first is how the
original drift started.

## Considered and rejected: auditing dependencies on every pull request

`cargo deny` and `npm audit` read a remote advisory database. They fail when
somebody publishes an advisory, not when somebody writes bad code, so on every
pull request they turn unrelated work red and hold it hostage to a dependency
bump. `deny.toml` already carries six ignored advisories, which is the
maintenance tail of exactly that pressure.

Removing them entirely was also rejected. A Dependabot pull request is the one
case where an audit result should change the merge decision, and it is
identifiable: it changes a lockfile. GitHub applies path filters at the
workflow level rather than the job level, so scoping the audits to lockfiles
required moving them into `audit.yml`. The weekly schedule covers newly
published advisories within seven days without ambushing a feature branch.

The cost of that split is that `needs:` cannot name a job in another workflow,
so the release jobs cannot depend on the audits. Running the audits on the tag
anyway was rejected as worse than not running them: they would report after the
release had already been created, which reads like a gate and is not one. A tag
is cut from a `main` that the schedule audited within the last week.

## Considered and rejected: path filters on the workflow trigger

`paths-ignore` on `on.pull_request` is the cheapest way to skip irrelevant
work, and it is incompatible with required status checks. A job skipped by an
`if:` condition reports success; a workflow that never starts reports nothing at
all, and GitHub holds the pull request pending forever waiting for a check that
will never arrive. Since the ruleset is the point — the previous eight
paragraphs are only advice without it — the path logic moved inside the
workflow, into the `changes` job.

## Consequences

`check-pr.sh` checks rather than rewrites. It no longer calls `format-all.sh`,
so it can now fail on formatting, which it never could before. `format-all.sh`
is the thing to run when it does.

Turning the Clippy gate on requires the seventeen warnings to be fixed first,
in their own change, or `main` goes red on the merge that adds the job. The
three `too_many_arguments` and three `type_complexity` warnings are fixed at the
call site rather than silenced workspace-wide in `[workspace.lints]`: a blanket
allow is permanent and invisible, and the next nine-argument function would
inherit it. A specific site that genuinely wants nine arguments carries a local
`#[allow]` with a reason.

The `test-postgres` job is gone. Its Postgres service moved onto the `test`
job, which sets `MV_TEST_POSTGRES_URL` for `cargo test --workspace`. Every
Postgres-gated test runs in a schema of its own on that server
(`pg_test_schema_url` in `crates/vault/server/src/db/engine.rs`, #435), so
running them inside the workspace suite introduces no race, and two checkouts
can run against one server at the same time. The server crate now compiles once per pull request
instead of twice.

The ruleset does not require a branch to be up to date with `main` before it
merges, so each pull request is checked against the `main` it branched from,
not the `main` it lands on. Two pull requests that are green on their own can
squash-merge into a `main` that does not compile. Requiring up-to-date branches
would prevent that at the cost of a rebase before every merge; a merge queue
would prevent it without the rebase, at the cost of one more CI run per merge.
Neither is in place. What is in place is detection: `ci.yml` cancels an
in-progress run only for a `pull_request` event, never for a push to `main` or
a tag, so every commit on `main` gets its own verdict and a bad combination
shows up on the merge that caused it. Before this, a burst of squash merges
cancelled every `main` run but the last — twelve merges on 2026-09-05 left
eleven cancelled runs and one result.

The `changes` job is load-bearing and worth testing before the ruleset is
enabled. A diff that is too broad runs the Rust matrix on a README edit; a diff
that is too narrow skips it on a code change. On a tag or a `workflow_dispatch`
there is no base commit to diff against, so every output is `true`.

`ci.yml` granted `contents: write` and `packages: write` to every job. Only
`github-release` writes anything with the GitHub token, and the Docker image
goes to Docker Hub under its own secret rather than to GitHub Packages. The
workflow now grants `contents: read`, and `github-release` alone asks for
`contents: write`.

`docs.yml` took two corrections while the pull-request docs build moved into
`ci.yml`. Its concurrency group was a bare `pages` with
`cancel-in-progress: true`, which let any other run in that group cancel a
production deploy; it is now scoped by `github.ref`. Its `pages: write` and
`id-token: write` permissions sat at workflow level; they now sit on the
`deploy` job alone. The pull-request docs build skips rustdoc, because
copying prebuilt rustdoc HTML into `public/` cannot change whether Astro
builds, and `cargo doc` failures are compile failures that the `test` job
already catches. It skips the HTTP API catalog copy for the same reason:
`docs/scripts/copy-http-api-reference.sh` writes into `public/`, which
`astro check` never reads, and `astro.config.mjs` excludes
`/vault/developer/rustdoc/**` from link checking, so the step changed nothing
the job verified. `docs.yml` still runs both before the build it deploys.

Every sentence in `AGENTS.md` and `CLAUDE.md` describing the old arrangement is
now false and was rewritten — in particular the three statements that CI does
not run Clippy, and the descriptions of `check-pr.sh` and the deleted
`lint-all.sh`.
