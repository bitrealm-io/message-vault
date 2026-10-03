# message-ir-format

Writes a `ConversationDocument` to JSON, JSON Lines, CSV, EML, or MBOX. A format that folds every conversation into one file is written through a `MergedArchive` the caller supplies. Obfuscation runs here when finishing an export. Media convert/compress runs before that, in the staging step. Readers exist for every format so a folder can be converted later.

Exporters and `message-reexport` use this crate. The desktop app's Export screen and the Convert tab in Settings use it through `message-reexport`.

## Build and test

```bash
cargo test -p message-ir-format
```

Workspace setup: [CONTRIBUTING.md](../../../CONTRIBUTING.md).

## Docs

This crate is a library. Contributor notes: [Common message](https://messagecrate.app/docs/developer/architecture/common-message/). Mail archives: https://messagecrate.app/docs/developer/formats/mail-archive/ . XML output: https://messagecrate.app/docs/developer/formats/sms-backup-restore-xml/

## License

Fair Core License. See the repository root `LICENSE.md`.
