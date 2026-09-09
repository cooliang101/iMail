use super::*;

#[tokio::test]
async fn gateway_websocket_enforces_origin_scope_account_and_live_revocation() {
    use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

    let directory = authentication_directory("gateway-websocket");
    let database = directory.join("imail.sqlite");
    let mut store = SqliteAuthStore::open_database(&database).unwrap();
    let owner = store
        .create_user("gateway-ws-owner", "Gateway WS", "owner-password-123")
        .unwrap();
    let account_id = Uuid::new_v4().to_string();
    store
        .upsert_account(&AccountRecord {
            id: account_id.clone(),
            owner_id: owner.id.clone(),
            provider: "custom".into(),
            email: "events@example.com".into(),
            display_name: "Events".into(),
            group: "工作".into(),
            group_icon: "folder".into(),
            color: "#168f78".into(),
            settings: json!({}),
            proxy: None,
            encrypted_secret: "encrypted-placeholder".into(),
            auth_method: Some("app-password".into()),
            created_at: "2026-08-10T00:00:00.000Z".into(),
            last_sync_at: None,
            status: "connected".into(),
            last_error: None,
            mailboxes: json!([]),
        })
        .unwrap();
    store
        .set_user_metadata(
            &owner.id,
            "external_access_v1",
            r#"{"gatewayEnabled":true,"mcpEnabled":false}"#,
        )
        .unwrap();
    let issued = store
        .issue_developer_token(
            &owner.id,
            "WebSocket",
            &["messages:read".into()],
            std::slice::from_ref(&account_id),
            3600,
        )
        .unwrap();
    drop(store);
    let mut config = HttpAdapterConfig::new(&directory);
    config.gateway = true;
    let router = build_router(config).unwrap();
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
    });

    let url = format!("ws://{address}/gateway/v1/events");
    let mut rejected = url.clone().into_client_request().unwrap();
    rejected
        .headers_mut()
        .insert("origin", "https://attacker.example".parse().unwrap());
    assert!(matches!(
        tokio_tungstenite::connect_async(rejected).await,
        Err(tokio_tungstenite::tungstenite::Error::Http(response))
            if response.status() == StatusCode::FORBIDDEN
    ));

    let mut request = url.into_client_request().unwrap();
    request.headers_mut().insert(
        "authorization",
        format!("Bearer {}", issued.raw).parse().unwrap(),
    );
    request
        .headers_mut()
        .insert("origin", format!("http://{address}").parse().unwrap());
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let connected = tokio::time::timeout(std::time::Duration::from_secs(2), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let Message::Text(connected) = connected else {
        panic!("expected connected text frame");
    };
    assert_eq!(
        serde_json::from_str::<Value>(&connected).unwrap()["type"],
        "connected"
    );

    let event_message = imail_protocol::MessageReadModel {
        headers: Default::default(),
        id: "ws-message".into(),
        account_id: account_id.clone(),
        mailbox: "INBOX".into(),
        mailbox_role: "inbox".into(),
        uid: 1,
        message_id: None,
        from: json!({ "name": "Sender", "address": "sender@example.net" }),
        to: json!([{ "name": "Events", "address": "events@example.com" }]),
        subject: "WebSocket event".into(),
        preview: "Event preview".into(),
        text: "must not be emitted".into(),
        html: Some("<p>must not be emitted</p>".into()),
        date: "2026-08-10T10:00:00.000Z".into(),
        unread: true,
        flagged: false,
        has_attachments: false,
        attachments: json!([]),
        labels: json!([]),
        snoozed_until: None,
    };
    SyncRuntimeStore::open_database(&database)
        .unwrap()
        .record_message_created(
            &account_id,
            "events@example.com",
            &[event_message],
            chrono::Utc::now(),
        )
        .unwrap();
    let event = tokio::time::timeout(std::time::Duration::from_secs(3), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let Message::Text(event) = event else {
        panic!("expected message.created text frame");
    };
    let event = serde_json::from_str::<Value>(&event).unwrap();
    assert_eq!(event["type"], "message.created");
    assert_eq!(event["data"]["message"]["id"], "ws-message");
    assert!(event["data"]["message"].get("text").is_none());
    assert!(event.to_string().find(&account_id).is_none());

    SqliteAuthStore::open_database(&database)
        .unwrap()
        .revoke_developer_token(&owner.id, &issued.token.id)
        .unwrap();
    let closed = tokio::time::timeout(std::time::Duration::from_secs(3), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(
        matches!(closed, Message::Close(Some(frame)) if frame.code == tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Policy)
    );
    drop(socket);
    server.abort();
    let _ = server.await;
    fs::remove_dir_all(directory).unwrap();
}
