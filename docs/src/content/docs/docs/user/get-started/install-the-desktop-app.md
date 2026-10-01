---
title: Install the desktop app
description: Download the desktop app, get past the unsigned-app warning, and log in to the Message Crate with the account from step 4.
---

The desktop app shows the same screens as the website and adds **Import** and **Export**.
This step installs it and logs in with the account made in [step 4](/docs/user/get-started/create-the-owner-and-an-account/).

## Download

The installers are on the [latest release on GitHub](https://github.com/messagecrate/message-crate/releases/latest), under **Assets**.

| Computer | File |
|---|---|
| Windows, 64-bit Intel or AMD | `.msi` |
| Linux, Debian or Ubuntu | `.deb` |
| Linux, any other distribution | `.AppImage` |
| Mac with Apple Silicon | `.dmg` |

There is no build for a Mac with an Intel processor.

## Install

The installers are not code-signed yet, so Windows and macOS warn before the first run.
The warning says the publisher is unknown. It doesn't mean the file is damaged.

### Windows

1. Run the `.msi` file.
2. If **Windows protected your PC** appears, select **More info**, then **Run anyway**.

### Linux

For the `.deb` file, `sudo apt install ./<file>.deb` installs the app and what it depends on.

For the AppImage, `chmod +x <file>.AppImage` makes the file runnable, and running it starts the app.

### macOS

:::caution[Not tested on a Mac]
These steps follow Apple's documentation. Nobody on the project has run them on a Mac. A wrong step is worth [an issue](https://github.com/messagecrate/message-crate/issues).
:::

1. Open the `.dmg` file and drag the app to **Applications**.
2. Open the app. macOS refuses the first time.
3. Open **System Settings → Privacy & Security**, scroll to **Security**, and select **Open Anyway**.

## Log in

The app opens on a card titled **Message Crate**.
The line under the title reads **Connected** when the app has found the server.

The app looks for the server at `http://127.0.0.1:8080`, which is this computer, so a Message Crate started with the command in step 2 is found with nothing to change.

1. Enter the username and password of the account from step 4. The Owner's login also works here, but the Owner can't import.
2. Select **Log in**.

What if the line reads **Disconnected**?

The server isn't answering at that address.
`docker ps` lists the running containers, and `message-crate` must be among them.
When the server was started on a different port, **Change server address** opens **Server Address**, where the address is entered and **Test** checks it.

## Check that it worked

The app shows the empty **Messages** list, and the left panel now has **Import** and **Export**, which the browser doesn't show.

Next: back up the phone. [iPhone](/docs/user/get-started/back-up-an-iphone/) or [Android](/docs/user/get-started/back-up-an-android-phone/).
