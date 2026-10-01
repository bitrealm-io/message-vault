---
title: Contacts and labels
description: Browse contacts in the vault and see where their names come from.
---

## Browse contacts

Open **Contacts** in the sidebar to see people and handles discovered from imports.

- Filter the list with the contacts filter control
- Open a contact to see related conversations and details
- Use [search](/docs/user/how-to/search/) in **Contacts** mode for name and handle queries

The vault keeps display names, handles (E.164 phone numbers where possible), and labels. It is not a full address-book manager (no synced VCF photos or notes).

## Where a contact's name comes from

A backup is an address book you already curated, so the vault takes it at its word. When a backup knows someone's name and the contact the vault has for them has none, that name goes on the contact. The first backup wins: a later one that spells the name differently does not change it. A name you type yourself, or one you load from an address book file, replaces the name an import gave.

Some backup types also accept a contacts file (VCF or contacts CSV) on the Import form.

## Labels

When labels exist for your account, they help organize contacts and can be used in search (`within:label`).

Loading a VCF into the vault from the terminal is a Developer command: [Server CLI](/docs/developer/reference/server-cli/).
