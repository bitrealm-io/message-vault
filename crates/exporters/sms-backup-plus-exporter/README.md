# sms-backup-plus-exporter

Rescue messages from an SMS Backup+ `.eml` mail archive into JSON Lines, JSON, CSV, EML, MBOX, or XML. This is a limited rescue import.

The desktop app Import screen uses this crate as a library.

## Build and test

```bash
cargo test -p sms-backup-plus-exporter
```

Workspace setup: [CONTRIBUTING.md](../../../CONTRIBUTING.md).

## Docs

How this crate fits the export pipeline: https://messagecrate.app/docs/developer/message-transfer/

Format notes: https://messagecrate.app/docs/developer/formats/sms-backup-plus/format/

Import mapping: https://messagecrate.app/docs/developer/formats/sms-backup-plus/mapping/

## License

Fair Core License. See the repository root `LICENSE.md`.
