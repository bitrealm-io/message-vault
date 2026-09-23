# GPL code only behind a process boundary

Message Vault is under the Fair Core License (`LICENSE.md`, `FCL-1.0-ALv2`),
and the best parser for Apple Messages, `imessage-database`, is
GPL-3.0-or-later. FCL is source-available with a non-compete restriction; the
GPL requires the whole conveyed work to be under GPL terms with no added
restriction, so a binary that links FCL code with GPL code cannot be
distributed. We keep the GPL parser and put it in a separate program,
`crates/helpers/imessage-reader`, published under the GPL. The FCL desktop
app starts it as a child process and talks to it over pipes. The two
link nothing in common except a permissive interface crate. The standing rules
that follow from this are under "The rules" below, and a change to one is made
here in the same pull request as the code.

## Why

`imessage-database` is maintained, tracks each iOS and macOS release, and
decodes the `typedstream` bodies, edits, tapbacks and balloons that a rewrite
would spend a year catching up on. The process boundary keeps a maintained
parser at the cost of one extra executable.

A library boundary is not enough. A library is linked into the same binary,
and the GPL reaches the whole of it.

## Considered and rejected

Both were weighed on issue #104.

**Replacing `imessage-database` with our own parser.** Replacing it would mean
a year of catching up on formats Apple changes with every release, for a
parser that would still trail the one we have.

**Asking the author for a licence exception.** It depends on one person's
answer and does not cover `crabapple` or `crabstep`. It stays available as a
fallback, below.

## Why the boundary holds, and where it is thin

The GPL reaches the whole of "the same program", so the question is whether
the reader is part of Message Vault or a separate program Message Vault runs.
The FSF's GPL FAQ says two programs that run as separate processes and exchange
data over pipes are separate works. GPL section 5 says that placing separate
works on one distribution medium is "mere aggregation" and does not extend
the licence to the other work. This is that arrangement: a GPL executable, an
FCL app that starts it and reads JSON lines from it, a permissive protocol
crate so neither side links the other, and both shipped in one installer. It
is the pattern under which commercial software ships ffmpeg and git.

Where it is thin: the same FAQ says two processes exchanging complex internal
data structures, or one that is meaningless without the other, may be one
program. The reader exists for this app and its protocol was designed for it.
The answer is to keep the reader a real program on its own. Its README shows
how to drive it from a shell. The protocol is a documented JSON shape rather
than shared memory. Nothing on the FCL side is a modified copy of GPL source
(the audit below). Should that ever feel too thin, the next steps are a
separate repository and release for the reader, or a commercial exception from
the library's author. Neither has been needed.

## Audit of the FCL side (22 September 2026)

Before commit `b9a24153` (PR #436, the split), the exporter crate linked
`imessage-database` directly. This audit asked whether any code left on the
FCL side is a modified copy of GPL source, as opposed to a caller of it. It
compared every file of `crates/exporters/imessage-ir-exporter/src/` at
`b9a24153^` and at HEAD, and `imessage-reader-protocol`, against
`imessage-database` 4.2.0, `crabapple` 0.4.7, and the `imessage-exporter`
command-line tool in the same upstream repository.

Found:

- `convert.rs` and `helper.rs` were created by the split and adapt nothing:
  no tapback classification, typedstream parsing, balloon parsing, Apple epoch
  maths, or attachment path resolution. Those live on the GPL side and arrive
  as plain fields.
- The protocol crate's types are an independent wire shape, not a copy of the
  library's structs. They have different field sets and different doc
  comments, and overlap only where Apple's own column names are used.
- The pre-split `backup.rs` (iPhone backup decryption) and `error.rs`
  (`RuntimeError`) were adapted from the `imessage-exporter` command-line
  tool, which is GPL-3.0-or-later. The split moved both into the GPL reader,
  which is the right home. What was missing was attribution, which was added
  to the top of each file, to `NOTICE.txt`, and to the README.
- Three user-facing sentences in the FCL `run.rs` were the tool's wording
  (two "will have no effect" warnings and one "not a valid ... mode" error).
  They were reworded. `default_macos_db_path` and `detect_platform` there
  restate two-line facts about where Apple keeps the database; they are not
  copies.
- `MESSAGES_DB_IN_IOS_BACKUP` and the contacts hash are Apple's fixed backup
  paths. They are public knowledge, not upstream code.

Verdict: the FCL side calls the reader and contains no adapted GPL code.
Sign-off is the review of the pull request that closes issue #646. Do not
repeat this audit. A future question is only about code added since, and the
rule under "Adapted GPL code" below covers it.

## Consequences

- `imessage-reader` is the one program beside the server, which ADR 0001's
  amendment accounts for.
- Every desktop installer conveys a GPL program and owes its recipients the
  licence text and the corresponding source.
- A future GPL-family dependency needs a second process on the same pattern,
  or a different crate.

## The rules

### Which licences a dependency may carry

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
no code in common except an interface crate that is itself permissive.

### The one exception today

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

### What we ship and what we owe

Every desktop installer conveys a GPL program, so its recipients are owed the
licence text and the corresponding source (GPL sections 4 and 6). What meets
that:

- **A licence file beside the reader.** `src-tauri/build.rs` joins
  `crates/helpers/imessage-reader/NOTICE.txt` (what the program is, what it
  links, what it adapted, and where its source is, with the Product Version
  filled in) and the crate's `LICENSE` (the GPL text) into
  `src-tauri/resources/imessage-reader-LICENSE.txt`, which `tauri.conf.json`
  lists under `bundle.resources`. The file is generated, not committed.
- **A notice in the app.** Settings → About shows a "Third-party software"
  block in the desktop app only (the website ships no reader), naming the
  Apple Messages reader (imessage-reader), its licence, and two links built
  from the app's own Build: the reader's folder at the release tag
  (`.../tree/v<version>/crates/helpers/imessage-reader`) and the GPL text at
  the same tag. The tag, not `main`, so an old install keeps pointing at the
  source that matches its binary. `web/src/lib/thirdPartySoftware.ts`.
- **Attribution inside the reader.** Files that adapt upstream GPL code say so
  at the top (GPL section 5a), and `NOTICE.txt` and the README repeat it.
- **No flags.** The reader stays a stdin program under ADR 0001; the notice is
  the file and the About block, not a `--version` banner.

### Adapted GPL code

Code adapted from GPL source goes in the reader with a notice at the top of
the file, never in an FCL crate. The FCL side calls the reader and contains no
adapted GPL code; the audit above established this.

### What checks it

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

### Adding a dependency

A crate under MIT, Apache-2.0, or another licence already on the list needs
nothing beyond the normal review. A crate under a permissive licence not yet
on the list needs the licence read and added to `allow` in the same pull
request, with the reason in the commit message. A crate under a GPL-family
licence needs a second process, on the pattern above, or a different crate;
there is no third option. Manifest and `LICENSE` file must agree for every
crate that is not FCL, and `scripts/check-license.sh` must know about it.
