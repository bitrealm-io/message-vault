---
title: "Mail archives"
description: "EML and MBOX layout and X-ME headers used when Message Vault writes a mail archive."
---

Design for a human-viewable export: **one folder per conversation**, **one `.eml` per message**, with structured `X-ME-*` headers for machine fidelity. Intended as an archive / interchange path before vault exists. Mail clients can open individual messages; translators can recover SMS, group MMS, and (later) iMessage semantics without relying on CSV.

**Status:** Writer in [`message-mail`](https://github.com/bitrealm-io/message-vault/blob/main/crates/libs/mail/). All GUI exporters support `--format eml` / `mbox`. All exporters (including iMessage via [`imessage-ir-exporter`](https://github.com/bitrealm-io/message-vault/blob/main/crates/exporters/imessage-ir-exporter/)) go backup → [shared conversation structure](/vault/developer/reference/export-structure/) ([`message-ir`](https://github.com/bitrealm-io/message-vault/blob/main/crates/libs/ir/)) → output format (see [message-ir architecture](/vault/developer/architecture/common-message/)). JSON is the default format. iMessage writes extension headers; handwriting attaches SVG. See also [CSV columns](/vault/developer/reference/csv-columns/).

## Goals

- Browseable archive (double-click an `.eml`; folder = conversation)
- Lossless-enough SMS and group MMS relative to today’s SBR / GO SMS Pro / SMS Backup+ CSV cores
- Stable `Message-ID` / guid for re-exports and threading
- Metadata in `X-ME-*` headers (not only human body text)
- Room for iMessage (tapbacks, balloons, parts, edits) without colliding with the SMS model

## Non-goals (this document)

- Vault import/export
- Replacing CSV as the default exporter output
- IMAP sync or SMS Backup+ wire compatibility
- Treating `.mbox` as the preferred packaging (derived export is available; folders of `.eml` remain preferred)
- Replaying send-effect animations or handwriting ink in clients

## Packaging

```text title="EML packaging"
output/
  <conversation-stem>/
    000001_<yyyy-mm-dd>_<hhmmss>_<guid8>.eml
    000002_<yyyy-mm-dd>_<hhmmss>_<guid8>.eml
    ...
```

- **Conversation stem:** same rules as CSV filenames from [`message-csv::conversation_filename`](https://github.com/bitrealm-io/message-vault/blob/main/crates/libs/csv/src/lib.rs), without the `.csv` suffix (e.g. `+15555550101`, `Family_Chat`, `group_+A_+B`).
- **Sequence prefix:** zero-padded decimal in chronological emit order so file browsers sort stably.
- **Timestamp in name:** local wall-clock of the message for skimming (authoritative time is still `Date` / `X-ME-Timestamp-Unix-Ms`).
- **`guid8`:** first 8 hex chars of `X-ME-Guid` (or Message-ID local-part hash) to avoid collisions when two messages share a second.

Each file is one RFC 5322 message. Prefer writing via a MIME builder (e.g. `mail-builder`) when implemented.

### Why not one `.mbox` per conversation

| Concern | Folder of `.eml` | Single `.mbox` |
|---------|------------------|----------------|
| Translate / reprocess | One message = one file | Must parse mbox + `From ` escaping |
| Crash safety | Partial export remains usable | Truncation can corrupt the last record |
| Plus anti-patterns | One message owns its MIME parts | Easier to regress into “fat archive” |
| Large chats | Open one message | Some clients load the whole file |
| Thunderbird “mailbox” UX | Import/drag varies | Often smoother import-as-folder |

**Preferred packaging is a folder of EMLs.** Derived **mboxrd** (`OutputFormat::Mbox` / GUI **MBOX**) is also available: one `<conversation-stem>.mbox` per chat, same MIME/`X-ME-*` payload as the `.eml` files. Outlook has poor native support for both; do not optimize the preferred packaging for Outlook.

### Explicit anti-pattern: SMS Backup+ archive EML

Do **not** pack many SMS into one MIME body and then hand the attachments out in order. Nothing in the mail says which picture belongs to which message, so the pairing can only ever be a guess. One message per file keeps the association exact.

## Lessons from SMS Backup+ (do not repeat)

| Pitfall | Instead |
|---------|---------|
| Multi-message archive EML + FCFS attachment leftover assignment | One `.eml` per message; each MIME part belongs to that message only |
| `*@sms-backup-plus.local` as sole identity | Synthetic addresses with E.164 in the local-part **and** `X-ME-*` handles |
| Chat keyed to owner when address is `owner~peer` | First non-owner peer / full roster in `X-ME-Participants` |
| Archive body times as ambiguous local wall-clock | `Date` in UTC (RFC 5322) + `X-ME-Timestamp-Unix-Ms` |
| Opaque Android type ints alone | Clear `X-ME-Direction` / `X-ME-Message-Kind` (+ optional `X-ME-Android-Type`) |
| Group archives with no roster | Always emit `X-ME-Participants` for groups |
| Dedupe / identity via `X-smssync-id` alone | Stable `X-ME-Guid` / `Message-ID` from content fingerprint or source guid |
| Free-text-only reactions (iMazing-style) | Structured tapback EMLs + headers (see iMessage section) |

Do **not** use the `X-smssync-*` header namespace. This format is not Plus-compatible.

## Container model (every message)

### Standard mail headers

| Header | Rule |
|--------|------|
| `From` / `To` / `Cc` | Browse mapping (below); synthetic `+E164@sms.local`, handle, or `…@chat.local` |
| `Date` | Message timestamp as RFC 5322 **UTC** |
| `Subject` | `Message with {peer name \| group title \| chat id}` — **not** message-body preview; SMS subject stays in `X-ME-Subject` |
| `Message-ID` | Stable, unique, deterministic (see below) |
| `MIME-Version` | `1.0` |
| `Content-Type` | `text/plain` or `multipart/mixed` (or `multipart/related`) when attachments exist |
| `In-Reply-To` / `References` | Set for iMessage replies and tapbacks; **unset** for ordinary SMS |

### Synthetic addresses

- Phone: `+15551234567@sms.local` (E.164 in local-part; `+` allowed in addr-spec via quoting if required by the builder).
- Email / Apple handle: `user=example.com@handle.local` or a documented safe encoding of the raw handle — never name-only as the sole identifier.
- Display name may appear in the phrase (`Alice <+1555…@sms.local>`).

### Message-ID

- Prefer source guid when present (iMessage): `<{apple-guid}@imessage.local>`.
- Otherwise: `<{sha256-fingerprint}@message-vault.local>` matching CSV `guid` construction where possible.
- Must be stable across re-exports of the same logical message.

### From / To / Cc mapping

Browse-oriented so mail-client **Correspondents** / Subject columns stay readable. Machine identity remains in `X-ME-*`.

**1:1 incoming:** `From` = peer (display name when known); `To` = owner as `Me <…>`.

**1:1 outgoing:** `From` = `Me <owner>`; `To` = peer.

**Group incoming:** `From` = actual sender; `To` = one conversation address (`Group Title <sanitized-chat-id@chat.local>`); full roster in `X-ME-Participants` only.

**Group outgoing:** `From` = `Me <owner>`; `To` = same conversation address; roster in `X-ME-Participants`.

Empty owner handle falls back to `me@sms.local` with display name `Me`. Outgoing rows set `X-ME-Sender-*` from owner identity (same as common message / CSV). Owner is always mirrored in `X-ME-Owner-*` when known.

Reverse import (EML/MBOX → common-message JSON) is available via [`message-ir-format`](https://github.com/bitrealm-io/message-vault/blob/main/crates/libs/ir-format/) (`read_conversation_eml_dir` / `read_conversation_mbox`).

## Core `X-ME-*` headers (SMS / MMS / shared)

Prefix: **`X-ME-`** (Message Vault). JSON header values are compact single-line JSON.

| Header | Values | Notes |
|--------|----------------|-------|
| `X-ME-Chat-Identifier` | string | Same role as CSV `chat_identifier` |
| `X-ME-Conversation-Type` | `individual` \| `group` | |
| `X-ME-Group-Title` | string | Empty/absent for 1:1 |
| `X-ME-Participants` | JSON `[{ "handle", "display_name" }]` | **Required for groups**; E.164 preferred for phones |
| `X-ME-Direction` | `incoming` \| `outgoing` | |
| `X-ME-Sender-Handle` | string | Peer or owner (outgoing); omit when unknown |
| `X-ME-Sender-Display-Name` | string | |
| `X-ME-Owner-Handle` | string | Export owner handle |
| `X-ME-Owner-Display-Name` | string | Export owner display (caller-id / `"Me"`) |
| `X-ME-Service` | lowercase common-message vocabulary preferred (`sms` / `imessage` / …) | Older exports may use `SMS` / `iMessage` |
| `X-ME-Message-Kind` | see taxonomy below | |
| `X-ME-Timestamp-Unix-Ms` | integer string | Authoritative epoch ms (UTC) |
| `X-ME-Timestamp-Display-TZ` | optional offset/name | When export used a non-host timezone |
| `X-ME-Subject` | string | When distinct from mail `Subject` |
| `X-ME-Guid` | hex / guid string | Matches CSV `guid` when possible |
| `X-ME-Export-Source` | string | e.g. `sms-backup-restore` |
| `X-ME-Export-Tool` | string | |
| `X-ME-Export-Tool-Version` | string | |
| `X-ME-Android-Type` | integer string | Optional; SMS `type` / MMS `msg_box` |
| `X-ME-Source-Fields` | JSON | Optional full-fidelity bag (CSV `source_fields_json` / PDU extras) |
| `X-ME-Attachment-Meta` | JSON array | Parallel to MIME attachment parts (see Attachments) |

### Message-kind taxonomy (shared)

| Kind | Meaning |
|------|---------|
| `sms` | SMS text |
| `mms` | MMS (may include media) |
| `imessage` | Normal iMessage text/media |
| `tapback` | Reaction row |
| `sticker_tapback` | Sticker used as reaction |
| `balloon` | App / URL / poll / Digital Touch / etc. |
| `announcement` | Group system message |
| `location_share` | Live location start/stop |

SMS writers use `sms` / `mms` only. Absence of iMessage-only headers means “not applicable,” not an empty array.

## Group MMS rules

1. Emit `X-ME-Participants` with every non-empty handle (sorted stably for hashing if needed).
2. Never drop the roster because `From`/`To` already list some addresses.
3. Incoming sender must be the real sender handle when known (not an arbitrary group member).
4. Untitled groups: stem from sorted participant phones (same as CSV); title may still be empty.

## Attachments

**v1: embed bytes as MIME parts** so offline mail clients show media.

- `Content-Type` from known mime; fallback `application/octet-stream`
- `Content-Disposition: attachment; filename="…"` using original name when known
- Part order is significant: index `0..n-1` of non-body MIME attachments matches `X-ME-Attachment-Meta` and iMessage `parts[].attachment_indices`

`X-ME-Attachment-Meta` JSON array (CSV `AttachmentCell`-aligned):

```json title="X-ME-Attachment-Meta"
[
  {
    "path": null,
    "original_name": "IMG_001.jpg",
    "mime_type": "image/jpeg",
    "is_sticker": false,
    "transcription": null,
    "sticker_effect": null,
    "digest_sha256": "…"
  }
]
```

- `path` may be null when bytes are embedded only; digest supports dedupe across re-exports.
- **Never** assign leftover MIME parts to the “last” message in a conversation (Plus archive anti-pattern).

Media is transformed then embedded; FormatSink removes the staged `attachments/` directory after write so the mail archive folder is the product.

## Body text

- Primary human body: `text/plain; charset=utf-8` (UTF-8).
- For multipart messages: first body part is flattened readable text; media follows as attachments.
- Placeholders such as `[attachment]` / `[app]` are acceptable when flattening iMessage parts.
- Optional `text/html` is deferred.

---

## iMessage extension

Align with the unified CSV inventory in [`message_ir_format::CSV_HEADERS`](https://github.com/bitrealm-io/message-vault/blob/main/crates/libs/ir-format/src/write.rs). Apple-only cells are empty for SMS rows.

### Threading (replies)

| Header | Role |
|--------|------|
| `Message-ID` | `<{apple-guid}@imessage.local>` |
| `In-Reply-To` / `References` | Originator `Message-ID` |
| `X-ME-Is-Reply` | `true` |
| `X-ME-Thread-Originator-Guid` | Apple guid of thread root |
| `X-ME-Thread-Originator-Part` | Part index within multipart bubble |
| `X-ME-Num-Replies` | On originator when known |

Ordinary SMS leaves reply headers unset (no fake threads).

### Tapbacks / reactions

Apple stores tapbacks as separate `message` rows (`associated_message_type` 2000–2005 add, 3000–3005 remove, plus sticker associations).

**Preferred: one `.eml` per tapback.**

| Header | Values |
|--------|--------|
| `X-ME-Message-Kind` | `tapback` \| `sticker_tapback` |
| `In-Reply-To` / `References` | Parent message `Message-ID` |
| `X-ME-Associated-Guid` | Parent Apple guid |
| `X-ME-Associated-Part` | Part index reacted to |
| `X-ME-Tapback-Kind` | `loved` \| `liked` \| `disliked` \| `laughed` \| `emphasized` \| `questioned` \| `emoji` \| `sticker` \| `removed_loved` \| … |
| `X-ME-Tapback-Emoji` | Custom emoji when present |
| `X-ME-Tapback-Action` | `add` \| `remove` |

Body `text/plain`: short human line (`Loved a message`, `😂 reacted`) so clients show something without parsing headers.

Sticker tapback: include sticker image MIME part + `X-ME-Attachment-Meta` with `is_sticker: true`.

**Optional aggregate on parent** (translator cache only):

```http title="X-ME-Tapbacks"
X-ME-Tapbacks: [{"part_index":0,"kind":"loved","reactor_handle":"+1555…","reactor_display_name":"Alex"}]
```

Readers SHOULD prefer per-message tapback EMLs. Do **not** store reactions only as free text in the parent body.

### Multipart bubbles (`X-ME-Parts`)

```http title="X-ME-Parts"
X-ME-Parts: [{"index":0,"kind":"text","text":"Hi","attachment_indices":[],"effects":[]},{"index":1,"kind":"attachment","attachment_indices":[0],"effects":[]}]
```

Aligned with CSV `PartRecord`: `index`, `kind` (`text` \| `attachment` \| `app` \| `retracted` \| …), `text?`, `attachment_indices[]`, `effects[]`, `emoji_image?`.

MIME: `multipart/mixed` (or `related`) with flattened `text/plain` first, then attachments in emit order. Text effects (mention, link, styles, animated) stay in `parts[].effects` for v1 (no HTML reconstruction required).

### Edits / unsends

- Body = **current** visible text (empty if unsent).
- `X-ME-Edits`: JSON array aligned with CSV `EditEventRecord`: `{ part_index, status, text, timestamp?, timestamp_utc?, guid? }` with `status` ∈ `original` \| `edited` \| `unsent`.
- `X-ME-Is-Deleted: true` when tombstoned/deleted in DB.
- Do not invent separate “edit event” EMLs.

### Send effects

- `X-ME-Send-Effect` — same label space as CSV `send_effect` / `expressive_label` (`Sent with Balloons`, Confetti, Invisible Ink, …).
- Plain body also appends that line so clients show the effect without parsing headers.

### Balloons / app messages

First-class messages: `X-ME-Message-Kind: balloon`.

| Header | Role |
|--------|------|
| `X-ME-Balloon-Bundle-Id` | Raw `balloon_bundle_id` |
| `X-ME-Balloon-Kind` | `url` \| `apple_pay` \| `poll` \| `handwriting` \| `digital_touch` \| `slideshow` \| `check_in` \| `find_my` \| `fitness` \| `business` \| `application` \| … |
| `X-ME-App` | JSON matching CSV `app_json` / `build_balloon_value` |

MIME:

- `text/plain` summary for preview (URL title, `Poll: …`, `Apple Pay`, …)
- Optional `application/json` part (`name=app.json`) when the payload is large
- Handwriting: `imessage-ir-exporter` attaches `HandwrittenMessage::render_svg` as `image/svg+xml`
- Digital Touch: bundle id + `X-ME-App` JSON only (no SVG path today)

### Announcements and location

- Announcement: `X-ME-Message-Kind: announcement`, `X-ME-Announcement: <text>`, body = same text
- Location: `X-ME-Message-Kind: location_share`, `X-ME-Shared-Location: started|stopped`, body may include map URL/text

### Stickers (message attachments)

Normal sticker sends: image MIME part + `X-ME-Attachment-Meta` (`is_sticker`, `sticker_effect?`, `transcription?`). Genmoji/memoji use the same path.

### Read receipts and participants

- `X-ME-Read-Receipt` — RFC 3339 when known
- `X-ME-Participants` — required for iMessage groups (full Apple roster)

### Client vs translator surfaces

| Surface | Mail client sees | Translator uses |
|---------|------------------|-----------------|
| Text / media | Body + MIME parts | same |
| Tapback | Short reply-like message | `X-ME-Tapback-*` + association |
| Balloon | Summary (+ optional image/JSON part) | `X-ME-App` |
| Edits | Current text only | `X-ME-Edits` |
| Effects | Trailing “Sent with …” line | `X-ME-Send-Effect` |
| Parts / mentions | Flattened text | `X-ME-Parts` |

---

## Mapping from today’s CSV cores (SMS)

| CSV column | Mail archive |
|------------|--------------|
| `chat_identifier` | `X-ME-Chat-Identifier` |
| `conversation_type` | `X-ME-Conversation-Type` |
| `group_title` | `X-ME-Group-Title` |
| `guid` | `X-ME-Guid` + `Message-ID` |
| `timestamp` / `timestamp_utc` / `timestamp_unix_ms` | `Date` + `X-ME-Timestamp-Unix-Ms` |
| `direction` | `X-ME-Direction` |
| `service` | `X-ME-Service` |
| `sender_handle` / `sender_display_name` | headers + `From` phrase |
| `subject` | `Subject` / `X-ME-Subject` |
| `text` | `text/plain` body |
| `attachments_json` | MIME parts + `X-ME-Attachment-Meta` |
| `message_kind` | `X-ME-Message-Kind` (`sms`/`mms`) |
| `android_type` | `X-ME-Android-Type` |
| `source_fields_json` / PDU extras | `X-ME-Source-Fields` |
| `export_*` | `X-ME-Export-*` |
| `participants_json` (iMessage) | `X-ME-Participants` |
| `tapbacks_json` | tapback EMLs (+ optional `X-ME-Tapbacks`) |
| `parts_json` / `edits_json` / `app_json` | `X-ME-Parts` / `X-ME-Edits` / `X-ME-App` |
| `send_effect` | `X-ME-Send-Effect` |
| `thread_originator_*` | `In-Reply-To` + `X-ME-Thread-*` |

## Implementation notes

1. Crate [`message-mail`](https://github.com/bitrealm-io/message-vault/blob/main/crates/libs/mail/) emits one `.eml` / mboxrd record per message (`write_mail_package`).
2. **Android / OpenExtract / iMazing / WhatsApp** exporters map pending rows → `MailMessage` (`--format eml|mbox`).
3. **iMessage** is [`imessage-ir-exporter`](https://github.com/bitrealm-io/message-vault/blob/main/crates/exporters/imessage-ir-exporter/) (`imessage-database` → common message → packaging).
4. Deferred: Digital Touch animation, translations UI, HEIC convert / obfuscate inside MIME, Askama HTML bodies.

## Related docs

- [CSV output conventions](/vault/developer/reference/csv-columns/)
- [Exporter capability matrix](/vault/developer/formats/)
- [SMS Backup+ EML input notes](/vault/developer/formats/sms-backup-plus/format/)
- [SMS Backup & Restore import mapping](/vault/developer/formats/sms-backup-restore/mapping/)
