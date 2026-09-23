# imessage-reader-protocol

The line-delimited JSON protocol between the Message Vault desktop app and the [`imessage-reader`](../imessage-reader/) program: the request the app writes on the program's stdin and the events the program writes on its stdout. Type definitions and their serde shapes, nothing else.

The session order, the tag on every enum, and the meaning of each field are documented on the types in [`src/lib.rs`](src/lib.rs). `PROTOCOL_VERSION` is bumped when a change would make an older program and a newer app misread each other.

## Why permissive

Both sides link this crate: the program is GPL-3.0-or-later and the app is under the Fair Core License. An FCL protocol crate would pull FCL terms into the GPL program and a GPL one would pull GPL terms into the app, so this crate is `MIT OR Apache-2.0`. Why the two sides are separate programs at all: [ADR 0014](../../../docs/adr/0014-gpl-code-only-behind-a-process-boundary.md).

## Build and test

```bash
cargo test -p imessage-reader-protocol
```

## License

MIT or Apache-2.0, at the reader's option. See [`LICENSE-MIT`](LICENSE-MIT) and [`LICENSE-APACHE`](LICENSE-APACHE).
