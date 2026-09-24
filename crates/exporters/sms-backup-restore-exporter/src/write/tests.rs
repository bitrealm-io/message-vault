use super::*;
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
    let (read, report) = crate::read_backup(&path, read_options(&owners)).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    read
}

/// Reader options that keep attachment bytes on the records.
fn read_options(owners: &[String]) -> crate::ReadOptions<'_> {
    crate::ReadOptions {
        owner_phones: owners,
        attachments_dir: None,
        copy_attachments: true,
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
fn a_group_mms_keeps_its_addresses_names_and_attachment() {
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
    assert_eq!(incoming.sender_display_name.as_deref(), Some("Lee"));
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
    let (docs, _) = crate::read_backup(&input, read_options(&owners)).unwrap();
    assert_eq!(docs[0].messages[0].attachments.len(), 2, "a.jpg, c.jpg");
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

    // Neither has a digest: they go to the payload parts in order, and
    // the last part, with nothing left to take, gets no data.
    for att in &mut transformed[0].messages[0].attachments {
        att.digest_sha256 = None;
    }
    let read = round_trip(&transformed);
    assert_eq!(part_payloads(&read[0].messages[0]), payloads(None));
}

#[test]
fn a_session_writes_every_conversation_into_one_smses_backup() {
    let tmp = tempfile::tempdir().unwrap();
    let mut session = SbrBackupSession::create(tmp.path()).unwrap();
    session
        .append_document(&message_ir::testutil::sample_document("hello ir"))
        .unwrap();
    session
        .append_document(&message_ir::testutil::sample_imessage_document())
        .unwrap();
    let path = session.finish().unwrap();
    assert_eq!(path.file_name().unwrap(), "smses.xml");
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains(r#"count="3""#)); // 1 SMS + 2 iMessage rows
    assert!(text.contains("hello ir"));
    assert!(text.contains(r#"type="1""#) || text.contains(r#"msg_box="1""#));
    assert!(text.contains("hello imessage"));
    // iMessage bags are not mirrored as Apple attrs.
    assert!(!text.contains("X-ME-"));
    assert!(!text.contains("Sent with Balloons"));
    assert!(!text.contains("tapback_kind"));
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
        attrs.insert("service_center".into(), json!("+15550009999"));
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
        .write(tmp.path(), std::slice::from_ref(&doc))
        .unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains(r#"service_center="+15550009999""#));
    assert!(text.contains("hello ir"));
}
