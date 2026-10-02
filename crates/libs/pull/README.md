# message-crate-pull

Download messages from a running server into a local JSON Lines folder (`*.jsonl` plus `attachments/`).

The desktop app **Export** screen uses this crate as a library. Create an API token under **Settings → Account** in the website.

## Build and test

```bash
cargo test -p message-crate-pull
```

Workspace setup: [CONTRIBUTING.md](../../../CONTRIBUTING.md).

## Docs

How this crate fits the export pipeline: https://messagecrate.app/docs/developer/message-transfer/

## License

Fair Core License. See the repository root `LICENSE.md`.
