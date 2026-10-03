---
title: Config and accounts
description: Instance config.toml, per-account data paths, and multi-tenant accounts.
---

## Instance config

Copy [`config/config.toml.example`](https://github.com/messagecrate/message-crate/blob/main/config/config.toml.example)
to `config/config.toml` (gitignored).

```toml title="config/config.toml"
[paths]
db = "data/messagecrate.db"
data_dir = "data"
assets_dir = "assets"
assets_converted_dir = "assets_converted"

[server]
bind = "127.0.0.1:8080"
cors_origins = [
  "http://localhost:5173",
  "http://127.0.0.1:5173",
  "https://tauri.localhost",
  "http://tauri.localhost",
  "tauri://localhost",
]
```

- A relative path resolves against the folder above the config file's folder: the repository root for `config/config.toml`. The rule holds for every path in the file (`db`, `data_dir`, `static_dir`) and for the flags that replace one of them (`--db`, `--static-dir`), so `--db data/messagecrate.db` names the same file as `db = "data/messagecrate.db"` whatever directory the command runs in. `serve --data-dir` reads no config file, so there `--data-dir` and `--static-dir` resolve against the directory the server is started in.
- `[server]` is required for `serve`.
- `reset-demo` rebuilds the Demo Account in the database `db` names and never writes the config file.
- `cors_origins` lists origins allowed on top of the three the packaged desktop app runs from (`tauri://localhost`, `http://tauri.localhost`, `https://tauri.localhost`), which the server allows whether or not you name them. The website the server serves is same-origin and needs no entry either, so an empty list is the right setting for most installs. Add the Vite origins (`http://localhost:5173`, `http://127.0.0.1:5173`) when running the dev UI against this server.
- `static_dir` is the folder holding the built website, served at `/`. It defaults to `static`.
- `serve` runs without a config file when given `--data-dir <folder>`: the database is `messagecrate.db` in that folder, the accounts' files sit beside it, and every `[server]` key has its default. `--bind` and `--static-dir` override `bind` and `static_dir`, with or without a config file. The desktop app starts the server this way.
- Source names are **not** listed in TOML — each import registers its own
  source slug for that account in the database. The files on disk carry no
  source: an account keeps one folder of originals and one of previews.

### Which file is the database

The server opens only a Message Crate database, and it changes no other file.
A Message Crate database carries SQLite's `application_id` set to `0x4d734372` (the bytes `MsCr`), which the server writes into every database it builds.
A file without that mark, such as Apple's `chat.db` or another program's backup, is refused by name before any statement changes it, because rebuilding it would drop that program's tables.

Only `serve`, `create-database` and `reset-demo` make a new database, at a path where no file exists or in an empty file.
Every other command (`import`, `imports discard`, `dedupe-cross-source`, `process-assets`, `create-owner`, `reset-owner-password`) stops with an error naming the path when no database is there, so a mistyped `--db` creates nothing.

A Message Crate database stamped with another Schema Fingerprint is still rebuilt empty on open, as after any schema change.

### Server asset limits

`[server]` also accepts one optional upload setting:

| Key | Default | Description |
|-----|---------|-------------|
| `asset_part_size` | `67108864` (64 MiB) | Largest chunk of a multipart upload. Must be greater than 0. Keep under ~100 MiB for Cloudflare-proxied setups. The chunk size a client is told is this or the attachment size limit, whichever is smaller. |

The server refuses a config file that carries a section or key it does not use.
Every command that loads the file, `serve` included, stops with an error that names each unknown key and its section, for example ``[server] has a key the server does not use: `bnd` ``.
Why: a misspelt key would otherwise load as its default, and a removed key would sit in the file looking as though it still held.

The attachment size limit is not a config key, and a file that still sets `[server] asset_max_bytes` is refused with a message saying where the limit is set now.
It is the largest attachment the server accepts, as a single `PUT /v1/assets/{sha256}` body or as the total declared bytes of a multipart upload, and it is also the cap on every other request body.
It is a Server Setting stored in the database: 512 MiB until the Owner changes it under **Server Settings**, or a program with the Owner's Session sends `PATCH /v1/server/settings` with `asset_max_bytes` in bytes.
A change holds from the next upload, with no restart.
A multipart upload already in progress keeps the part size it started with, so lowering the limit does not break it.
`GET /v1/server` reports the limit as `asset_max_bytes` to any client, with no credential, because the desktop app reads it before Staging.

The limit is whatever the Owner set, and a part is never larger than the limit.
A limit below `asset_part_size` is accepted, and the server then hands out parts the size of the limit.
The part size is worked out on each upload, so neither a change to the limit nor an edit to `asset_part_size` can leave a server that does not start.
The server refuses only a limit of zero, or one above 9223372036854775807, with `422 Unprocessable Entity`.

### Logging

The server writes its log to stderr through `tracing`: one `INFO` line per HTTP response with the method, path, status and latency, an `ERROR` line with the full cause chain behind every `500`, and `WARN` lines for work the server could not complete but did not fail the request over. `RUST_LOG` sets the level and accepts the usual filter syntax, for example `RUST_LOG=debug` or `RUST_LOG=message_crate_server=debug,tower_http=info`. Unset, the level is `info`. The `import`, `dedupe-cross-source`, `process-assets` and `reset-demo` subcommands print their progress to stdout as before; that is their output, not the log.

## Per-account asset files

Created on first use if missing:

- `data/<account_id>/assets/`
- `data/<account_id>/assets_converted/`

An attachment file is named by the SHA-256 of its bytes, so a file imported
from two sources is stored once. It is removed when no message of the
account, from any source, still names it.

## Accounts

Rows are scoped by `account_id` in a shared `messagecrate.db`. The owner is
always account `1` and the demo account `2`; every other account takes an id
from `100` up.

- Web login uses username + password (Argon2id hash in `accounts.password_hash`).
  An account may have no password (`password_hash` NULL); an empty password is
  accepted only for those accounts.
- Guesses at one account's password are rate-limited to 20 per 60 seconds:
  every login as the account, under any spelling of its username, and every
  wrong current password sent to change the owner's password or to delete the
  account.
  Creating an account and claiming Message Crate are each limited to 20 attempts
  per 60 seconds across the whole server, whatever the username.
- Each account can create named **API tokens** for programs that call the HTTP API
  (stored hashed; shown once when created). GUI sessions use a separate rotating
  token.
- Four columns on `accounts` govern what a logged-in session may do, each
  enforced by a guard in `server.rs` rather than left as decoration:
  - `disabled` — may not log in; an existing session or API token for a
    disabled account stops working immediately.
  - `can_import`, `can_export`, `can_delete` — may call the import endpoints,
    the export endpoints, and the endpoints that destroy message data,
    respectively. New accounts default to all three. A named API token
    carries import and export only, never delete: permanent deletion is a
    person's act, so it always needs a logged-in session.
  The owner sets another account's flags from Owner Home
  (`PATCH /v1/accounts/{id}`), resets a password
  (`PUT /v1/accounts/{id}/password`), deletes an account's messages
  (`DELETE /v1/accounts/{id}/messages`) or the account
  (`DELETE /v1/accounts/{id}`). The owner has no flags of its own: it holds
  no messages, and nothing can disable or delete it. `create-owner` and
  `reset-owner-password` on the server CLI are the only ways to make or
  recover the owner's login.
- The Demo Account: username `demo`, never a password. `serve` adds it to a
  database that does not exist yet; `reset-demo` rebuilds it; the login card's
  **Explore Demo Account** button logs in as it.

See [Account and Profile](/docs/user/features/settings/account-and-profile/).
