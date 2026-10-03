use super::*;
use message_staging::AttachmentSpool;
use serde_json::{Map, json};
use std::fs;

const OWNER: &str = "+15555550100";

/// Write `docs` with a session, then read the backup back with the SBR reader.
fn round_trip(docs: &[ConversationDocument]) -> Vec<ConversationDocument> {
    let tmp = tempfile::tempdir().unwrap();
    let mut session = SbrBackupSession::create(tmp.path()).unwrap();
    for doc in docs {
        session.append_document(doc).unwrap();
    }
    let path = session.finish().unwrap();
    let owners = [OWNER.to_string()];
    let (read, report) = read_with_bytes(&path, &owners);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    read
}

/// Read `path` with the SBR reader, then put each attachment's bytes back
/// on it from the spool, so a test can compare payloads.
fn read_with_bytes(
    path: &Path,
    owners: &[String],
) -> (Vec<ConversationDocument>, crate::ReadReport) {
    let spool_dir = tempfile::tempdir().unwrap();
    let spool = AttachmentSpool::open(spool_dir.path()).unwrap();
    let (mut docs, report) = crate::read_backup(path, read_options(owners, &spool)).unwrap();
    for att in docs
        .iter_mut()
        .flat_map(|doc| doc.messages.iter_mut())
        .flat_map(|msg| msg.attachments.iter_mut())
    {
        att.bytes = att
            .digest_sha256
            .as_deref()
            .and_then(|digest| spool.path(digest))
            .map(|spooled| fs::read(spooled).unwrap());
    }
    (docs, report)
}

/// Reader options that spool attachment payloads and stage nothing.
fn read_options<'a>(owners: &'a [String], spool: &'a AttachmentSpool) -> crate::ReadOptions<'a> {
    crate::ReadOptions {
        owner_phones: owners,
        attachments_dir: None,
        spool: Some(spool),
        stage_attachments: false,
        media: media::MediaMode::Disabled,
        compress: media::CompressOptions::default(),
        log: None,
        progress: None,
        cancel: None,
    }
}

/// Each `<addr>` of an MMS as (address, type).
fn addrs(msg: &IrMessage) -> Vec<(String, String)> {
    msg.source.as_ref().unwrap().fields["addrs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            (
                a["address"].as_str().unwrap().to_string(),
                a["type"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

/// Each MMS part's name with the bytes its `data` decoded to, if any.
fn part_payloads(msg: &IrMessage) -> Vec<(String, Option<Vec<u8>>)> {
    msg.source.as_ref().unwrap().fields["parts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let name = p["name"].as_str().or(p["ct"].as_str()).unwrap();
            let bytes = p["data_sha256"].as_str().map(|digest| {
                msg.attachments
                    .iter()
                    .find(|a| a.digest_sha256.as_deref() == Some(digest))
                    .and_then(|a| a.bytes.clone())
                    .unwrap()
            });
            (name.to_string(), bytes)
        })
        .collect()
}

fn ir_attachment(name: &str, bytes: &[u8]) -> IrAttachment {
    IrAttachment {
        path: None,
        original_name: Some(name.into()),
        mime_type: Some("image/jpeg".into()),
        digest_sha256: None,
        is_sticker: false,
        transcription: None,
        sticker_effect: None,
        size_bytes: None,
        missing_reason: None,
        bytes: Some(bytes.to_vec()),
    }
}

fn pair(address: &str, addr_type: &str) -> (String, String) {
    (address.to_string(), addr_type.to_string())
}

#[test]
fn a_group_mms_keeps_its_addresses_and_attachment() {
    let mut doc = message_ir::testutil::sample_document("look");
    doc.conversation.chat_identifier = "chat-group".into();
    doc.conversation.conversation_type = IrConversationType::Group;
    doc.conversation.participants = vec![
        message_ir::IrParticipant {
            handle: Some("+15555550101".into()),
            display_name: Some("Ana".into()),
            handle_type: Some(message_ir::HandleType::Phone),
        },
        message_ir::IrParticipant {
            handle: Some("+15555550102".into()),
            display_name: Some("Lee".into()),
            handle_type: Some(message_ir::HandleType::Phone),
        },
    ];
    let incoming = &mut doc.messages[0];
    incoming.message_kind = IrMessageKind::Mms;
    incoming.source = None;
    incoming.sender_handle = Some("+15555550102".into());
    incoming.sender_display_name = Some("Lee".into());
    incoming.attachments = vec![ir_attachment("pic.jpg", b"first image")];
    let mut outgoing = incoming.clone();
    outgoing.guid = "outgoing".into();
    outgoing.timestamp_unix_ms += 1000;
    outgoing.direction = IrDirection::Outgoing;
    outgoing.sender_handle = Some(OWNER.into());
    outgoing.sender_display_name = None;
    outgoing.text = "nice".into();
    outgoing.attachments.clear();
    doc.messages.push(outgoing);

    let read = round_trip(&[doc]);
    assert_eq!(read.len(), 1, "both messages land in one group");
    let [incoming, outgoing] = read[0].messages.as_slice() else {
        panic!("two messages: {:?}", read[0].messages)
    };

    assert_eq!(incoming.direction, IrDirection::Incoming);
    assert_eq!(incoming.text, "look");
    assert_eq!(incoming.sender_handle.as_deref(), Some("+15555550102"));
    // A group MMS's `contact_name` names the group, so the reader takes no
    // sender name from it.
    assert_eq!(incoming.sender_display_name, None);
    assert_eq!(
        addrs(incoming),
        [
            pair("+15555550102", MMS_ADDR_FROM),
            pair(OWNER, MMS_ADDR_TO),
            pair("+15555550101", MMS_ADDR_TO),
        ]
    );
    assert_eq!(
        part_payloads(incoming),
        [
            ("text_0.txt".to_string(), None),
            ("pic.jpg".to_string(), Some(b"first image".to_vec())),
        ]
    );

    assert_eq!(outgoing.direction, IrDirection::Outgoing);
    assert_eq!(outgoing.text, "nice");
    assert_eq!(
        addrs(outgoing),
        [
            pair(OWNER, MMS_ADDR_FROM),
            pair("+15555550101", MMS_ADDR_TO),
            pair("+15555550102", MMS_ADDR_TO),
        ]
    );
}

#[test]
fn an_incoming_sms_keeps_its_sender_and_contact_name() {
    let mut doc = message_ir::testutil::sample_document("hello ir");
    doc.messages[0].source = None;
    let read = round_trip(&[doc]);
    let conversation = &read[0].conversation;
    assert_eq!(conversation.participants.len(), 1);
    assert_eq!(
        conversation.participants[0].handle.as_deref(),
        Some("+15555550101")
    );
    assert_eq!(
        conversation.participants[0].display_name.as_deref(),
        Some("Sam")
    );
    let msg = &read[0].messages[0];
    assert_eq!(
        msg.message_kind,
        IrMessageKind::Sms,
        "a text stays an <sms>"
    );
    assert_eq!(msg.direction, IrDirection::Incoming);
    assert_eq!(msg.sender_handle.as_deref(), Some("+15555550101"));
    assert_eq!(msg.text, "hello ir");
}

/// An SMS from another app, such as iMessage's SMS fallback, has no SBR
/// fields to restore, so the element is built from the message alone. A
/// photo in a 1:1 chat makes it an `<mms>`, whatever kind the source app
/// called the message.
#[test]
fn a_direct_photo_from_another_app_keeps_its_chat_sender_subject_and_bytes() {
    let mut doc = message_ir::testutil::sample_document("look at this");
    let msg = &mut doc.messages[0];
    msg.source = None;
    msg.message_kind = IrMessageKind::Sms;
    msg.subject = Some("Holiday".into());
    msg.attachments = vec![IrAttachment {
        mime_type: Some("image/png".into()),
        ..ir_attachment("beach.png", b"png bytes")
    }];

    let read = round_trip(&[doc]);
    assert_eq!(read.len(), 1);
    let conversation = &read[0].conversation;
    assert_eq!(conversation.chat_identifier, "+15555550101");
    assert_eq!(
        conversation.conversation_type,
        IrConversationType::Individual
    );
    assert_eq!(
        conversation.participants[0].display_name.as_deref(),
        Some("Sam")
    );
    let msg = &read[0].messages[0];
    assert_eq!(msg.direction, IrDirection::Incoming);
    assert_eq!(msg.sender_handle.as_deref(), Some("+15555550101"));
    assert_eq!(msg.subject.as_deref(), Some("Holiday"));
    assert_eq!(msg.text, "look at this");
    assert_eq!(msg.attachments.len(), 1);
    let photo = &msg.attachments[0];
    assert_eq!(photo.original_name.as_deref(), Some("beach.png"));
    assert_eq!(photo.mime_type.as_deref(), Some("image/png"));
    assert_eq!(photo.bytes.as_deref(), Some(b"png bytes".as_slice()));
}

/// A group message with no attachment is still an `<mms>`: an `<sms>` has
/// one address and would land in a 1:1 chat with whoever sent it.
#[test]
fn a_text_only_group_message_from_another_app_stays_in_its_group() {
    let mut doc = message_ir::testutil::sample_document("who is in?");
    doc.conversation.chat_identifier = "chat-group".into();
    doc.conversation.conversation_type = IrConversationType::Group;
    doc.conversation
        .participants
        .push(message_ir::IrParticipant {
            handle: Some("+15555550102".into()),
            display_name: Some("Lee".into()),
            handle_type: Some(message_ir::HandleType::Phone),
        });
    let incoming = &mut doc.messages[0];
    incoming.source = None;
    incoming.message_kind = IrMessageKind::Sms;
    incoming.sender_handle = Some("+15555550102".into());
    incoming.sender_display_name = Some("Lee".into());
    let mut outgoing = incoming.clone();
    outgoing.timestamp_unix_ms += 1000;
    outgoing.direction = IrDirection::Outgoing;
    outgoing.sender_handle = Some(OWNER.into());
    outgoing.sender_display_name = None;
    outgoing.text = "me".into();
    doc.messages.push(outgoing);

    let read = round_trip(&[doc]);
    assert_eq!(read.len(), 1, "both messages land in one group");
    let conversation = &read[0].conversation;
    assert_eq!(conversation.conversation_type, IrConversationType::Group);
    let mut handles: Vec<_> = conversation
        .participants
        .iter()
        .filter_map(|p| p.handle.as_deref())
        .collect();
    handles.sort_unstable();
    assert_eq!(handles, ["+15555550101", "+15555550102"]);
    let [incoming, outgoing] = read[0].messages.as_slice() else {
        panic!("two messages: {:?}", read[0].messages)
    };
    assert_eq!(incoming.direction, IrDirection::Incoming);
    assert_eq!(incoming.sender_handle.as_deref(), Some("+15555550102"));
    assert_eq!(incoming.sender_display_name, None);
    assert_eq!(incoming.text, "who is in?");
    assert!(incoming.attachments.is_empty());
    assert_eq!(outgoing.direction, IrDirection::Outgoing);
    assert_eq!(outgoing.text, "me");
    assert_eq!(
        outgoing.source.as_ref().unwrap().fields["attrs"]["address"],
        "+15555550101~+15555550102"
    );
}

/// Apps other than SMS Backup & Restore put the owner's own address on a
/// sent message. The `<sms>` is still addressed to the other person.
#[test]
fn a_sent_sms_carrying_the_owners_address_stays_in_the_peers_chat() {
    let mut doc = message_ir::testutil::sample_document("on my way");
    let msg = &mut doc.messages[0];
    msg.source = None;
    msg.direction = IrDirection::Outgoing;
    msg.sender_handle = Some(OWNER.into());
    msg.sender_display_name = Some("Me".into());
    msg.subject = Some("Tonight".into());

    let read = round_trip(&[doc]);
    assert_eq!(read.len(), 1);
    let conversation = &read[0].conversation;
    assert_eq!(conversation.chat_identifier, "+15555550101");
    assert_eq!(conversation.participants.len(), 1);
    assert_eq!(
        conversation.participants[0].handle.as_deref(),
        Some("+15555550101")
    );
    assert_eq!(
        conversation.participants[0].display_name.as_deref(),
        Some("Sam")
    );
    let msg = &read[0].messages[0];
    assert_eq!(msg.message_kind, IrMessageKind::Sms);
    assert_eq!(msg.direction, IrDirection::Outgoing);
    assert_eq!(msg.subject.as_deref(), Some("Tonight"));
    assert_eq!(msg.text, "on my way");
}

/// A received message whose sender address is an empty string is from the
/// chat's other person, as an `<sms>` and as an `<mms>`.
#[test]
fn a_received_message_with_an_empty_sender_address_is_from_the_peer() {
    let mut doc = message_ir::testutil::sample_document("text");
    let text = &mut doc.messages[0];
    text.source = None;
    text.sender_handle = Some(String::new());
    let mut photo = text.clone();
    photo.timestamp_unix_ms += 1000;
    photo.text = "photo".into();
    photo.attachments = vec![ir_attachment("pic.jpg", b"image")];
    doc.messages.push(photo);

    let read = round_trip(&[doc]);
    assert_eq!(read.len(), 1, "both messages land in the peer's chat");
    assert_eq!(read[0].conversation.chat_identifier, "+15555550101");
    let [text, photo] = read[0].messages.as_slice() else {
        panic!("two messages: {:?}", read[0].messages)
    };
    assert_eq!(text.sender_handle.as_deref(), Some("+15555550101"));
    assert_eq!(photo.sender_handle.as_deref(), Some("+15555550101"));
    assert_eq!(
        addrs(photo),
        [
            pair("+15555550101", MMS_ADDR_FROM),
            pair(OWNER, MMS_ADDR_TO)
        ]
    );
}

/// Attachment bytes go back on the part they came from, whether the
/// attachment still carries the part's digest or a media transform
/// cleared it. A text part and a part whose data was empty get none.
#[test]
fn restored_mms_parts_get_back_their_own_attachment_bytes() {
    let b64 = encode_part_data;
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.xml");
    fs::write(
        &input,
        format!(
            r#"<smses><mms date="1400773400000" msg_box="1" address="+15555550101"><parts><part seq="0" ct="application/smil" name="smil.xml" data="{}"/><part seq="1" ct="text/plain" name="text_0.txt" text="hi"/><part seq="2" ct="image/jpeg" name="a.jpg" data="{}"/><part seq="3" ct="image/png" name="empty.png" data=""/><part seq="4" ct="image/jpeg" name="bad.jpg" data="@@@not-base64@@@"/><part seq="5" ct="image/jpeg" name="c.jpg" data="{}"/><part seq="6" ct="image/jpeg" name="a-copy.jpg" data="{}"/></parts><addrs><addr address="+15555550101" type="137"/><addr address="{OWNER}" type="151"/></addrs></mms></smses>"#,
            b64(b"<smil><body/></smil>"),
            b64(b"first"),
            b64(b"second"),
            b64(b"first"),
        ),
    )
    .unwrap();
    let owners = [OWNER.to_string()];
    let (docs, _) = read_with_bytes(&input, &owners);
    assert_eq!(
        docs[0].messages[0].attachments.len(),
        3,
        "a.jpg, c.jpg, a-copy.jpg"
    );
    let payloads = |a_copy: Option<&[u8]>| {
        vec![
            ("smil.xml".to_string(), None),
            ("text_0.txt".to_string(), None),
            ("a.jpg".to_string(), Some(b"first".to_vec())),
            ("empty.png".to_string(), None),
            ("bad.jpg".to_string(), None),
            ("c.jpg".to_string(), Some(b"second".to_vec())),
            ("a-copy.jpg".to_string(), a_copy.map(<[u8]>::to_vec)),
        ]
    };

    let read = round_trip(&docs);
    assert_eq!(
        part_payloads(&read[0].messages[0]),
        payloads(Some(b"first"))
    );

    // The second attachment lost its digest: it falls back to the first
    // attachment not already taken, not to the first in the list.
    let mut transformed = docs.clone();
    transformed[0].messages[0].attachments[1].digest_sha256 = None;
    let read = round_trip(&transformed);
    assert_eq!(
        part_payloads(&read[0].messages[0]),
        payloads(Some(b"first"))
    );

    // No attachment has a digest: they go to the payload parts in order,
    // and with the third attachment gone the last part, with nothing left
    // to take, gets no data.
    transformed[0].messages[0].attachments.truncate(2);
    for att in &mut transformed[0].messages[0].attachments {
        att.digest_sha256 = None;
    }
    let read = round_trip(&transformed);
    assert_eq!(part_payloads(&read[0].messages[0]), payloads(None));
}

/// SMS Backup & Restore can describe only SMS and MMS. An iMessage or a
/// WhatsApp message written as `<sms>` would come back from a re-import as
/// an SMS under a new id, so the archive leaves it out and counts it (ADR 0021).
#[test]
fn the_archive_writes_only_sms_and_mms_and_counts_the_rest() {
    let mut imessage = message_ir::testutil::sample_imessage_document();
    // iMessage's SMS fallback is an SMS, whatever app it came from.
    let mut fallback = imessage.messages[0].clone();
    fallback.guid = "SMS-FALLBACK-0001".into();
    fallback.service = message_ir::IrService::Sms;
    fallback.message_kind = IrMessageKind::Sms;
    fallback.imessage = None;
    fallback.text = "hello fallback".into();
    imessage.messages.push(fallback);
    // A Mac `chat.db` row with no service is read as an SMS by its kind.
    let mut no_service = imessage.messages[0].clone();
    no_service.guid = "NO-SERVICE-0001".into();
    no_service.service = message_ir::IrService::Unknown;
    no_service.message_kind = IrMessageKind::Sms;
    no_service.imessage = None;
    no_service.text = "hello no service".into();
    imessage.messages.push(no_service);
    let docs = [
        message_ir::testutil::sample_document("hello ir"),
        imessage,
        message_ir::testutil::sample_whatsapp_document("hello whatsapp"),
    ];

    let tmp = tempfile::tempdir().unwrap();
    let mut report = message_crate_core::ExportReport::default();
    let path = SbrArchive.write(tmp.path(), &docs, &mut report).unwrap();
    assert_eq!(path.file_name().unwrap(), "smses.xml");
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains(r#"count="3""#), "{text}");
    assert!(text.contains("hello ir"));
    assert!(text.contains("hello fallback"));
    assert!(text.contains("hello no service"), "{text}");
    assert!(!text.contains("hello imessage"), "{text}");
    assert!(!text.contains("Loved a message"), "{text}");
    assert!(!text.contains("hello whatsapp"), "{text}");
    // The WhatsApp conversation had nothing left to write, so no element
    // names its address.
    assert!(!text.contains("+15555550102"), "{text}");
    // Two iMessage rows and one WhatsApp row.
    assert_eq!(report.extra(NOT_SMS_OR_MMS_LEFT_OUT), 3);
}

#[test]
fn the_archive_restores_source_fields_as_attrs() {
    let mut doc = message_ir::testutil::sample_document("hello ir");
    // SyncTech-shaped bag (the same shape the reader stores).
    if let Some(source) = doc.messages[0].source.as_mut() {
        let mut attrs = Map::new();
        attrs.insert("protocol".into(), json!("0"));
        attrs.insert("address".into(), json!("+15555550101"));
        attrs.insert("date".into(), json!("1400773261000"));
        attrs.insert("type".into(), json!("1"));
        attrs.insert("body".into(), json!("hello ir"));
        attrs.insert("service_center".into(), json!("+15555550114"));
        attrs.insert("contact_name".into(), json!("Sam"));
        source.fields = {
            let mut m = Map::new();
            m.insert("kind".into(), json!("sms"));
            m.insert("attrs".into(), Value::Object(attrs));
            m
        };
    }
    let tmp = tempfile::tempdir().unwrap();
    let path = SbrArchive
        .write(
            tmp.path(),
            std::slice::from_ref(&doc),
            &mut message_crate_core::ExportReport::default(),
        )
        .unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains(r#"service_center="+15555550114""#));
    assert!(text.contains("hello ir"));
}

#[test]
fn the_archive_counts_the_characters_xml_cannot_carry() {
    let doc = message_ir::testutil::sample_document("bell\u{7} and escape\u{1b}");
    let tmp = tempfile::tempdir().unwrap();
    let mut report = message_crate_core::ExportReport::default();
    let path = SbrArchive
        .write(tmp.path(), std::slice::from_ref(&doc), &mut report)
        .unwrap();
    assert_eq!(report.extra(CHARACTERS_LEFT_OUT), 2);
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains(r#"body="bell and escape""#), "{text}");
}
