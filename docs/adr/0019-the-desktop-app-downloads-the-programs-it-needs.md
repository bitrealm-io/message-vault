# The desktop app downloads the programs it needs and ships none of them

The desktop app runs two programs it does not contain: ffmpeg (with ffprobe),
which converts attachments and makes browser previews, and wtsexporter, which
reads WhatsApp data. Each time the app starts it checks for them and
downloads what is missing into the Tools Directory, in the background, with
no button and no prompt (#1053). Each download is pinned in the app to one
release and one checksum, and a file that doesn't match is refused.
wtsexporter is always the app's own copy. ffmpeg is not downloaded when it is
on `PATH`.

Decided, not built yet: #1053 tracks the work, and until it lands the app
downloads nothing.

## Why

The conversions Message Crate runs ask ffmpeg for `libx264` and `libx265`,
which only a GPL build of ffmpeg has. The repository is under the Fair Core
License, so Message Crate must not hand out that build, in an installer or
from a host of its own. When the person's computer fetches it from a third
party, Message Crate distributes nothing.

A Message Crate the app starts must behave like one Docker runs, and the
Docker image has ffmpeg. An app that waits for the person to install ffmpeg
leaves the two routes different for most people.

A click to get functionality was refused. Most people never open Settings,
and a WhatsApp import that first asks for a download is one more step in the
hardest import there is. Because nobody chooses the download, the pinned
checksum is what stands between the app and a file that was changed on the
way.

ffmpeg on `PATH` is used as it is, because a person who installed ffmpeg has
chosen it, and 80 to 100 MB is a large download to repeat for nothing.
wtsexporter gets no such rule: a `pipx` install whose Python has gone is
still found on `PATH` and fails only when it is run.

## Considered and rejected

**Shipping both in the installer,** as the Apple Messages Reader is. It works
with no internet, and it was rejected because it makes Message Crate a
distributor of a GPL ffmpeg build and adds about 100 MB to every installer.

**Shipping wtsexporter and downloading ffmpeg.** wtsexporter is small and
under the MIT licence, so nothing forbids it. It was rejected because two
programs would then reach the computer by two mechanisms.

**A download button in Settings and on the Import form.** It was rejected
because it is a click to get functionality.

**An LGPL build of ffmpeg.** It could be shipped, and it was rejected because
it has neither encoder the conversions use.

**Keeping the ffmpeg directory setting as an override.** It was rejected
because `PATH` is already the first place the app looks, so the setting
would be a second way to say the same thing.

## Consequences

The first WhatsApp import and the first conversion on a new computer need an
internet connection, and both depend on two GitHub projects keeping their
release files in place: `eugeneware/ffmpeg-static` and
`KnugiHK/WhatsApp-Chat-Exporter`. When a download fails the app says so only
where the program is needed, and the user guide's troubleshooting section
tells a person how to install ffmpeg with a package manager or put either
program in the Tools Directory by hand.

The Tools Directory belongs to the app. A release that pins a newer version
replaces what is there, whoever put it there, and deletes the old file only
after the new one has passed its checksum.

Moving to a newer ffmpeg or wtsexporter is a change to the app: a new pinned
release and checksum for every platform, in one pull request.

ffmpeg from `PATH` is whatever version the person installed, so the app can
run a version no release was tested with.
