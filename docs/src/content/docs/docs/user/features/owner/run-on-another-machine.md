---
title: Run on another machine
description: Run the server on a different computer from the browser and the desktop app, such as a home server, and move an existing Message Crate there.
---

The first-time path runs the server, the browser, and the desktop app on one computer.
This page covers a server on a second computer, such as a home server, reached over the home network.

:::caution[Not tested]
The commands on this page follow Docker's documentation, and the screens are described from the product's source. Nobody on the project has run a Message Crate across two computers this way. A wrong step is worth [an issue](https://github.com/messagecrate/message-crate/issues).
:::

## What changes

Three things differ from the first-time path:

- The server must be published on the network. The first-time command publishes it to its own computer only.
- The browser and the desktop app need the server's address in place of `localhost`.
- A Message Crate that already holds messages has to be moved, by copying its Docker volume.

## Publish the server on the network

The command in [Start your own Message Crate](/docs/user/get-started/start-your-own-message-crate/) has `-p 127.0.0.1:8080:8080`.
The `127.0.0.1` in front means only that computer reaches the server.

Without it, Docker publishes port 8080 on every network the computer is connected to.

```bash title="Start a Message Crate that the network can reach"
docker run -d --name message-crate \
  --restart unless-stopped \
  -p 8080:8080 \
  -v message-crate-data:/app/data \
  bitrealm/message-crate:latest
```

A server that is already running with the first-time command is removed first with `docker stop message-crate` and `docker rm message-crate`.
Both leave the `message-crate-data` volume alone, so the messages stay.

### Only on a trusted network

The server speaks plain HTTP.
Passwords, Session tokens, and messages cross the network unencrypted, so anyone who can listen on that network can read them.

That is acceptable on a home network where every device is trusted.
Anything beyond that, such as reaching the Message Crate from the internet, needs a reverse proxy with TLS in front of the server.
Setting one up is outside this guide.

Two settings matter more once other computers can reach the server:

- The Owner's password is the only thing between the network and every account.
- **Let anyone who can reach this server create their own account**, under [Server Settings](/docs/user/features/owner/owner-home/#server-settings), should stay off, because "anyone" now means every device on the network.

## Open it in a browser

The address is `http://`, the server computer's name or IP address, and `:8080`.
For a server at `192.168.1.20`, that is `http://192.168.1.20:8080`.

The website is served by the server itself, so the browser needs nothing configured.

## Point the desktop app at it

The desktop app looks for the server at `http://127.0.0.1:8080`, which is its own computer.
With the server elsewhere, the login card reads **Disconnected**.

1. On the login card, select **Change server address**. The card changes to **Server Address**.
2. Enter the server's address in **Address**, such as `http://192.168.1.20:8080`. The address must start with `http://` or `https://`.
3. Select **Test**. **Connection Status** reads **Connecting**, then **Connected** or **Disconnected**.
4. Select **Use this address**. The login card returns and reads **Connected**.

**Connection Status** reads **Not tested** for an address that has been typed and not tried yet.
**Use this address** stays unavailable until **Address** holds a different address from the one in use.
**Cancel** returns to the login card and keeps the old address.

### Does the server need to be told about the desktop app?

No.

A server refuses requests from web pages it doesn't know, which is the browser rule called CORS.
The desktop app's own origins, `tauri://localhost`, `http://tauri.localhost`, and `https://tauri.localhost`, are built into the server, so the installed desktop app is allowed whatever the server's configuration says.

The browser needs no entry either, because it loads the website from the same server it then talks to.

The Docker image's configuration lists two more origins, `http://localhost:5173` and `http://127.0.0.1:5173`.
They are for developing Message Crate and play no part here.

## Move an existing Message Crate

Everything a Message Crate holds is in the Docker volume `message-crate-data`: the database and the attachment files.
Moving the Message Crate means copying that volume to the new computer.

Both computers should run the same release.
A newer server that finds an older database layout rebuilds the database empty, as [Update Message Crate](/docs/user/features/owner/update/#when-the-database-layout-changes) describes.

### Save the volume to a file

The server must be stopped first, because a database copied while the server is writing to it can be incomplete.

```bash title="On the old computer: stop the server"
docker stop message-crate
```

The next command starts a temporary container that packs the volume into `message-crate-data.tar.gz` in the current folder.

```bash title="On the old computer: save the volume"
docker run --rm \
  -v message-crate-data:/data \
  -v "$PWD":/backup \
  alpine tar czf /backup/message-crate-data.tar.gz -C /data .
```

`"$PWD"` is the current folder in a Linux or macOS shell.
PowerShell on Windows takes `"${PWD}"` in its place.

The file then goes to the new computer by whatever means is at hand: a USB drive, a network share, or `scp`.

### Restore the volume

The first command makes an empty volume with the same name.
The second unpacks the file into it, from the folder that holds `message-crate-data.tar.gz`.

```bash title="On the new computer: restore the volume"
docker volume create message-crate-data
docker run --rm \
  -v message-crate-data:/data \
  -v "$PWD":/backup \
  alpine tar xzf /backup/message-crate-data.tar.gz -C /data
```

The `docker run` command under [Publish the server on the network](#publish-the-server-on-the-network) then starts the server on the restored volume.

### Check that it worked

1. Open the new address in a browser and log in as the Owner.
2. Open **Dashboard**. **Contents** shows the same counts of messages, attachments, conversations, and contacts as on the old computer.

The old computer's container stays stopped from here on.
Two running copies would each take new imports, and nothing merges them afterwards.

The old volume remains on the old computer until `docker volume rm message-crate-data` deletes it.
