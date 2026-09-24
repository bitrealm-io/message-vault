//! Structured helpers for mail headers (parts, edits, balloons).
//!

use imessage_database::{
    message_types::{
        app::AppMessage,
        edited::{EditStatus, EditedMessage},
        expressives::Expressive,
        text_effects::text_effect::TextEffect,
        url::URLMessage,
        variants::{BalloonProvider, CustomBalloon, URLOverride, Variant},
    },
    tables::{
        attachment::Attachment,
        messages::{
            Message,
            models::{BubbleComponent, SharedLocation},
        },
    },
    util::{
        bundle_id::parse_balloon_bundle_id, dates::get_local_time, platform::Platform,
        plist::parse_ns_keyed_archiver,
    },
};
use rusqlite::Connection;
use serde::Serialize;
use serde_json::{Value, json};
use std::path::Path;

use crate::body::{AttachmentResolver, resolve_run};

/// One historical edit (or unsent marker) for a body part.
#[derive(Serialize)]
pub(crate) struct EditEventRecord {
    pub part_index: usize,
    pub status: &'static str,
    pub text: String,
    pub timestamp: Option<String>,
    pub timestamp_utc: Option<String>,
    pub guid: Option<String>,
}

/// One logical message body part.
#[derive(Serialize)]
pub(crate) struct PartRecord {
    pub index: usize,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attachment_indices: Vec<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub emoji_image: bool,
}

/// Compact tapback row for parent `X-ME-Tapbacks`.
#[derive(Serialize)]
pub(crate) struct TapbackCell {
    pub part_index: usize,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reactor_handle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reactor_display_name: Option<String>,
}

/// Local and UTC RFC 3339 strings from a date result, or `(None, None)` on error.
pub(crate) fn optional_rfc3339(
    result: Result<
        chrono::DateTime<chrono::Local>,
        imessage_database::error::message::MessageError,
    >,
) -> (Option<String>, Option<String>) {
    match result {
        Ok(date) => (Some(date.to_rfc3339()), Some(date.to_utc().to_rfc3339())),
        Err(_) => (None, None),
    }
}

/// Screen-effect label (slam, loud, invisible ink, and similar), if any.
pub(crate) fn expressive_label(expressive: Expressive<'_>) -> Option<String> {
    let label = expressive.to_string();
    if label.is_empty() { None } else { Some(label) }
}

/// `"started"` or `"stopped"` for a live-location share.
pub(crate) fn shared_location_label(kind: SharedLocation) -> &'static str {
    match kind {
        SharedLocation::Started => "started",
        SharedLocation::Stopped => "stopped",
    }
}

/// Compact label for a typed-stream text effect.
fn effect_label(effect: &TextEffect) -> String {
    match effect {
        TextEffect::Default => "default".to_string(),
        TextEffect::Mention(id) => format!("mention:{id}"),
        TextEffect::Link(url) => format!("link:{url}"),
        TextEffect::OTP => "otp".to_string(),
        TextEffect::Address(_) => "address".to_string(),
        TextEffect::Styles(styles) => format!("styles:{styles:?}"),
        TextEffect::Animated(anim) => format!("animated:{anim:?}"),
        TextEffect::Conversion(_) => "conversion".to_string(),
        TextEffect::Currency(_) => "currency".to_string(),
        TextEffect::Tracking(_) => "tracking".to_string(),
        TextEffect::Flight(_) => "flight".to_string(),
    }
}

/// Build edit-history records from parsed [`EditedMessage`] metadata.
pub(crate) fn build_edit_records(edited: &EditedMessage, offset: &i64) -> Vec<EditEventRecord> {
    let mut out = Vec::new();
    for (part_index, part) in edited.parts.iter().enumerate() {
        let status = match part.status {
            EditStatus::Edited => "edited",
            EditStatus::Unsent => "unsent",
            EditStatus::Original => "original",
        };
        if part.edit_history.is_empty() {
            if matches!(part.status, EditStatus::Unsent | EditStatus::Edited) {
                out.push(EditEventRecord {
                    part_index,
                    status,
                    text: String::new(),
                    timestamp: None,
                    timestamp_utc: None,
                    guid: None,
                });
            }
            continue;
        }
        for event in &part.edit_history {
            let (timestamp, timestamp_utc) = optional_rfc3339(get_local_time(event.date, *offset));
            out.push(EditEventRecord {
                part_index,
                status,
                text: event.text.clone(),
                timestamp,
                timestamp_utc,
                guid: event.guid.clone(),
            });
        }
    }
    out
}

/// Build multipart `parts` from the message body components.
pub(crate) fn build_part_records(message: &Message, attachments: &[Attachment]) -> Vec<PartRecord> {
    if message.components.is_empty() {
        return Vec::new();
    }

    let mut resolver = AttachmentResolver::new(attachments);
    let mut parts = Vec::new();

    for (index, component) in message.components.iter().enumerate() {
        match component {
            BubbleComponent::Run(ranges) => {
                let resolved = resolve_run(ranges, &mut resolver);
                let mut texts = Vec::new();
                let mut attachment_indices = Vec::new();
                let mut effects = Vec::new();
                let mut emoji_image = false;

                for (range, att_idx) in resolved {
                    if let Some(idx) = att_idx {
                        if idx < attachments.len() {
                            attachment_indices.push(idx);
                        }
                    } else if let Some(text) = message.text.as_deref() {
                        let start = range.start.min(text.len());
                        let end = range.end.min(text.len());
                        if start < end {
                            texts.push(text[start..end].to_string());
                        }
                    }
                    for effect in &range.effects {
                        let label = effect_label(effect);
                        if label != "default" {
                            effects.push(label);
                        }
                    }
                    if range.emoji_image {
                        emoji_image = true;
                    }
                }

                let joined = texts.concat();
                parts.push(PartRecord {
                    index,
                    kind: "run",
                    text: if joined.is_empty() {
                        None
                    } else {
                        Some(joined)
                    },
                    attachment_indices,
                    effects,
                    emoji_image,
                });
            }
            BubbleComponent::App => parts.push(PartRecord {
                index,
                kind: "app",
                text: None,
                attachment_indices: Vec::new(),
                effects: Vec::new(),
                emoji_image: false,
            }),
            BubbleComponent::Retracted => parts.push(PartRecord {
                index,
                kind: "retracted",
                text: None,
                attachment_indices: Vec::new(),
                effects: Vec::new(),
                emoji_image: false,
            }),
        }
    }

    parts
}

/// Collect transcription text from body ranges matching an attachment GUID.
pub(crate) fn transcription_for_attachment(
    message: &Message,
    attachment: &Attachment,
) -> Option<String> {
    let guid = attachment.guid.as_deref()?;
    for component in &message.components {
        let BubbleComponent::Run(ranges) = component else {
            continue;
        };
        for range in ranges {
            if let Some(meta) = &range.attachment
                && meta.guid.as_deref() == Some(guid)
                && let Some(t) = &meta.transcription
                && !t.is_empty()
            {
                return Some(t.clone());
            }
        }
    }
    None
}

/// JSON object for an iMessage app bubble payload.
fn app_message_json(bubble: &AppMessage<'_>) -> Value {
    json!({
        "image": bubble.image,
        "url": bubble.url,
        "title": bubble.title,
        "subtitle": bubble.subtitle,
        "caption": bubble.caption,
        "subcaption": bubble.subcaption,
        "trailing_caption": bubble.trailing_caption,
        "trailing_subcaption": bubble.trailing_subcaption,
        "app_name": bubble.app_name,
        "ldtext": bubble.ldtext,
    })
}

/// JSON object for a rich-link (URL) bubble payload.
fn url_message_json(bubble: &URLMessage<'_>) -> Value {
    json!({
        "title": bubble.title,
        "summary": bubble.summary,
        "url": bubble.url,
        "original_url": bubble.original_url,
        "item_type": bubble.item_type,
        "images": bubble.images,
        "icons": bubble.icons,
        "site_name": bubble.site_name,
        "placeholder": bubble.placeholder,
    })
}

/// Best-effort structured balloon/app payload for archival.
pub(crate) fn build_balloon_value(db: &Connection, message: &Message) -> Option<Value> {
    let Variant::App(balloon) = message.variant() else {
        return None;
    };
    if message.is_handwriting() {
        return Some(json!({
            "kind": "handwriting",
            "bundle_id": message.balloon_bundle_id,
        }));
    }
    if message.is_digital_touch() {
        return Some(json!({
            "kind": "digital_touch",
            "bundle_id": message.balloon_bundle_id,
        }));
    }
    if message.is_poll() {
        return Some(poll_value(db, message));
    }
    let Some(payload) = message.payload_data(db) else {
        return Some(payloadless_value(&balloon, message));
    };
    let parsed = parse_ns_keyed_archiver(&payload).ok()?;
    if message.is_url() {
        return url_override_value(&parsed);
    }
    let bubble = AppMessage::from_map(&parsed).ok()?;
    Some(app_bubble_value(&balloon, message, &bubble))
}

/// The stable name of a balloon kind, as written in the `kind` field.
fn balloon_kind_name(balloon: &CustomBalloon) -> &'static str {
    match balloon {
        CustomBalloon::ApplePay => "apple_pay",
        CustomBalloon::Fitness => "fitness",
        CustomBalloon::Slideshow => "slideshow",
        CustomBalloon::CheckIn => "check_in",
        CustomBalloon::FindMy => "find_my",
        CustomBalloon::Business => "business",
        CustomBalloon::Application(_) => "application",
        CustomBalloon::URL => "url",
        CustomBalloon::Handwriting => "handwriting",
        CustomBalloon::DigitalTouch => "digital_touch",
        CustomBalloon::Polls => "poll",
    }
}

/// A poll's options and votes in display order, or an `unparseable` marker.
fn poll_value(db: &Connection, message: &Message) -> Value {
    let Ok(Some(poll)) = message.as_poll(db) else {
        return json!({ "kind": "poll", "error": "unparseable" });
    };
    let options: Vec<Value> = poll
        .order
        .iter()
        .filter_map(|id| {
            let opt = poll.options.get(id)?;
            Some(json!({
                "id": id,
                "text": opt.text,
                "creator": opt.creator,
                "votes": opt.votes.iter().map(|v| json!({
                    "voter": v.voter,
                    "option_id": v.option_id,
                })).collect::<Vec<_>>(),
            }))
        })
        .collect();
    json!({ "kind": "poll", "options": options })
}

/// A balloon with no payload blob: only its kind, bundle id, and text are known.
fn payloadless_value(balloon: &CustomBalloon, message: &Message) -> Value {
    if message.is_url() {
        return json!({ "kind": "url", "text": message.text });
    }
    json!({
        "kind": balloon_kind_name(balloon),
        "bundle_id": message.balloon_bundle_id,
        "text": message.text,
    })
}

/// A URL balloon's rich preview, by the override Messages stored for it.
fn url_override_value(parsed: &plist::Value) -> Option<Value> {
    let override_msg = URLMessage::get_url_message_override(parsed).ok()?;
    Some(match override_msg {
        URLOverride::Normal(b) => json!({ "kind": "url", "data": url_message_json(&b) }),
        URLOverride::AppleMusic(b) => json!({
            "kind": "apple_music",
            "data": {
                "url": b.url,
                "preview": b.preview,
                "artist": b.artist,
                "album": b.album,
                "track_name": b.track_name,
                "lyrics": b.lyrics,
            }
        }),
        URLOverride::Collaboration(b) => json!({
            "kind": "collaboration",
            "data": {
                "url": b.url,
                "title": b.title,
                "bundle_id": b.bundle_id,
            }
        }),
        URLOverride::AppStore(b) => json!({
            "kind": "app_store",
            "data": {
                "url": b.url,
                "original_url": b.original_url,
                "app_name": b.app_name,
                "description": b.description,
                "platform": b.platform,
                "genre": b.genre,
            }
        }),
        URLOverride::SharedPlacemark(b) => json!({
            "kind": "placemark",
            "data": {
                "url": b.url,
                "name": b.placemark.name,
                "address": b.placemark.address,
                "city": b.placemark.city,
                "state": b.placemark.state,
                "country": b.placemark.country,
                "postal_code": b.placemark.postal_code,
                "street": b.placemark.street,
            }
        }),
    })
}

/// An app balloon's parsed bubble. Third-party apps also carry the bundle id
/// as parsed from the balloon's own field.
fn app_bubble_value(balloon: &CustomBalloon, message: &Message, bubble: &AppMessage) -> Value {
    match balloon {
        CustomBalloon::Application(bundle_id) => json!({
            "kind": "application",
            "bundle_id": bundle_id,
            "parsed_bundle_id": parse_balloon_bundle_id(message.balloon_bundle_id.as_deref()),
            "data": app_message_json(bubble),
        }),
        other => json!({
            "kind": balloon_kind_name(other),
            "bundle_id": message.balloon_bundle_id,
            "data": app_message_json(bubble),
        }),
    }
}

/// Balloon kind string for `X-ME-Balloon-Kind` (from `X-ME-App` JSON or variant).
pub(crate) fn balloon_kind_label(app: &Value) -> Option<String> {
    app.get("kind").and_then(|v| v.as_str()).map(str::to_string)
}

/// Plain-text summary for balloon messages.
pub(crate) fn balloon_summary(app: &Value, fallback_text: Option<&str>) -> String {
    if let Some(data) = app.get("data") {
        for key in ["title", "caption", "app_name", "url", "track_name", "name"] {
            if let Some(s) = data
                .get(key)
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                return s.to_string();
            }
        }
    }
    if let Some(t) = app
        .get("text")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        return t.to_string();
    }
    if let Some(t) = fallback_text.map(str::trim).filter(|t| !t.is_empty()) {
        return t.to_string();
    }
    match app.get("kind").and_then(|v| v.as_str()) {
        Some("handwriting") => "Handwriting".into(),
        Some("digital_touch") => "Digital Touch".into(),
        Some("poll") => "Poll".into(),
        Some("apple_pay") => "Apple Pay".into(),
        Some(other) => other.replace('_', " "),
        None => "App Message".into(),
    }
}

/// Sticker presentation extras when available from the attachment row.
pub(crate) fn sticker_extras(
    attachment: &Attachment,
    platform: &Platform,
    db_path: &Path,
    attachment_root: Option<&str>,
) -> (Option<String>, Option<String>) {
    if !attachment.is_sticker {
        return (None, None);
    }
    let prompt = attachment.emoji_description.clone();
    let effect = attachment
        .get_sticker_effect(platform, db_path, attachment_root)
        .ok()
        .flatten()
        .map(|e| format!("{e:?}"));
    (prompt, effect)
}

/// Parse the part index from a thread identifier like `"0/1"`.
pub(crate) fn parse_thread_part(part: &str) -> Option<u32> {
    part.split('/').next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use imessage_database::{
        error::message::MessageError,
        message_types::{
            edited::{EditedEvent, EditedMessagePart},
            expressives::{BubbleEffect, Expressive},
            text_effects::{style::Style, text_effect::TextEffect},
        },
        tables::messages::models::{AttachmentMeta, AttributedRange, BubbleComponent},
        util::dates::get_offset,
    };

    /// A message row with every field blank, for tests that set a few.
    ///
    /// `Message` has no `Default` and all of its fields are public, so this
    /// is the constructor. A new field in a later `imessage-database`
    /// release shows up here as a compile error, which is wanted.
    fn message() -> Message {
        Message {
            rowid: 1,
            guid: "guid-1".to_string(),
            text: None,
            service: Some("iMessage".to_string()),
            handle_id: Some(1),
            destination_caller_id: None,
            subject: None,
            date: 0,
            date_read: 0,
            date_delivered: 0,
            is_from_me: false,
            is_read: false,
            item_type: 0,
            other_handle: None,
            share_status: false,
            share_direction: None,
            group_title: None,
            group_action_type: 0,
            associated_message_guid: None,
            associated_message_type: Some(0),
            balloon_bundle_id: None,
            expressive_send_style_id: None,
            thread_originator_guid: None,
            thread_originator_part: None,
            date_edited: 0,
            associated_message_emoji: None,
            chat_id: Some(1),
            num_attachments: 0,
            deleted_from: None,
            num_replies: 0,
            components: Vec::new(),
            edited_parts: None,
        }
    }

    /// A message whose balloon is the given bundle id.
    fn balloon_message(bundle_id: &str) -> Message {
        Message {
            balloon_bundle_id: Some(bundle_id.to_string()),
            ..message()
        }
    }

    fn attachment(guid: Option<&str>) -> Attachment {
        Attachment {
            rowid: 0,
            guid: guid.map(str::to_string),
            filename: None,
            uti: None,
            mime_type: None,
            transfer_name: None,
            total_bytes: 0,
            is_sticker: false,
            hide_attachment: 0,
            emoji_description: None,
            copied_path: None,
        }
    }

    fn app_message<'a>() -> AppMessage<'a> {
        AppMessage {
            image: None,
            url: Some("https://example.com/game"),
            title: Some("A game"),
            subtitle: None,
            caption: Some("Your move"),
            subcaption: None,
            trailing_caption: None,
            trailing_subcaption: None,
            app_name: Some("Games"),
            ldtext: None,
        }
    }

    /// `{"richLinkMetadata": {"title": ..., "URL": {"URL": ...}}}`, the
    /// shape a URL balloon's payload takes once the archive is unwrapped.
    fn url_payload(title: &str, url: &str) -> plist::Value {
        let mut inner = plist::Dictionary::new();
        inner.insert("URL".to_string(), plist::Value::String(url.to_string()));
        let mut metadata = plist::Dictionary::new();
        metadata.insert("title".to_string(), plist::Value::String(title.to_string()));
        metadata.insert("URL".to_string(), plist::Value::Dictionary(inner));
        let mut root = plist::Dictionary::new();
        root.insert(
            "richLinkMetadata".to_string(),
            plist::Value::Dictionary(metadata),
        );
        plist::Value::Dictionary(root)
    }

    #[test]
    fn a_date_becomes_two_rfc3339_strings_and_an_error_becomes_none() {
        // The offset is the 1970-to-2001 epoch difference, as the session
        // carries it; a stamp of zero is then Apple's epoch.
        let (local, utc) = optional_rfc3339(get_local_time(0, get_offset()));
        assert_eq!(utc.as_deref(), Some("2001-01-01T00:00:00+00:00"));
        assert!(local.is_some());

        assert_eq!(
            optional_rfc3339(Err(MessageError::InvalidTimestamp(0))),
            (None, None)
        );
    }

    #[test]
    fn an_expressive_is_its_label_and_no_expressive_is_none() {
        assert_eq!(
            expressive_label(Expressive::Bubble(BubbleEffect::Slam)).as_deref(),
            Some("Sent with Slam")
        );
        assert_eq!(expressive_label(Expressive::None), None);
    }

    #[test]
    fn a_shared_location_is_started_or_stopped() {
        assert_eq!(shared_location_label(SharedLocation::Started), "started");
        assert_eq!(shared_location_label(SharedLocation::Stopped), "stopped");
    }

    #[test]
    fn text_effects_are_labelled() {
        assert_eq!(effect_label(&TextEffect::Default), "default");
        assert_eq!(
            effect_label(&TextEffect::Mention("+1555".to_string())),
            "mention:+1555"
        );
        assert_eq!(
            effect_label(&TextEffect::Link("https://example.com".to_string())),
            "link:https://example.com"
        );
        assert_eq!(effect_label(&TextEffect::OTP), "otp");
        assert!(effect_label(&TextEffect::Styles(vec![Style::Bold])).starts_with("styles:"));
    }

    /// An unsent part with no history still gets one record, so the app
    /// knows a part was unsent; an original part with no history gets none.
    #[test]
    fn edit_records_follow_the_edit_history_and_mark_unsent_parts() {
        let edited = EditedMessage {
            parts: vec![
                EditedMessagePart {
                    status: EditStatus::Original,
                    edit_history: Vec::new(),
                },
                EditedMessagePart {
                    status: EditStatus::Unsent,
                    edit_history: Vec::new(),
                },
                EditedMessagePart {
                    status: EditStatus::Edited,
                    edit_history: vec![
                        EditedEvent {
                            date: 0,
                            text: "first".to_string(),
                            components: Vec::new(),
                            guid: None,
                        },
                        EditedEvent {
                            date: 60,
                            text: "second".to_string(),
                            components: Vec::new(),
                            guid: Some("edit-guid".to_string()),
                        },
                    ],
                },
            ],
        };

        let records = build_edit_records(&edited, &get_offset());
        assert_eq!(records.len(), 3);
        assert_eq!((records[0].part_index, records[0].status), (1, "unsent"));
        assert_eq!(records[0].text, "");
        assert_eq!(records[0].timestamp_utc, None);
        assert_eq!((records[1].part_index, records[1].status), (2, "edited"));
        assert_eq!(records[1].text, "first");
        assert_eq!(
            records[1].timestamp_utc.as_deref(),
            Some("2001-01-01T00:00:00+00:00")
        );
        assert_eq!(records[2].text, "second");
        assert_eq!(records[2].guid.as_deref(), Some("edit-guid"));
        assert_eq!(
            records[2].timestamp_utc.as_deref(),
            Some("2001-01-01T00:01:00+00:00")
        );
    }

    #[test]
    fn parts_carry_text_attachments_effects_and_the_emoji_hint() {
        let attachments = vec![attachment(Some("att-1"))];
        let text = "hi \u{FFFC} there";
        let mut msg = Message {
            text: Some(text.to_string()),
            ..message()
        };
        msg.components = vec![
            BubbleComponent::Run(vec![
                AttributedRange::text(0, 3, vec![TextEffect::Default, TextEffect::OTP]),
                AttributedRange::inline_attachment(
                    3,
                    6,
                    AttachmentMeta {
                        guid: Some("att-1".to_string()),
                        ..AttachmentMeta::default()
                    },
                ),
                AttributedRange::text(6, text.len(), Vec::new()),
            ]),
            BubbleComponent::App,
            BubbleComponent::Retracted,
        ];

        let parts = build_part_records(&msg, &attachments);
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].kind, "run");
        assert_eq!(parts[0].text.as_deref(), Some("hi  there"));
        assert_eq!(parts[0].attachment_indices, vec![0]);
        assert_eq!(parts[0].effects, vec!["otp".to_string()]);
        assert!(parts[0].emoji_image);
        assert_eq!((parts[1].index, parts[1].kind), (1, "app"));
        assert_eq!((parts[2].index, parts[2].kind), (2, "retracted"));

        // A body that never parsed has no parts at all.
        assert!(build_part_records(&message(), &attachments).is_empty());
    }

    /// Each part carries the rows its own ranges refer to, by GUID or by
    /// position, and a range with no row left to point at adds nothing.
    #[test]
    fn parts_carry_only_the_attachment_rows_that_exist() {
        let attachments = vec![attachment(None), attachment(Some("att-b"))];
        let meta = |guid: Option<&str>| AttachmentMeta {
            guid: guid.map(str::to_string),
            ..AttachmentMeta::default()
        };
        let mut msg = message();
        msg.components = vec![
            BubbleComponent::Run(vec![AttributedRange::attachment(0, 1, meta(Some("att-b")))]),
            BubbleComponent::Run(vec![
                AttributedRange::attachment(1, 2, meta(None)),
                AttributedRange::attachment(2, 3, meta(None)),
            ]),
        ];

        let parts = build_part_records(&msg, &attachments);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].attachment_indices, vec![1]);
        assert_eq!(
            parts[1].attachment_indices,
            vec![0],
            "the second range in the part has no row left"
        );
    }

    #[test]
    fn a_transcription_is_found_by_attachment_guid() {
        let mut msg = message();
        msg.components = vec![BubbleComponent::Run(vec![AttributedRange::attachment(
            0,
            1,
            AttachmentMeta {
                guid: Some("voice-1".to_string()),
                transcription: Some("call me back".to_string()),
                ..AttachmentMeta::default()
            },
        )])];
        assert_eq!(
            transcription_for_attachment(&msg, &attachment(Some("voice-1"))).as_deref(),
            Some("call me back")
        );
        assert_eq!(
            transcription_for_attachment(&msg, &attachment(Some("other"))),
            None
        );
        assert_eq!(transcription_for_attachment(&msg, &attachment(None)), None);
    }

    #[test]
    fn app_and_url_bubbles_serialize_every_field() {
        let app = app_message_json(&app_message());
        assert_eq!(app["title"], "A game");
        assert_eq!(app["app_name"], "Games");
        assert_eq!(app["image"], Value::Null);

        let url = url_message_json(&URLMessage {
            title: Some("Example"),
            url: Some("https://example.com"),
            images: vec!["https://example.com/a.png"],
            placeholder: true,
            ..URLMessage::default()
        });
        assert_eq!(url["title"], "Example");
        assert_eq!(url["images"][0], "https://example.com/a.png");
        assert_eq!(url["placeholder"], true);
        assert_eq!(url["summary"], Value::Null);
    }

    #[test]
    fn every_balloon_kind_has_a_stable_name() {
        assert_eq!(balloon_kind_name(&CustomBalloon::ApplePay), "apple_pay");
        assert_eq!(balloon_kind_name(&CustomBalloon::Fitness), "fitness");
        assert_eq!(balloon_kind_name(&CustomBalloon::Slideshow), "slideshow");
        assert_eq!(balloon_kind_name(&CustomBalloon::CheckIn), "check_in");
        assert_eq!(balloon_kind_name(&CustomBalloon::FindMy), "find_my");
        assert_eq!(balloon_kind_name(&CustomBalloon::Business), "business");
        assert_eq!(
            balloon_kind_name(&CustomBalloon::Application("com.example")),
            "application"
        );
        assert_eq!(balloon_kind_name(&CustomBalloon::URL), "url");
        assert_eq!(
            balloon_kind_name(&CustomBalloon::Handwriting),
            "handwriting"
        );
        assert_eq!(
            balloon_kind_name(&CustomBalloon::DigitalTouch),
            "digital_touch"
        );
        assert_eq!(balloon_kind_name(&CustomBalloon::Polls), "poll");
    }

    #[test]
    fn a_balloon_without_a_payload_keeps_its_kind_bundle_id_and_text() {
        let msg = Message {
            text: Some("Sent $5".to_string()),
            ..balloon_message("com.apple.PassbookUIService.PeerPaymentMessagesExtension")
        };
        let value = payloadless_value(&CustomBalloon::ApplePay, &msg);
        assert_eq!(value["kind"], "apple_pay");
        assert_eq!(
            value["bundle_id"],
            "com.apple.PassbookUIService.PeerPaymentMessagesExtension"
        );
        assert_eq!(value["text"], "Sent $5");

        // A URL balloon with no payload is only its text.
        let msg = Message {
            text: Some("https://example.com".to_string()),
            ..balloon_message("com.apple.messages.URLBalloonProvider")
        };
        let value = payloadless_value(&CustomBalloon::URL, &msg);
        assert_eq!(
            value,
            json!({ "kind": "url", "text": "https://example.com" })
        );
    }

    #[test]
    fn a_url_override_is_a_normal_rich_link_unless_it_is_something_more() {
        let value = url_override_value(&url_payload("Example", "https://example.com")).unwrap();
        assert_eq!(value["kind"], "url");
        assert_eq!(value["data"]["title"], "Example");
        assert_eq!(value["data"]["url"], "https://example.com");

        // A payload with no metadata is not a rich link.
        assert_eq!(
            url_override_value(&plist::Value::Dictionary(plist::Dictionary::new())),
            None
        );
    }

    /// A third-party app carries the bundle id twice: as the variant parsed
    /// it and as the row stores it, so the app can tell the two apart when
    /// Messages wrapped the id in a
    /// `com.apple.messages.MSMessageExtensionBalloonPlugin:...:` prefix.
    #[test]
    fn an_app_bubble_names_the_app_and_carries_its_data() {
        let msg = balloon_message(
            "com.apple.messages.MSMessageExtensionBalloonPlugin:0000000000:com.example.game.MessagesExtension",
        );
        let value = app_bubble_value(
            &CustomBalloon::Application("com.example.game.MessagesExtension"),
            &msg,
            &app_message(),
        );
        assert_eq!(value["kind"], "application");
        assert_eq!(value["bundle_id"], "com.example.game.MessagesExtension");
        assert_eq!(
            value["parsed_bundle_id"],
            "com.example.game.MessagesExtension"
        );
        assert_eq!(value["data"]["caption"], "Your move");

        let msg = balloon_message("com.apple.ActivityMessagesApp.MessagesExtension");
        let value = app_bubble_value(&CustomBalloon::Fitness, &msg, &app_message());
        assert_eq!(value["kind"], "fitness");
        assert_eq!(
            value["bundle_id"],
            "com.apple.ActivityMessagesApp.MessagesExtension"
        );
        assert_eq!(value["data"]["title"], "A game");
    }

    /// The whole balloon path over an empty database: a plain text is no
    /// balloon, handwriting and Digital Touch never read the payload, a
    /// poll with no payload is marked unparseable, and an app with no
    /// payload blob falls back to the payloadless shape.
    #[test]
    fn balloon_values_over_a_database_with_no_payloads() {
        let db = Connection::open_in_memory().unwrap();

        assert_eq!(build_balloon_value(&db, &message()), None);

        let value = build_balloon_value(
            &db,
            &balloon_message("com.apple.Handwriting.HandwritingProvider"),
        )
        .unwrap();
        assert_eq!(value["kind"], "handwriting");

        let value = build_balloon_value(
            &db,
            &balloon_message("com.apple.DigitalTouchBalloonProvider"),
        )
        .unwrap();
        assert_eq!(value["kind"], "digital_touch");

        let value = build_balloon_value(&db, &balloon_message("com.apple.messages.Polls")).unwrap();
        assert_eq!(value, json!({ "kind": "poll", "error": "unparseable" }));
        assert_eq!(
            poll_value(&db, &balloon_message("com.apple.messages.Polls")),
            json!({ "kind": "poll", "error": "unparseable" })
        );

        let msg = Message {
            text: Some("Your move".to_string()),
            ..balloon_message("com.example.game")
        };
        let value = build_balloon_value(&db, &msg).unwrap();
        assert_eq!(value["kind"], "application");
        assert_eq!(value["bundle_id"], "com.example.game");
        assert_eq!(value["text"], "Your move");
    }

    #[test]
    fn the_balloon_kind_is_read_off_the_value() {
        assert_eq!(
            balloon_kind_label(&json!({ "kind": "url" })).as_deref(),
            Some("url")
        );
        assert_eq!(balloon_kind_label(&json!({ "data": {} })), None);
    }

    /// The summary prefers the first non-empty field of the data in a fixed
    /// order, then the balloon's own text, then the row's text, and only
    /// then names the kind.
    #[test]
    fn a_balloon_summary_prefers_data_then_text_then_the_kind() {
        assert_eq!(
            balloon_summary(
                &json!({ "kind": "url", "data": { "title": "", "url": "https://x" } }),
                Some("ignored")
            ),
            "https://x"
        );
        assert_eq!(
            balloon_summary(&json!({ "kind": "url", "text": "the text" }), None),
            "the text"
        );
        assert_eq!(
            balloon_summary(&json!({ "kind": "fitness" }), Some("  a workout  ")),
            "a workout"
        );
        assert_eq!(
            balloon_summary(&json!({ "kind": "handwriting" }), None),
            "Handwriting"
        );
        assert_eq!(
            balloon_summary(&json!({ "kind": "digital_touch" }), Some("  ")),
            "Digital Touch"
        );
        assert_eq!(balloon_summary(&json!({ "kind": "poll" }), None), "Poll");
        assert_eq!(
            balloon_summary(&json!({ "kind": "apple_pay" }), None),
            "Apple Pay"
        );
        assert_eq!(
            balloon_summary(&json!({ "kind": "check_in" }), None),
            "check in"
        );
        assert_eq!(balloon_summary(&json!({}), None), "App Message");
    }

    #[test]
    fn sticker_extras_are_empty_unless_the_attachment_is_a_sticker() {
        let plain = attachment(None);
        assert_eq!(
            sticker_extras(&plain, &Platform::macOS, Path::new("chat.db"), None),
            (None, None)
        );

        // A sticker whose file is gone still reports its prompt; the effect
        // needs the file and is `None` without it.
        let sticker = Attachment {
            is_sticker: true,
            emoji_description: Some("a cat".to_string()),
            filename: Some("/nowhere/sticker.heic".to_string()),
            ..attachment(Some("sticker-1"))
        };
        let (prompt, effect) =
            sticker_extras(&sticker, &Platform::macOS, Path::new("chat.db"), None);
        assert_eq!(prompt.as_deref(), Some("a cat"));
        assert_eq!(effect, None);
    }

    #[test]
    fn a_thread_part_is_the_number_before_the_slash() {
        assert_eq!(parse_thread_part("0/1"), Some(0));
        assert_eq!(parse_thread_part("3/ABC-DEF"), Some(3));
        assert_eq!(parse_thread_part("7"), Some(7));
        assert_eq!(parse_thread_part("x/1"), None);
        assert_eq!(parse_thread_part(""), None);
    }
}
