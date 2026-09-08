---
title: Install the desktop app
description: Download the desktop app from GitHub Releases, install FFmpeg and wtsexporter, or build from source.
---

The desktop app reads phone backups and imports them into the vault. Browsing can stay in the website; Import and Export need this app. Run the vault with Docker first — see [Try the vault](/vault/user/get-started/try-the-vault/).

## Download

Open the [latest release on GitHub](https://github.com/bitrealm-io/message-vault/releases) and install the build for the operating system.

### Linux

1. Download the `.deb` (Debian/Ubuntu) or the AppImage.
2. Install the `.deb`, or mark the AppImage executable and run it.

### Windows

1. Download the `.msi` installer and run it.
2. If SmartScreen shows a warning, choose the option to run it once — the app is not code-signed yet.

### macOS

1. Download the `.dmg` for Apple Silicon (M-series and later).
2. Open the disk image and install the app.
3. If Gatekeeper blocks the app, allow it once in the security prompt — it is not code-signed yet.

## Helpers for Convert and WhatsApp

**Convert** / **Compress** need FFmpeg (`ffmpeg` and `ffprobe`). WhatsApp extract needs `wtsexporter`. The desktop app looks for both on `PATH`. The Docker vault already includes FFmpeg for playback in the browser.

| Tool | Windows | Linux | macOS |
|------|---------|-------|-------|
| FFmpeg | `winget install -e --id Gyan.FFmpeg` | `sudo apt-get install ffmpeg` | `brew install ffmpeg` |
| wtsexporter | `pipx install "whatsapp-chat-exporter[android_backup,crypt15]"` | same command | same command |

If those commands fail, download FFmpeg from [ffmpeg.org](https://ffmpeg.org/download.html) and `wtsexporter` from [WhatsApp-Chat-Exporter releases](https://github.com/KnugiHK/WhatsApp-Chat-Exporter/releases) or [wts.knugi.dev](https://wts.knugi.dev/). Put the programs on `PATH`.

Confirm the tools are visible:

```bash title="Check helpers"
ffmpeg -version
ffprobe -version
wtsexporter --help
```

## Build from source

Compiling the app and the vault from a git checkout: [Contributing](/vault/developer/contributing/#build-and-run).

## Next

Sign in with **http://127.0.0.1:8080** and the vault username and password, then [Import from a backup](/vault/user/import-from-a-backup/). The desktop app uses the IPv4 address because `localhost` can resolve to IPv6, which a local Docker vault does not listen on.

If Connect fails in a release build (AppImage, `.deb`, `.msi`, or `.dmg`) but `curl http://127.0.0.1:8080/v1/session` answers, the vault’s `[server] cors_origins` list is missing the packaged-app origin. Add all three of these, restart the vault, and try again:

```toml
cors_origins = [
  "tauri://localhost",
  "http://tauri.localhost",
  "https://tauri.localhost",
]
```

Linux AppImage and macOS send `tauri://localhost`. Windows sends `http://tauri.localhost` by default, or `https://tauri.localhost` when the window uses the HTTPS scheme. Adding only the HTTPS origin leaves AppImage Connect failing. Dev builds that load Vite on port 5173 also need `http://localhost:5173` and `http://127.0.0.1:5173`.
