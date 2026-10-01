---
title: Look around the demo data
description: Log in as demo in a browser, try the conversation list, search, and contacts, then delete the demo Message Crate.
---

This step uses a browser only.
The desktop app comes later, in [step 5](/docs/user/get-started/install-the-desktop-app/), because looking at messages doesn't need it.

## Log in as demo

1. Open [http://localhost:8080](http://localhost:8080).
2. Enter `demo` in **Username**.
3. Leave **Password** empty.
4. Select **Log in**.

The `demo` account is the only account that logs in with an empty password.

## Things to try

![The conversation list of the demo account](../../../../../assets/user-guide/demo-messages.png)

**Open a conversation.**
**Messages** in the left panel shows every conversation.
Selecting one opens its messages on the right.
The year buttons above the messages jump to that year, and **Find in conversation** searches inside the one conversation.

**Search.**
The search box at the top takes plain words, and words with a meaning of their own.
`attachment:any` narrows the list to conversations that hold a photo, video, or file.
`name:Carolyn` narrows it to conversations with someone of that name.
[Search](/docs/user/features/messages/search/) lists every search word.

![The conversation list narrowed by the search attachment:any](../../../../../assets/user-guide/demo-search.png)

**Open Contacts.**
A contact gathers one person's phone numbers and email addresses, so their conversations from different apps appear together.

The demo text is sampled from *Pride and Prejudice*, which is why the conversations read oddly.

## Two accounts, and why

Demo data arrives with a second login: the Owner, with the username `admin` and the password `admin`.
The Owner manages accounts and server settings and holds no messages.
Logging in as `admin` shows **Owner Home** in place of the conversation list.
The round button at the top right opens the account menu, which holds **Log out**.

A password of `admin` is acceptable only because this Message Crate holds nothing real.
That is the reason real messages go on a separate Message Crate, where the Owner's password is chosen in [step 4](/docs/user/get-started/start-your-own-message-crate/).

## Delete the demo Message Crate

```bash title="Remove the demo container and its data"
docker rm -f message-crate-demo
docker volume rm message-crate-demo-data
```

The first command stops and removes the server.
The second deletes the demo database.
[http://localhost:8080](http://localhost:8080) no longer answers once both have run.

Next: [Start a Message Crate for real messages](/docs/user/get-started/start-your-own-message-crate/).
