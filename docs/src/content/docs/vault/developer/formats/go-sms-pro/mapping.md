---
title: "GO SMS Pro import mapping"
description: "How GO SMS Pro XML and PDU fields become the shared conversation structure."
---

How `gosms_sys*.xml` `<SMS>` elements and `I_*.pdu` MMS files become shared `ConversationDocument` values, including validation, skipped records, and retained source data.

Shared model: [message-ir](/vault/developer/architecture/common-message/). CSV projection: [CSV columns](/vault/developer/reference/csv-columns/) and [`message_ir_format::CSV_HEADERS`](https://github.com/bitrealm-io/message-vault/blob/main/crates/libs/ir-format/src/write.rs).

## Goal / non-goal

- **Goal:** Document how GO SMS Pro fields fill shared conversation fields and the retained source-data bag.
- **Non-goal:** Define output-format layouts or a private CSV schema. All output formats are projections of the shared model.

## Pipeline / output

Source XML/PDU → `ConversationDocument` → [`message_ir_format::FormatSink`](https://github.com/bitrealm-io/message-vault/blob/main/crates/libs/ir-format/src/format_sink.rs).

The desktop app's Import screen runs this exporter and always writes JSON Lines, one file per conversation, because Import and Push read conversation files in that form (`src-tauri/src/commands/extract.rs`).
Every other format (JSON, CSV, EML, MBOX, SMS Backup & Restore XML) is a rewrite of that output through [Convert](/vault/developer/formats/convert/), which Export and **Settings → Convert** run.

In CSV form: one file per conversation. PDU media under `attachments/` when copying/embedding. Filenames: 1:1 → `+E164.csv`; untitled groups → `group_+A_+B_….csv` (max 10 phones, then a hash). The XML form is a single SyncTech `smses.xml`.

Diagnostic skip lists (`skipped_invalid_address.csv`, `skipped_empty_pdu.csv`, `skipped_no_party.csv`) are **not** conversation CSVs; they use their own small headers (see [Skipped records](#skipped-records)).

## XML structure

```xml title="gosms XML"
<GoSms>
  <SMSCount>…</SMSCount>
  <SMS>
    <address>…</address>
    <contactName>…</contactName>
    <date>…</date>          <!-- Unix ms -->
    <type>1|2</type>        <!-- 1 = inbox, 2 = sent -->
    <body>…</body>
    <!-- any other Telephony-style children are kept in source_fields_json -->
  </SMS>
</GoSms>
```

Each `<SMS>` becomes one message in a shared conversation. `chat_identifier` holds the peer’s E.164 handle.

## Known XML children → shared fields

| XML child | Shared field(s) | Notes |
|-----------|------------------|--------|
| `<address>` | `chat_identifier`, `sender_handle` | Digits sanitized then E.164. For sent (`type=2`), address is the peer (not the sender). For received (`type=1`), address is also `sender_handle` unless Google Voice voicemail parsing overrides it from `<body>`. |
| `<contactName>` | `sender_display_name` | Display name filled for incoming when present. |
| `<date>` | `timestamp_unix_ms`, `timestamp`, `timestamp_utc`, `timestamp_display` | Raw ms in `timestamp_unix_ms`. Converted to local/UTC RFC3339 and a human display string. |
| `<type>` | `android_type`, `direction` | `1` → `incoming`, `2` → `outgoing`. Other values are skipped. |
| `<body>` | `text` | GO SMS emoji codes (e.g. `+g1f602`) decoded to Unicode. |
| *(all children)* | `source_fields_json` | Child element name → text (plus `source_kind`, see below). |

## Other shared fields

| Shared field | Source |
|---------------|--------|
| `conversation_type` | Always `individual` for XML SMS; `group` from PDU PLMN lists |
| `group_title` | Derived for PDU groups; empty for XML |
| `participants_json` | Peer handles for the conversation |
| `guid` | SHA-256 of chat id + local timestamp + direction + text + attachment digests |
| `service` | Always `SMS` |
| `sender_handle` / `sender_display_name` | Outgoing uses export owner; incoming from address / contactName |
| `attachments_json` | `[]` for XML; media paths for PDU |
| `message_kind` | `sms` or `mms` (PDU with attachments → `mms`) |
| `export_source` / `export_tool` / `export_tool_version` | `go-sms-pro` / `GO SMS Pro` / (empty until pinned) |
| `owner_handle` / `owner_display_name` | Export owner |
| `android_type` | Raw `<type>` (`1`/`2`); empty for PDU |
| `source_fields_json` | Vendor bag (below) |

Apple-only columns stay empty.

## `source_fields_json`

Always includes `source_kind`: `"xml"` or `"pdu"`.

**XML rows:** every `<SMS>` child name → text (for example `address`, `body`, `read`, `status`, `date_sent`, …), merged with `source_kind`.

**PDU rows** additionally may include:

| Bag key | Meaning |
|---------|---------|
| `pdu_filename` | Source PDU basename |
| `pdu_decode` | `structured` / `mixed` / `heuristic` confidence for body, attachments, and direction |
| `pdu_fields` | Optional MMS headers object (keys below) |
| `android_group_title` | Synthetic group label when present (data only; not used for filenames) |

`pdu_fields` keys when present: `subject`, `message_id`, `message_type`, `mms_version`, `message_size`, `message_class`, `transaction_id`, `priority`, `delivery_report`, `read_report`, `report_allowed`, `delivery_time`, `expiry`, `status`, `response_status`, `response_text`, `sender_visibility`, `bcc` (comma-joined), plus `app:<name>` for non-well-known MMS application headers.

`message_size` is the WAP-209 Message-Size long-integer (advisory octets). GO SMS Pro `0x8e` + `filename\0` named parts are unrelated and are not decoded as Message-Size.

## Skipped records

The exporter counts every record it drops on its `ExportReport`, and the run summary in the Import screen's log lists each count that is not zero (`ExportReport::summary_lines` in `crates/core/message-vault-io-core/src/pipeline.rs`).
Invalid dates have a field of their own on the report and print as `skipped N invalid-date rows`; the other counts live in the report's `extra` map and print as `name: N`.
The same lines also show `xml_messages_seen`, `pdu_messages`, and `pdu_group_messages`, which are totals rather than skips.

| Summary line | Meaning |
|-------|---------|
| `skipped N invalid-date rows` | XML `<date>` was missing or not a number. A missing date is skipped rather than read as 1970-01-01, because every such row would share timestamp 0 and could falsely deduplicate. |
| `skipped_unknown_type` | XML `<type>` was not `1` (inbox) or `2` (sent) |
| `skipped_unknown_address` | XML SMS whose `<address>` had fewer than four digits once non-digits are stripped (empty, a name, junk), the minimum `phone::sanitize_number` (`crates/libs/phone/src/lib.rs`) accepts. 4–6 digit short codes (e.g. AT&T `7535`) are kept. An incoming Google Voice voicemail still exports when the caller can be parsed from `<body>`. Full list: `skipped_invalid_address.csv`. |
| `skipped_empty_pdu` | Hollow PDU stub with no participants, From/To, body, or attachments (the common GO SMS Pro placeholder is only `application/smil` + null). Full list: `skipped_empty_pdu.csv`. |
| `skipped_no_other_party` | Non-empty PDU classified as non-group (`< 3` unique participants) where every decoded number was empty or one of the owner phone numbers entered on the Import form. Full list: `skipped_no_party.csv` (`pdu_filename`, `participants`, `is_sent`, `has_from`, `has_to`). |
| `skipped_unparseable_pdu` | PDU filename/timestamp could not be parsed. The first twenty are named as `error:` lines in the summary. |

Each `skipped_*.csv` names at most the first twenty records (`MAX_SKIP_DETAILS` in `crates/exporters/go-sms-pro-exporter/src/emit.rs`) and closes with a `...and N more entries not shown` row when there were more, so a large backup does not produce a diagnostic file the size of the export.
A run that skips nothing of that kind writes no file, and removes a stale one left by an earlier run into the same folder.

## PDU rows

MMS from `I_<unix>_*.pdu` files use the same shared conversation model. Differences:

| Shared field | PDU behavior |
|---------------|--------------|
| `chat_identifier` / `conversation_type` / `group_title` | From PLMN participants; groups use `chat-group-…` ids |
| `timestamp*` / `timestamp_unix_ms` | MMS `Date` header when present; else filename `I_<unix>_` (seconds). Filename still required to accept the file. |
| `text` | Content-Location text parts / multipart `text/*` (emoji-decoded); marker/`</smil>` fallback if needed |
| `attachments_json` | Named/typed media parts, else magic-byte splits under `attachments/` |
| `android_type` | Empty |
| `source_fields_json` | `source_kind=pdu` plus `pdu_filename` / `pdu_decode` / `pdu_fields` as above |

### MMS parse path

1. **Structured decode** (`go-sms-mms` / `mms_enc`): WAP-209 headers (From/To/Cc/Bcc/Date/Subject/Status/…) + Content-Location named parts + mid-file / offset-0 multipart (part Content-ID, Content-Disposition/Filename, Content-Type Name/Filename/Start/Type/Start-info). Direction from decoded address roles; body from named parts, multipart text (including SMIL `cid:` → Content-ID), or Subject; attachments from named/typed parts and SMIL `src` / `cid:` / filename.
2. **Heuristic fallback**: PLMN regex for raw address lists, legacy `text_*.txt` markers / `</smil>` printable tails, and magic-byte attachment splits — only when the structured path left that field empty.

Algorithm reference: OMA WAP-209 / WAP-230 and the decode concepts in [python-messaging](https://github.com/pmarti/python-messaging) `messaging/mms` (not a dependency; not copied).
