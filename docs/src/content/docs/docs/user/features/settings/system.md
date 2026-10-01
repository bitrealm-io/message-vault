---
title: System
description: What the System tab of Settings holds, the Staging Directory, remembered importer paths, the ffmpeg folder, and the app's version.
---

The **System** tab of **Settings** holds the settings of one installed desktop app.
They are stored on the computer, not in the account, so a second computer has its own.

In a browser the tab shows **About** and nothing else to change.
A line under it says the other settings are available in the desktop app, because they name folders on the computer and a web page can't reach those.

The Owner's own Settings has no **System** tab, because every setting here serves Import and Export and the Owner holds no messages.

## Staging

### Staging directory

**Staging directory** is the folder where Import and Export write their temporary files.
The default is `message-crate` in the person's home folder, shown as `~/message-crate`.

Each job writes into its own folder inside it.
The line under the field gives an example, `~/message-crate/staging-iphone-ios-260809-143022`.

The field takes a typed path or a folder chosen with the picker.
A change is saved as it is typed.

Two kinds of path are not saved, and the default stays in force:

- A relative path, because it would resolve against wherever the app happened to start.
- The root of a file system, such as `/` or `C:`, because a job would then write beside every other folder on the disk.

Emptying the field returns to the default.

### Remember importer paths

**Remember importer paths** is a checkbox, off by default.
When it is on, Import restores the last backup path used for each import source.

## Media

### ffmpeg directory

**ffmpeg directory** names a folder that holds both `ffmpeg` and `ffprobe`.
The field is empty by default, and the app then finds both on the system `PATH`.

The app checks the folder as soon as a path is entered.
Two lines under the field report the result, one per tool:

- A check mark with `Found ffmpeg` and the full path of the program.
- A cross with `ffmpeg not found`.

The folder is saved only when both tools are found in it, because one without the other can't convert media.
A saved folder is applied again each time the app starts.

**Install help** opens [Attachments and media](/docs/user/features/messages/attachments-and-media/), which covers what the two tools are used for and how to install them.

## About

**Version** shows the Build of the app.
A Build made from a release is the Product Version alone, such as `0.9.0`.
Any other Build adds the commit it was built from, such as `0.9.0+343fe0d8`.
A browser tab left open across a server upgrade keeps reporting the Build it loaded.

## Third-party software

This section appears in the desktop app only.

It states that Apple Messages are read by the Apple Messages reader (imessage-reader), a separate program installed beside the app.
That program is free software under the GNU General Public License, version 3 or later.

**Source** and **License** link to the program's source and license for the installed Build.
The browser does not show the section, because the website ships no such program.
