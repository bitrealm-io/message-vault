# imessage-ir-exporter

Export Apple Messages from a Mac `chat.db` or an iPhone backup into JSON Lines, JSON, CSV, EML, MBOX, or XML.

The desktop app Import screen uses this crate as a library.

This crate does not open `chat.db` itself. The library that parses it, `imessage-database`, is GPL-3.0-or-later and this crate is under the Fair Core License, so the reading happens in a separate program, [`imessage-reader`](../../helpers/imessage-reader/). `run` finds that program beside the app (or in `MESSAGE_VAULT_IO_BIN`, or on `PATH`), starts it, streams its records back over stdout, and turns them into the shared conversation structure the writers consume. The wire types are [`imessage-reader-protocol`](../../helpers/imessage-reader-protocol/). Why: [ADR 0014](../../../docs/adr/0014-gpl-code-only-behind-a-process-boundary.md).

## Build and test

```bash
cargo test -p imessage-ir-exporter
```

The integration tests in `tests/helper_process.rs` build `imessage-reader` with cargo and run the exporter through the real process against a small `chat.db` they write themselves.

Workspace setup: [CONTRIBUTING.md](../../../CONTRIBUTING.md).

## Docs

How this crate fits the export pipeline: https://bitrealm.io/vault/developer/message-transfer/

## License

Fair Core License. See the repository root `LICENSE.md`. No GPL code is linked here; `cargo tree -p imessage-ir-exporter` shows none.
