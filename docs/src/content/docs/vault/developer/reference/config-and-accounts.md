---
title: Config and accounts
description: Instance config.toml, per-account data paths, and multi-tenant accounts.
---

## Instance config

Copy [`config/config.toml.example`](https://github.com/bitrealm-io/message-vault/blob/main/config/config.toml.example)
to `config/config.toml` (gitignored).

```toml title="config/config.toml"
[paths]
db = "data/vault.db"
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
- `cors_origins` lists origins allowed on top of the three the packaged desktop app runs from (`tauri://localhost`, `http://tauri.localhost`, `https://tauri.localhost`), which the server allows whether or not you name them. The website the vault serves is same-origin and needs no entry either, so an empty list is the right setting for most installs. Add the Vite origins (`http://localhost:5173`, `http://127.0.0.1:5173`) when running the dev UI against this vault.
- Source names are **not** listed in TOML — each import registers its own
  source slug for that account under `data/<account_id>/<source_id>/`.

### Server asset limits

`[server]` also accepts optional upload limits. Both keys default to sensible values for most installs:

| Key | Default | Description |
|-----|---------|-------------|
| `asset_max_bytes` | `536870912` (512 MiB) | Maximum size for one attachment — a single `PUT /v1/assets/{sha256}` body or the total declared bytes for a multipart upload. Must be greater than 0. |
| `asset_part_size` | `67108864` (64 MiB) | Chunk size advertised to clients for multipart uploads. Must not exceed `asset_max_bytes`. Keep under ~100 MiB for Cloudflare-proxied setups. |

The environment variable `VAULT_ASSET_PART_SIZE` can override the advertised part size at runtime (clamped to `1..=asset_part_size`). Set it before starting the server. This is a test and operations knob — use `config.toml` for normal configuration.

Web env overrides (optional): `VAULT_DB`, `VAULT_DATA_DIR`.

### Logging

The server writes its log to stderr through `tracing`: one `INFO` line per HTTP response with the method, path, status and latency, an `ERROR` line with the full cause chain behind every `500`, and `WARN` lines for work the server could not complete but did not fail the request over. `RUST_LOG` sets the level and accepts the usual filter syntax, for example `RUST_LOG=debug` or `RUST_LOG=message_vault_server=debug,tower_http=info`. Unset, the level is `info`. The `import`, `dedupe`, `process-assets` and `reset-demo` subcommands print their progress to stdout as before; that is their output, not the log.

## Per-account asset files

Created on first use if missing:

- `data/<account_id>/<source_id>/assets/`
- `data/<account_id>/<source_id>/assets_converted/`

## Accounts

Rows are scoped by `account_id` in a shared `vault.db`. The vault owner is
always account `1` and the demo account `2`; every other account takes an id
from `100` up.

- Web login uses username + password (Argon2id hash in `accounts.password_hash`).
  An account may have no password (`password_hash` NULL); an empty password is
  accepted only for those accounts.
- Signing in, creating an account, and claiming the vault are each
  rate-limited to 20 attempts per username per 60 seconds, tracked separately.
- Each account can create named **API tokens** for programs that call the HTTP API
  (stored hashed; shown once when created). GUI sessions use a separate rotating
  token.
- Four columns on `accounts` govern what a signed-in session may do, each
  enforced by a guard in `server.rs` rather than left as decoration:
  - `disabled` — may not sign in; an existing session or API token for a
    disabled account stops working immediately.
  - `can_import`, `can_export`, `can_delete` — may call the import endpoints,
    the export endpoints, and the endpoints that destroy message data,
    respectively. New accounts default to all three; a named API token
    defaults to import and export but not delete, since destruction is
    asked for rather than inherited.
  The vault owner sets another account's flags from its console
  (`PATCH /v1/accounts/{id}`), resets a password
  (`PUT /v1/accounts/{id}/password`), deletes an account's messages
  (`DELETE /v1/accounts/{id}/messages`) or the account
  (`DELETE /v1/accounts/{id}`). The owner has no flags of its own: it holds
  no messages, and nothing can disable or delete it. `create-owner` and
  `reset-owner-password` on the server CLI are the only ways to make or
  recover the owner's login.
- Demo seed identity: username `demo` (`crates/vault/demo-seed/config/seed.toml`), always
  no-password. Sign-in stays username `demo` and an empty password.

See [Settings](/vault/user/how-to/settings/).
