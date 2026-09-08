---
title: Rust doc style
description: How to write rustdoc comments and HTTP API descriptions that developers can use.
---

This page is the standard for `///` and `//!` comments and utoipa annotations in the Rust crates. Workspace rustdoc publishes at [/vault/developer/rustdoc/](/vault/developer/rustdoc/); the HTTP catalog at [/vault/developer/rustdoc/http/](/vault/developer/rustdoc/http/). Utoipa annotations on HTTP handlers become the summaries and descriptions shown in that catalog.

## First sentence states what the item is

Open every doc comment with a single sentence stating what the item is or does. Put examples, rationale, and error notes in later sentences.

- `crates/vault/server/src/api_tokens_api.rs:112` — "`GET /v1/account/api-tokens`" — Bad: the route echo as the opening summary adds nothing; the first sentence should describe the operation.
- `crates/vault/server/src/assets.rs:27–29` — "SHA-256 fingerprint of `data` as 64 lowercase hex digits. SHA-256 is a short fingerprint of the file contents." — Bad: the second sentence restates the first; later sentences must add information.
- `crates/libs/ir/src/lib.rs:469` — "Intermediate message before conversion to [`IrMessage`]." — Good: one sentence states what the type is.

## Module `//!` intros state responsibility

Give every module a `//!` intro that states its responsibility or invariants. Keep any contracts it enumerates (events, formats) complete and current.

- `crates/vault/server/src/operation_lock.rs:1` — "Cross-process exclusion between the HTTP server and database replacement." — Good: two lines state the invariant and the failure mode.
- `src-tauri/src/commands/extract.rs:4` — "Progress is sent back as Tauri events: `extract:log` (one log line), `extract:finished` (a summary string or JSON object), and `extract:error`." — Bad: the event list omits `extract:progress`, which this command emits.
- `src-tauri/src/commands/mod.rs:8` — "Progress, log lines, and errors are sent as Tauri events (`extract:log`, `extract:progress`, `extract:finished`, `extract:error`)." — Bad: the contract omits `extract:issue`, which push.rs emits and the web layer listens for.

## Examples when behavior is non-obvious

Include a concrete example or quantified rationale whenever a field, constant, or parser behaves non-obviously. Name the exact values involved.

- `crates/core/message-vault-io-core/src/progress.rs` — "A setup step before any message is read, such as decrypting an iOS backup or caching chat tables. `step` of `total`, with a short label for the person watching ("Deriving backup keys")." — Good: a concrete example of the value, and the reason the variant is separate from message counts.
- `crates/libs/vault-push/src/run.rs:63` — "Kept far under Cloudflare's ~100 MiB upload cap so a large group chat is split into several requests instead of one giant one that gets rejected." — Good: names the external constraint behind the chosen value.
- `crates/libs/media/src/lib.rs:132` — "Build compress options from CLI-style fields (min_size like `20M`)." — Bad: waves at four parameters as "CLI-style fields" without naming them or documenting the error case.

## No filler, invented terms, or jargon

Never write self-contradictory filler, invented terms, or unexplained jargon in doc comments or `--help` text.

- `crates/libs/ir/src/lib.rs:296` — "Why bytes were not imported (`too_large`, `file_missing`). Absent when present." — Bad: "Absent when present" is self-contradictory and explains nothing about when the field is `None`.
- `crates/libs/vault-push/src/lib.rs` — "Import mode: append adds to existing data and is safe to re-run; replace deletes existing messages for this source, then imports" — Good: replaces the invented term "resume-safe" with a plain statement of what each mode does.
- `crates/libs/ir/src/lib.rs:220` — "Platform identity stored on `handles.service` (not per-message SMS/iMessage/RCS)." — Good: names the distinction a reader would otherwise get wrong, instead of leaving "service" to mean two things.
- `crates/libs/vault-pull/src/http.rs:143` — "Fastmail-style search query. Sent even when empty." — Bad: "Fastmail-style" is unexplained jargon; readers must already know Fastmail's syntax to know what queries are valid.

## Handler docs describe the operation, not the route

Write each HTTP handler's doc comment as plain prose. The summary says what the route does; the description says when and why. Never echo the route path: it adds nothing over the OpenAPI path itself.

- `crates/vault/server/src/profile.rs:47` — "`GET /v1/account/profile`" — Bad: the entire doc comment is the route path; zero information added. The same pattern repeats at `crates/vault/server/src/profile.rs:202`.
- `crates/vault/server/src/auth.rs:338` — "`POST /v1/auth/register` — create an account and return an API token." — Bad: repeats the route verbatim before the one-line hint.
- `crates/vault/server/src/api_tokens_api.rs:112` — "`GET /v1/account/api-tokens`" — Bad: route echo flagged ECHOED-ROUTE-SUMMARY by the audit; describe what the endpoint does instead.

## No `# Errors` sections in OpenAPI descriptions

Keep `# Errors` rustdoc sections out of handler docs that become OpenAPI descriptions. Fold failure cases into the description prose.

- `crates/vault/server/src/api_tokens_api.rs:112` — "`GET /v1/account/api-tokens` … `# Errors` … Returns an API error when the caller is not a signed-in session or the list cannot be loaded." — Bad: the heading leaks rustdoc boilerplate into the OpenAPI description (flagged ERRORS-SECTION-IN-DESCRIPTION).
- `crates/vault/server/src/profile.rs:202` — "`PATCH /v1/account/profile` … `# Errors` … Returns an API error when the caller is not a signed-in session, a handle service is unsupported, or the update fails." — Bad: same boilerplate; describe the error cases as prose instead.

## Cover every public item

Document every public item — type, variant, field, const, function. Add `#![warn(missing_docs)]` so undocumented pub items fail the build.

- `crates/libs/sbr/src/read.rs:35` — "pub fn_attr: String," — Bad: no doc, and the name misreads as "function attribute" instead of the XML `fn` attribute it holds.
- `crates/libs/media/src/tools.rs:63` — "pub struct FfmpegToolsProbe {" — Bad: a re-exported public type with no doc comment at all, despite being the GUI's probe result type.
- `crates/libs/vault-pull/src/run.rs:22` — "pub const DEFAULT_PAGE_LIMIT: usize = 100;" — Bad: no doc while the very next const has one; forgotten rather than deliberate.
- `crates/core/message-vault-io-core/src/exporters.rs:373` — "Packaging format projected from the common message (`json` default)." — Good: the lone documented field of ~32 in the GUI-facing `Form` struct; the violation at scale.

## Say when a `Result` never errors

When a function returns `Result` but never errors, say so explicitly and explain why the `Result` type exists.

- `crates/core/message-vault-io-core/src/process.rs:159` — "# Errors … The `Result` is for a stable GUI API; this method currently always returns `Ok`." — Good: an honest error section for an infallible `Result`, so callers are not misled.
- `crates/exporters/go-sms-pro-exporter/src/emit.rs:530` — "Currently always returns `Ok`. The `Result` matches the other exporters." — Good: explains the surprising infallible-Result shape and its cross-exporter consistency.

## Document the reason behind non-obvious choices

Document the reason behind non-obvious choices — ordering, omitted files, performance tradeoffs — so future readers do not "fix" them.

- `crates/vault/demo-seed/src/assets.rs:66` — "One path, `attachments/missing-file.heic`, is left out on purpose so import can show a missing-file warning." — Good: a deliberate-looking omission stated explicitly.
- `crates/vault/server/src/assets.rs:59` — "Used only when streaming an authenticated download. … Hashing the whole file first would read every download twice." — Good: the performance tradeoff is explained instead of just describing the lookup.
- `crates/libs/vault-push/src/run.rs:16` — "Attachments first, then messages. Messages point at attachments by a content fingerprint (sha256). The vault must already have that file, or the import would fail." — Good: explains the invariant that drove the upload ordering.

## Link to real documentation

Point doc links at existing documentation and use [`Item`] rustdoc references. Never use stale relative paths or plain-text mentions.

- `crates/libs/ir/src/lib.rs:7` — `"See the [message-ir architecture](../../../docs/maintainers/architecture/message-ir.md)."` — Bad: broken link; the target moved to the docs site, and rustdoc warns on the unresolved relative link.
- `crates/core/message-vault-io-core/src/exporters.rs:262` — "Matching `message-media` mode used by FormatSink." — Bad: names a flag that does not exist (the flag is `--media-mode`) and leaves FormatSink as plain text instead of a [`FormatSink`] link.
- `crates/core/message-vault-io-core/src/config.rs:20` — "Per-conversation folder of `.eml` files (see https://bitrealm.io/vault/developer/formats/mail-archive/)." — Good: points at real documentation instead of restating the variant name.
