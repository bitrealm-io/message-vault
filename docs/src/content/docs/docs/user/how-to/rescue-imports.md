---
title: Rescue imports
description: Limited importers for GO SMS Pro, iMazing, OpenExtract, and SMS Backup+.
---

import { Aside } from '@astrojs/starlight/components';

These sources are rescue paths. They read incomplete or reverse-engineered formats and may produce less complete results than a supported backup. Use them only when the export is the only copy you have.

On the **Import** source list they appear as **GO SMS Pro**, **iMazing**, **OpenExtract**, and **SMS Backup+**.

:::caution[Prefer a supported backup when possible]
Use SMS Backup & Restore XML over GO SMS Pro or SMS Backup+. Use an iPhone backup or `chat.db` over iMazing Messages CSV. Use a native WhatsApp database or backup over iMazing WhatsApp CSV.
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

- **What you need**: the CSV export, plus a contacts file for name resolution
- **Known gaps**: identity and attachment information can be limited. Group conversations may not include all participants

## SMS Backup+

Reads offline `.eml` files, not a live email account. SMS Backup+ was an older Android backup app that stored messages in Gmail.

- **What you need**: the `.eml` files from a local backup, owner phone numbers, and owner email addresses
- **Known gaps**: sent-message detection depends on correct owner identity. Some MMS content and group threading may be incomplete

Field-level mapping: [Formats](/docs/developer/formats/).
