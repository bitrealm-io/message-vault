---
title: "GO SMS Pro import mapping"
description: "How GO SMS Pro XML and PDU fields become the shared conversation structure."
---

How `gosms_sys*.xml` `<SMS>` elements and `I_*.pdu` / `S_*.pdu` MMS files become shared `ConversationDocument` values, including validation, skipped records, and retained source data.

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
| `<address>` | `chat_identifier`, `sender_handle` | Digits sanitized then E.164. For sent (`type=2`), address is the peer (not the sender). For received (`type=1`), address is also `sender_handle`. A voicemail notice from Google Voice is an SMS from the Google Voice number like any other; the vault does not read who called out of its body. |
| `<contactName>` | `sender_display_name` | Display name filled for incoming when present. |
| `<date>` | `timestamp_unix_ms`, `timestamp`, `timestamp_utc`, `timestamp_display` | Raw ms in `timestamp_unix_ms`. Converted to local/UTC RFC3339 and a human display string. |
| `<type>` | `android_type`, `direction` | `1` → `incoming`, `2` → `outgoing`. Other values are skipped. |
| `<body>` | `text` | GO SMS emoji codes (e.g. `+g1f602`) decoded to Unicode. |
| *(all children)* | `source_fields_json` | Child element name → text (plus `source_kind`, see below). |

## Other shared fields

| Shared field | Source |
|---------------|--------|
| `conversation_type` | Always `individual` for XML SMS; `group` for a PDU with two or more people besides the owner |
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

**PDU rows** additionally include:

| Bag key | Meaning |
|---------|---------|
| `pdu_filename` | Source PDU basename |
| `pdu_fields` | The MMS headers as an object (keys below) |
| `android_group_title` | Synthetic group label when present (data only; not used for filenames) |

`pdu_fields` keys are the WAP-209 header names in lower case with dashes: always `message-type`, `content-type`, `transaction-id`, `mms-version` and `delivery-report`; and when the phone wrote them, `subject`, `message-id`, `message-class`, `priority`, `read-report`, `report-allowed`, `expiry`, `delivery-time`, `message-size`, `status`, `response-status`, `response-text`, `sender-visibility`, `retrieve-status`, `retrieve-text`, `read-status`, `reply-charging`, `reply-charging-deadline`, `reply-charging-id`, `reply-charging-size`, `previously-sent-by` and `previously-sent-date`. Expiry and delivery time read `absolute:<unix seconds>` or `relative:<seconds>`.

## Skipped records

The exporter counts every record it drops on its `ExportReport`, and the run summary in the Import screen's log lists each count that is not zero (`ExportReport::summary_lines` in `crates/core/message-vault-io-core/src/pipeline.rs`).
Invalid dates have a field of their own on the report and print as `skipped N invalid-date rows`; the other counts live in the report's `extra` map and print as `name: N`.
The same lines also show `xml_messages_seen`, `pdu_messages`, and `pdu_group_messages`, which are totals rather than skips.

| Summary line | Meaning |
|-------|---------|
| `skipped N invalid-date rows` | XML `<date>` was missing or not a number. A missing date is skipped rather than read as 1970-01-01, because every such row would share timestamp 0 and could falsely deduplicate. |
| `skipped_unknown_type` | XML `<type>` was not `1` (inbox) or `2` (sent) |
| `skipped_unknown_address` | XML SMS whose `<address>` had fewer than four digits once non-digits are stripped (empty, a name, junk), the minimum `phone::sanitize_number` (`crates/libs/phone/src/lib.rs`) accepts. 4–6 digit short codes (e.g. AT&T `7535`) are kept. Full list: `skipped_invalid_address.csv`. |
| `skipped_empty_pdu` | A stub: a `.pdu` file that does not start with the X-Mms-Message-Type header. GO SMS Pro writes a 17-byte `application/smil` placeholder for an MMS it never downloaded; 709 of the 2,004 files in one real backup are stubs. Full list: `skipped_empty_pdu.csv`. |
| `skipped_no_other_party` | A PDU whose every number is one of the owner phone numbers entered on the Import form, such as an MMS the owner sent to themself. Full list: `skipped_no_party.csv` (`pdu_filename`, `sender`, `recipients`, `is_sent`). |
| `skipped_unparseable_pdu` | A PDU that breaks a WAP-209 or WSP rule, or records a transaction that is not a message (a delivery report, a notification). The first twenty are named as `error:` lines in the summary with the rule broken and the byte offset. |

Each `skipped_*.csv` names at most the first twenty records (`MAX_SKIP_DETAILS` in `crates/exporters/go-sms-pro-exporter/src/emit.rs`) and closes with a `...and N more entries not shown` row when there were more, so a large backup does not produce a diagnostic file the size of the export.
A run that skips nothing of that kind writes no file, and removes a stale one left by an earlier run into the same folder.

## PDU rows

MMS from `I_<unix>_*.pdu` (received) and `S_<unix>_*.pdu` (sent) files use the same shared conversation model. Each file is the MMS PDU the phone's MMS stack held, byte for byte, and the `go-sms-mms` crate decodes it by the WAP-209 (MMS Encapsulation) and WAP-230 (WSP) rules alone. Differences from XML rows:

| Shared field | PDU behavior |
|---------------|--------------|
| `direction` | `m-send-req` is outgoing, `m-retrieve-conf` is incoming. The file name prefix says the same thing and is not consulted. |
| `sender_handle` | The From header's number on a received message. A sent message carries the Insert-address-token instead of a number, so the export owner is the sender. |
| `chat_identifier` / `conversation_type` / `group_title` | From the From, To, Cc and Bcc numbers that are not the owner's: one number is a 1:1 chat, two or more a group with a `chat-group-…` id. |
| `timestamp*` / `timestamp_unix_ms` | The MMS `Date` header; the file name's seconds when the header is absent. |
| `text` | Every `text/plain` part, in wire order, joined with a newline, with GO SMS Pro emoji codes decoded. When there is no text part, the Subject. |
| `attachments_json` | Every part that is not `text/plain` and not `application/smil`, with its content type and the name its part headers give it, under `attachments/` |
| `android_type` | Empty |
| `source_fields_json` | `source_kind=pdu` plus `pdu_filename` / `pdu_fields` as above |

### MMS decode rules

The decoder is strict: each value is read in the one shape WAP-230 gives it, and a byte that breaks the shape makes the file unparseable rather than a guess. There is no scanning of raw bytes for addresses, text markers or image magics. The rules, each with its reason, are the module documentation of `crates/libs/go-sms-mms/src/wsp.rs`, `mms.rs` and `pdu.rs`; in short:

1. The first header is X-Mms-Message-Type, which is how a stub is told from a message.
2. Headers run until Content-Type; the bytes after it are the body. Nothing in the body is read as a header, so image bytes can never become an address.
3. Every header code has one value shape, from WAP-209 table 8; an unknown code is an error, because its value has no length prefix and the walk cannot stay aligned past it.
4. The body is a WSP multipart: a part count, then for each part its headers length, its data length, the headers and the data. The two lengths alone say where a part's bytes are.
5. Well-known content types are looked up in WAP-230 table 40 by their assigned number. `0x33` is `application/vnd.wap.multipart.related`, the type of every real MMS.

A real backup of 2,004 files (1,295 messages and 709 stubs, from 2014 to 2015) decodes under these rules without one error; the earlier decoder, which tolerated broken shapes and scanned the raw bytes for anything address-like, read JPEG bytes as phone numbers in 263 of those messages. `crates/exporters/go-sms-pro-exporter/tests/real_backup.rs` runs the exporter over such a backup when `GO_SMS_PRO_BACKUP` names one.

Algorithm reference: OMA WAP-209 (MMS Encapsulation) and WAP-230 (WSP); the content type table is checked against [python-messaging](https://github.com/pmarti/python-messaging) `messaging/mms/wsp_pdu.py` (not a dependency; not copied).
