---
title: Troubleshooting
description: What to check when the server can't be reached, a login is refused, the desktop app won't start, or an import stops.
---

Each entry names what is seen and what fixes it.

## Reaching the server

### The login card reads Disconnected

The server isn't answering at the address the browser or the desktop app is using.

1. `docker ps` lists the running containers. `message-crate` must be among them.
2. `docker logs message-crate` shows what the server printed, including why it stopped.
3. In the desktop app, **Change server address** on the login card opens **Server Address**. The **Address** field must hold the server's address, and **Test** checks it. The app starts with `http://127.0.0.1:8080`, which is the same computer.

A server on a different computer needs more than the address.
[Run on another machine](/docs/user/features/owner/run-on-another-machine/) covers it.

### Docker refuses to start the server because the port is in use

Another program holds port 8080.
Changing the first `8080` in the `docker run` command to a free port, such as `-p 127.0.0.1:8090:8080`, moves the server to `http://localhost:8090`.
The desktop app then needs `http://127.0.0.1:8090` under **Change server address**.

### The browser shows Create Owner after an update

The new release changed the database layout, and the server rebuilt the database empty.
[Update Message Crate](/docs/user/features/owner/update/#when-the-database-layout-changes) describes what is gone and the way back.

## Logging in

### "this account is disabled"

The Owner has set the account's **Status** to **Disabled**.
The Owner sets it back to **Active** under **User Accounts**, in the account's [User Settings](/docs/user/features/owner/owner-home/#account).

### An account's password is forgotten

The Owner sets a new one under **User Accounts**, in the account's [User Settings](/docs/user/features/owner/owner-home/#account), with **Change password**.
The Owner doesn't need the old password.

### The Owner's password is forgotten

Nothing in the browser or the desktop app can set the Owner's password, because no account stands above the Owner.
The server program sets it from a command line on the computer that runs Docker.

:::caution[Not tested]
The command is the server's own `reset-owner-password`. Nobody on the project has run it through `docker exec` as written here. A wrong step is worth [an issue](https://github.com/messagecrate/message-crate/issues).
:::

```bash title="Set a new Owner password"
docker exec message-crate \
  message-crate-server reset-owner-password --password 'the-new-password'
```

The command prints the Owner's username and ends every Session the Owner had open.

### There is no Create Account on the login card

The login card offers **Create Account** only when the Owner has turned on **Let anyone who can reach this server create their own account** under [Server Settings](/docs/user/features/owner/owner-home/#server-settings).
With it off, the Owner creates every account with **Add account**.

## Desktop app

### Windows or macOS warns before the first run

The installers are not code-signed yet, so both systems warn that the publisher is unknown.
[Install the desktop app](/docs/user/get-started/install-the-desktop-app/#install) has the steps for each system.

### Import and Export are missing

**Import** and **Export** are in the desktop app only.
The browser doesn't show them.

The Owner doesn't have them in either, because the Owner holds no messages.
Importing needs an account, and that account needs **Import** on under **Message Permissions**.

## Import

### "The backup is encrypted — fill Encryption password."

The iPhone backup was made with encryption, and the Import form's **Encryption password** is empty.
The password is the one chosen when the backup was made.

### "This backup is not encrypted. Clear Encryption password."

The iPhone backup was made without encryption, and the Import form's **Encryption password** holds a value.
Import continues once the field is empty.

### Import can't find wtsexporter

A WhatsApp import runs a separate program, `wtsexporter`, which the desktop app doesn't include.
[WhatsApp](/docs/user/import-sources/whatsapp/#install-wtsexporter) has the install command and says how the app finds the program.

### A WhatsApp import doesn't stop when cancelled

A run can't be cancelled while `wtsexporter` is working. It ends when the program finishes.
[WhatsApp](/docs/user/import-sources/whatsapp/#limits) lists this with the other limits.

### ffmpeg or ffprobe not found

Two of the **Attachments** choices on the Import form run `ffmpeg` and `ffprobe`, which the desktop app doesn't include.
[Attachments and media](/docs/user/features/messages/attachments-and-media/#ffmpeg) has the install commands.

In the desktop app, **Settings → System** has **ffmpeg directory** under **Media**.
Left empty, the app looks on the system `PATH`.
A folder entered there must hold both programs.
The lines under the field report each program as found, with its path, or not found.

## Convert

### "Choose a different output folder."

**Convert** can't write into the folder it reads from.
The message goes away once **Output folder** names a different folder.

## Getting help

Problems not listed here belong in an issue on [GitHub](https://github.com/messagecrate/message-crate/issues).

A useful issue names the operating system, the **Version** shown under **Server Settings**, the kind of backup, and the exact error text.
It must leave out passwords, API Tokens, phone numbers, and message text, because issues are public.
