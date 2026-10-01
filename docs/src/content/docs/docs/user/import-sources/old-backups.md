---
title: Old backups
description: Import a backup that already exists from GO SMS Pro, iMazing, OpenExtract, or SMS Backup+.
---


This page is for a backup that already exists, made years ago by a tool that is no longer the best route.
It has no steps for making a new backup with these tools.
They read incomplete or reverse-engineered formats and may produce less complete results than a current backup, so they are for the case where the old export is the only copy.

On the **Import** source list they appear as **GO SMS Pro**, **iMazing**, **OpenExtract**, and **SMS Backup+**.

:::caution[Prefer a supported backup when possible]
When the phone still exists, a new backup is better: [SMS Backup & Restore](/docs/user/your-messages/back-up-an-android-phone/) over GO SMS Pro or SMS Backup+, an [iPhone backup](/docs/user/your-messages/back-up-an-iphone/) over iMazing Messages CSV, and a [WhatsApp](/docs/user/import-sources/whatsapp/) database over iMazing WhatsApp CSV.
:::

## GO SMS Pro

Reads a backup directory with `gosms_sys*.xml` (SMS) and `I_*.pdu` files (MMS).

- **What you need**: the backup directory, the owner phone number
- **Known gaps**: MMS PDU decoding is best-effort, and many PDU files are empty placeholders

## iMazing

Reads Messages or WhatsApp CSV files from the third-party iMazing backup tool.

- **What you need**: iMazing CSV export files, chat folders, or a full device export tree
- **Known gaps**: CSV format omits information present in a native iPhone backup. Group membership, reactions, and reply threads may be incomplete or absent

## OpenExtract

Reads `all_conversations.csv` or `conversation_*.csv` files from the OpenExtract tool.

- **What you need**: the CSV export folder
- **Known gaps**: identity and attachment information can be limited. Group conversations may not include all participants

## SMS Backup+

Reads offline `.eml` files, not a live email account. SMS Backup+ was an older Android backup app that stored messages in Gmail.

- **What you need**: the `.eml` files from a local backup, owner phone numbers, and owner email addresses
- **Known gaps**: sent-message detection depends on correct owner identity. Some MMS content and group threading may be incomplete

Field-level mapping: [Formats](/docs/developer/formats/).
