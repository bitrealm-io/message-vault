---
title: Rust doc style
description: How to write rustdoc comments and HTTP API descriptions that developers can use.
---

This page is the standard for `///` and `//!` comments and utoipa annotations in the Rust crates. Workspace rustdoc publishes at [/vault/developer/rustdoc/](/vault/developer/rustdoc/); the HTTP catalog at [/vault/developer/rustdoc/http/](/vault/developer/rustdoc/http/). Utoipa annotations on HTTP handlers become the summaries and descriptions shown in that catalog.

Each example below names a file and the item inside it, never a line number, because line numbers move with every edit and a stale one is worse than none. When an example stops showing what the page says it shows, replace it with one that does.

## First sentence states what the item is

Open every doc comment with a single sentence stating what the item is or does. Put examples, rationale, and error notes in later sentences.

- `crates/vault/server/src/server.rs`, `fn is_asset_download` — "`GET /v1/assets/{sha256}`: the asset's own bytes, in its own media type." — Bad: the function answers whether a request is an asset download, and the opening names the route and what it serves instead.
- `crates/vault/server/src/assets_api.rs`, `fn sha256_hex` — "SHA-256 fingerprint of `data` as 64 lowercase hex digits. SHA-256 is a short fingerprint of the file contents." — Bad: the second sentence restates the first; later sentences must add information.
- `crates/libs/ir/src/lib.rs`, `struct PendingMessage` — "Intermediate message before conversion to [`IrMessage`]." — Good: one sentence states what the type is.

## Module `//!` intros state responsibility

Give every module a `//!` intro that states its responsibility or invariants. Keep any contracts it enumerates (events, formats) complete and current.

- `crates/vault/server/src/operation_lock.rs`, the module intro — "Cross-process exclusion between the HTTP server and database replacement." — Good: the first line states the invariant, and the next paragraph names the two commands it keeps apart and how long the lock is held.
- `src-tauri/src/commands/mod.rs`, the module intro — "Progress, log lines, issues, and errors are sent as Tauri events (`extract:log`, `extract:progress`, `extract:issue`, `extract:finished`, `extract:error`)." — Good: the list names every event the commands emit, including `extract:issue` from `push.rs`, which the web layer listens for. The names themselves live as constants in `src-tauri/src/commands/events.rs`, so a new event has one place to be added and one list to be kept current.
- `src-tauri/src/commands/extract.rs`, the module intro — "Progress is sent back as Tauri events: `extract:log` (one human-readable log line), `extract:progress` (one typed [`ExtractProgressEvent`] …), `extract:finished` (a summary string or JSON object), and `extract:error` ([`ExtractErrorEvent`])." — Good: each event says what its payload is, and the payload types are rustdoc links.

## Examples when behavior is non-obvious

Include a concrete example or quantified rationale whenever a field, constant, or parser behaves non-obviously. Name the exact values involved.

- `crates/core/message-vault-io-core/src/progress.rs`, `ProgressEvent::Setup` — "A setup step before any message is read, such as decrypting an iOS backup or caching chat tables. `step` of `total`, with a short label for the person watching ("Deriving backup keys")." — Good: a concrete example of the value, and the reason the variant is separate from message counts.
- `crates/libs/vault-push/src/run.rs`, `const MAX_IMPORT_BODY_BYTES` — "Kept under Cloudflare's ~100 MiB upload cap so a large group chat is split into several requests instead of one giant one that gets rejected." — Good: names the external constraint behind the chosen value.
- `crates/libs/media/src/lib.rs`, `fn compress_options_from_cli` — "Build compress options from CLI-style fields (`min_size` like `20M`)." — Bad: waves at four parameters as "CLI-style fields" without naming them, and there is no command line for the fields to be in the style of; only `min_size` is explained, in the error section.

## No filler, invented terms, or jargon

Never write self-contradictory filler, invented terms, or unexplained jargon in doc comments.

- `crates/libs/ir/src/lib.rs`, `IrAttachment::missing_reason` — "None when the attachment was imported; set only when bytes were skipped, to one of a closed set: `file_missing`, `too_large`, `not_copied`, `convert_failed: <detail>`, or `unknown: <raw>`." — Good: says exactly when the field is `None` and lists the values it can hold. It replaced "Absent when present", which was self-contradictory and explained nothing.
- `crates/libs/vault-api-types/src/lib.rs`, `ImportMode::Append` — "Keep existing messages and add only new ones. The default, and what the HTTP API assumes when a request names no mode: it never removes anything." — Good: a plain statement of what the mode does, instead of an invented term such as "resume-safe".
- `crates/libs/vault-push/src/run.rs`, `VaultPushConfig::mode` — "`Append` adds to existing data; `Replace` clears then imports (with force)." — Bad: "with force" is unexplained; the reader cannot tell what is forced or what the alternative would be.
- `crates/libs/ir/src/lib.rs`, `enum HandleService` — "Platform identity stored on `handles.service` (not per-message SMS/iMessage/RCS)." — Good: names the distinction a reader would otherwise get wrong, instead of leaving "service" to mean two things.

## Handler docs describe the operation, not the route

Write each HTTP handler's doc comment as plain prose. The summary says what the route does; the description says when and why. Never echo the route path: it adds nothing over the OpenAPI path itself.

- `crates/vault/server/src/api_tokens_api.rs`, `fn list_api_tokens` — "List the account's named API tokens with their permissions and masked secrets." — Good: the summary says what comes back, and the path appears only once, in the OpenAPI path.
- `crates/vault/server/src/accounts_api.rs`, `fn update_account` — "Change an account. Its display name, time zone and handles are set by the account itself or by the vault owner; only the vault owner sets an account's disabled flag and its import, export and delete permissions." — Good: the summary names the operation, and the description says who may change what.

## No `# Errors` sections in OpenAPI descriptions

Keep `# Errors` rustdoc sections out of handler docs that become OpenAPI descriptions. Fold failure cases into the description prose.

- `crates/vault/server/src/conversations_api.rs`, `fn delete_conversation` — "Trash is the only door to deletion, so a conversation that is not in the trash answers 409 rather than being deleted from wherever it was." — Good: the failure case and its reason are description prose, so the reference reads correctly.
- `crates/vault/server/src/trash_api.rs`, `fn remove_orphaned_files` — "# Errors … `Internal` when a file exists and cannot be removed, or when a stored path would escape the directory it belongs under." — Good: a `# Errors` section belongs on a function that is not a handler, because rustdoc is the only place it appears.

## Cover every public item

Document every public item — type, variant, field, const, function. The workspace `Cargo.toml` and `src-tauri/Cargo.toml` set `missing_docs = "warn"` under `[lints.rust]`, and Clippy runs with `-D warnings`, so an undocumented item on a crate's public surface fails CI. The lint does not see `pub` items in a private module or `pub(crate)` items, so those still need a reader.

- `crates/vault/demo-seed/src/personas.rs`, `const EMPTY_GROUP_HANDLE`, `const EMPTY_THREAD_HANDLE`, `const ORPHAN_SENDER` — no doc on any of the three, and `struct Roster` just above them documents the struct but none of its fields. Bad: the module is private, so the lint is silent, and nothing says why an empty group, an empty thread, and an orphaned sender exist in the demo data.
- `crates/libs/vault-pull/src/http.rs`, `struct ExportMessagesArgs` — `export_id` is documented and `base_url`, `key`, `limit`, `offset` are not. Bad: forgotten rather than deliberate, and `pub(crate)` keeps it out of the lint.
- `crates/libs/sbr/src/read.rs`, `MmsPart::filename_attr` — "Filename from the XML `fn` attribute (not a function attribute)." — Good: the field used to be `fn_attr` with no doc; the rename and the doc together say what it holds and head off the misreading.
- `crates/core/message-vault-io-core/src/exporters.rs`, `struct Form` — every one of its fields carries a doc, for example "Packaging format projected from the common message (`json` default)." on `output_format`. Good: the GUI-facing form is where an undocumented field costs the most, because the desktop app is built against it.

## Say when a `Result` never errors

When a function returns `Result` but never errors, say so explicitly and explain why the `Result` type exists.

- `crates/libs/ir-format/src/format_sink.rs`, `FormatSink::open` — "# Errors … Currently always succeeds. The `Result` matches the other constructors." — Good: an honest error section for an infallible `Result`, with the reason the type is there.
- `crates/libs/ir-format/src/format_sink.rs`, `FormatSink::write_document` — "# Errors … Currently always succeeds." — Bad: says it never errors but not why it returns `Result`, so a reader cannot tell whether the type is a trait obligation or an oversight.

## Document the reason behind non-obvious choices

Document the reason behind non-obvious choices — ordering, omitted files, performance tradeoffs — so future readers do not "fix" them.

- `crates/vault/demo-seed/src/assets.rs`, `fn write_attachment_blobs` — "One path, `attachments/missing-file.heic`, is left out on purpose so import can show a missing-file warning." — Good: a deliberate-looking omission stated explicitly.
- `crates/vault/server/src/assets_api.rs`, `fn lookup_by_sha256_unverified` — "Used only when streaming an authenticated download. … Hashing the whole file first would read every download twice." — Good: the performance tradeoff is explained instead of just describing the lookup.
- `crates/libs/vault-push/src/run.rs`, the module intro — "Attachments first, then messages. Messages point at attachments by a content fingerprint (sha256). The vault must already have that file, or the import would fail." — Good: explains the invariant that drove the upload ordering.

## Link to real documentation

Point doc links at existing documentation and use [`Item`] rustdoc references. Never use stale relative paths or plain-text mentions.

- `crates/libs/ir/src/lib.rs`, the module intro — "See the [common message](https://bitrealm.io/vault/developer/architecture/common-message/) page." — Good: an absolute link to the published page. It replaced a relative path into `docs/maintainers/`, which had moved, so rustdoc warned on it and readers landed nowhere.
- `crates/core/message-vault-io-core/src/exporters.rs`, `AttachmentMedia::media_mode` — "The `media::MediaMode` this GUI choice maps to (the same mode the `--media-mode` CLI flag selects)." — Bad: names a command-line flag that does not exist (no crate but the vault server has a command line), and leaves `media::MediaMode` as plain text instead of a [`MediaMode`] link.
- `crates/core/message-vault-io-core/src/config.rs`, `OutputFormat::Eml` — "Per-conversation folder of `.eml` files (see https://bitrealm.io/vault/developer/formats/mail-archive/)." — Good: points at real documentation instead of restating the variant name.
