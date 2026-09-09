use super::*;

#[tokio::test]
async fn serves_owned_mail_queries_and_commits_remote_mail_operations() {
    let directory = authentication_directory("mail-http");
    let database = directory.join("imail.sqlite");
    let key_hex = "91".repeat(32);
    fs::write(directory.join("master.key"), &key_hex).unwrap();
    let key = MasterKey::from_hex(&key_hex).unwrap();
    let mut store = SqliteAuthStore::open_database(&database).unwrap();
    let owner = store
        .create_user("mail-owner", "Owner", "owner-password-123")
        .unwrap();
    let other = store
        .create_user("mail-other", "Other", "other-password-123")
        .unwrap();
    let owner_session = store.create_session(&owner.id).unwrap();
    let other_session = store.create_session(&other.id).unwrap();
    let settings = json!({
        "imapHost":"imap.example.com","imapPort":993,"imapSecure":true,
        "smtpHost":"smtp.example.com","smtpPort":465,"smtpSecure":true
    });
    let account = |id: String, owner_id: String, email: &str| AccountRecord {
        id,
        owner_id,
        provider: "custom".into(),
        email: email.into(),
        display_name: "Mail Owner".into(),
        group: "工作".into(),
        group_icon: "folder".into(),
        color: "#168f78".into(),
        settings: settings.clone(),
        proxy: None,
        encrypted_secret: key
            .encrypt_json(&json!({"authType":"app-password","password":"mail-secret"}))
            .unwrap(),
        auth_method: Some("app-password".into()),
        created_at: "2026-08-10T00:00:00.000Z".into(),
        last_sync_at: None,
        status: "connected".into(),
        last_error: None,
        mailboxes: json!([{"name":"Projects","path":"Projects/2026","selectable":true}]),
    };
    let account_id = Uuid::new_v4().to_string();
    let other_account_id = Uuid::new_v4().to_string();
    store
        .upsert_account(&account(
            account_id.clone(),
            owner.id.clone(),
            "owner@example.com",
        ))
        .unwrap();
    store
        .upsert_account(&account(
            other_account_id.clone(),
            other.id.clone(),
            "other@example.com",
        ))
        .unwrap();
    let message_id = "owned-message".to_string();
    let message = |id: String, account_id: String, sender: &str| imail_protocol::MessageReadModel {
        headers: Default::default(),
        id,
        account_id,
        mailbox: "INBOX".into(),
        mailbox_role: "inbox".into(),
        uid: 12,
        message_id: Some("<owned@example.com>".into()),
        from: json!({"name":"Sender","address":sender}),
        to: json!([{"name":"Owner","address":"owner@example.com"}]),
        subject: "Quarterly invoice".into(),
        preview: "Invoice preview".into(),
        text: "full confidential body".into(),
        html: Some("<p>full confidential body</p>".into()),
        date: "2026-08-10T08:00:00.000Z".into(),
        unread: true,
        flagged: false,
        has_attachments: true,
        attachments: json!([{"filename":"report.txt","contentType":"text/plain","size":5,"index":0}]),
        labels: json!(["客户"]),
        snoozed_until: None,
    };
    store
        .upsert_message(
            &owner.id,
            &message(message_id.clone(), account_id.clone(), "sender@example.com"),
        )
        .unwrap();
    let mut project_message = message(
        "project-message".into(),
        account_id.clone(),
        "sender@example.com",
    );
    project_message.mailbox = "Projects/2026".into();
    project_message.mailbox_role = "custom".into();
    project_message.subject = "Project note".into();
    project_message.message_id = Some("<project@example.com>".into());
    project_message.headers.reply.in_reply_to = vec!["<owned@example.com>".into()];
    project_message.date = "2026-08-09T08:00:00.000Z".into();
    store.upsert_message(&owner.id, &project_message).unwrap();
    store
        .upsert_message(
            &other.id,
            &message(
                "foreign-message".into(),
                other_account_id,
                "foreign@example.com",
            ),
        )
        .unwrap();
    store
        .upsert_contact(&imail_protocol::ContactReadModel {
            owner_id: owner.id.clone(),
            address: "sender@example.com".into(),
            name: "Sender".into(),
            message_count: 1,
            last_contact_at: "2026-08-10T08:00:00.000Z".into(),
            logo_key: Some("domain:example.com".into()),
            logo_content_type: Some("image/png".into()),
            logo_source_url: Some("https://example.com/logo.png".into()),
            logo_fetched_at: Some("2026-08-10T08:01:00.000Z".into()),
        })
        .unwrap();
    let draft_id = Uuid::new_v4().to_string();
    store
        .upsert_draft(
            &owner.id,
            &imail_protocol::DraftReadModel {
                envelope: Default::default(),
                id: draft_id.clone(),
                account_id: account_id.clone(),
                to: json!([]),
                cc: json!([]),
                subject: String::new(),
                text: String::new(),
                html: String::new(),
                attachments: json!([]),
                created_at: "2026-08-10T08:00:00.000Z".into(),
                updated_at: "2026-08-10T08:00:00.000Z".into(),
            },
        )
        .unwrap();
    store
        .set_user_metadata(
            &owner.id,
            "external_access_v1",
            r#"{"gatewayEnabled":true,"mcpEnabled":false}"#,
        )
        .unwrap();
    let gateway_token = store
        .issue_developer_token(
            &owner.id,
            "Mail gateway",
            &[
                "accounts:read".into(),
                "messages:read".into(),
                "messages:send".into(),
            ],
            std::slice::from_ref(&account_id),
            3600,
        )
        .unwrap()
        .raw;
    drop(store);
    let original_source = b"From: Sender <sender@example.com>\r\nSubject: Quarterly invoice\r\nX-Exact: untouched\r\n\r\n<html><script>kept-as-bytes</script></html>";
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute(
            "INSERT INTO message_sources(message_id,source) VALUES (?1,?2)",
            (&message_id, original_source.as_slice()),
        )
        .unwrap();

    use sha2::Digest as _;
    let logo_directory = directory.join("sender-logos");
    fs::create_dir(&logo_directory).unwrap();
    let logo_digest = format!("{:x}", sha2::Sha256::digest(b"domain:example.com"));
    let logo_bytes = [137, 80, 78, 71, 13, 10, 26, 10, 1, 2, 3, 4];
    fs::write(
        logo_directory.join(format!("{logo_digest}.json")),
        r#"{"contentType":"image/png","sourceUrl":"https://example.com/logo.png","fetchedAt":"2026-08-10T08:01:00.000Z"}"#,
    )
    .unwrap();
    fs::write(
        logo_directory.join(format!("{logo_digest}.bin")),
        logo_bytes,
    )
    .unwrap();

    let mail_state = Arc::new(Mutex::new(MailTestState::default()));
    let source = b"From: Sender <sender@example.com>\r\nTo: Owner <owner@example.com>\r\nSubject: Attachment\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=x\r\n\r\n--x\r\nContent-Type: text/plain\r\n\r\nbody\r\n--x\r\nContent-Type: text/plain; name=report.txt\r\nContent-Disposition: attachment; filename=report.txt\r\nContent-Transfer-Encoding: base64\r\n\r\naGVsbG8=\r\n--x--\r\n".to_vec();
    let mut config = HttpAdapterConfig::new(&directory).with_mail_transport_factory(Arc::new(
        TestMailTransportFactory {
            state: Arc::clone(&mail_state),
            source,
        },
    ));
    config.gateway = true;
    let router = build_router(config).unwrap();
    let request = |method: Method, uri: String, session: &str, body: Body| {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(HOST, "127.0.0.1:8787")
            .header(CONTENT_TYPE, "application/json")
            .header("cookie", format!("imail_session={session}"))
            .body(body)
            .unwrap()
    };

    let conversation_uri = format!("/api/messages/{message_id}/conversation");
    let conversation = router
        .clone()
        .oneshot(request(
            Method::GET,
            conversation_uri.clone(),
            &owner_session,
            Body::empty(),
        ))
        .await
        .unwrap();
    assert_eq!(conversation.status(), StatusCode::OK);
    let conversation = json(conversation).await;
    assert_eq!(conversation["messages"].as_array().unwrap().len(), 2);
    assert_eq!(conversation["messages"][0]["id"], "project-message");
    assert_eq!(conversation["messages"][0]["mailbox"], "Projects/2026");
    assert!(conversation["messages"][0].get("text").is_none());
    assert!(conversation["messages"]
        .as_array()
        .unwrap()
        .iter()
        .all(|message| message["unread"] == true));
    let foreign = router
        .clone()
        .oneshot(request(
            Method::GET,
            conversation_uri,
            &other_session,
            Body::empty(),
        ))
        .await
        .unwrap();
    assert_eq!(foreign.status(), StatusCode::NOT_FOUND);
    let anonymous = router
        .clone()
        .oneshot(request(
            Method::GET,
            format!("/api/messages/{message_id}/conversation"),
            "",
            Body::empty(),
        ))
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

    let listed = router
        .clone()
        .oneshot(request(
            Method::GET,
            "/api/messages?q=invoice&unread=true&limit=10&offset=0".into(),
            &owner_session,
            Body::empty(),
        ))
        .await
        .unwrap();
    assert_eq!(listed.status(), StatusCode::OK);
    let listed = json(listed).await;
    assert_eq!(listed["total"], 1);
    assert_eq!(listed["messages"][0]["id"], message_id);
    assert!(listed["messages"][0].get("text").is_none());
    assert!(listed["messages"][0].get("html").is_none());
    assert_eq!(
        listed["messages"][0]["from"]["logo"]["key"],
        "domain:example.com"
    );
    assert!(listed["messages"][0]["from"]["logo"]["url"]
        .as_str()
        .unwrap()
        .contains("sender%40example.com"));
    let by_mailbox_name = json(
        router
            .clone()
            .oneshot(request(
                Method::GET,
                "/api/messages?mailboxName=projects".into(),
                &owner_session,
                Body::empty(),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(by_mailbox_name["total"], 1);
    assert_eq!(by_mailbox_name["messages"][0]["id"], "project-message");

    let foreign_list = json(
        router
            .clone()
            .oneshot(request(
                Method::GET,
                "/api/messages".into(),
                &other_session,
                Body::empty(),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(foreign_list["total"], 1);
    assert_eq!(foreign_list["messages"][0]["id"], "foreign-message");

    let detail = json(
        router
            .clone()
            .oneshot(request(
                Method::GET,
                format!("/api/messages/{message_id}"),
                &owner_session,
                Body::empty(),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(detail["message"]["text"], "full confidential body");
    let blocked_image_download = router
        .clone()
        .oneshot(request(
            Method::POST,
            "/api/resources/image-download".into(),
            &owner_session,
            Body::from(r#"{"url":"http://127.0.0.1/private.png"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(blocked_image_download.status(), StatusCode::BAD_REQUEST);
    let source_preview = router
        .clone()
        .oneshot(request(
            Method::GET,
            format!("/api/messages/{message_id}/source"),
            &owner_session,
            Body::empty(),
        ))
        .await
        .unwrap();
    assert_eq!(source_preview.status(), StatusCode::OK);
    assert_eq!(
        source_preview.headers()["cache-control"],
        "private, no-store, max-age=0"
    );
    assert_eq!(source_preview.headers()["pragma"], "no-cache");
    assert_eq!(source_preview.headers()["expires"], "0");
    let source_download = router
        .clone()
        .oneshot(request(
            Method::GET,
            format!("/api/messages/{message_id}/source/download"),
            &owner_session,
            Body::empty(),
        ))
        .await
        .unwrap();
    assert_eq!(source_download.status(), StatusCode::OK);
    assert_eq!(source_download.headers()[CONTENT_TYPE], "message/rfc822");
    assert_eq!(
        source_download.headers()["cache-control"],
        "private, no-store, max-age=0"
    );
    assert_eq!(source_download.headers()["pragma"], "no-cache");
    assert_eq!(source_download.headers()["expires"], "0");
    assert!(source_download.headers()[CONTENT_DISPOSITION]
        .to_str()
        .unwrap()
        .contains("message.eml"));
    assert_eq!(
        to_bytes(source_download.into_body(), 1024).await.unwrap(),
        original_source.as_slice()
    );
    let foreign_source_download = router
        .clone()
        .oneshot(request(
            Method::GET,
            format!("/api/messages/{message_id}/source/download"),
            &other_session,
            Body::empty(),
        ))
        .await
        .unwrap();
    assert_eq!(foreign_source_download.status(), StatusCode::NOT_FOUND);
    let stats = json(
        router
            .clone()
            .oneshot(request(
                Method::GET,
                "/api/message-stats".into(),
                &owner_session,
                Body::empty(),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(stats["total"], 1);
    assert_eq!(stats["unread"], 1);
    assert_eq!(stats["byAccount"][0]["accountId"], account_id);
    let contacts = json(
        router
            .clone()
            .oneshot(request(
                Method::GET,
                "/api/contacts".into(),
                &owner_session,
                Body::empty(),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert!(contacts.to_string().contains("domain:example.com"));
    assert!(!contacts.to_string().contains("ownerId"));
    let logo = router
        .clone()
        .oneshot(request(
            Method::GET,
            "/api/contacts/logo?address=sender%40example.com".into(),
            &owner_session,
            Body::empty(),
        ))
        .await
        .unwrap();
    assert_eq!(logo.status(), StatusCode::OK);
    assert_eq!(logo.headers()[CONTENT_TYPE], "image/png");
    assert_eq!(
        to_bytes(logo.into_body(), 1024).await.unwrap().as_ref(),
        logo_bytes
    );
    let message_logo = router
        .clone()
        .oneshot(request(
            Method::GET,
            format!("/api/messages/{message_id}/sender-logo"),
            &owner_session,
            Body::empty(),
        ))
        .await
        .unwrap();
    assert_eq!(message_logo.status(), StatusCode::OK);
    let foreign_logo = router
        .clone()
        .oneshot(request(
            Method::GET,
            "/api/contacts/logo?address=sender%40example.com".into(),
            &other_session,
            Body::empty(),
        ))
        .await
        .unwrap();
    assert_eq!(foreign_logo.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        json(
            router
                .clone()
                .oneshot(request(
                    Method::GET,
                    "/api/labels".into(),
                    &owner_session,
                    Body::empty(),
                ))
                .await
                .unwrap()
        )
        .await["labels"],
        json!(["客户"])
    );

    let forbidden = router
        .clone()
        .oneshot(request(
            Method::GET,
            format!("/api/messages/{message_id}"),
            &other_session,
            Body::empty(),
        ))
        .await
        .unwrap();
    assert_eq!(forbidden.status(), StatusCode::NOT_FOUND);

    let patched = router
        .clone()
        .oneshot(request(
            Method::PATCH,
            format!("/api/messages/{message_id}"),
            &owner_session,
            Body::from(r#"{"unread":false,"flagged":true,"labels":["重点"],"snoozedUntil":null}"#),
        ))
        .await
        .unwrap();
    assert_eq!(patched.status(), StatusCode::OK);
    let patched = json(patched).await;
    assert_eq!(patched["message"]["unread"], false);
    assert_eq!(patched["message"]["flagged"], true);
    assert_eq!(patched["message"]["labels"], json!(["重点"]));

    mail_state.lock().unwrap().reject_flags = true;
    let rejected_patch = router
        .clone()
        .oneshot(request(
            Method::PATCH,
            format!("/api/messages/{message_id}"),
            &owner_session,
            Body::from(r#"{"unread":true}"#),
        ))
        .await
        .unwrap();
    assert_eq!(rejected_patch.status(), StatusCode::BAD_GATEWAY);
    let rejected_patch = json(rejected_patch).await;
    assert!(!rejected_patch.to_string().contains("remote-secret"));
    assert!(
        !SqliteAuthStore::open_database(&database)
            .unwrap()
            .list_messages(&owner.id)
            .unwrap()[0]
            .unread
    );
    mail_state.lock().unwrap().reject_flags = false;

    let foreign_attachment = router
        .clone()
        .oneshot(request(
            Method::GET,
            format!("/api/messages/{message_id}/attachments/0"),
            &other_session,
            Body::empty(),
        ))
        .await
        .unwrap();
    assert_eq!(foreign_attachment.status(), StatusCode::NOT_FOUND);
    assert_eq!(mail_state.lock().unwrap().fetches, 0);

    let attachment = router
        .clone()
        .oneshot(request(
            Method::GET,
            format!("/api/messages/{message_id}/attachments/0"),
            &owner_session,
            Body::empty(),
        ))
        .await
        .unwrap();
    assert_eq!(attachment.status(), StatusCode::OK);
    assert_eq!(attachment.headers()[CONTENT_TYPE], "text/plain");
    assert!(attachment.headers()["content-disposition"]
        .to_str()
        .unwrap()
        .contains("report.txt"));
    assert_eq!(
        to_bytes(attachment.into_body(), 1024).await.unwrap(),
        "hello"
    );
    let fetches_after_download = mail_state.lock().unwrap().fetches;

    let preview = router
        .clone()
        .oneshot(request(
            Method::POST,
            format!("/api/messages/{message_id}/attachments/0/preview"),
            &owner_session,
            Body::empty(),
        ))
        .await
        .unwrap();
    assert_eq!(preview.status(), StatusCode::CREATED);
    let preview = json(preview).await;
    assert_eq!(preview["descriptor"]["kind"], "text");
    assert_eq!(
        preview["descriptor"]["contentType"],
        "text/plain; charset=utf-8"
    );
    assert_eq!(
        mail_state.lock().unwrap().fetches,
        fetches_after_download,
        "preview should reuse the attachment downloaded into the local cache"
    );
    let preview_id = preview["previewId"].as_str().unwrap();

    let foreign_preview = router
        .clone()
        .oneshot(request(
            Method::GET,
            format!("/api/attachment-previews/{preview_id}/content"),
            &other_session,
            Body::empty(),
        ))
        .await
        .unwrap();
    assert_eq!(foreign_preview.status(), StatusCode::NOT_FOUND);

    let mut range_request = request(
        Method::GET,
        format!("/api/attachment-previews/{preview_id}/content"),
        &owner_session,
        Body::empty(),
    );
    range_request
        .headers_mut()
        .insert("range", HeaderValue::from_static("bytes=1-3"));
    let ranged = router.clone().oneshot(range_request).await.unwrap();
    assert_eq!(ranged.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(ranged.headers()["content-range"], "bytes 1-3/5");
    assert_eq!(to_bytes(ranged.into_body(), 1024).await.unwrap(), "ell");

    let removed = router
        .clone()
        .oneshot(request(
            Method::DELETE,
            format!("/api/attachment-previews/{preview_id}"),
            &owner_session,
            Body::empty(),
        ))
        .await
        .unwrap();
    assert_eq!(removed.status(), StatusCode::NO_CONTENT);

    let moved = router
        .clone()
        .oneshot(request(
            Method::POST,
            format!("/api/messages/{message_id}/move"),
            &owner_session,
            Body::from(r#"{"destination":"archive"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(moved.status(), StatusCode::OK);
    let moved = json(moved).await;
    assert_eq!(moved["message"]["mailbox"], "Archive");
    assert_eq!(moved["message"]["mailboxRole"], "archive");
    assert_eq!(moved["message"]["uid"], 44);

    let invalid_send = router
        .clone()
        .oneshot(request(
            Method::POST,
            "/api/send".into(),
            &owner_session,
            Body::from(format!(
                r#"{{"accountId":"{account_id}","to":["recipient@example.com"],"subject":"Hello","text":"Body","attachments":[{{"id":"a1","filename":"hello.txt","contentType":"text/plain","size":5,"data":"not-base64"}}],"draftId":"{draft_id}"}}"#
            )),
        ))
        .await
        .unwrap();
    assert_eq!(invalid_send.status(), StatusCode::BAD_REQUEST);
    assert_eq!(mail_state.lock().unwrap().sends, 0);
    assert_eq!(
        SqliteAuthStore::open_database(&database)
            .unwrap()
            .list_drafts(&owner.id)
            .unwrap()
            .len(),
        1
    );

    let gateway_send = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/gateway/v1/send")
                .header(HOST, "127.0.0.1:8787")
                .header("authorization", format!("Bearer {gateway_token}"))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"mailbox":"owner@example.com","to":["gateway@example.com"],"subject":"Gateway","text":"Gateway body"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(gateway_send.status(), StatusCode::CREATED);
    assert_eq!(
        json(gateway_send).await["delivery"]["messageId"],
        "<sent@example.com>"
    );
    let gateway_attachment = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/gateway/v1/messages/{message_id}/attachments/0"))
                .header(HOST, "127.0.0.1:8787")
                .header("authorization", format!("Bearer {gateway_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(gateway_attachment.status(), StatusCode::OK);
    assert_eq!(gateway_attachment.headers()[CONTENT_TYPE], "text/plain");

    let sent = router
        .clone()
        .oneshot(request(
            Method::POST,
            "/api/send".into(),
            &owner_session,
            Body::from(format!(
                r#"{{"accountId":"{account_id}","to":["recipient@example.com"],"subject":"Hello","text":"Body","attachments":[{{"id":"a1","filename":"hello.txt","contentType":"text/plain","size":5,"data":"aGVsbG8="}}],"draftId":"{draft_id}"}}"#
            )),
        ))
        .await
        .unwrap();
    assert_eq!(sent.status(), StatusCode::CREATED);
    assert_eq!(json(sent).await["messageId"], "<sent@example.com>");
    assert!(SqliteAuthStore::open_database(&database)
        .unwrap()
        .list_drafts(&owner.id)
        .unwrap()
        .is_empty());
    let state = mail_state.lock().unwrap();
    assert_eq!(state.flag_updates, 2);
    assert_eq!(state.fetches, 2);
    assert_eq!(state.moves, 1);
    assert_eq!(state.sends, 2);
    assert_eq!(
        state.last_sent.as_ref().unwrap().attachments[0].content,
        b"hello"
    );
    drop(state);
    fs::remove_dir_all(directory).unwrap();
}
