# ios-backup

Read an iPhone backup before, or beside, an import. An iPhone backup is a container that several sources read, so what is asked of the backup itself lives here rather than in one exporter:

- `ios_backup_encrypted_flag`: whether `Manifest.plist` marks the backup encrypted.
- `ios_backup_phone_number`: the phone number in `Info.plist`.
- `backup_identities`: the addresses the device sent from, for the Import identity check. It also reads a Mac `chat.db`.
- `decrypt_ios_backup_domain`: every file of one domain of an encrypted backup, decrypted into a folder. The WhatsApp importer uses it.

Opening a backup's databases and decrypting its files needs `imessage-database` and `crabapple`, which are GPL-3.0-or-later, so that work happens in the separate program [`imessage-reader`](../../helpers/imessage-reader/). `Helper` finds that program, starts it, and reads its answer over [`imessage-reader-protocol`](../../helpers/imessage-reader-protocol/); `ScratchDir` is the folder one request decrypts into. [`imessage-ir-exporter`](../../exporters/imessage-ir-exporter/) starts the program through the same two types for an export. Why: [ADR 0014](../../../docs/adr/0014-gpl-code-only-behind-a-process-boundary.md).

The desktop app, `imessage-ir-exporter`, and `whatsapp-exporter` use this crate.

## Build and test

```bash
cargo test -p ios-backup
```

`tests/helper_process.rs` builds `imessage-reader` with cargo and reads the identities of a small `chat.db` through the real process. The unit tests use fake programs from `testutil` (Unix only); the `testutil` feature makes them available to the exporter's tests too.

Workspace setup: [CONTRIBUTING.md](../../../CONTRIBUTING.md).

## Docs

This crate is a library the desktop app and two exporters call in process. It builds no binary.

## License

Fair Core License. See the repository root `LICENSE.md`. No GPL code is linked here; `cargo tree -p ios-backup` shows none.
