# No command line except the vault server

Message Vault converts phone backups into files, which is command-line shaped
work, so a reader will reasonably expect the exporters to be commands. They
are not: every exporter, `message-reexport`, `vault-push`, and `vault-pull` are
library crates with no binary, and the desktop app calls them in process. The
only command line in the product belongs to `message-vault-server`, because a
self-hosted server needs one.

## Why

The exporters were commands first and the desktop app was built around them,
but that relationship had already disappeared from the code:
`src-tauri/src/commands/extract.rs` imports `run` from each exporter crate
directly, and every exporter dependency in `src-tauri/Cargo.toml` is declared
`default-features = false`, so the desktop app never even compiled clap.

What remained was a surface that the documentation site presented to people and
that no release contained. `dump-cli-docs` generated a reference page for each
of eleven commands, and the User Guide told readers to build the workspace to
get them, while the `release` job in `.github/workflows/ci.yml` built only the
Tauri installers and the `docker` job built only the server image. The commands
were documented as a product and distributed as a build artifact of a clone.

Headless and scripted use is a real audience and the eventual reason to have a
command line again. It is deferred, not dismissed. When it returns it should be
one `message-vault` command with subcommands, not seven exporter binaries plus
a push and a pull.

## Considered and rejected: keeping `vault-push` and `vault-pull`

These two were nearly kept, on the reasoning that they are the *server's*
interface rather than the desktop app's, that a self-hosted vault with no
desktop machine would otherwise have no terminal route for its data, and that
they already worked and cost nothing to leave alone.

That last claim did not survive checking. No workflow, script, or test has ever
invoked either one as a command. A shell smoke-test script looked like the
counterexample and was not: it started the server and drove the HTTP API with
`curl`, never touching the `vault-push` binary, and nothing ran the script
either — which is also why it was later deleted rather than kept. Keeping the
binaries would have preserved the appearance of a headless path
rather than a working one, and drawn the line on a distinction the evidence
did not support.

Anyone re-proposing a command line should re-derive it from the audience, not
from the observation that these crates are one `main.rs` away from being
commands again. They are, and that is not the question.

## Amendment: a helper process is not a command line

`imessage-reader` (`crates/helpers/imessage-reader`) is a program, and this
decision still holds. It exists for a licence reason and no other: it links
`imessage-database` and `crabapple`, which are GPL-3.0-or-later, and the rest
of the repository is under the Fair Core License, which the GPL does not let
a single binary combine with. The desktop app therefore starts the reader as
a separate process, writes one request on its stdin, and reads the messages
back off its stdout (`crates/helpers/imessage-reader-protocol`). A process
boundary is what keeps the GPL on its own side; a library boundary would not.
The decision is ADR 0014 (`docs/adr/0014-gpl-code-only-behind-a-process-boundary.md`).

That program is not a command line in this decision's sense. Nobody types it:
it takes one JSON line on stdin and speaks only to the app, the installer puts
it beside the app and nowhere on `PATH`, and it appears on no documentation
page as something to run. It is an implementation detail with a process
around it. The audience question above — who would use a command, and for
what — has the same answer it had: nobody yet, and when it changes the answer
is one `message-vault` command, not a reader that happens to be executable.

## Consequences

- `crates/cli/` no longer exists. `vault-push` and `vault-pull` live in
  `crates/libs/` because that is what they are.
- `dump-cli-docs` is a `message-vault-server` subcommand beside `dump-openapi`,
  since the server's own page is the only one left to generate.
- The exporter crates keep `run` as their entire public entry point. Adding a
  binary back to one of them is a decision about product surface, not a
  convenience. `imessage-reader` is the one program beside the server, and it
  is not an exporter: it reads the database and the FCL exporter still does
  the exporting.
- The documentation pages for these commands were deleted without redirects.
  Before a stable release this project keeps no compatibility path — not for
  database schemas, not for stored vault data, and not for URLs — so those
  addresses return 404 rather than pointing somewhere that does not answer the
  question they were bookmarked for.
