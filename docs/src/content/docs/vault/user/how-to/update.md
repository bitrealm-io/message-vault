---
title: Update the vault
description: Upgrade the published Docker image and the desktop app without losing the database volume.
---

## Updating the vault (published image)

Stop the container, pull the new image, and start it again on the **same** named volume:

```bash title="Upgrade with docker run"
docker stop message-vault
docker rm message-vault
docker pull bitrealm/message-vault:latest
docker run -d --name message-vault \
  -p 8080:8080 \
  -v message-vault-data:/app/data \
  bitrealm/message-vault:latest
```

The named volume keeps the database and assets. `DEMO_DATA` only controls seeding when the volume is empty, so it does not need to be set during an upgrade. Schema upgrades apply when the server starts.

Copy the database and `data/` directory somewhere safe before you upgrade.

If you started from [docker/compose.yml](https://github.com/bitrealm-io/message-vault/blob/main/docker/compose.yml), upgrade in that same folder:

```bash title="Upgrade with Compose"
docker compose pull
docker compose up -d
```

A release-shaped image from a git checkout: [Docker](/vault/developer/docker/).

## Updating the desktop app

Download the new installer from [GitHub Releases](https://github.com/bitrealm-io/message-vault/releases) and install it over the current app (`.deb` / AppImage, `.msi`, or `.dmg`).

If Import is in use, the `.vault-import-state.jsonl` journal in the work directory is forward-compatible.

## When the database schema changes

Some releases change the shape of the vault's own database rather than the JSONL it imports. When that happens, the server rebuilds its tables empty the first time it starts on the new version, on SQLite and Postgres alike, and the release notes say so. The vault comes back the way it looked the first time you ran it. You create your account again, then import your conversations again from the backups they came from. Anything that lived only in the vault starts fresh too: the contacts you renamed, your tags, whatever you had moved to the trash, and any API tokens you had issued.

The release that introduces Import Runs changes the schema. Upgrading to it starts your vault empty, and you import your conversations again afterward. Keep the backups you imported from where you can get to them: a `chat.db`, an XML export, whatever the source was.

## Compatibility

The vault reads one JSONL schema version at a time, currently version 4. If you import a file written by an older desktop app, the vault refuses it and tells you which version it expected — re-export from the current desktop app and import again.

The Docker tag `latest` points at the most recent release. For a specific version, use `bitrealm/message-vault:0.8.3` (no `v` prefix) or a tag from [Releases](https://github.com/bitrealm-io/message-vault/releases).
