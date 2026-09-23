//! Parse `.eml` / mboxrd back into [`MailMessage`].

use crate::headers as hn;
use crate::{MailAttachment, MailMessage, Participant};
use anyhow::{Context, Result};
use mailparse::{MailHeader, MailHeaderMap, ParsedMail};
use message_ir::{IrDirection, IrImessage, IrMessage, IrMessageKind, IrService, IrSource};
use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct AttachmentMetaCell {
    original_name: Option<String>,
    mime_type: Option<String>,
    #[serde(default)]
    is_sticker: bool,
    transcription: Option<String>,
    sticker_effect: Option<String>,
    digest_sha256: Option<String>,
}

/// Parse one RFC 5322 / MIME message (EML bytes) into [`MailMessage`].
///
/// # Errors
///
/// Returns an error when the bytes are not a valid email or a required
/// `X-ME-*` header is missing.
pub fn mail_message_from_eml_bytes(bytes: &[u8]) -> Result<MailMessage> {
    let mail = mailparse::parse_mail(bytes).context("parse eml bytes")?;
    let headers = &mail.headers;

    let chat_identifier = required_header(headers, hn::CHAT_IDENTIFIER)?;
    let conversation_type = header_or(headers, hn::CONVERSATION_TYPE, "individual");
    let group_title = optional_header(headers, hn::GROUP_TITLE);
    let participants = parse_participants(headers);
    let guid = required_header(headers, hn::GUID)?;
    let timestamp_unix_ms = required_header(headers, hn::TIMESTAMP_UNIX_MS)?
        .parse::<i64>()
        .context("parse X-ME-Timestamp-Unix-Ms")?;
    let direction = match header_or(headers, hn::DIRECTION, "incoming")
        .to_ascii_lowercase()
        .as_str()
    {
        "outgoing" => IrDirection::Outgoing,
        _ => IrDirection::Incoming,
    };
    let service = IrService::parse(&header_or(headers, hn::SERVICE, "sms"));
    let message_kind = IrMessageKind::parse(&header_or(headers, hn::MESSAGE_KIND, "sms"));
    let sender_handle = optional_header(headers, hn::SENDER_HANDLE);
    let sender_display_name = optional_header(headers, hn::SENDER_DISPLAY_NAME);
    let owner_handle = optional_header(headers, hn::OWNER_HANDLE).unwrap_or_default();
    let owner_display_name = optional_header(headers, hn::OWNER_DISPLAY_NAME);
    let subject = optional_header(headers, hn::SUBJECT);
    let export_source = header_or(headers, hn::EXPORT_SOURCE, "");
    let export_tool = header_or(headers, hn::EXPORT_TOOL, "");
    let export_tool_version = header_or(headers, hn::EXPORT_TOOL_VERSION, "");

    let text = extract_text_body(&mail).unwrap_or_default();
    let attachments = merge_attachments(&mail, headers);

    let source = {
        let android_type =
            optional_header(headers, hn::ANDROID_TYPE).and_then(|s| s.trim().parse::<i32>().ok());
        let fields = optional_header(headers, hn::SOURCE_FIELDS)
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let src = IrSource {
            android_type,
            fields,
        };
        if src.android_type.is_none() && src.fields.is_empty() {
            None
        } else {
            Some(src)
        }
    };

    let imessage = {
        let bag = IrImessage {
            is_reply: header_bool(headers, hn::IS_REPLY),
            in_reply_to_guid: optional_header(headers, hn::THREAD_ORIGINATOR_GUID),
            thread_originator_part: header_u32(headers, hn::THREAD_ORIGINATOR_PART),
            num_replies: header_u32(headers, hn::NUM_REPLIES),
            is_deleted: header_bool(headers, hn::IS_DELETED),
            send_effect: optional_header(headers, hn::SEND_EFFECT),
            shared_location: optional_header(headers, hn::SHARED_LOCATION),
            announcement: optional_header(headers, hn::ANNOUNCEMENT),
            read_receipt_rfc3339: optional_header(headers, hn::READ_RECEIPT),
            parts: header_json(headers, hn::PARTS),
            edits: header_json(headers, hn::EDITS),
            tapbacks: header_json(headers, hn::TAPBACKS),
            app: header_json(headers, hn::APP),
            balloon_bundle_id: optional_header(headers, hn::BALLOON_BUNDLE_ID),
            balloon_kind: optional_header(headers, hn::BALLOON_KIND),
            associated_guid: optional_header(headers, hn::ASSOCIATED_GUID),
            associated_part: header_u32(headers, hn::ASSOCIATED_PART),
            tapback_kind: optional_header(headers, hn::TAPBACK_KIND),
            tapback_emoji: optional_header(headers, hn::TAPBACK_EMOJI),
            tapback_action: optional_header(headers, hn::TAPBACK_ACTION),
        };
        if bag.is_empty() { None } else { Some(bag) }
    };

    Ok(MailMessage {
        chat_identifier,
        conversation_type,
        group_title,
        participants,
        owner_handle,
        owner_display_name,
        export_source,
        export_tool,
        export_tool_version,
        filename_suffix: None,
        message: IrMessage {
            guid,
            timestamp_unix_ms,
            direction,
            service,
            message_kind,
            sender_handle,
            sender_display_name,
            owner_handle: None,
            subject,
            text,
            // Attachment payloads live in `MailMessage::attachments`; readers
            // that build IR fill this list from there.
            attachments: Vec::new(),
            imessage,
            source,
        },
        attachments,
    })
}

/// Parse a JSON header cell, treating blank and `null` as `None`.
fn header_json(headers: &[MailHeader<'_>], name: &str) -> Option<serde_json::Value> {
    let s = optional_header(headers, name)?;
    let t = s.trim();
    if t.is_empty() || t == "null" {
        return None;
    }
    serde_json::from_str(t).ok()
}

/// Read an mboxrd file and parse each record into [`MailMessage`].
///
/// # Errors
///
/// Returns an error when the file cannot be read or a record cannot be parsed.
pub fn mail_messages_from_mbox(path: &Path) -> Result<Vec<MailMessage>> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let records = split_mboxrd(&text);
    let mut out = Vec::with_capacity(records.len());
    for (i, eml) in records.iter().enumerate() {
        let msg = mail_message_from_eml_bytes(eml)
            .with_context(|| format!("parse mbox record {} in {}", i + 1, path.display()))?;
        out.push(msg);
    }
    Ok(out)
}

/// Split mboxrd text into raw EML payloads (envelope `From ` lines removed).
pub(crate) fn split_mboxrd(text: &str) -> Vec<Vec<u8>> {
    let mut records = Vec::new();
    let mut current: Option<Vec<String>> = None;
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.starts_with("From ") {
            if let Some(cur) = current.take() {
                records.push(join_eml_lines(&cur));
            }
            current = Some(Vec::new());
            continue;
        }
        if let Some(ref mut cur) = current {
            cur.push(unescape_mboxrd_line(line).to_string());
        }
    }
    if let Some(cur) = current {
        records.push(join_eml_lines(&cur));
    }
    records
}

/// Join mbox body lines back into EML bytes with one trailing newline.
fn join_eml_lines(lines: &[String]) -> Vec<u8> {
    let mut body = lines.join("\n");
    while body.ends_with('\n') {
        body.pop();
    }
    body.push('\n');
    body.into_bytes()
}

/// Strip one `>` from an mboxrd-escaped `From ` line.
fn unescape_mboxrd_line(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i] == b'>' {
        i += 1;
    }
    if i > 0 && bytes[i..].starts_with(b"From ") {
        &line[1..]
    } else {
        line
    }
}

/// A header value, failing when it is missing or empty.
fn required_header(headers: &[MailHeader<'_>], name: &str) -> Result<String> {
    optional_header(headers, name)
        .filter(|s| !s.is_empty())
        .with_context(|| format!("missing required header {name}"))
}

/// A header value trimmed, or `None` when missing or empty.
fn optional_header(headers: &[MailHeader<'_>], name: &str) -> Option<String> {
    headers
        .get_first_value(name)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// A header value, or `default` when missing.
fn header_or(headers: &[MailHeader<'_>], name: &str, default: &str) -> String {
    optional_header(headers, name).unwrap_or_else(|| default.to_string())
}

/// True when the header is `true` or `1`.
fn header_bool(headers: &[MailHeader<'_>], name: &str) -> bool {
    optional_header(headers, name).is_some_and(|s| s.eq_ignore_ascii_case("true") || s == "1")
}

/// A header value parsed as a number.
fn header_u32(headers: &[MailHeader<'_>], name: &str) -> Option<u32> {
    optional_header(headers, name)?.parse().ok()
}

/// Participants from the JSON header, or none when absent or malformed.
fn parse_participants(headers: &[MailHeader<'_>]) -> Vec<Participant> {
    let Some(raw) = optional_header(headers, hn::PARTICIPANTS) else {
        return Vec::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

/// The message text: the body of a simple mail, or the first `text/plain` part.
fn extract_text_body(mail: &ParsedMail<'_>) -> Option<String> {
    if mail.subparts.is_empty() {
        return mail.get_body().ok().map(|s| trim_body(&s));
    }
    for part in mail.parts() {
        let mime = part.ctype.mimetype.to_ascii_lowercase();
        if mime == "text/plain"
            && let Ok(body) = part.get_body()
        {
            return Some(trim_body(&body));
        }
    }
    // Fallback: first non-multipart body.
    for part in mail.parts() {
        if part.subparts.is_empty()
            && part.ctype.mimetype.starts_with("text/")
            && let Ok(body) = part.get_body()
        {
            return Some(trim_body(&body));
        }
    }
    None
}

/// A body without trailing line endings.
fn trim_body(s: &str) -> String {
    s.trim_end_matches(['\r', '\n']).to_string()
}

/// Attachments from the MIME parts, matched to the metadata header by position.
fn merge_attachments(mail: &ParsedMail<'_>, headers: &[MailHeader<'_>]) -> Vec<MailAttachment> {
    let meta: Vec<AttachmentMetaCell> = optional_header(headers, hn::ATTACHMENT_META)
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();

    let mut mime_atts = Vec::new();
    collect_mime_attachments(mail, &mut mime_atts);

    let n = meta.len().max(mime_atts.len());
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let m = meta.get(i);
        let (bytes, mime_fallback, name_fallback) = mime_atts
            .get(i)
            .cloned()
            .unwrap_or_else(|| (Vec::new(), None, None));
        out.push(MailAttachment {
            bytes,
            meta: message_ir::AttachmentMeta {
                path: None,
                original_name: m.and_then(|c| c.original_name.clone()).or(name_fallback),
                mime_type: m.and_then(|c| c.mime_type.clone()).or(mime_fallback),
                digest_sha256: m.and_then(|c| c.digest_sha256.clone()),
            },
            is_sticker: m.is_some_and(|c| c.is_sticker),
            transcription: m.and_then(|c| c.transcription.clone()),
            sticker_effect: m.and_then(|c| c.sticker_effect.clone()),
        });
    }
    out
}

/// Collect every attachment part's bytes, file name, and MIME type.
fn collect_mime_attachments(
    mail: &ParsedMail<'_>,
    out: &mut Vec<(Vec<u8>, Option<String>, Option<String>)>,
) {
    if mail.subparts.is_empty() {
        return;
    }
    for part in &mail.subparts {
        if !part.subparts.is_empty() {
            collect_mime_attachments(part, out);
            continue;
        }
        let mime = part.ctype.mimetype.to_ascii_lowercase();
        if mime == "text/plain" || mime == "text/html" {
            continue;
        }
        let disp = part.get_content_disposition();
        let is_attachment = disp.disposition == mailparse::DispositionType::Attachment
            || disp.params.get("filename").is_some_and(|s| !s.is_empty())
            || (!mime.starts_with("text/") && !mime.starts_with("multipart/"));
        if !is_attachment {
            continue;
        }
        let bytes = part.get_body_raw().unwrap_or_default();
        let name = disp
            .params
            .get("filename")
            .cloned()
            .or_else(|| part.ctype.params.get("name").cloned());
        out.push((bytes, Some(part.ctype.mimetype.clone()), name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MailMessage, Participant, write_message_file};

    #[test]
    fn roundtrip_eml_headers_and_body() {
        let msg = MailMessage {
            chat_identifier: "+15555550101".into(),
            conversation_type: "individual".into(),
            group_title: None,
            participants: vec![Participant {
                handle: "+15555550101".into(),
                display_name: Some("Sam".into()),
            }],
            owner_handle: "+15555550100".into(),
            owner_display_name: Some("Me".into()),
            export_source: "sms-backup-restore".into(),
            export_tool: "SMS Backup & Restore".into(),
            export_tool_version: "10.26.003".into(),
            filename_suffix: None,
            message: IrMessage {
                guid: "aabbccddeeff00112233445566778899".into(),
                timestamp_unix_ms: 1_400_773_261_000,
                direction: IrDirection::Outgoing,
                service: IrService::Sms,
                message_kind: IrMessageKind::Sms,
                sender_handle: Some("+15555550100".into()),
                sender_display_name: Some("Me".into()),
                owner_handle: None,
                subject: None,
                text: "hello roundtrip".into(),
                attachments: Vec::new(),
                imessage: None,
                source: Some(IrSource {
                    android_type: Some(2),
                    fields: serde_json::from_str(r#"{"address":"+15555550101"}"#).unwrap(),
                }),
            },
            attachments: vec![],
        };

        let tmp = tempfile::tempdir().unwrap();
        let path = write_message_file(&tmp.path().join("chat"), 1, &msg).unwrap();
        let bytes = fs::read(&path).unwrap();
        let parsed = mail_message_from_eml_bytes(&bytes).unwrap();
        assert_eq!(parsed.message.text, "hello roundtrip");
        assert_eq!(parsed.message.direction, IrDirection::Outgoing);
        assert_eq!(
            parsed.message.sender_handle.as_deref(),
            Some("+15555550100")
        );
        assert_eq!(parsed.owner_handle, "+15555550100");
        assert_eq!(parsed.owner_display_name.as_deref(), Some("Me"));
        assert_eq!(
            parsed.message.source.as_ref().and_then(|s| s.android_type),
            Some(2)
        );
    }

    /// Every optional `X-ME-*` header pair the first roundtrip test leaves
    /// unexercised: group/roster headers, subject, source fields, the full
    /// iMessage extension bag, and attachment metadata.
    #[test]
    fn roundtrip_group_imessage_full_extension_bag() {
        let imessage = message_ir::IrImessage {
            is_reply: true,
            in_reply_to_guid: Some("parent-guid-1111".into()),
            thread_originator_part: Some(1),
            num_replies: Some(3),
            is_deleted: true,
            send_effect: Some("Sent with Balloons".into()),
            shared_location: Some("Cupertino".into()),
            announcement: Some("named the conversation".into()),
            read_receipt_rfc3339: Some("2014-05-22T15:41:01Z".into()),
            parts: serde_json::from_str(r#"[{"index":0,"kind":"run","text":"hi"}]"#).ok(),
            edits: serde_json::from_str(r#"[{"part":0,"texts":["hi","hi!"]}]"#).ok(),
            tapbacks: serde_json::from_str(r#"[{"part_index":0,"kind":"loved"}]"#).ok(),
            app: serde_json::from_str(r#"{"name":"Games"}"#).ok(),
            balloon_bundle_id: Some("com.apple.messages.URLBalloonProvider".into()),
            balloon_kind: Some("url".into()),
            associated_guid: Some("assoc-guid-2222".into()),
            associated_part: Some(0),
            tapback_kind: Some("loved".into()),
            tapback_emoji: Some("\u{2764}".into()),
            tapback_action: Some("add".into()),
        };
        let msg = MailMessage {
            chat_identifier: "chat-group1".into(),
            conversation_type: "group".into(),
            group_title: Some("Family".into()),
            participants: vec![
                Participant {
                    handle: "+15555550101".into(),
                    display_name: Some("Sam".into()),
                },
                Participant {
                    handle: "+15555550102".into(),
                    display_name: None,
                },
            ],
            owner_handle: "+15555550100".into(),
            owner_display_name: Some("Me".into()),
            export_source: "imessage".into(),
            export_tool: "imessage-exporter".into(),
            export_tool_version: "3.1.0".into(),
            filename_suffix: None,
            message: IrMessage {
                guid: "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE".into(),
                timestamp_unix_ms: 1_400_773_261_000,
                direction: IrDirection::Incoming,
                service: IrService::IMessage,
                message_kind: IrMessageKind::IMessage,
                sender_handle: Some("+15555550101".into()),
                sender_display_name: Some("Sam".into()),
                owner_handle: None,
                subject: Some("MMS subject".into()),
                text: "full bag".into(),
                attachments: Vec::new(),
                imessage: Some(imessage.clone()),
                source: Some(IrSource {
                    android_type: Some(1),
                    fields: serde_json::from_str(r#"{"address":"+15555550101"}"#).unwrap(),
                }),
            },
            attachments: vec![MailAttachment {
                bytes: b"\xff\xd8\xfffakejpeg".to_vec(),
                meta: message_ir::AttachmentMeta {
                    path: None,
                    original_name: Some("photo.jpg".into()),
                    mime_type: Some("image/jpeg".into()),
                    digest_sha256: Some("deadbeef".into()),
                },
                is_sticker: true,
                transcription: Some("a beach".into()),
                sticker_effect: Some("stroke".into()),
            }],
        };

        let tmp = tempfile::tempdir().unwrap();
        let path = write_message_file(&tmp.path().join("chat"), 1, &msg).unwrap();
        let bytes = fs::read(&path).unwrap();
        let parsed = mail_message_from_eml_bytes(&bytes).unwrap();

        assert_eq!(parsed.chat_identifier, "chat-group1");
        assert_eq!(parsed.conversation_type, "group");
        assert_eq!(parsed.group_title.as_deref(), Some("Family"));
        assert_eq!(parsed.participants.len(), 2);
        assert_eq!(parsed.participants[0].handle, "+15555550101");
        assert_eq!(parsed.participants[0].display_name.as_deref(), Some("Sam"));
        assert_eq!(parsed.participants[1].display_name, None);
        assert_eq!(parsed.message.sender_display_name.as_deref(), Some("Sam"));
        assert_eq!(parsed.message.subject.as_deref(), Some("MMS subject"));
        assert_eq!(parsed.export_source, "imessage");
        assert_eq!(parsed.export_tool, "imessage-exporter");
        assert_eq!(parsed.export_tool_version, "3.1.0");
        let source = parsed.message.source.as_ref().expect("source bag");
        assert_eq!(source.android_type, Some(1));
        assert_eq!(
            serde_json::to_value(&source.fields).unwrap(),
            serde_json::json!({"address": "+15555550101"})
        );

        // The whole extension bag must survive field for field.
        let parsed_bag = parsed.message.imessage.as_ref().expect("imessage bag");
        assert_eq!(
            serde_json::to_value(parsed_bag).unwrap(),
            serde_json::to_value(&imessage).unwrap()
        );

        assert_eq!(parsed.attachments.len(), 1);
        let att = &parsed.attachments[0];
        assert_eq!(att.meta.original_name.as_deref(), Some("photo.jpg"));
        assert_eq!(att.meta.mime_type.as_deref(), Some("image/jpeg"));
        assert_eq!(att.meta.digest_sha256.as_deref(), Some("deadbeef"));
        assert!(att.is_sticker);
        assert_eq!(att.transcription.as_deref(), Some("a beach"));
        assert_eq!(att.sticker_effect.as_deref(), Some("stroke"));
        assert_eq!(att.bytes, b"\xff\xd8\xfffakejpeg".to_vec());
    }

    #[test]
    fn split_mboxrd_unescapes_from() {
        let text = "From me@x Tue May 20 00:00:00 2014\nX-ME-Guid: a\n\n>From spoofed\nbody\n\nFrom me@x Tue May 20 00:01:00 2014\nX-ME-Guid: b\n\nsecond\n\n";
        let records = split_mboxrd(text);
        assert_eq!(records.len(), 2);
        let a = String::from_utf8_lossy(&records[0]);
        assert!(a.contains("From spoofed"));
        assert!(!a.contains(">From spoofed"));
    }
}
