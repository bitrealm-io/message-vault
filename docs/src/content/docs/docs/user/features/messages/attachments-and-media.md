---
title: Attachments and media
description: What the Attachments setting on the Import form does with photos, videos, and files, what ffmpeg is needed for, and what Obfuscate replaces.
---

The **Attachments** setting on the **Import** form decides what an Import Run does with the photos, videos, audio, and other files in a backup.
The form offers it for **iMessage**, **WhatsApp**, **SMS Backup & Restore**, **GO SMS Pro**, and **SMS Backup+**.
**iMazing** and **OpenExtract** have no **Attachments** setting.

## The four choices

| Setting | What it does | Needs ffmpeg |
|---|---|---|
| **Copy** | Imports every file as it is. The default. | No |
| **Convert** | Rewrites photos, video, and audio into `.jpg`, `.mp4`, and `.mp3`. | Yes |
| **Compress & Convert** | Converts, and also re-encodes files to make them smaller, at the cost of some quality. | Yes |
| **Skip** | Copies no files. The messages are imported, and each attachment shows as `skipped`. | No |

With **Convert** or **Compress & Convert**, an Import Run gains a **Media** stage and a **Media Review** between the Staging Review and Upload.
[Import](/docs/user/features/messages/import/#stages-and-approvals) describes them.

A file that is not a photo, a video, or audio is never changed by either setting.
A `.gif` is never changed either, because converting it to `.jpg` would keep one frame of the animation.

## What Convert produces

| Kind | Result |
|---|---|
| Photo | A `.jpg` at high quality. A file that is already `.jpg` or `.jpeg` is left as it is. |
| Audio | An `.mp3`. A file that is already `.mp3` is left as it is. |
| Video | An `.mp4`. |

A video's picture and sound are copied into the `.mp4` unchanged when ffmpeg can do that.
Only when it can't is the video re-encoded, to H.264 at 30 frames per second with AAC sound.
An HEVC video that is copied stays HEVC inside its `.mp4`, so Convert alone does not make it playable in a browser that can't play HEVC.

## What Compress & Convert produces

| Kind | Result |
|---|---|
| Photo | A `.jpg` at a lower quality than Convert uses. |
| Audio | A mono `.mp3` at 96 kbit/s. |
| Video | An `.mp4`, re-encoded when it is large enough to be worth it. |

A `.jpg` of 500 KB or less and an `.mp3` of 100 KB or less are left as they are.
A larger `.jpg` or `.mp3` is replaced only when the new file comes out smaller.

Three settings appear under **Attachments** when **Compress & Convert** is chosen.
They apply to video only.

| Setting | Options | Default | What it does |
|---|---|---|---|
| **Target resolution** | `720`, `1080`, `4k` | `720` | Caps the longer side of the picture at 1280, 1920, or 3840 pixels. A smaller video is not enlarged. |
| **Max FPS** | A number | `30` | The frame rate of the re-encoded video. |
| **Minimum Video File Size (Megabytes)** | A number | `20` | A video smaller than this is not re-encoded. |

A video is re-encoded to H.265.
H.264 is used when the installed ffmpeg can't write H.265.

Two kinds of video are not re-encoded, and are only copied into an `.mp4` when they aren't one already.
The first is a video under the minimum size.
The second is a video that is already H.265, within the target resolution, at 12 Mbit/s or less.

## ffmpeg

**Convert** and **Compress & Convert** run the programs `ffmpeg` and `ffprobe`, which the desktop app doesn't include.

| Windows | Linux | macOS |
|---|---|---|
| `winget install -e --id Gyan.FFmpeg` | `sudo apt-get install ffmpeg` | `brew install ffmpeg` |

[ffmpeg.org](https://ffmpeg.org/download.html) has downloads for systems those commands don't cover.

The desktop app finds the programs on `PATH`.
When they are somewhere else, [**Settings → System**](/docs/user/features/settings/system/) has an **ffmpeg directory** field under **Media**.
The folder must contain both `ffmpeg` and `ffprobe`.
Under the field, each program reads `Found` with its path, or `not found`.

An Import Run looks for ffmpeg at the Staging Review, not when the run starts, because Staging copies the original files and needs neither program.
When ffmpeg is missing, the review reads `Media needs ffmpeg. Set its folder in Settings, then come back to Import.` and the **Convert media** or **Compress media** button is disabled.
The run keeps waiting at the review until the folder is set.

## The size limit

The desktop app uploads no file larger than 50 MB.
The Staging Review and the Media Review show the limit as **Size limit per file**, and list the files over it under **Files over the limit**.
A file over the limit is not uploaded, and its message shows the attachment as `missing — too large`.

**Compress & Convert** is the setting that can bring a large video under the limit.
The Staging Review estimates which files it will.

## Copy and the browser

With **Copy**, the Message Crate stores each file exactly as the backup held it, and sends those same bytes to the browser.
The server does not convert a file for the browser when it is uploaded or when it is shown.

A photo in HEIC or a video in HEVC, the formats an iPhone uses, therefore shows only in a browser that can display that format itself.
In any other browser it does not show.
Importing with **Convert** stores a `.jpg` in place of the HEIC photo.

## Obfuscate

**Obfuscate**, under **Processing Options (Advanced)**, replaces what a backup says with made-up substitutes before anything is stored.
It exists for sharing an import with someone else, as a demonstration or a bug report, without sharing the messages.

The Import form offers it for **iMessage** with **Platform** set to **iPhone backup**, and for **SMS Backup & Restore**, **GO SMS Pro**, and **SMS Backup+**.
It is not offered for **Mac Messages**, **WhatsApp**, **iMazing**, or **OpenExtract**.

When Obfuscate is on, an Import Run replaces:

- Phone numbers. The country code and the number of digits stay, and the other digits change.
- Email addresses, which become addresses at `example.invalid`.
- The names of people.
- Message text, subjects, and group titles. Letters and digits are replaced one for one, so a message keeps its length, its spaces, and its punctuation. A link becomes a link to `example.invalid`.
- Attachments. A photo becomes one placeholder picture, a video becomes one placeholder video, and every other file, audio included, becomes one placeholder file. File names become `attachment` with the placeholder's extension.

Edit history, link previews, and shared locations are left out.
Reactions stay, with the person who reacted replaced.

Within one Import Run, the same real value always becomes the same substitute.
A phone number that appears in three conversations is the same made-up number in all three.

Obfuscate changes what is imported, not the backup it was read from.
