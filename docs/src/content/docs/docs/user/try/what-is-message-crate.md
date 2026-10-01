---
title: What Message Crate is
description: Message Crate reads conversations out of phone backups and makes them searchable on a server its owner runs.
---

Message Crate pulls conversations out of phone backups and makes them searchable.
It runs on a computer its owner controls.
Nothing is sent to a Message Crate cloud service, because there isn't one.

## Three pieces

- **The server** stores the messages and serves the website. The desktop app carries it and starts it, and it can also run in Docker on a computer that is always on.
- **The website** is where messages are browsed and searched. Any browser that can reach the server opens it.
- **The desktop app** shows the same screens as the website and adds **Import** and **Export**. Import lives in the desktop app because reading a phone backup needs access to files on the computer, which a browser doesn't have.

One running installation is called *a Message Crate*.
It holds one or more accounts, and each account sees only its own messages.

## Who provides the messages

The person whose messages they are.
Message Crate reads a backup that person makes of their own phone.

Why can't Message Crate fetch the messages itself?

Apple, Google, and WhatsApp offer no way for a program to log in and download a message history.
The messages are already in files, though:

- An iPhone's messages are inside the backup that Finder, iTunes, or Apple Devices makes on a computer.
- An Android phone's SMS and MMS can be written to XML files by the SMS Backup & Restore app.
- WhatsApp keeps its own database on the phone.

The desktop app reads those files on the computer where they sit.
Every tool that works with message history works this way, so the backup step is a limit of the phones, not of Message Crate.

## What a Message Crate does with them

- Shows every conversation in one list, whichever phone or app it came from.
- Searches message text, people, dates, and attachments.
- Keeps photos, videos, and other attachments with their messages.
- Groups conversations by contact, so one person's messages from several phone numbers sit together.
- Exports conversations to files again.

## The path from here

The guide has two parts.

**Try Message Crate** takes a few minutes and needs no phone:

1. This page.
2. [Install the desktop app](/docs/user/try/install-the-desktop-app/).
3. [Look around the demo data](/docs/user/try/look-around-the-demo-data/).

**Your own messages** puts real messages into the same Message Crate:

1. [Create the Owner and an account](/docs/user/your-messages/create-the-owner-and-an-account/).
2. Back up the phone: [iPhone](/docs/user/your-messages/back-up-an-iphone/) or [Android](/docs/user/your-messages/back-up-an-android-phone/).
3. [Import the backup](/docs/user/your-messages/import-your-backup/).
4. [Where to go next](/docs/user/your-messages/where-to-go-next/).

The first part answers whether Message Crate is worth the work of the second.

Next: [Install the desktop app](/docs/user/try/install-the-desktop-app/).
