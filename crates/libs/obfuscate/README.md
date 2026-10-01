# obfuscate

Rewrite an export so chats, timestamps, and attachment counts stay, while names, numbers, message bodies, and media bytes are replaced. Remaps are stable for a given seed and are not reversible from the output alone.

`message-ir-format` applies this when obfuscation is on, for every export source.

## Build and test

```bash
cargo test -p obfuscate
```

Workspace setup: [CONTRIBUTING.md](../../../CONTRIBUTING.md).

## Docs

This crate is a library. User options: https://messagecrate.app/docs/user/how-to/media-and-privacy/

## License

Fair Core License. See the repository root `LICENSE.md`.
