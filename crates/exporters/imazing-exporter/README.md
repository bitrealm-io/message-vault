# imazing-exporter

Rescue messages from an iMazing Messages or WhatsApp CSV export into JSON Lines, JSON, CSV, EML, MBOX, or XML. This is a limited rescue import.

The desktop app Import screen uses this crate as a library.

## Build and test

```bash
cargo test -p imazing-exporter
```

Workspace setup: [CONTRIBUTING.md](../../../CONTRIBUTING.md).

## Docs

How this crate fits the export pipeline: https://messagecrate.app/docs/developer/message-transfer/

Input format: https://messagecrate.app/docs/developer/formats/imazing/input/

Importer design: https://messagecrate.app/docs/developer/formats/imazing/design/

## License

Fair Core License. See the repository root `LICENSE.md`.
