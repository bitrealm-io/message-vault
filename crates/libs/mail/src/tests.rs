use super::*;
use mailparse::MailHeaderMap;

fn base_sms() -> MailMessage {
    MailMessage {
        chat_identifier: "+15555550101".into(),
        conversation_type: "individual".into(),
        group_title: None,
        participants: vec![Participant {
            handle: "+15555550101".into(),
            display_name: Some("Sam".into()),
        }],
        owner_handle: "+15555550100".into(),
        owner_display_name: None,
        export_source: "sms-backup-restore".into(),
        export_tool: "SMS Backup & Restore".into(),
        export_tool_version: "10.26.003".into(),
        filename_suffix: None,
        message: IrMessage {
            guid: "aabbccddeeff00112233445566778899".into(),
            timestamp_unix_ms: 1_400_773_261_000,
            direction: IrDirection::Incoming,
            service: message_ir::IrService::Sms,
            message_kind: message_ir::IrMessageKind::Sms,
            sender_handle: Some("+15555550101".into()),
            sender_display_name: Some("Sam".into()),
            subject: None,
            text: "hello from sms".into(),
            attachments: Vec::new(),
            imessage: None,
            source: Some(message_ir::IrSource {
                android_type: Some(1),
                fields: serde_json::from_str(r#"{"address":"+15555550101"}"#).unwrap(),
            }),
        },
        attachments: vec![],
    }
}

/// The extension bag, creating it when the base message has none.
fn im_mut(msg: &mut MailMessage) -> &mut message_ir::IrImessage {
    msg.message.imessage.get_or_insert_with(Default::default)
}

#[test]
fn writes_individual_sms_text_only() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = write_conversation(tmp.path(), &[base_sms()]).unwrap();
    assert_eq!(dir.file_name().unwrap(), "+15555550101");

    let mut emls: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("eml"))
        .collect();
    emls.sort();
    assert_eq!(emls.len(), 1);
    assert!(
        emls[0]
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("000001_")
    );

    let bytes = fs::read(&emls[0]).unwrap();
    let mail = mailparse::parse_mail(&bytes).unwrap();
    let headers = mail.get_headers();
    assert_eq!(
        headers.get_first_value("X-ME-Chat-Identifier").as_deref(),
        Some("+15555550101")
    );
    assert_eq!(
        headers.get_first_value("X-ME-Direction").as_deref(),
        Some("incoming")
    );
    assert_eq!(
        headers.get_first_value("X-ME-Message-Kind").as_deref(),
        Some("sms")
    );
    assert_eq!(
        headers.get_first_value("X-ME-Guid").as_deref(),
        Some("aabbccddeeff00112233445566778899")
    );
    assert_eq!(
        headers.get_first_value("X-ME-Export-Source").as_deref(),
        Some("sms-backup-restore")
    );
    let mid = headers.get_first_value("Message-ID").unwrap();
    assert!(mid.contains("aabbccddeeff00112233445566778899@message-vault-io.local"));
    assert!(headers.get_first_value("In-Reply-To").is_none());
    let from = headers.get_first_value("From").unwrap();
    assert!(from.contains("Sam"), "From was {from}");
    assert!(from.contains("+15555550101@sms.local"), "From was {from}");
    let to = headers.get_first_value("To").unwrap();
    assert!(to.contains("Me"), "To was {to}");
    assert!(to.contains("+15555550100@sms.local"), "To was {to}");
    assert_eq!(
        headers.get_first_value("Subject").as_deref(),
        Some("Message with Sam")
    );
    let body = mail.get_body().unwrap();
    assert!(body.contains("hello from sms"));
    assert!(!mail.ctype.mimetype.starts_with("multipart/"));
}

#[test]
fn writes_group_mms_with_image_part() {
    let mut msg = base_sms();
    msg.chat_identifier = "chat-group1".into();
    msg.conversation_type = "group".into();
    msg.group_title = Some("Family".into());
    msg.message.message_kind = message_ir::IrMessageKind::Mms;
    msg.participants = vec![
        Participant {
            handle: "+15555550101".into(),
            display_name: Some("Sam".into()),
        },
        Participant {
            handle: "+15555550102".into(),
            display_name: Some("Alex".into()),
        },
    ];
    msg.attachments = vec![MailAttachment {
        bytes: b"\xff\xd8\xfffakejpeg".to_vec(),
        meta: message_ir::AttachmentMeta {
            path: None,
            original_name: Some("photo.jpg".into()),
            mime_type: Some("image/jpeg".into()),
            digest_sha256: Some("deadbeef".into()),
        },
        is_sticker: false,
        transcription: None,
        sticker_effect: None,
    }];

    let tmp = tempfile::tempdir().unwrap();
    let dir = write_conversation(tmp.path(), &[msg]).unwrap();
    assert_eq!(dir.file_name().unwrap(), "Family");

    let eml = fs::read_dir(&dir).unwrap().next().unwrap().unwrap().path();
    let bytes = fs::read(&eml).unwrap();
    let mail = mailparse::parse_mail(&bytes).unwrap();
    let headers = mail.get_headers();
    assert_eq!(
        headers.get_first_value("X-ME-Conversation-Type").as_deref(),
        Some("group")
    );
    assert_eq!(
        headers.get_first_value("X-ME-Group-Title").as_deref(),
        Some("Family")
    );
    assert_eq!(
        headers.get_first_value("Subject").as_deref(),
        Some("Message with Family")
    );
    let to = headers.get_first_value("To").unwrap();
    assert!(to.contains("Family"), "To was {to}");
    assert!(to.contains("chat-group1@chat.local"), "To was {to}");
    assert!(
        !to.contains("+15555550102"),
        "group To should be the chat, not the full roster: {to}"
    );
    let participants = headers.get_first_value("X-ME-Participants").unwrap();
    assert!(participants.contains("+15555550101"));
    assert!(participants.contains("+15555550102"));
    let meta = headers.get_first_value("X-ME-Attachment-Meta").unwrap();
    assert!(meta.contains("photo.jpg"));
    assert!(meta.contains("deadbeef"));
    assert!(mail.ctype.mimetype.starts_with("multipart/"));

    let mut found_image = false;
    fn walk(m: &mailparse::ParsedMail<'_>, found: &mut bool) {
        if m.ctype.mimetype == "image/jpeg" {
            *found = true;
        }
        for sub in &m.subparts {
            walk(sub, found);
        }
    }
    walk(&mail, &mut found_image);
    assert!(found_image, "expected image/jpeg MIME part");
}

#[test]
fn encodes_email_handles_and_imessage_message_id() {
    let mut msg = base_sms();
    msg.chat_identifier = "friend@icloud.com".into();
    msg.participants = vec![Participant {
        handle: "friend@icloud.com".into(),
        display_name: Some("Friend".into()),
    }];
    msg.message.sender_handle = Some("friend@icloud.com".into());
    msg.owner_handle = "me@icloud.com".into();
    msg.message.service = message_ir::IrService::IMessage;
    msg.message.message_kind = message_ir::IrMessageKind::IMessage;
    msg.message.guid = "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE".into();
    msg.export_source = "imessage".into();

    let tmp = tempfile::tempdir().unwrap();
    let path = write_message_file(&tmp.path().join("chat"), 1, &msg).unwrap();
    let bytes = fs::read(&path).unwrap();
    let mail = mailparse::parse_mail(&bytes).unwrap();
    let headers = mail.get_headers();
    let from = headers.get_first_value("From").unwrap();
    assert!(
        from.contains("friend=icloud.com@handle.local"),
        "From was {from}"
    );
    let to = headers.get_first_value("To").unwrap();
    assert!(to.contains("me=icloud.com@handle.local"), "To was {to}");
    let mid = headers.get_first_value("Message-ID").unwrap();
    assert!(
        mid.contains("AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE@imessage.local"),
        "Message-ID was {mid}"
    );
}

#[test]
fn outgoing_uses_me_and_stable_subject() {
    let mut msg = base_sms();
    msg.message.direction = IrDirection::Outgoing;
    msg.message.sender_handle = Some("+15555550100".into());
    msg.message.sender_display_name = Some("Me".into());
    msg.message.text = "body must not become subject".into();

    let tmp = tempfile::tempdir().unwrap();
    let path = write_message_file(&tmp.path().join("chat"), 1, &msg).unwrap();
    let bytes = fs::read(&path).unwrap();
    let mail = mailparse::parse_mail(&bytes).unwrap();
    let headers = mail.get_headers();
    let from = headers.get_first_value("From").unwrap();
    assert!(from.contains("Me"), "From was {from}");
    let to = headers.get_first_value("To").unwrap();
    assert!(to.contains("Sam"), "To was {to}");
    assert_eq!(
        headers.get_first_value("Subject").as_deref(),
        Some("Message with Sam")
    );
    assert!(
        !headers
            .get_first_value("Subject")
            .unwrap()
            .contains("body must not")
    );
    assert_eq!(
        headers.get_first_value("X-ME-Sender-Handle").as_deref(),
        Some("+15555550100")
    );
    assert_eq!(
        headers.get_first_value("X-ME-Owner-Handle").as_deref(),
        Some("+15555550100")
    );
    assert_eq!(
        headers
            .get_first_value("X-ME-Owner-Display-Name")
            .as_deref(),
        None // unset on base_sms unless set
    );
}

#[test]
fn caller_id_owner_display_and_imessage_extension_headers() {
    let mut msg = base_sms();
    msg.message.direction = IrDirection::Outgoing;
    msg.message.sender_handle = Some("+15555550100".into());
    msg.message.sender_display_name = Some("+15555550100".into());
    msg.owner_display_name = Some("+15555550100".into());
    msg.export_source = "imessage".into();
    msg.message.message_kind = message_ir::IrMessageKind::IMessage;
    im_mut(&mut msg).is_reply = true;
    im_mut(&mut msg).in_reply_to_guid = Some("parent-guid-1111".into());
    im_mut(&mut msg).thread_originator_part = Some(0);
    im_mut(&mut msg).num_replies = Some(2);
    im_mut(&mut msg).send_effect = Some("Sent with Balloons".into());
    msg.message.text = "hello\n\nSent with Balloons".into();
    im_mut(&mut msg).tapbacks = serde_json::from_str(r#"[{"part_index":0,"kind":"loved"}]"#).ok();
    im_mut(&mut msg).parts =
        serde_json::from_str(r#"[{"index":0,"kind":"run","text":"hello"}]"#).ok();
    im_mut(&mut msg).announcement = None;

    let tmp = tempfile::tempdir().unwrap();
    let path = write_message_file(&tmp.path().join("chat"), 1, &msg).unwrap();
    let bytes = fs::read(&path).unwrap();
    let mail = mailparse::parse_mail(&bytes).unwrap();
    let headers = mail.get_headers();
    let from = headers.get_first_value("From").unwrap();
    assert!(from.contains("+15555550100"), "From was {from}");
    assert!(!from.contains("Me <"), "From was {from}");
    assert_eq!(
        headers.get_first_value("X-ME-Sender-Handle").as_deref(),
        Some("+15555550100")
    );
    assert_eq!(
        headers
            .get_first_value("X-ME-Owner-Display-Name")
            .as_deref(),
        Some("+15555550100")
    );
    assert_eq!(
        headers.get_first_value("X-ME-Is-Reply").as_deref(),
        Some("true")
    );
    assert_eq!(
        headers
            .get_first_value("X-ME-Thread-Originator-Guid")
            .as_deref(),
        Some("parent-guid-1111")
    );
    assert_eq!(
        headers.get_first_value("X-ME-Send-Effect").as_deref(),
        Some("Sent with Balloons")
    );
    assert_eq!(
        headers.get_first_value("X-ME-Num-Replies").as_deref(),
        Some("2")
    );
    let irt = headers.get_first_value("In-Reply-To").unwrap();
    assert!(irt.contains("parent-guid-1111@imessage.local"), "{irt}");
    assert!(mail.get_body().unwrap().contains("Sent with Balloons"));
}

#[test]
fn tapback_and_handwriting_svg_headers() {
    let mut msg = base_sms();
    msg.export_source = "imessage".into();
    msg.message.message_kind = message_ir::IrMessageKind::Tapback;
    im_mut(&mut msg).associated_guid = Some("parent-guid".into());
    im_mut(&mut msg).associated_part = Some(0);
    im_mut(&mut msg).tapback_kind = Some("loved".into());
    im_mut(&mut msg).tapback_action = Some("add".into());
    im_mut(&mut msg).in_reply_to_guid = Some("parent-guid".into());
    msg.message.text = "Loved a message".into();
    msg.attachments = vec![MailAttachment {
        bytes: b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>".to_vec(),
        meta: message_ir::AttachmentMeta {
            path: None,
            original_name: Some("handwriting.svg".into()),
            mime_type: Some("image/svg+xml".into()),
            digest_sha256: None,
        },
        is_sticker: false,
        transcription: None,
        sticker_effect: None,
    }];
    // Handwriting would normally be message_kind=balloon; this only checks MIME.
    let tmp = tempfile::tempdir().unwrap();
    let path = write_message_file(&tmp.path().join("chat"), 1, &msg).unwrap();
    let bytes = fs::read(&path).unwrap();
    let mail = mailparse::parse_mail(&bytes).unwrap();
    let headers = mail.get_headers();
    assert_eq!(
        headers.get_first_value("X-ME-Tapback-Kind").as_deref(),
        Some("loved")
    );
    assert_eq!(
        headers.get_first_value("X-ME-Associated-Guid").as_deref(),
        Some("parent-guid")
    );
    let mut found_svg = false;
    fn walk(m: &mailparse::ParsedMail<'_>, found: &mut bool) {
        if m.ctype.mimetype == "image/svg+xml" {
            *found = true;
        }
        for sub in &m.subparts {
            walk(sub, found);
        }
    }
    walk(&mail, &mut found_svg);
    assert!(found_svg, "expected image/svg+xml MIME part");
}

#[test]
fn escape_mboxrd_from_lines() {
    assert_eq!(escape_mboxrd_line("Hello"), "Hello");
    assert_eq!(escape_mboxrd_line("From me"), ">From me");
    assert_eq!(escape_mboxrd_line(">From me"), ">>From me");
    assert_eq!(escape_mboxrd_line("Fromage"), "Fromage");
}

#[test]
fn writes_conversation_mboxrd() {
    let mut a = base_sms();
    a.message.text = "first\nFrom spoofed\nlast".into();
    a.message.timestamp_unix_ms = 1_400_773_261_000;
    let mut b = base_sms();
    b.message.guid = "bbccddeeff00112233445566778899aa".into();
    b.message.text = "second".into();
    b.message.timestamp_unix_ms = 1_400_773_361_000;

    let tmp = tempfile::tempdir().unwrap();
    let path = write_conversation_mbox(tmp.path(), &[b, a]).unwrap();
    assert_eq!(path.file_name().unwrap(), "+15555550101.mbox");

    let text = fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("From "));
    assert!(text.contains(">From spoofed"));
    // Chronological: first then second
    let first_pos = text.find("first").unwrap();
    let second_pos = text.find("second").unwrap();
    assert!(first_pos < second_pos);
    assert_eq!(text.matches("\nFrom ").count(), 1); // one additional From_ between records
    assert!(text.contains("X-ME-Guid: aabbccddeeff00112233445566778899"));
    assert!(text.contains("X-ME-Guid: bbccddeeff00112233445566778899aa"));
}
