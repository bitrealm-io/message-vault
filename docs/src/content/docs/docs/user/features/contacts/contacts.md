---
title: Contacts
description: Browse contacts in Message Crate and see where their names come from.
---

## Browse contacts

Open **Contacts** in the sidebar to see people and handles discovered from imports.

- Filter the list with the contacts filter control
- Open a contact to see related conversations and details
- Use [search](/docs/user/features/messages/search/) in **Contacts** mode for name and handle queries

Message Crate keeps display names and handles (E.164 phone numbers where possible). It is not a full address-book manager (no synced VCF photos or notes).

## Where a contact's name comes from

A backup is an address book you already curated, so Message Crate takes it at its word. When a backup knows someone's name and the existing contact for them has none, that name goes on the contact. The first backup wins: a later one that spells the name differently does not change it. A name you type yourself, or one you load from an address book file, replaces the name an import gave.

The Import form takes an **Apple Contacts file** for **Mac Messages** only. No other source has a contacts field.

## Contact Groups

A Contact Group collects contacts under a name. **Contact Groups** in the left panel lists them, and `group:` in [search](/docs/user/features/messages/search/) narrows a list to one.

Loading a VCF from the terminal is a Developer command: [Server CLI](/docs/developer/reference/server-cli/).
