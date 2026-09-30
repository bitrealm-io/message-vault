---
title: Try Message Crate
description: Look at sample conversations in the browser — click Try it on a hosted Message Crate, or run Docker and log in as demo.
---

Getting a phone backup takes work. The sample account answers whether search, contacts, and media in Message Crate are useful before that work starts.

## Hosted Message Crate

If Message Crate is already on a public URL, open that URL in a browser and click **Try it**. The website is enough.

**Try it** logs in to a private copy of the sample conversations. The copy lasts 24 hours, or until logout. Import and Export are not in the browser.

To keep a personal archive there, create an account and continue at [Use your own messages](/docs/user/get-started/your-own-messages/).

## Self-hosted Message Crate

Connect with the **website**. Importing your own messages later needs the [desktop app](/docs/user/get-started/install-the-desktop-app/) — more on that when you [use your own messages](/docs/user/get-started/your-own-messages/).

Already sure you want your own data? Skip to [Use your own messages](/docs/user/get-started/your-own-messages/).

## Prerequisites

- [Docker Desktop](https://www.docker.com/products/docker-desktop/) on Windows or macOS, or [Docker Engine](https://docs.docker.com/engine/install/) on Linux

## Start the server

To start, pull the latest published image `bitrealm/message-crate:latest` from Docker Hub.

```bash title="Start with docker run"
docker run -d --name message-crate \
  -p 8080:8080 \
  -e DEMO_DATA=true \
  -v message-crate-data:/app/data \
  bitrealm/message-crate:latest
```

Or with Compose — save [docker/compose.yml](https://github.com/messagecrate/message-crate/blob/main/docker/compose.yml) and start it:

```bash title="Start with Compose"
mkdir message-crate && cd message-crate
curl -fsSL -o compose.yml \
  https://raw.githubusercontent.com/messagecrate/message-crate/main/docker/compose.yml
docker compose up -d
```

Both commands start the server and, on first start, generate sample conversations for the `demo` user. The website and the import API share **port 8080**. The `message-crate-data` Docker volume keeps the database between restarts. Compose and `docker run` use that same volume name, so you can switch methods without copying the database.

Edit the Compose file to change the published port, set `DEMO_DATA=false` to skip generating sample conversations, or pin `bitrealm/message-crate:0.9.0` instead of `latest`.

`DEMO_DATA=true` only seeds when the volume is new. Changing the variable later does not add or remove accounts.

With `DEMO_DATA=false` the server starts unclaimed: it offers **Create Owner** and nothing else until someone claims it. Sample data arrives already claimed, so the login below works straight away.

## Log in as demo

Open **http://localhost:8080**. Log in with username `demo` and an empty password.

`demo` is a sample account filled with invented messages, so you can try Message Crate without making an account of your own. Everything works: browse **Conversations**, open a thread, try [search](/docs/user/how-to/search/), import, export. The demo account cannot delete itself; `reset-demo` puts it back the way it was.

Sample data also arrives with an owner, logged in as `admin` with the password `admin`. The owner manages accounts and reads no messages, and those credentials exist only with sample data: a real Message Crate asks for a username and a password on the Create Owner screen, and the owner must have a password. Any other account may have one of any length, or none.

## After you have looked around

Log out. Create your own account on **this same Message Crate**, and continue at [Use your own messages](/docs/user/get-started/your-own-messages/).

## Build from source instead

- Compiling the server and the desktop app: [Contributing](/docs/developer/contributing/#build-and-run).
- Building a Docker image from a git checkout: [Docker](/docs/developer/docker/).
