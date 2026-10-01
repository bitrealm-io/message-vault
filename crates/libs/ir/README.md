# message-ir

Shared conversation types for Message Crate: `ConversationDocument`, messages, attachments, and participants. This crate has no I/O and no formatting. Attachment bytes are never serialized to JSON; paths and hashes point at sidecar files.

Exporters, `message-ir-format`, `message-crate-push`, `message-crate-pull`, and the server use this crate.

## Build and test

```bash
cargo test -p message-ir
```

Workspace setup: [CONTRIBUTING.md](../../../CONTRIBUTING.md).

## Docs

This crate is a library. Schema notes for contributors: [Common message](https://messagecrate.app/docs/developer/architecture/common-message/). JSON Lines layout for imports: https://messagecrate.app/docs/developer/reference/export-structure/

## License

Fair Core License. See the repository root `LICENSE.md`.
