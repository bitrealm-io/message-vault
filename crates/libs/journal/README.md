# journal

JSON Lines state journals: append-only logs rewritten by sorted compaction. A
journal is one JSON object per line, and readers rebuild skip-sets from it, so
a run that stops partway can pick up where it left off rather than starting
over.

`message-crate-push` and `message-crate-pull` use this crate to resume a transfer.

## Build and test

```bash
cargo test -p journal
```

Workspace setup: [CONTRIBUTING.md](../../../CONTRIBUTING.md).

## Docs

This crate is a library used by the push and pull crates, which the desktop app
calls in process. It builds no binary.

## License

Fair Core License. See the repository root `LICENSE.md`.
