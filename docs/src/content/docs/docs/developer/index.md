---
title: Developer
description: Environment setup, releasing, system design, message transfer, Docker, CLI tools, the HTTP API, formats, and instance internals.
---

These pages are for people who compile the server, run Compose, or call the HTTP API. The [User Guide](/docs/user/) is the try-it and import path.

- [Contributing](/docs/developer/contributing/) — environment setup, tests, pull requests
- [Release](/docs/developer/release/) — how product versions ship
- **Architecture** — [System Design](/docs/developer/design/), [Message Transfer](/docs/developer/message-transfer/), [Common message](/docs/developer/architecture/common-message/)
- [Docker](/docs/developer/docker/) — build the server image from a checkout, and how that relates to the Docker Hub image
- [HTTP API](/docs/developer/reference/api/) — tokens and import flow; [route reference](/docs/developer/rustdoc/http/)
- [Rust crate docs](/docs/developer/rustdoc/) — `cargo doc` HTML for workspace crates (not the HTTP route list)
- [Formats](/docs/developer/formats/) — converter capabilities and mapping tables
- [Config and accounts](/docs/developer/reference/config-and-accounts/) — `config.toml` and local accounts
- [Database](/docs/developer/reference/database/)
- [Export structure](/docs/developer/reference/export-structure/) — JSONL folder layout
- [CSV columns](/docs/developer/reference/csv-columns/)
- [Server CLI](/docs/developer/reference/server-cli/)
