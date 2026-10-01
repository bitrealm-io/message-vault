---
title: Update Message Crate
description: Replace the server with a newer release while keeping the data volume, update the desktop app, and what happens when a release changes the database layout.
---

A Message Crate the desktop app starts is updated by installing the new desktop app: the server is inside it.
A Message Crate in Docker has two parts to update, the server and the desktop app.
Both carry the same version number, and both should be updated to the same release.

## Before updating

Two checks come first, because an update can empty the database.

1. The [changelog](https://github.com/messagecrate/message-crate/blob/main/CHANGELOG.md) has an **Upgrading** heading under a release when that release needs something done. A release that changes the database layout rebuilds the database empty, as described [below](#when-the-database-layout-changes).
2. The phone backups the messages were imported from must still be at hand, because they are what a rebuilt Message Crate is filled from again.

For the app's own Message Crate, a copy of the data folder (**Settings → System → Open data folder**), made while the app is closed, allows going back. For Docker, a copy of the `message-crate-data` volume, made as in [Run on another machine](/docs/user/features/owner/run-on-another-machine/#save-the-volume-to-a-file), does the same.

## Update a server in Docker

:::caution[Not tested]
These steps follow Docker's documentation. Nobody on the project has run them. A wrong step is worth [an issue](https://github.com/messagecrate/message-crate/issues).
:::

The steps match the server started in [Run Message Crate with Docker](/docs/user/features/owner/run-with-docker/): a container named `message-crate` on the volume `message-crate-data`.

`docker stop` and `docker rm` remove the running server.
They leave the volume alone, so the messages stay.

```bash title="Remove the old server"
docker stop message-crate
docker rm message-crate
```

`docker pull` downloads the newest release.

```bash title="Download the new release"
docker pull bitrealm/message-crate:latest
```

The same command as the first time starts the new server on the same volume.

```bash title="Start the new server"
docker run -d --name message-crate \
  --restart unless-stopped \
  -p 127.0.0.1:8080:8080 \
  -v message-crate-data:/app/data \
  bitrealm/message-crate:latest
```

A server that was started with a different `-p` value, as in [Run on another machine](/docs/user/features/owner/run-on-another-machine/), is started again with that value.

The server adds demo data only to a volume with no database, so starting on this volume changes nothing it holds.

### Check that it worked

1. Log in as the Owner at [http://localhost:8080](http://localhost:8080).
2. Open **Server Settings**. **Version** shows the new release.

### A fixed release in place of the newest

`latest` is the newest release.
A tag with a version number, such as `bitrealm/message-crate:0.9.0`, stays on that release.
The tag has no `v` in front.

## Update the desktop app

The installers are on the [latest release on GitHub](https://github.com/messagecrate/message-crate/releases/latest), under **Assets**.
[Install the desktop app](/docs/user/try/install-the-desktop-app/) lists which file belongs to which computer.

A desktop app from a different release than the server still connects, and the server serves it.
The Owner sees the difference on the account's **Profile** tab in [Owner Home](/docs/user/features/owner/owner-home/#profile), under **App**.

## When the database layout changes

Message Crate doesn't convert an old database to a new layout.
A server whose database layout differs from the one in the volume rebuilds the database empty when it starts.

How is a rebuild recognised?

- The browser shows **Create Owner**, as it did the first time.
- **Schema fingerprint** under **Server Settings** shows a different number than before the update. The number is the same for every server with the same database layout.
- `docker logs message-crate` shows the line `database schema differs from this server's; rebuilding empty (re-import your data)`.

Everything the database held is gone after a rebuild:

- the Owner and every account,
- every message, conversation, and contact,
- contact names that were typed in, Contact Groups, Message Tags, Saved Searches, and the Trash,
- every API Token.

The way back is the first-time path again: [create the Owner and an account](/docs/user/your-messages/create-the-owner-and-an-account/#create-the-owner), then [import the backups](/docs/user/your-messages/import-your-backup/).
