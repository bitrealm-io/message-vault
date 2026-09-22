# imessage-reader

Reads Apple Messages (a Mac `chat.db` or an iPhone backup folder) for the Message Vault desktop app, as a separate program.

The app starts this program, writes one JSON request on its stdin, and reads JSON events off its stdout: log lines, one record per conversation, one record per message, then a done line. For an encrypted backup the app then asks for attachments one at a time and the program decrypts each into a scratch folder the app owns. The protocol is [`imessage-reader-protocol`](../imessage-reader-protocol/). The installer places it beside the app and the app finds it there; nothing asks you to type it, though you can (below).

## Why a separate program

This crate links [`imessage-database`](https://crates.io/crates/imessage-database) and [`crabapple`](https://crates.io/crates/crabapple), which are GPL-3.0-or-later. Message Vault is under the Fair Core License, and the GPL does not allow the two to be distributed as one binary. A process boundary keeps the GPL on this side of it. The policy is [`docs/agents/licences.md`](../../../docs/agents/licences.md); the reasoning against ADR 0001's rule of no command line is in that ADR's amendment.

What lives here is the reading: opening the database, decrypting a backup, caching chats, handles, contacts and tapbacks, and classifying each row. Turning the records into the shared conversation structure, writing files, and media handling stay in [`imessage-ir-exporter`](../../exporters/imessage-ir-exporter/), which is FCL.

## Driving it yourself

Nothing in Message Vault requires you to run this program by hand, but it is a
complete program on its own and needs nothing from the app to work. It reads
one JSON request from stdin and writes one JSON event per line to stdout.

Read every message out of a Mac's Messages database:

```bash
cargo build -p imessage-reader
echo '{"op":"export","source":{"db_path":"/Users/you/Library/Messages/chat.db","platform":"mac_os","backup_password":null},"attachment_root":null,"contacts_path":null,"use_caller_id":false,"scratch_dir":null}' \
  | target/debug/imessage-reader
```

For an iPhone backup, `db_path` is the backup folder (the one holding
`Manifest.plist`), `platform` is `"ios"`, and `backup_password` is the backup
password when the backup is encrypted.

What comes back, one JSON object per line, in this order:

1. `{"event":"source","protocol_version":1,"encrypted":false}` once.
2. Any number of `log`, `progress`, `conversation`, and `message` lines,
   interleaved. Each `conversation` names a chat and its participants; each
   `message` is one already-classified message with its attachments listed.
3. `{"event":"export_done","messages_seen":N,"failures":N}`, or
   `{"event":"error","message":"..."}` if the run failed.

Two other requests exist. `{"op":"identities","db_path":...,"platform":...,"backup_password":...}`
answers with one `identities` line listing the addresses the device sent
from. After an export of an encrypted backup, `{"op":"attachment","path":"..."}`
decrypts one attachment the export named and answers with an `attachment` line
giving the decrypted file's path; the program keeps running until stdin is
closed so the app can ask for as many as it needs.

Every field and event is defined, with its meaning, in
[`imessage-reader-protocol`](../imessage-reader-protocol/src/lib.rs).

## Build and test

```bash
cargo build -p imessage-reader
cargo test -p imessage-reader
```

`src-tauri/build.rs` builds this crate and copies the binary to `src-tauri/binaries/` for Tauri to bundle, so `cargo tauri dev` and `cargo tauri build` need no separate step.

Workspace setup: [CONTRIBUTING.md](../../../CONTRIBUTING.md).

## License

GNU General Public License v3.0 or later. See [`LICENSE`](LICENSE) in this folder. This is the one crate in the repository not under the Fair Core License.

Parts of `src/backup.rs` and `src/error.rs` are adapted from [`imessage-exporter`](https://github.com/ReagentX/imessage-exporter) by Christopher Sardegna, GPL-3.0-or-later; each file says which parts. Every desktop installer ships this program with `imessage-reader-LICENSE.txt` beside it, made from [`NOTICE.txt`](NOTICE.txt) and `LICENSE` by `src-tauri/build.rs`, and the app's Settings → About names the program and links to the source for the version installed.
