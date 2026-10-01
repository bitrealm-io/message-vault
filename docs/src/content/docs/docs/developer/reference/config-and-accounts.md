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

- Paths resolve relative to the repo root (parent of `config/`).
- `[server]` is required for `serve`. The demo config comments it out.
- `cors_origins` lists origins allowed on top of the three the packaged desktop app runs from (`tauri://localhost`, `http://tauri.localhost`, `https://tauri.localhost`), which the server allows whether or not you name them. The website the server serves is same-origin and needs no entry either, so an empty list is the right setting for most installs. Add the Vite origins (`http://localhost:5173`, `http://127.0.0.1:5173`) when running the dev UI against this server.
- `static_dir` is the folder holding the built website, served at `/`. It defaults to `static`, relative to the directory the server is started in.
- `serve` runs without a config file when given `--data-dir <folder>`: the database is `messagecrate.db` in that folder, the accounts' files sit beside it, and every `[server]` key has its default. `--bind` and `--static-dir` override `bind` and `static_dir`, with or without a config file. The desktop app starts the server this way.
- Source names are **not** listed in TOML — each import registers its own
  source slug for that account under `data/<account_id>/<source_id>/`.

### Server asset limits

`[server]` also accepts one optional upload setting:

| Key | Default | Description |
|-----|---------|-------------|
| `asset_part_size` | `67108864` (64 MiB) | Chunk size advertised to clients for multipart uploads. Must be greater than 0 and must not exceed the attachment size limit. Keep under ~100 MiB for Cloudflare-proxied setups. |

The server refuses a config file that carries a section or key it does not use.
Every command that loads the file, `serve` included, stops with an error that names each unknown key and its section, for example ``[server] has a key the server does not use: `bnd` ``.
Why: a misspelt key would otherwise load as its default, and a removed key would sit in the file looking as though it still held.

The attachment size limit is not a config key, and a file that still sets `[server] asset_max_bytes` is refused with a message saying where the limit is set now.
It is the largest attachment the server accepts, as a single `PUT /v1/assets/{sha256}` body or as the total declared bytes of a multipart upload, and it is also the cap on every other request body.
It is a Server Setting stored in the database: 512 MiB until the Owner changes it under **Server Settings**, or a program with the Owner's Session sends `PATCH /v1/server/settings` with `asset_max_bytes` in bytes.
A change holds from the next upload, with no restart.
`GET /v1/server` reports the limit as `asset_max_bytes` to any client, with no credential, because the desktop app reads it before Staging.

The server refuses a limit of zero or one below `asset_part_size` with `422 Unprocessable Entity`.
When `asset_part_size` is raised above a limit already stored, `serve` stops at startup and names both numbers.

Web env overrides (optional): `MC_DB`, `MC_DATA_DIR`.

### Logging

The server writes its log to stderr through `tracing`: one `INFO` line per HTTP response with the method, path, status and latency, an `ERROR` line with the full cause chain behind every `500`, and `WARN` lines for work the server could not complete but did not fail the request over. `RUST_LOG` sets the level and accepts the usual filter syntax, for example `RUST_LOG=debug` or `RUST_LOG=message_crate_server=debug,tower_http=info`. Unset, the level is `info`. The `import`, `dedupe`, `process-assets` and `reset-demo` subcommands print their progress to stdout as before; that is their output, not the log.

## Per-account asset files

Created on first use if missing:

- `data/<account_id>/<source_id>/assets/`
- `data/<account_id>/<source_id>/assets_converted/`

## Accounts

Rows are scoped by `account_id` in a shared `messagecrate.db`. The owner is
always account `1` and the demo account `2`; every other account takes an id
from `100` up.

- Web login uses username + password (Argon2id hash in `accounts.password_hash`).
  An account may have no password (`password_hash` NULL); an empty password is
  accepted only for those accounts.
- Logging in is rate-limited to 20 attempts per username per 60 seconds.
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
