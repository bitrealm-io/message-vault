---
title: Install the desktop app
description: Download the desktop app, get past the unsigned-app warning, and open it. Opening it starts a Message Crate on this computer.
---

The desktop app is the one thing to install.
It carries the server inside it and starts it when the app opens, so a Message Crate is running on this computer for as long as the app is open.

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

## Open the app

The app opens on a card titled **Message Crate**.
On the first start the line under the title reads **Setting up Message Crate for the first time…** for a few seconds, while the server writes the demo data.
Later starts read **Starting Message Crate…** and are quicker.

:::caution[Firewall questions are not documented yet]
Windows or macOS may ask whether Message Crate may accept network connections when the server first starts. What each system really shows hasn't been checked. Declining is fine: the Message Crate is used from this computer only unless [other devices are let in](/docs/user/your-messages/where-to-go-next/#read-from-a-phone-or-another-computer).
:::

## Check that it worked

The line under the title reads **Connected**, and the card shows a **Create Owner** form and an **Explore Demo Account** button.

![The first screen of a new Message Crate: Create Owner, and Explore Demo Account](../../../../../assets/user-guide/login.png)

While the app is open, a browser on this computer shows the same card at [http://localhost:8080](http://localhost:8080).

## Where the messages are kept

The Message Crate keeps its database and attachments in one folder on this computer.
**Settings → System → Open data folder** opens it once logged in.
A copy of that folder is a backup of the Message Crate.

Closing the app stops the server. The folder stays, and the next start picks it up.

What if the card says another program is using port 8080?

The app's server listens on port 8080, and something else on this computer already holds it.
Closing that program and selecting **Try again** starts the server.
When the other program is a Message Crate, such as one [run with Docker](/docs/user/features/owner/run-with-docker/), the app uses it and starts nothing.

Next: [Look around the demo data](/docs/user/try/look-around-the-demo-data/).
