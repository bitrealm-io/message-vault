---
title: Use your own messages
description: Create an account on the same Message Crate you used for demo, then import a real backup.
---

A personal archive is a **second account** on the Message Crate that is already running. Do not delete the Docker volume or start a second container to “go personal.” Log out of `demo`, register, then import.

If the server is not running yet, start it with the command on [Try Message Crate](/docs/user/get-started/try-message-crate/), then come back here instead of logging in as `demo`.

## 1. Create an account

Open **http://localhost:8080** (or log out if you are still `demo`). Create an account with a username and password. That pair is the login for your account.

On first login the site asks for a **display name** and **handles** (phone numbers or emails). Those handles mark messages you sent when you import.

## 2. Prepare a backup

Follow [Prepare a backup](/docs/user/prepare-a-backup/) for your phone and app.

## 3. Install the desktop app

Import is not in the browser. [Install the desktop app](/docs/user/get-started/install-the-desktop-app/), then log in with the same server URL (`http://127.0.0.1:8080`) and the account you just created.

## 4. Import and browse

[Import from a backup](/docs/user/import-from-a-backup/), then [browse your messages](/docs/user/browse-your-messages/).
