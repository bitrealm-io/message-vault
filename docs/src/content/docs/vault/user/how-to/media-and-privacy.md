---
title: Media and privacy
description: Choose how attachments are handled and whether to replace personal information.
---

Choose attachment and privacy settings on **Import**, **Extract**, or **Format** before starting a run.

## Attachment modes

| Setting | What it does |
|---|---|
| **Copy** | Includes the original media files. The default for a full archive. |
| **Do not copy** | Leaves media out. Only message text is kept. |
| **Convert** | Converts media to common formats — `.jpg`, `.mp4`, or `.mp3`. |
| **Convert & compress** | Re-encodes media with size and quality limits you choose — resolution, frame rate, minimum file size. |

**Convert** and **Convert & compress** need `ffmpeg` and `ffprobe` on `PATH`. Install them with the commands on [Install the desktop app](/vault/user/get-started/install-the-desktop-app/).

When compression is on, you can set:

- Maximum resolution (for example, 1080p)
- Maximum frame rate
- Minimum file size below which videos are not re-encoded
- Whether efficient HEVC video under the limit should be left unchanged

JSONL, JSON, and CSV keep media in an `attachments/` folder. EML, MBOX, and Android XML embed it in the output files.

## Obfuscation

Turn on **Obfuscate** to replace names, phone numbers, message text, and media with stable substitutes. Use this when sharing an export — demonstrations, support reports, or testing.

When obfuscation is on:

- Real attachment files are not copied — the output uses three shared placeholder files (`placeholder.jpg`, `placeholder.mp4`, `placeholder.bin`) based on each attachment's type
- Attachment modes like Copy or Convert are ignored
- Group titles are replaced in full, and edit history, link previews, and shared locations are left out
- You can enter a seed (64 hex characters) to make the same input produce repeatable results every run

Obfuscation changes the output copy, not the source backup.

## Practical defaults

- **Copy** for a private personal archive
- **Convert** or **Convert & compress** only when you need smaller or more compatible media files
- **Do not copy** when message text is enough
- **Obfuscate** before an export leaves your machine
