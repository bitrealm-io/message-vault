//! Shared test fixture for crate tests (behind the `testutil` feature).

use crate::{
    ConversationDocument, ConversationMeta, ConversationStats, ExportMeta, HandleType,
    IrConversationType, IrDirection, IrImessage, IrMessage, IrMessageKind, IrParticipant,
    IrService, IrSource, SCHEMA_VERSION,
};
use serde_json::json;

/// One-message conversation fixture: an incoming SMS from `+15555550101`.
///
/// `text` becomes the message body. Stats are computed on return.
pub fn sample_document(text: &str) -> ConversationDocument {
    let mut doc = ConversationDocument {
        schema_version: SCHEMA_VERSION,
        export: ExportMeta {
            source: "sms-backup-restore".into(),
            tool: "SMS Backup & Restore".into(),
            tool_version: "10.26.003".into(),
            owner_handle: Some("+15555550100".into()),
            owner_display_name: Some("Me".into()),
        },
        conversation: ConversationMeta {
            chat_identifier: "+15555550101".into(),
            conversation_type: IrConversationType::Individual,
            group_title: None,
            participants: vec![IrParticipant {
                handle: Some("+15555550101".into()),
                display_name: Some("Sam".into()),
                handle_type: Some(crate::HandleType::Phone),
            }],
            stats: ConversationStats::default(),
        },
        messages: vec![IrMessage {
            guid: "aabbccddeeff00112233445566778899".into(),
            timestamp_unix_ms: 1_400_773_261_000,
            direction: IrDirection::Incoming,
            service: IrService::Sms,
            message_kind: IrMessageKind::Sms,
            sender_handle: Some("+15555550101".into()),
            sender_display_name: Some("Sam".into()),
            subject: None,
            text: text.into(),
            attachments: vec![],
            imessage: None,
            source: Some(IrSource {
                android_type: Some(1),
                fields: {
                    let mut m = serde_json::Map::new();
                    m.insert("address".into(), serde_json::json!("+15555550101"));
                    m
                },
            }),
        }],
        packaging_stem_suffix: None,
    };
    doc.finalize_stats();
    doc
}

/// Two-message iMessage conversation fixture: an incoming reply with a
/// send effect, tapbacks and parts, then the owner's outgoing tapback on
/// it. Every iMessage-only field a writer might mirror is set, so a format
/// that must not leak them has something to leak.
pub fn sample_imessage_document() -> ConversationDocument {
    let mut doc = ConversationDocument {
        schema_version: SCHEMA_VERSION,
        export: ExportMeta {
            source: "imessage".into(),
            tool: "imessage-ir-exporter".into(),
            tool_version: "0.1.0".into(),
            owner_handle: Some("+15555550100".into()),
            owner_display_name: Some("Me".into()),
        },
        conversation: ConversationMeta {
            chat_identifier: "+15555550101".into(),
            conversation_type: IrConversationType::Individual,
            group_title: None,
            participants: vec![IrParticipant {
                handle: Some("+15555550101".into()),
                display_name: Some("Sam".into()),
                handle_type: Some(HandleType::Phone),
            }],
            stats: ConversationStats::default(),
        },
        messages: vec![
            IrMessage {
                guid: "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE".into(),
                timestamp_unix_ms: 1_400_773_261_000,
                direction: IrDirection::Incoming,
                service: IrService::IMessage,
                message_kind: IrMessageKind::IMessage,
                sender_handle: Some("+15555550101".into()),
                sender_display_name: Some("Sam".into()),
                subject: None,
                text: "hello imessage".into(),
                attachments: vec![],
                imessage: Some(IrImessage {
                    is_reply: true,
                    in_reply_to_guid: Some("parent-guid-1111".into()),
                    thread_originator_part: Some(0),
                    num_replies: Some(2),
                    send_effect: Some("Sent with Balloons".into()),
                    tapbacks: Some(json!([{"part_index": 0, "kind": "loved"}])),
                    parts: Some(json!([{"index": 0, "kind": "run", "text": "hello imessage"}])),
                    ..IrImessage::default()
                }),
                source: None,
            },
            IrMessage {
                guid: "TAPBACK-GUID-0001".into(),
                timestamp_unix_ms: 1_400_773_262_000,
                direction: IrDirection::Outgoing,
                service: IrService::IMessage,
                message_kind: IrMessageKind::Tapback,
                sender_handle: Some("+15555550100".into()),
                sender_display_name: Some("Me".into()),
                subject: None,
                text: "Loved a message".into(),
                attachments: vec![],
                imessage: Some(IrImessage {
                    associated_guid: Some("parent-guid-1111".into()),
                    associated_part: Some(0),
                    tapback_kind: Some("loved".into()),
                    tapback_action: Some("add".into()),
                    in_reply_to_guid: Some("parent-guid-1111".into()),
                    ..IrImessage::default()
                }),
                source: None,
            },
        ],
        packaging_stem_suffix: None,
    };
    doc.finalize_stats();
    doc
}
