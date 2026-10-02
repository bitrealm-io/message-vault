# media

FFmpeg wrapper used when an export copies, converts, or compresses attachment files. Convert/compress runs in `message-crate-core`'s attachment jobs, in the `message-staging` transcode pass, and in the server's import media processing.

Converters and the desktop app use this crate. `ffmpeg` and `ffprobe` must be beside the binary, in `MESSAGE_CRATE_BIN`, or on `PATH`.

## Build and test

```bash
cargo test -p media
```

Workspace setup: [CONTRIBUTING.md](../../../CONTRIBUTING.md).

## Docs

This crate is a library. User options: https://messagecrate.app/docs/user/features/messages/attachments-and-media/

## License

Fair Core License. See the repository root `LICENSE.md`.
