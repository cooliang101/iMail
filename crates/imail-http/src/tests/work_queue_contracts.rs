use super::*;

#[tokio::test]
async fn manages_owner_scoped_work_queue_and_confirmed_reply_delivery() {
    let directory = authentication_directory("mail-work-queue");
    let mut store = SqliteAuthStore::open_database(directory.join("imail.sqlite")).unwrap();
    let owner = store
        .create_user("queue-owner", "Owner", "owner-password-123")
        .unwrap();
    let other = store
        .create_user("queue-other", "Other", "other-password-123")
        .unwrap();
    let owner_session = store.create_session(&owner.id).unwrap();
    let other_session = store.create_session(&other.id).unwrap();
    let account_id = Uuid::new_v4().to_string();
    store
        .upsert_account(&AccountRecord {
            id: account_id.clone(),
            owner_id: owner.id.clone(),
            provider: "custom".into(),
            email: "owner@example.com".into(),
            display_name: "Owner".into(),
            group: "个人".into(),
            group_icon: "folder".into(),
            color: "#168f78".into(),
            settings: json!({}),
            proxy: None,
            encrypted_secret: "encrypted".into(),
            auth_method: Some("app-password".into()),
            created_at: "2026-09-01T00:00:00.000Z".into(),
            last_sync_at: None,
            status: "connected".into(),
            last_error: None,
            mailboxes: json!([]),
        })
        .unwrap();
    let message_id = "queue-message";
    store.upsert_message(&owner.id, &imail_protocol::MessageReadModel {
        headers: imail_protocol::MailHeaders {
            cc: vec![imail_protocol::MailAddressView { name: "Copy".into(), address: "copy@example.net".into() }],
            reply_to: vec![imail_protocol::MailAddressView { name: "Reply".into(), address: "reply@example.net".into() }],
            reply: imail_protocol::ReplyHeaders { in_reply_to: vec![], references: vec!["<root@example.net>".into()] },
        },
        id: message_id.into(), account_id: account_id.clone(), mailbox: "INBOX".into(), mailbox_role: "inbox".into(), uid: 1,
        message_id: Some("<source@example.net>".into()), from: json!({"name":"Sender","address":"sender@example.net"}),
        to: json!([{"name":"Owner","address":"owner@example.com"},{"name":"Team","address":"team@example.net"}]),
        subject: "Question".into(), preview: "Please reply".into(), text: "Body".into(), html: None,
        date: "2026-09-01T00:00:00.000Z".into(), unread: true, flagged: false,
        has_attachments: false, attachments: json!([]), labels: json!([]), snoozed_until: None,
    }).unwrap();
    drop(store);
    let router = build_router(HttpAdapterConfig::new(&directory)).unwrap();
    let set = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri(format!("/api/messages/{message_id}/work-item"))
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::from(
                    r#"{"status":"needsReply","note":"Answer today"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(set.status(), StatusCode::OK);
    assert_eq!(json(set).await["item"]["status"], "needsReply");
    let isolated = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/mail-work-items")
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={other_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(json(isolated).await["items"], json!([]));
    let reply = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/messages/{message_id}/reply-draft"))
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::from(r#"{"mode":"replyAll","text":"Thanks"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(reply.status(), StatusCode::CREATED);
    let reply = json(reply).await;
    assert_eq!(reply["item"]["status"], "needsReview");
    assert_eq!(
        reply["draft"]["to"],
        json!(["reply@example.net", "team@example.net"])
    );
    assert_eq!(reply["draft"]["cc"], json!(["copy@example.net"]));
    assert_eq!(reply["draft"]["inReplyTo"], json!(["<source@example.net>"]));
    let draft_id = reply["draft"]["id"].as_str().unwrap();
    let unconfirmed = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/drafts/{draft_id}/schedule"))
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::from(format!(
                    r#"{{"requestId":"{}","sendAt":"2026-09-01T18:00:00Z","confirmed":false}}"#,
                    Uuid::new_v4()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unconfirmed.status(), StatusCode::CONFLICT);
    let send_at = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
    let scheduled = router.clone().oneshot(Request::builder().method(Method::POST)
        .uri(format!("/api/drafts/{draft_id}/schedule")).header(HOST,"127.0.0.1:8787")
        .header(CONTENT_TYPE,"application/json").header("cookie",format!("imail_session={owner_session}"))
        .body(Body::from(json!({"requestId":Uuid::new_v4().to_string(),"sendAt":send_at,"confirmed":true}).to_string())).unwrap()).await.unwrap();
    assert_eq!(scheduled.status(), StatusCode::CREATED);
    assert_eq!(json(scheduled).await["item"]["status"], "scheduled");
    let queue = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/mail-work-items")
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(json(queue).await["items"][0]["item"]["status"], "waiting");
    let completed = router
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri(format!("/api/messages/{message_id}/work-item"))
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(completed.status(), StatusCode::OK);
    fs::remove_dir_all(directory).unwrap();
}
