# chat-db-fixture

A small Apple Messages `chat.db`, written with rusqlite, for tests. `write_chat_db` puts a database with two people, two chats, three messages and one attachment into a directory and returns its path. The tests of [`imessage-reader`](../imessage-reader/) open it in process, so the session, the emitter and the attachment code are reachable without building the binary; the process-seam test in [`imessage-ir-exporter`](../../exporters/imessage-ir-exporter/) spawns the built binary against the same file. Every row is made up; nothing comes from a real backup.

## Why permissive

Both a GPL crate and an FCL crate use it in their tests, and it must carry no GPL code into the exporter's test binary, so it depends on rusqlite alone and is `MIT OR Apache-2.0`, like [`imessage-reader-protocol`](../imessage-reader-protocol/). Why the two sides are separate programs at all: [`docs/agents/licences.md`](../../../docs/agents/licences.md).

## Build and test

```bash
cargo test -p chat-db-fixture
```

## License

MIT or Apache-2.0, at the reader's option. See [`LICENSE-MIT`](LICENSE-MIT) and [`LICENSE-APACHE`](LICENSE-APACHE).
