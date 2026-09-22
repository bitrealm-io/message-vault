# Dependency licences

Message Vault is under the Fair Core License (`LICENSE.md`, `FCL-1.0-ALv2`).
FCL is source-available with a non-compete restriction, which makes it
incompatible with copyleft licences: the GPL requires the whole conveyed work
to be under GPL terms with no added restriction, and FCL adds one. A binary
that links FCL code with GPL code cannot be distributed. This page says which
licences a dependency may carry, where the one exception lives, and what
checks it.

## The rule

Permissive licences are accepted for dependencies. The list is the `allow`
array under `[licenses]` in `deny.toml`: MIT, Apache-2.0, the BSD variants,
ISC, Zlib, Unicode, CC0, BSL-1.0, MPL-2.0, and the handful of other permissive
texts the current graph carries. A crate under a licence not on that list
fails `cargo deny check licenses`, and the answer is to find a permissively
licensed alternative or to add the licence to the list after reading it, in
the same pull request, with the reason in the commit message.

GPL-family licences (GPL, LGPL, AGPL, and anything `-or-later`) are accepted
only behind a process boundary. The GPL code lives in its own program, that
program is published under the GPL, and the FCL code starts it as a child
process and talks to it over pipes. The two share no address space and link
no code in common except an interface crate that is itself permissive. A
library boundary is not enough, because a library is linked into the same
binary and the GPL reaches the whole of it.

Why not simply avoid GPL dependencies? Because the best parser for Apple
Messages is one. `imessage-database` is maintained, tracks each iOS and macOS
release, and decodes the `typedstream` bodies, edits, tapbacks and balloons
that a rewrite would spend a year catching up on. Replacing it and asking its
author for a licence exception were both considered on issue #104; the process
boundary keeps a maintained parser at the cost of one extra executable.

## The one exception today

`crates/helpers/imessage-reader` is that program. It links `imessage-database`,
its `typedstream` parser `crabstep`, and `crabapple` (the iPhone backup
decrypter), all GPL-3.0-or-later, so its own manifest says
`license = "GPL-3.0-or-later"` and its `LICENSE` file is the GPL text. It
builds a binary and nothing else.

`crates/helpers/imessage-reader-protocol` is the interface: the serde types
for the request the app writes and the events the reader answers with, one
JSON object per line. Both sides link it, so it is `MIT OR Apache-2.0`; an FCL
protocol crate would have pulled FCL terms into the GPL program, and a GPL
one would have pulled GPL terms into the app.

`crates/helpers/chat-db-fixture` writes the small `chat.db` both sides test
against: the reader's own tests open it in process and the exporter's
process-seam test spawns the built reader against it. It links rusqlite and
nothing else, so it carries no GPL code into the exporter's test binary, and
it is `MIT OR Apache-2.0` for the same reason the protocol crate is. It is a
dev-dependency only; no shipped binary links it.

`crates/exporters/imessage-ir-exporter` stays FCL. It validates the options,
starts the reader, relays its progress lines and cancel, and turns the
records it streams into the shared conversation structure the writers
consume. `cargo tree -p imessage-ir-exporter` shows no GPL crate.

The desktop app ships the reader as a Tauri `externalBin`.
`src-tauri/build.rs` builds it from the workspace into `target/sidecar/` and
copies it to `src-tauri/binaries/imessage-reader-<target triple>`, where
`tauri-build` picks it up: beside the app binary for `cargo tauri dev`, and
inside every installer for `cargo tauri build`. The app finds it beside its
own executable at run time (`imessage_ir_exporter::helper::locate`), then in
`MESSAGE_VAULT_IO_BIN`, then on `PATH`; `MESSAGE_VAULT_IMESSAGE_READER` names
one file outright. The Docker image is unaffected, because the server never
links an exporter.

## What checks it

`cargo deny check licenses bans` runs in `.github/workflows/audit.yml` on the
workspace and, separately, on `src-tauri/Cargo.toml` with the same
`deny.toml`. It runs whenever a lockfile, a manifest, or `deny.toml` changes
and on the weekly schedule, the same trigger as the advisory check. A new
dependency is a lockfile change, so it cannot arrive unchecked.

Three settings in `deny.toml` carry the rule:

- `[licenses] allow` is the permissive list, plus the repository's own
  `LicenseRef-FCL-1.0-ALv2` so the workspace crates pass.
- `[licenses] exceptions` names the four crates that may carry
  `GPL-3.0-or-later`: `imessage-database`, `crabstep`, `crabapple`, and
  `imessage-reader`. A GPL licence on any other crate fails.
- `[bans] deny` lists the GPL libraries with `wrappers`, so
  `imessage-database` and `crabapple` may be depended on by `imessage-reader`
  alone, and `crabstep` by `imessage-database` alone. An FCL crate that adds
  one of them fails the bans check even though the licence check would have
  let the crate through on its exception.

`scripts/check-license.sh` (in CI on every pull request) checks the other
direction: every tracked `Cargo.toml` declares `LicenseRef-FCL-1.0-ALv2`
except the three crates in its `LICENCE_EXCEPTIONS` table, which must declare
exactly the licence recorded there.

`./scripts/check-all.sh` runs both `cargo deny` invocations locally when
`cargo-deny` is installed (`cargo install cargo-deny`).

## Adding a dependency

A crate under MIT, Apache-2.0, or another licence already on the list needs
nothing beyond the normal review. A crate under a permissive licence not yet
on the list needs the licence read and added to `allow` in the same pull
request, with the reason in the commit message. A crate under a GPL-family
licence needs a second process, on the pattern above, or a different crate;
there is no third option. Manifest and `LICENSE` file must agree for every
crate that is not FCL, and `scripts/check-license.sh` must know about it.
