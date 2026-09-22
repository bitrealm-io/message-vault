[![Issues][issues-shield]][issues-url]
[![project_license][license-shield]][license-url]
[![source_available][source-available-shield]][license-url]

<a id="readme-top"></a>

<br />
<div align="center">
  <a href="https://github.com/bitrealm-io/message-vault">
    <img src="docs/img/vault_icon.png" alt="Message Vault icon" width="250" height="250">
  </a>

<h1 align="center">Message Vault</h1>
  <p align="center">
    Your chat history, in a vault you run yourself.
    <br />
    <br />
    <a href="https://bitrealm.io/vault/user/"><strong>Explore the docs »</strong></a>
    <br />
    <a href="https://bitrealm.io/vault/user/get-started/try-the-vault/">Try the vault</a>
    &middot;
    <a href="https://github.com/bitrealm-io/message-vault/issues/new?labels=bug&template=bug_report.md">Report Bug</a>
    &middot;
    <a href="https://github.com/bitrealm-io/message-vault/issues/new?labels=enhancement&template=feature_request.md">Request Feature</a>
  </p>
</div>

<!-- TABLE OF CONTENTS -->
<details>
  <summary>Table of Contents</summary>
  <ol>
    <li><a href="#about-the-project">About The Project</a></li>
    <li><a href="#who-the-project-is-for">Who The Project Is For</a></li>
    <li><a href="#getting-started">Getting Started</a></li>
    <li><a href="#contributing">Contributing</a></li>
    <li><a href="#additional-documentation">Additional documentation</a></li>
    <li><a href="#license">License</a></li>
    <li><a href="#project-status">Project Status</a></li>
    <li><a href="#maintainers">Maintainers</a></li>
    <li><a href="#related-projects">Related Projects</a></li>
  </ol>
</details>

## About The Project

[![Docker][Docker]][Docker-url] [![React][React.js]][React-url] [![Rust][Rust-dev]][Rust-url] [![SQLite][SQLite]][SQLite-url] [![Tauri][Tauri]][Tauri-url] [![Vite][Vite]][Vite-url]

Message Vault copies your conversations out of chat apps and phone backups — iMessage, WhatsApp, Android SMS — and stores them in a self-hosted, searchable archive that you control. Read old threads in a browser, search years of messages, and export them as ordinary files whenever you like.

### Project Details

The Message Vault software has three parts:

- **Backend** - The core system that runs on your computer. It keeps you signed in, stores your messages, and powers the search feature.
- **Desktop App** - A program that imports your messages into the vault from phone backups and app exports. You can also view, organize, and export your messages from here.
- **Website** - A read-only version of the desktop app running in a web browser.

You can bring in:

- Apple Messages from an iPhone backup, or from Messages on a Mac
- Android texts and picture messages from an SMS Backup & Restore file
- WhatsApp from an iPhone backup or from WhatsApp's Android files

A few older export formats still work if that is all you have left.

Once messages are in the vault you can:

- Read threads the way you would in an app or on a phone, including group chats. Photos, videos, and other attachments are included.
- Search across years of conversations
- Save a copy back out as ordinary files if you want a folder on disk
- Combine texts from more than one phone or app into one archive

## Who The Project Is For

This project is for people who want a personal copy of their phone messages. That includes anyone replacing a phone, leaving a chat app, or keeping a long-term archive of texts.

## Getting Started

Follow the [User Guide](https://bitrealm.io/vault/user/get-started/what-is-message-vault/) to run the demo and import your own data.

The [Developer Guide](https://bitrealm.io/vault/developer/) covers setting up a local development environment and compiling from source.

## Contributing

Contributions are welcome. The [Contributing guide](https://bitrealm.io/vault/developer/contributing/) covers the development environment, running the code, and how pull requests work.

## Additional documentation

Most documentation lives in the guidebook at [bitrealm.io](https://bitrealm.io):

- [User Guide](https://bitrealm.io/vault/user/)
- [Developer Guide](https://bitrealm.io/vault/developer/) — including [Architecture](https://bitrealm.io/vault/developer/vault-design/) (Vault Design, Message Transfer, Common message)

## License

Message Vault is **source-available, not open source**. It is distributed under the [Fair Core License 1.0](LICENSE.md) (`FCL-1.0-ALv2`): you can read the code, change it, build it, and run it for yourself, but you may not offer it as a product that competes with Message Vault. Two years after each version is released, that version becomes available under the Apache License 2.0.

See [LICENSE.md](LICENSE.md) for the full terms.

## Project Status

This project is currently under heavy development and moving towards a v1.0.0 release.

## Maintainers

Matt Beisser - [vault@bitrealm.io](mailto:vault@bitrealm.io)

## Related Projects

- [ChatLab](https://github.com/ChatLab/ChatLab) - Local-first tool that analyzes chat history with AI.
- [Discord Export](https://discordexport.com/discord-user-list) - Hosted helpers for pulling Discord data, including a server member list.
- [DiscordChatExporter](https://github.com/tyrrrz/discordchatexporter) - Exports Discord channel history to HTML, JSON, CSV, and other files.
- [iMazing](https://imazing.com/) - Manages iOS devices and can export message threads from an iPhone or iTunes backup.
- [imessage-exporter](https://github.com/ReagentX/imessage-exporter) - Command-line tool that exports Apple Messages from `chat.db` to several text formats.
- [msgvault](https://github.com/kenn-io/msgvault) - Offline archive for email and chat with search and analytics on SQLite and DuckDB.
- [OMA WAP Forum](https://www.openmobilealliance.org/specifications/affiliates/wap-forum) - Specifications for WAP and MMS that some Android SMS apps still encode.
- [OpenExtract](https://www.openextract.app/) - Pulls iMessage, SMS, photos, and voicemail out of a local iPhone backup.
- [SMS Backup & Restore](https://www.synctech.com.au/sms-backup-restore) - Android app from SyncTech that writes SMS and MMS to an XML file.
- [SMS Backup+](https://github.com/jberkel/sms-backup-plus) - Android app that backs SMS and MMS up to an IMAP mailbox (often Gmail).

<p align="right">(<a href="#readme-top">back to top</a>)</p>

[issues-shield]: https://img.shields.io/github/issues/bitrealm-io/message-vault.svg
[issues-url]: https://github.com/bitrealm-io/message-vault/issues
[license-shield]: https://img.shields.io/badge/license-FCL_1.0-blue
[source-available-shield]: https://img.shields.io/badge/source--available-not_open_source-orange
[license-url]: https://github.com/bitrealm-io/message-vault/blob/main/LICENSE.md

[React.js]: https://img.shields.io/badge/React-%2320232a.svg?logo=react&logoColor=%2361DAFB
[React-url]: https://reactjs.org/

[Rust-dev]: https://img.shields.io/badge/Rust-%23000000.svg?e&logo=rust&logoColor=white
[Rust-url]: https://rust-lang.org/

[Tauri]: https://img.shields.io/badge/Tauri-24C8D8?logo=tauri&logoColor=fff
[Tauri-url]: https://github.com/tauri-apps/tauri

[Vite]: https://img.shields.io/badge/Vite-646CFF?logo=vite&logoColor=fff
[Vite-url]: https://vite.dev/

[SQLite]: https://img.shields.io/badge/SQLite-%2307405e.svg?logo=sqlite&logoColor=white
[SQLite-url]: https://sqlite.org/

[Docker]: https://img.shields.io/badge/Docker-2496ED?logo=docker&logoColor=fff
[Docker-url]: https://www.docker.com/
