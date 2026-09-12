---
title: "Converter capabilities"
description: "What each backup converter writes, where it falls short, and links to input-format and mapping pages."
---

These pages are Developer docs (CLI converters and field mapping). Day-to-day Import from a phone backup is in the [User Guide](/vault/user/import-from-a-backup/).

What each converter writes (and where it falls short). Marks: **yes** / **partial** / **no**.

## Shared model

All converters build a **common message** per conversation (`ConversationDocument`, schema v4 in [`message-ir`](../../crates/libs/ir/)), then project the user-picked format via `FormatSink` in [`message-ir-format`](../../crates/libs/ir-format/) (default **JSON**). When packaging is CSV, columns follow [`CSV_HEADERS`](../../crates/libs/ir-format/src/write.rs). Across the board:

- The peer is `chat_identifier` — there is **no** dedicated receiver-phone column
- Every participant is a **typed handle**: `handle_type` (`phone` / `email` / `username` / `other`) on each JSON/JSONL participant and inside `participants_json`; the CSV `handle_type` column carries the sender's type, inferred from the handle when the source doesn't supply it
- Direction is `direction` (`incoming` / `outgoing`) — there is **no** `is_from_me` column
- Outgoing rows fill `sender_handle` / `sender_display_name` from owner identity (`owner_handle` / `owner_display_name` columns)
- Vendor leftovers live in `source_fields_json` (not `xml_fields_json`)

## Capabilities

| | GO SMS Pro | SMS Backup & Restore | SMS Backup+ | OpenExtract | iMazing | WhatsApp | iMessage |
|---|---|---|---|---|---|---|---|
| **Output** | Per-chat CSV / EML / MBOX / JSON(+L) / **XML** | Per-chat CSV / EML / MBOX / JSON(+L) / **XML** | Per-chat CSV / EML / MBOX / JSON(+L) / **XML** | Per-chat CSV / EML / MBOX / JSON(+L) / **XML** | Per-chat CSV / EML / MBOX / JSON(+L) / **XML** (`__whatsapp` for WA) | Per-chat CSV / EML / MBOX / JSON(+L) / **XML** (`__whatsapp`) | Per-chat CSV / EML / MBOX / JSON(+L) / **XML** |
| **Peer phone** (`chat_identifier`) | yes | yes | yes (or `unknown`) | partial (name stem if unresolved) | partial (name stem if unresolved) | yes (JID → E.164) | yes (Apple chat id) |
| **Sender phone** (`sender_handle`, incoming) | yes | yes | yes | yes | yes | yes (groups via sender JID) | yes |
| **Names** | yes (XML + contacts) | yes (XML + contacts) | yes (contacts + name-mapping) | partial (contacts critical) | yes (vCard CSV) | yes (`wa.db` via wtsexporter) | yes (AddressBook / backup) |
| **Direction** | yes | yes | yes | yes (`Is From Me` / Direction) | yes (`Type`) | yes (`from_me`) | yes (`is_from_me` in DB) |
| **Groups** | partial (PDU MMS) | yes (MMS) | partial (flat multi-address) | no | partial (WhatsApp roster weak) | yes (title + sender phones) | yes (full DB roster) |
| **Attachments** | partial (PDU only; XML none) | yes (MMS) | yes (archive pairing heuristic) | no (flag only) | yes | yes (media paths via wtsexporter) | yes |
| **Media modes** (`clone`/`convert`/`compress`) | yes | yes | yes | no | yes | yes | yes (`clone`/`basic`/`full`/`disabled`) |
| **Contacts** | optional | optional | optional | recommended | recommended | via `--wa` / wtsexporter | optional |
| **Owner phone CLI** | required | required | required (+ owner email) | no | no | no | no |

## Deficiencies

| Exporter | Main gaps |
|---|---|
| **GO SMS Pro** | Proprietary MMS `.pdu` (heuristic decode); many empty stub PDUs; `export_tool_version` unpinned; SMS attachments not in XML |
| **SMS Backup & Restore** | Call logs ignored; drafts / failed / queued skipped; encrypted ZIP not supported (unlock first) |
| **SMS Backup+** | Offline `.eml` only (no IMAP); archive attachment→message pairing is guesswork; unresolved peers → `unknown.csv` |
| **OpenExtract** | No media extraction; no groups; thin source format; name-only chats common without a good VCF |
| **iMazing** | Reactions/replies are free text; WhatsApp groups lack full roster; naive dates need `--timezone` |
| **WhatsApp** | Requires external `wtsexporter` (pip or bundled binary); LID / non-phone JIDs stay raw; full group roster depends on upstream JSON |
| **iMessage** (`imessage-ir-exporter`) | No WhatsApp; links GPL `imessage-database` (combined binaries must satisfy the GPL); needs Mac/`chat.db` or iOS backup; no TXT/HTML |

## Other dimensions

| | GO SMS Pro | SMS Backup & Restore | SMS Backup+ | OpenExtract | iMazing | WhatsApp | iMessage |
|---|---|---|---|---|---|---|---|
| **WhatsApp** | no | no | no | no | yes (CSV) | yes (native DB) | no |
| **Discord** | no | no | no | no | no | no | no |
| **Signal** | no | no | no | no | no | no | no |
| **Telegram** | no | no | no | no | no | no | no |
| **Slack** | no | no | no | no | no | no | no |
| **`participants_json`** | yes (unified CSV) | yes | yes | yes | yes | yes | yes |
| **Reactions / tapbacks** | no | no | no | no | free-text in `source_fields_json` | reactions in `source_fields_json` | structured `tapbacks_json` |
| **Edits / replies** | no | no | no | no | raw dates / free-text | reply in `source_fields_json` | `edits_json` / thread GUIDs |
| **Source extras** | `pdu_*` (in `source_fields_json`) | `subject`, `message_kind`, `source_fields_json` | `smssync_id`, `eml_path` (in `source_fields_json`) | `source_kind`, `has_attachments` (in `source_fields_json`) | vendor cols (in `source_fields_json`) | `jid` / `key_id` (in `source_fields_json`) | `parts_json`, `app_json`, … |
| **Timezone** | XML/PDU epoch | XML epoch | EML dates | vendor `Date` | naive + `--timezone` | epoch from wtsexporter | DB epoch + offset |
| **Skip diagnostics** | `skipped_*.csv` (invalid address, empty PDU, no party) | counters on stderr | counters on stderr | unresolved phone count | counters on stderr | counters on stderr | counters on stderr |

Discord, Signal, Telegram, and Slack are recognized services in the shared model (`IrService`), but no exporter parses those backup sources yet — they are future sources, listed as **no** until an exporter lands.

## Technical docs

| Exporter | Mapping / design |
|---|---|
| GO SMS Pro | [Import mapping](/vault/developer/formats/go-sms-pro/mapping/) |
| SMS Backup & Restore | [Input format](/vault/developer/formats/sms-backup-restore/input/) · [Import mapping](/vault/developer/formats/sms-backup-restore/mapping/) |
| SMS Backup+ | [Format](/vault/developer/formats/sms-backup-plus/format/) · [Import mapping](/vault/developer/formats/sms-backup-plus/mapping/) |
| OpenExtract | Parser notes live with the crate |
| iMazing | [Input format](/vault/developer/formats/imazing/input/) · [Design](/vault/developer/formats/imazing/design/) |
| WhatsApp | [Import methods](/vault/user/prepare-a-backup/android-whatsapp/) |
| iMessage | [Prepare a backup](/vault/user/prepare-a-backup/iphone-ipad/) |

**Common message:** end-user [export structure](/vault/developer/reference/export-structure/); schema [message-ir architecture](/vault/developer/architecture/common-message/). All exporters parse to `ConversationDocument` then project via `message_ir_format::FormatSink` (per-chat JSON/JSONL/CSV/EML/MBOX, or one SyncTech `smses.xml` with `--format xml`). Output formats: [mail archives](/vault/developer/formats/mail-archive/) and [SMS Backup & Restore XML](/vault/developer/formats/sms-backup-restore-xml/). Attachment modes (none / copy / convert / compress) and obfuscate apply through `FormatSink` for every format.

**Convert:** [`message-reexport`](/vault/developer/formats/convert/) converts an existing Message Vault output directory to another format, detecting the input format from the folder. Export uses it for any format other than JSON Lines. Not a vendor backup source.
