# media

FFmpeg wrapper used when an export copies, converts, or compresses attachment files. Convert/compress runs inside format packaging (`message-ir-format`), not as a separate CSV post-step.

Converters and the desktop app use this crate. `ffmpeg` and `ffprobe` must be beside the binary, in `MESSAGE_VAULT_IO_BIN`, or on `PATH`.

## Build and test

```bash
cargo test -p media
```

Workspace setup: [CONTRIBUTING.md](../../../CONTRIBUTING.md).

## Docs

This crate is a library. User options: https://messagecrate.app/docs/user/how-to/media-and-privacy/

## License

Fair Core License. See the repository root `LICENSE.md`.
