---
title: Attachments and media
description: What the Attachments setting on the Import form does with photos, videos, and files, what ffmpeg is needed for, and what Obfuscate replaces.
---

The **Attachments** setting on the **Import** form decides what happens to the photos, videos, and other files in a backup.

## The four choices

| Setting | What it does | Needs ffmpeg |
|---|---|---|
| **Copy** | Uploads every file as it is. The default. | No |
| **Convert** | Changes photos, video, and audio into formats every browser shows: `.jpg`, `.mp4`, `.mp3`. | Yes |
| **Compress & Convert** | Converts, and also re-encodes video to a smaller size. | Yes |
| **Skip** | Leaves attachments out. Only message text is imported. | No |

**Compress & Convert** adds three settings: **Target resolution** (`720p`, `1080p`, or `4k`), **Max FPS**, and **Minimum Video File Size**, below which a video is left alone.

With **Convert** or **Compress & Convert**, an Import Run gains a **Media** stage and a **Media Review** between the Staging Review and Upload.
[Import](/docs/user/features/messages/import/#stages-and-approvals) describes them.

## ffmpeg

**Convert** and **Compress & Convert** run the programs `ffmpeg` and `ffprobe`, which the desktop app doesn't include.

| Windows | Linux | macOS |
|---|---|---|
| `winget install -e --id Gyan.FFmpeg` | `sudo apt-get install ffmpeg` | `brew install ffmpeg` |

[ffmpeg.org](https://ffmpeg.org/download.html) has downloads for systems those commands don't cover.

The desktop app finds the programs on `PATH`.
When they are somewhere else, **Settings → System** has a field for the folder that holds them.

## Obfuscate

**Obfuscate**, under **Processing Options (Advanced)**, replaces names, phone numbers, message text, and attachments with stable substitutes.
It exists for sharing an import with someone else, as a demonstration or a bug report, without sharing the messages.
The Import form offers it for an iPhone backup and for the Android SMS sources.

When Obfuscate is on:

- Real attachment files are not used. Each is replaced by a placeholder of the same kind.
- The **Attachments** setting has no effect.
- Group titles are replaced, and edit history, link previews, and shared locations are left out.

Obfuscate changes what is imported, not the backup it was read from.
