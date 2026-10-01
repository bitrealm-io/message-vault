# vault-push

Import a Message Vault JSON Lines export folder into a running vault server.

The desktop app **Import** screen uses this crate as a library. The `vault-push` command is the same importer from a terminal. Create an API token under **Settings → Account** in the vault website.

## Build and test

```bash
cargo test -p vault-push
```

Workspace setup: [CONTRIBUTING.md](../../../CONTRIBUTING.md).

## Docs

How this crate fits the export pipeline: https://messagecrate.app/docs/developer/message-transfer/

## License

Fair Core License. See the repository root `LICENSE.md`.
