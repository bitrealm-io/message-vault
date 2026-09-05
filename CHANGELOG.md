# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

**Dating rule:** every bullet under `[Unreleased]` and under a version heading must
start with an ISO date (`YYYY-MM-DD`), the day the change landed (or is recorded).
Released version headings also carry a date: `## [0.8.0] - 2026-08-24`.

## [Unreleased]

### Added

- 2026-09-05: Permanent delete, from the trash only. **Delete** on a trashed conversation removes it, its messages, and any attachment file no other message still uses; **Delete** on a trashed contact does what a phone's Delete Contact does — the name, edits and Contact Group memberships go, the contact becomes Unknown, and its conversations stay, showing the handle. **Empty Trash** does both for everything in the trash. On the HTTP interface: `DELETE /v1/conversations/{id}` and `DELETE /v1/contacts/{id}` (409 for an item not in the trash) and `DELETE /v1/trash`, each needing a signed-in session whose account may delete. (#314)
- 2026-09-05: Settings → Convert, a desktop-only tool that rewrites a folder of exported files into another format (JSON Lines, JSON, CSV, EML, MBOX, or Android XML) without reading a backup or the vault. The input format is detected from the folder; the output folder must differ from the input, and the screen says so before a run starts.

### Removed

- 2026-09-05: The `[server] asset_hash_threshold_bytes` config key. The vault parsed it and then ignored it, because multipart uploads have verified the SHA-256 fingerprint for every size since the threshold was dropped. A config file that still carries the key no longer loads; delete the line.
- 2026-09-05: The 8-character obfuscation seed. A seed is exactly 64 hex characters, which is the length the exporter prints when it generates one. Shorter seeds were accepted by the run but refused by the desktop form, so a person could never paste a printed seed back in.
- 2026-09-05: The desktop app no longer reads import progress out of the exporters' log lines. `src-tauri/src/commands/progress.rs`, which parsed `…500/12345`, `attachments 2/3 100/500`, and `Preparing N conversation file(s)` with string heuristics, is gone.
- 2026-09-03: The export cursor, the `source=` parameter on `GET /v1/export/messages` and `/count` (write `source:imessage` in the query instead), the `savedSearches` and `savedSearch` fields, the `ok` and `account_ok` flags, and `vault-pull`'s unused `compose_query`.
- 2026-08-31: The legacy Slint desktop GUI (`crates/message-vault-io-gui`) is gone. The Tauri desktop app is the product path. Its screens are recorded, one image per exporter, in `docs/superpowers/reference/legacy-slint-gui.md`.

### Fixed

- 2026-09-03: Importing a file the vault cannot read now answers with a 400 that says what is wrong — which schema version the file has and which the vault reads, or which line is not valid — instead of "internal server error" with the reason on the server's log only.
- 2026-09-03: `POST /v1/import` without a `source` now returns the vault's own JSON error ("query param source is required") instead of the framework's plain-text rejection.
- 2026-08-30: Message Vault Settings no longer reports an address it has not probed. Editing the address after a failed Test used to fall back to the card's own connection — which belongs to the vault already saved — and turn the line green. An address typed but not tried now reads `Not tested`.
- 2026-08-30: The login and profile-setup pages no longer carry a scrollbar on a viewport tall enough to hold the card. The centring page measured a full viewport height plus its own padding, so opening a select, which locks scrolling, shifted the card sideways.
- 2026-08-30: Profile setup refuses an account already in the list, marking the later row rather than the first to carry it. Numbers are compared regardless of formatting, and the same number on Text Message and on WhatsApp stays two accounts.
- 2026-08-30: Profile setup leaves `+ Add account` disabled while the row above it is empty, and re-shows a repeated validation message by clearing the line first, so a second check is visible rather than looking like nothing happened.
- 2026-08-27: Desktop Import opens the vault session as `imessage` or `whatsapp` instead of the Platform method id (`imessage-ios`, `whatsapp-android`, …), so conversation uploads no longer fail with a source mismatch.
- 2026-08-27: Import Errors groups identical `step` + `reason` + `kind` into one row with an `N files` count. Expanding the row lists the filenames. Stored per-file issues are unchanged.

### Changed

- 2026-09-05: The desktop app reads Apple Messages through a separate program, `imessage-reader`, which the installer ships beside the app. The parser it uses (`imessage-database`) is GPL and the app is under the Fair Core License, so the two can no longer be one binary. Nothing changes on screen. A dependency-licence policy (`docs/agents/licences.md`) and a `cargo deny` licence check in the Audit workflow enforce the boundary.
- 2026-09-05: The vault's media conversion is the `media` crate's, the same code the desktop export runs. `--media convert|compress` on import and the browser previews `process-assets` writes now come out of `media::transcode_file_as` instead of a second copy of the ffmpeg recipes inside the server, so the two can no longer drift. The visible differences: previews use the crate's compress recipe (HEVC at up to 1080p with an H.264 fallback, and a source that is already efficient is remuxed rather than re-encoded) where the server used to force H.264 at 720p; ffmpeg is found the way the desktop app finds it (beside the binary, `MESSAGE_VAULT_IO_BIN`, then `PATH`); and `--media` accepts `copy` as the name for the mode the crate calls `clone`.
- 2026-09-05: Exporters report progress as typed events (`ProgressEvent` on `ExporterConfig.progress`: setup step, messages read, attachments staged with bytes, conversation files prepared, media pass) emitted from the shared write layer, and the desktop Import screen draws its progress bar from those. Log lines are for people only, so a wording change can no longer stop the bar. The read row now narrates an encrypted iPhone backup's setup steps ("Deriving backup keys (1/5)") instead of sitting on "Reading backup…", and the queue write path reports conversation files as they land rather than only at the start. `vault-push` sends one `Issue` event per failed or skipped conversation itself, so any consumer sees them, not only the desktop.
- 2026-09-03: Every list on the HTTP interface answers `{items, total, limit, offset}` and takes `offset` and `limit`; a `limit` above 500 or an `offset` above 50 000 is a 400 instead of a silent clamp. Conversation ids are integers. Every failure is `{error}` with the status, including a malformed query parameter, path, or JSON body, an unknown `/v1` path, and a wrong method. No response carries an `ok` flag; acknowledgements with nothing else to say are 204. Saved searches list as `items`, and creating or renaming one answers the row. (ADR-0005)
- 2026-09-03: `GET /v1/export/messages` pages by `offset` and `limit`, reports `total`, and has no offset cap. The desktop Export walks it in pages of 500.
- 2026-08-31: The `/v1/thread-tags` route group is now `/v1/message-tags`, matching the product's Message Tag vocabulary; the OpenAPI tag reads "Message tags" and the backing tables are `message_tags` / `message_tag_members` (vault schema 7; existing databases are rebuilt empty). Saved searches and the `tag:` operator are unchanged.
- 2026-08-30: The vault allows the packaged desktop app's three origins (`tauri://localhost`, `http://tauri.localhost`, `https://tauri.localhost`) without being configured to. A vault built from source ships `cors_origins` commented out, and the desktop app pointed at it failed in a way that reads as an unreachable server while `curl` to the same port succeeded. `cors_origins` still governs every other origin, and `["*"]` is unchanged.
- 2026-08-28: Postgres JSONL staging writes up to 1000 rows per `INSERT` (SQLite stays at the 999-bind cap, about 55 message rows). Same statement shape on both engines.
- 2026-08-27: Import writes staging messages, attachments, and tapbacks in multi-row chunks and reuses sender handle ids for the rest of the import, so JSONL staging is no longer one database call per row.
- 2026-08-27: Import promote runs `ANALYZE` on `messages`, `attachments`, and `tapbacks` before it opens the write transaction, so a later source can use the guid index. `--reset-demo` runs `VACUUM` once after all three sources, dedupe, and media. Analyze or vacuum failure is a warning; the import still succeeds.
- 2026-08-27: Import promote hashes content keys on a thread pool and writes them in multi-row batches. The later dedupe pass only fills missing keys instead of hashing every message again. Server logs print hash and write progress during a long fill.
- 2026-08-27: Desktop Import shows four steps: parse the backup, copy or convert attachments (file count and size), prepare conversation files, then upload. Attachment work no longer appears as an instant second step. Import history stores `attachments_ms` and `prepare_ms` instead of `convert_ms` (vault schema 2; existing databases are rebuilt empty).
- 2026-08-26: Import lists one **iMessage** source with methods Mac Messages, iPhone backup, and Jailbroken iPhone. Mac and jailbreak can set an attachment folder and an Apple Contacts file. Encrypted iPhone backups require the password in the form; the app does not prompt in a terminal. Extract errors for missing paths, leftover password, and missing ffmpeg use the locked Import-language sentences.
- 2026-08-26: iMessage Import labels the method list **Platform** (Mac Messages and iPhone backup). Required paths use a red asterisk; empty optional paths say (Optional). The User Guide Import pages match that form and no longer describe a jailbreak method.
- 2026-08-26: Import lists one **WhatsApp** source with Platform Android or iPhone. Android can decrypt a crypt12/14/15 file in the backup folder with a key; iPhone forwards the Finder backup as `-b`. Optional contacts, media, and message-database paths stay empty when those files already sit in the folder.

## [0.8.3] - 2026-08-25

### Fixed

- 2026-08-25: When `demo-seed` cannot rename the sample inbox into place (Docker overlay `EXDEV`), copy then delete so the release image can finish `cargo run -p demo-seed`.

## [0.8.2] - 2026-08-25

### Fixed

- 2026-08-25: Stop ignoring every `data/` folder in the Docker build context. The release image needs `crates/vault/demo-seed/data/corpus/` so `demo-seed` can write the sample inbox.

## [0.8.1] - 2026-08-25

### Fixed

- 2026-08-25: Copy `vendor/` into the release Docker rust-builder so `cargo` can load the patched `sqlx-sqlite` crate. The `v0.8.0` image build failed without it.

## [0.8.0] - 2026-08-25

### Added

- 2026-08-25: Show a grey / green / red light next to **Server URL** on login. The light probes `GET /health` (this origin when the field is blank) so it is clear whether the vault is reachable before Connect.

### Removed

- 2026-08-25: Drop Extract and Format from the desktop login screen. Login puts Connect or Sign in first, with **Try it** under an OR divider (disabled for now; demo-login code stays). Import after sign-in still stores a backup in the vault. JSONL-on-disk and format conversion stay on the CLI exporters / `message-reexporter`.

### Fixed

- 2026-08-25: Opening a contact from a message thread stubs the overlay handles table from that thread’s participant handles, so the card does not jump from one empty Loading row to the real identity count.
- 2026-08-24: Make the left nav resize grip reachable (it sat under the list column), raise its max width to 520px, and keep the header brand slot aligned while dragging.
- 2026-08-24: Document and ship CORS origins for packaged desktop builds (`tauri://localhost`, `http://tauri.localhost`, `https://tauri.localhost`). Release AppImages were blocked from Connect when the vault only allowed Vite `:5173` origins.

### Changed

- 2026-08-25: Settings → System applies import staging and ffmpeg directory changes
  immediately (no Save button), keeps both path labels on one line with aligned help
  text, renames the field to **ffmpeg directory**, and shows whether `ffmpeg` /
  `ffprobe` were found under Media.
- 2026-08-25: Contact name editor stays at most half the title slot and matches the pencil row height. Click-away, Tab, and blur discard the draft; Enter still saves.
- 2026-08-25: Contacts list shows the “N–M of total” range in a floating pill at the bottom of the panel (always visible), with extra scroll space so the last contact is not covered. Messages still uses the top toolbar label.
- 2026-08-25: Left nav: Messages / Contacts / Trash icons line up with Contact Groups chevrons, and nested group, tag, and saved-search icons line up with section titles. Named saved searches use a hover ellipsis (Rename… / Delete) instead of a trash can. The Thread Tags section is now titled Message Tags.
- 2026-08-25: Contact Groups and Tags menus left-align with their buttons. Contact Groups shows a down chevron, both filter from the search row, and a miss says “No matching groups” instead of “No groups”.

- 2026-08-25: Settings → System uses **Import Staging Directory** (default
  `$HOME/message-vault`) as the real parent for import `staging-*` folders. The
  field no longer nests an extra `message-vault` directory under a custom path,
  and opening a staging folder respects that chosen parent.
- 2026-08-24: Desktop app: closing the window signs out of the vault (best-effort)
  session revoke, then clear the saved login) so the next launch shows sign-in.
- 2026-08-24: Faster HTTP vault-push: skip per-file asset HEAD until this run sees
  `already_present`, raise JSONL import batches from 8 MiB to 64 MiB, let the
  desktop app flush imports on size only, overlap more prepare/upload work, and
  keep 64 idle HTTP connections per host. One preflight HEAD of the first queued
  digest avoids re-uploading a burst of files the vault already has.
- 2026-08-24: Make the Messages/Contacts nav width-draggable, and let the conversation/contact list shrink to nothing when the window is narrow so the thread stays readable.
- 2026-08-24: **Desktop host:** share the cancel/spawn/error scaffolding across the four
  job commands, wire the shared cancel flag into push, drop the runtime
  `MESSAGE_VAULT_IO_BIN` environment writes (sound env access), split the
  extract progress parser into its own module, document the IPC DTO wire
  contract, and gate src-tauri with clippy and tests in CI. Push now honors
  Cancel; the only other product delta is that a KnugiHK binary placed in a
  custom tools folder is no longer found by WhatsApp-Android export.
- 2026-08-23: Server crate cleanup: rustdoc and HTTP API descriptions rewritten, handlers
  moved out of `server.rs`, thread-tag and contact-group CRUD unified, and
  API-token label validation typed. No API behavior change.
- 2026-08-23: **Libraries:** add the `missing_docs` gate to every lib crate and document
  the full public surface, share one `AttachmentMeta` across the IR, CSV,
  and mail layers, switch csv parsers to `anyhow` errors, expose the
  unsafe-attachment-path message as a const, share one test fixture, and
  split the go-sms-mms unit decoders into their own module. No API behavior
  change.
- 2026-08-23: **Exporters:** hoist the duplicated exporter pipeline, CLI driver, output
  preamble, attachment naming, and mechanical helpers into
  `message-vault-io-core` and the shared lib crates, document and gate the
  core config/form surfaces, split the four oversized emit.rs files, and
  wire imessage-ir's previously ignored media flags. CLI help text and
  exported output are unchanged; imessage-ir now honors `--media-mode`
  convert/compress when passed.
- 2026-08-23: **CLI tools:** extract the duplicated JSONL journal and vault HTTP client
  into two shared lib crates, replace substring retry classification with a
  typed classifier (all 4xx failures are permanent), wire demo-seed's
  name-shape and label-name config into the generator, and document the
  dump-cli-docs surface. Retry and truncation edge cases fixed; journal
  files, error text, and the demo dataset unchanged.

### Added

- 2026-08-23: Generated HTTP API route catalog at `/vault/developer/rustdoc/http/`, plus an optional explorer at `/docs` when `[server] openapi_ui` is true
- 2026-08-22: CLI reference pages on the docs site generated from clap
- 2026-08-22: Workspace rustdoc on the docs site at `/vault/developer/rustdoc/`

Installable builds and release notes also appear on
[GitHub Releases](https://github.com/bitrealm-io/message-vault/releases).

The public site summary is at <https://bitrealm.io/changelog/>.
