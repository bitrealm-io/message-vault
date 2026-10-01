# message-ir-format

Writes a `ConversationDocument` to JSON, JSON Lines, CSV, EML, MBOX, or a single SyncTech `smses.xml`. Media convert/compress and obfuscation run here when finishing an export. Readers exist for every format so a folder can be converted later.

Exporters and `message-reexport` use this crate. The desktop app Format tab uses it through `message-reexport`.

## Build and test

```bash
cargo test -p message-ir-format
```

Workspace setup: [CONTRIBUTING.md](../../../CONTRIBUTING.md).

## Docs

This crate is a library. Contributor notes: [Common message](https://messagecrate.app/docs/developer/architecture/common-message/). Mail archives: https://messagecrate.app/docs/developer/formats/mail-archive/ . XML output: https://messagecrate.app/docs/developer/formats/sms-backup-restore-xml/

## License

Fair Core License. See the repository root `LICENSE.md`.
