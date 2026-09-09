# Four crates in the export pipeline, one job each

The code that turns a phone backup into files on disk is four crates, and each
one holds a single job.

- **`message-vault-io-core`** is the run model: the configuration a run is given,
  the report it produces, the shared run skeleton, and the one function that
  stages a conversation's attachments.
- **`message-ir-format`** reads and writes the formats Message Vault itself
  emits — JSON, JSON Lines, CSV, EML, MBOX — and nothing else.
- **`message-staging`** is the resumable write path: the bounded write queue, the
  transcode pass, the staging summary, and the `ExportWriter` that drives them.
- **`sms-backup-restore-exporter`** owns the SMS Backup & Restore wire shape in
  both directions, reading `smses.xml` and writing it.

The dependencies run in one direction: `io-core` at the bottom, then
`ir-format`, then `message-staging`, with the vendor exporters on top. No crate
in the pipeline names a vendor format except the crate that owns that format.

## Why

`message-ir-format` was three crates wearing one name. It held 7,951 lines
across 21 files, and only about half of them read or wrote a format:

| Tenant | Lines |
| --- | --- |
| Staging engine (`write_queue.rs`, `transcode.rs`, `staging_summary.rs`, with tests) | 3,636 |
| SBR vendor import (`read_sbr.rs`) | 708 |
| Exporter run plumbing (`pipeline.rs`, `export_transforms.rs`) | 433 |
| Format readers and writers, and everything else | 3,174 |

The consumers were already disjoint, which is the clearest sign the crate held
more than one thing. `src-tauri` imported the staging engine and one unrelated
constant. `vault-push`, `vault-pull`, `message-reexport` and the vault server
imported no staging item at all. Nothing but the six vendor exporters needed
both halves, and they reached the staging half only through `ExportWriter`.

A crate boundary was chosen over module boundaries because only a crate boundary
is enforced. The crate had already drifted once under module discipline:
`ExportWriter` was added to the staging API after the problem was first written
up in issue #279, and nothing rejected it. Dependency isolation was a weaker
argument than expected and is not the reason — `ir-format` has no async runtime
and links no ffmpeg crate, because all media work goes through the `media`
crate, so the only dependencies exclusive to staging were `fs2` and `serde`.

Two things forced more than a set of file moves.

**The vault server linked a format crate for one string.** Its only import from
`message-ir-format` was the constant `UNSAFE_ATTACHMENT_PATH_PREFIX`. That
constant existed because the path-escape check was written twice — once in
`ir-format`'s `safe_attachment_path`, once in the server's `safe_rel_path` at
`crates/vault/server/src/config.rs:144` — with a doc comment instructing the
next reader to keep the two error strings identical. A defence against directory
traversal held in step by a shared string is a defence waiting to diverge, so
the check itself moves into `message-ir`, both call sites use it, and the server
drops `message-ir-format` from its manifest entirely.

**A generic sink knew about one vendor's format.** `FormatSink::finish` branched
on `is_sbr_xml()` and constructed an `SbrBackupSession` directly, and
`FormatSinkResult` — the result type every exporter passes around — carried a
field named `xml_path`, documented as the path of the written `smses.xml`. Eight
call sites set that field to `None` purely to satisfy one vendor, among them
`write_queue.rs` and `imessage-ir-exporter/src/convert.rs`. The predicate itself,
`is_sbr_xml()`, sat in `io-core`'s configuration module, so the shared core knew
the name of a vendor format too. Inverting that seam is what allows the SBR
writer to leave without a dependency cycle, and it is worth doing on its own
terms: after `xml_path` is removed, `FormatSinkResult` holds a media report and
a count of obfuscated documents, which is a run report rather than anything
about formats, and it moves to `io-core` with the rest of the run model.

## Considered and rejected: enforcing the split with modules

Three modules inside one crate — `format`, `sbr`, `staging` — with a disciplined
`lib.rs` would have cost nothing to create and nothing to undo.

It was rejected because a module boundary is a convention, and this crate had
already demonstrated that the convention does not hold. Every consumer would
also continue to compile every dependency, so a caller that only reads JSON Lines
would keep building the queue's. A crate boundary turns a reach across the seam
into a compile error, which is the only enforcement that survives the next
person in a hurry.

## Considered and rejected: making staging a sibling of the format crate

The staging files are not self-contained: `write_queue.rs` calls
`crate::write::write_format`, and both `transcode.rs` and `staging_summary.rs`
call `read_json` and `util`. Those shared helpers could have moved down into
`message-ir` or into a new small crate, leaving staging and format as peers that
know nothing of each other.

It was rejected because the dependency it removes is honest. Staging genuinely
reads and writes the archive's own formats — that is what a resumable write path
does — so an edge from `message-staging` to `message-ir-format` describes the
code rather than papering over it. Symmetry would have cost an additional crate
and bought nothing that the layering does not already give.

## Considered and rejected: lifting the SBR special case into Convert

Rather than a trait, the XML branch could have been deleted from `FormatSink`
outright and handled by `message-reexport`, which is the only caller that
produces XML today. Convert reaches it through
`src-tauri/src/commands/format.rs`, which maps the string `"xml"` to
`OutputFormat::Xml`; Extract cannot produce XML at all, because
`src-tauri/src/commands/extract.rs` pins its output format to JSON Lines. One
producer means one caller could own the special case.

It was rejected because the single producer is a fact about what has been built,
not about what the product is for. SMS Backup & Restore will gain an import
screen, and when it does, a person exports their vault as `smses.xml` and loads
it back onto the phone — which means an export path, not only Convert, has to be
able to ask for XML. A trait seam in `ir-format`, implemented by the SBR crate
and supplied by whichever caller wants XML, keeps that reachable. Lifting the
case into Convert would have to be undone.

## Consequences

- `message-staging` is the name, and neither "desktop" nor "transcode" appears in
  it. The engine is not desktop-only: `src-tauri` is one consumer, and seven
  exporters reach the same code through `ExportWriter`. Transcode is on
  `CONTEXT.md`'s avoid list because it collides with **Convert**. `CONTEXT.md`
  gains one entry, **Staging**, for the operation, beside the existing **Staging
  Directory** for the place.
- `message-ir-format` keeps `write_sbr.rs`'s job but not its home. Writing
  `smses.xml` is production behaviour, asserted at
  `crates/libs/reexport/src/tests.rs:128`, and it moves to the SBR crate behind
  the new trait rather than being deleted or left where it was.
- The three copies of the assemble-jobs, run-jobs, clear-bytes sequence collapse
  onto `io-core`'s `stage_conversation_attachments`. The copy in
  `message-reexport` had already diverged: it passed no cancel flag, no log sink
  and an empty progress closure, and never counted staged attachments or cleared
  the in-memory bytes. Convert regains cancellation and progress reporting as a
  consequence of the unification rather than as a separate fix.
- `ExportWriter` moves to `message-staging` even though it does not always use
  the queue. The queue path is taken when the format is JSON Lines and
  obfuscation is off; every other run stages through `io-core` and writes through
  the sink. Both arms are live — turning obfuscation on during an extract takes
  the non-queue arm today — so neither can be deleted, and `ExportWriter` sits
  above both.
- The `contacts` dependency is removed from `message-ir-format`. It was declared
  in the manifest and referenced nowhere in the crate's sources.
- The work lands as six pull requests, split for reviewability and for clean
  reverts: unify the path check, unify the staging sequences, invert the SBR
  seam, move the SBR reader and writer, push the run model down into `io-core`,
  extract `message-staging`. The first two change behaviour and are deliberately
  not carried along with a file move. The SBR seam has to be inverted before the
  writer can move, and before `FormatSinkResult` travels to `io-core`.
- Nothing here is kept for compatibility. Public items are renamed, moved between
  crates and removed wherever the result is simpler, and tests are rewritten to
  match rather than preserved.
