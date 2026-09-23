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
that follow from this are in `docs/agents/licences.md`.

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
rule in `docs/agents/licences.md` covers it.

## Consequences

- `imessage-reader` is the one program beside the server, which ADR 0001's
  amendment accounts for.
- Every desktop installer conveys a GPL program and owes its recipients the
  licence text and the corresponding source.
- A future GPL-family dependency needs a second process on the same pattern,
  or a different crate.
