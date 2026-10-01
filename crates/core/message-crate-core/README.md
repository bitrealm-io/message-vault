# message-crate-core

Shared form models (`Form`, `Exporter`, `ExporterConfig`), job spawning (`spawn_job` with cancel and progress), and `export.ini` load/save.

The Tauri desktop app in `src-tauri/` uses this crate. Exporter libraries use the same config types.

## Build and test

```bash
cargo test -p message-crate-core
```

Workspace setup: [CONTRIBUTING.md](../../../CONTRIBUTING.md).

## Docs

This crate is a library used by the desktop app and the converters. The Slint GUI that once used it is gone; its screens are recorded in
[docs/superpowers/reference/legacy-slint-gui.md](../../../docs/superpowers/reference/legacy-slint-gui.md).

## License

Fair Core License. See the repository root `LICENSE.md`.
