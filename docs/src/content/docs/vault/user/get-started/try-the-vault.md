---
title: Try the vault
description: Look at sample conversations in the browser — click Try it on a hosted vault, or run Docker and sign in as demo.
---

Getting a phone backup takes work. The sample account answers whether search, contacts, and media in the vault are useful before that work starts.

## Hosted vault

If the vault is already on a public URL, open that URL in a browser and click **Try it**. The website is enough.

**Try it** signs in to a private copy of the sample conversations. The copy lasts 24 hours, or until sign-out. Import and Export are not in the browser.

To keep a personal archive on that same vault, create an account and continue at [Use your own messages](/vault/user/get-started/your-own-messages/).

## Self-hosted vault

Connect with the **website**. Importing your own messages later needs the [desktop app](/vault/user/get-started/install-the-desktop-app/) — more on that when you [use your own messages](/vault/user/get-started/your-own-messages/).

Already sure you want your own data? Skip to [Use your own messages](/vault/user/get-started/your-own-messages/).

## Prerequisites

- [Docker Desktop](https://www.docker.com/products/docker-desktop/) on Windows or macOS, or [Docker Engine](https://docs.docker.com/engine/install/) on Linux

## Start the vault

To start, pull the latest published image `bitrealm/message-vault:latest` from Docker Hub.

```bash title="Start with docker run"
docker run -d --name message-vault \
  -p 8080:8080 \
  -e DEMO_DATA=true \
  -v message-vault-data:/app/data \
  bitrealm/message-vault:latest
```

Or with Compose — save [docker/compose.yml](https://github.com/bitrealm-io/message-vault/blob/main/docker/compose.yml) and start it:

```bash title="Start with Compose"
mkdir message-vault && cd message-vault
curl -fsSL -o compose.yml \
  https://raw.githubusercontent.com/bitrealm-io/message-vault/main/docker/compose.yml
docker compose up -d
```

Both commands start the vault and, on first start, generate sample conversations for the `demo` user. The website and the import API share **port 8080**. The `message-vault-data` Docker volume keeps the database between restarts. Compose and `docker run` use that same volume name, so you can switch methods without copying the database.

Edit the Compose file to change the published port, set `DEMO_DATA=false` to skip generating sample conversations, or pin `bitrealm/message-vault:0.9.0` instead of `latest`.

`DEMO_DATA=true` only seeds when the volume is new. Changing the variable later does not add or remove accounts.

With `DEMO_DATA=false` the vault starts unclaimed: it offers **Create Vault Owner** and nothing else until someone claims it. Sample data arrives already claimed, so the sign-in below works straight away.

## Sign in as demo

Open **http://localhost:8080**. Sign in with username `demo` and an empty password.

`demo` is a sample account filled with invented messages, so you can try the vault without making an account of your own. Everything works: browse **Conversations**, open a thread, try [search](/vault/user/how-to/search/), import, export. The demo account cannot delete itself; `reset-demo` puts it back the way it was.

A sample vault also arrives with a vault owner, signed in as `admin` with the password `admin`. The vault owner manages accounts and reads no messages, and those credentials exist only in a sample vault: a real one asks for a username and a password on the Create Vault Owner screen, and the password must be at least eight characters.

## After you have looked around

Sign out. Create your own account on **this same vault**, and continue at [Use your own messages](/vault/user/get-started/your-own-messages/).

## Build from source instead

- Compiling the vault and the desktop app: [Contributing](/vault/developer/contributing/#build-and-run).
- Building a Docker image from a git checkout: [Docker](/vault/developer/docker/).
