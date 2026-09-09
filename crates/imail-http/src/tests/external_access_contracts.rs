use super::*;

#[tokio::test]
async fn manages_user_scoped_external_access_and_developer_tokens() {
    let directory = authentication_directory("developer-controls");
    let database = directory.join("imail.sqlite");
    let mut store = SqliteAuthStore::open_database(&database).unwrap();
    let owner = store
        .create_user("developer-owner", "Developer Owner", "owner-password-123")
        .unwrap();
    let other = store
        .create_user("developer-other", "Developer Other", "other-password-123")
        .unwrap();
    let owner_session = store.create_session(&owner.id).unwrap();
    let other_session = store.create_session(&other.id).unwrap();
    let account_id = Uuid::new_v4().to_string();
    store
        .upsert_account(&AccountRecord {
            id: account_id.clone(),
            owner_id: owner.id.clone(),
            provider: "icloud".into(),
            email: "owner@example.com".into(),
            display_name: "Owner Mail".into(),
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
    for (id, uid, date, subject) in [
        ("gateway-message-2", 2, "2026-08-10T09:00:00.000Z", "Newest"),
        ("gateway-message-1", 1, "2026-08-10T08:00:00.000Z", "Older"),
    ] {
        store
            .upsert_message(
                &owner.id,
                &imail_protocol::MessageReadModel {
                    headers: Default::default(),
                    id: id.into(),
                    account_id: account_id.clone(),
                    mailbox: "INBOX".into(),
                    mailbox_role: "inbox".into(),
                    uid,
                    message_id: None,
                    from: json!({ "name": "Sender", "address": "sender@example.net" }),
                    to: if id == "gateway-message-2" {
                        json!([{ "name": "Private", "address": "private-alias@icloud.com" }])
                    } else {
                        json!([{ "name": "Owner", "address": "owner@example.com" }])
                    },
                    subject: subject.into(),
                    preview: "Gateway preview".into(),
                    text: "Gateway body".into(),
                    html: Some("<p>Gateway body</p>".into()),
                    date: date.into(),
                    unread: true,
                    flagged: false,
                    has_attachments: false,
                    attachments: json!([]),
                    labels: json!(["gateway"]),
                    snoozed_until: None,
                },
            )
            .unwrap();
    }
    store
        .upsert_apple_hme_address(&AppleHmeAddressRecord {
            account_id: account_id.clone(),
            user_id: owner.id.clone(),
            anonymous_id: "gateway-hme-1".into(),
            email: "private-alias@icloud.com".into(),
            label: "Gateway private mailbox".into(),
            note: String::new(),
            forward_to_email: "owner@example.com".into(),
            active: true,
            origin: "WEB".into(),
            created_at: Some("2026-08-10T07:00:00.000Z".into()),
            updated_at: "2026-08-10T07:00:00.000Z".into(),
        })
        .unwrap();
    drop(store);
    let mut config = HttpAdapterConfig::new(&directory);
    config.gateway = true;
    let router = build_router(config).unwrap();
    let request = |method: Method, uri: &str, session: &str, body: Body| {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(HOST, "127.0.0.1:8787")
            .header(CONTENT_TYPE, "application/json")
            .header("cookie", format!("imail_session={session}"))
            .body(body)
            .unwrap()
    };

    let defaults = json(
        router
            .clone()
            .oneshot(request(
                Method::GET,
                "/api/external-access",
                &owner_session,
                Body::empty(),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        defaults["settings"],
        json!({ "gatewayEnabled": false, "mcpEnabled": false })
    );
    let updated = json(
        router
            .clone()
            .oneshot(request(
                Method::PATCH,
                "/api/external-access",
                &owner_session,
                Body::from(r#"{"gatewayEnabled":true}"#),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        updated["settings"],
        json!({ "gatewayEnabled": true, "mcpEnabled": false })
    );
    let other_settings = json(
        router
            .clone()
            .oneshot(request(
                Method::GET,
                "/api/external-access",
                &other_session,
                Body::empty(),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(other_settings["settings"]["gatewayEnabled"], false);

    let missing_mailbox = router
        .clone()
        .oneshot(request(
            Method::POST,
            "/api/developer-tokens",
            &owner_session,
            Body::from(r#"{"name":"Bad","scopes":["messages:read"],"mailboxes":["missing@example.com"],"ttlSeconds":3600}"#),
        ))
        .await
        .unwrap();
    assert_eq!(missing_mailbox.status(), StatusCode::BAD_REQUEST);
    let created = router
        .clone()
        .oneshot(request(
            Method::POST,
            "/api/developer-tokens",
            &owner_session,
            Body::from(r#"{"name":"Gateway","scopes":["messages:read","accounts:read"],"mailboxes":["OWNER@example.com"],"ttlSeconds":3600}"#),
        ))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    let created = json(created).await;
    let raw = created["token"].as_str().unwrap().to_string();
    let token_id = created["detail"]["id"].as_str().unwrap().to_string();
    assert!(raw.starts_with("imail_"));
    assert_eq!(created["detail"]["mailboxes"], json!(["owner@example.com"]));
    assert!(created["detail"].get("ownerId").is_none());
    assert!(created["detail"].get("accountIds").is_none());
    assert!(created["detail"].get("tokenHash").is_none());

    let disable = router
        .clone()
        .oneshot(request(
            Method::PATCH,
            "/api/external-access",
            &owner_session,
            Body::from(r#"{"gatewayEnabled":false}"#),
        ))
        .await
        .unwrap();
    assert_eq!(disable.status(), StatusCode::OK);
    let gateway_request = |uri: String, token: &str| {
        Request::builder()
            .uri(uri)
            .header(HOST, "127.0.0.1:8787")
            .header("authorization", format!("Bearer {token}"))
            .header("x-request-id", "gateway.test:1")
            .body(Body::empty())
            .unwrap()
    };
    let disabled_gateway = router
        .clone()
        .oneshot(gateway_request("/gateway/v1/messages".into(), &raw))
        .await
        .unwrap();
    assert_eq!(disabled_gateway.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        json(disabled_gateway).await["error"]["code"],
        "GATEWAY_DISABLED"
    );
    assert_eq!(
        router
            .clone()
            .oneshot(request(
                Method::PATCH,
                "/api/external-access",
                &owner_session,
                Body::from(r#"{"gatewayEnabled":true}"#),
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let mailboxes = router
        .clone()
        .oneshot(gateway_request("/gateway/v1/mailboxes".into(), &raw))
        .await
        .unwrap();
    assert_eq!(mailboxes.status(), StatusCode::OK);
    assert_eq!(mailboxes.headers()["x-request-id"], "gateway.test:1");
    let mailboxes = json(mailboxes).await;
    assert_eq!(mailboxes["mailboxes"][0]["email"], "owner@example.com");
    assert!(mailboxes["mailboxes"][0].get("id").is_none());
    assert!(mailboxes["mailboxes"][0].get("settings").is_none());
    let first_page = json(
        router
            .clone()
            .oneshot(gateway_request(
                "/gateway/v1/messages?limit=1&unread=true".into(),
                &raw,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(first_page["messages"][0]["id"], "gateway-message-2");
    assert!(first_page["messages"][0].get("text").is_none());
    assert_eq!(first_page["page"]["hasMore"], true);
    let cursor = first_page["page"]["nextCursor"].as_str().unwrap();
    let second_page = json(
        router
            .clone()
            .oneshot(gateway_request(
                format!("/gateway/v1/messages?limit=1&cursor={cursor}"),
                &raw,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(second_page["messages"][0]["id"], "gateway-message-1");
    let hme_page = json(
        router
            .clone()
            .oneshot(gateway_request(
                "/gateway/v1/mailboxes/private-alias@icloud.com/messages".into(),
                &raw,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(hme_page["messages"].as_array().unwrap().len(), 1);
    assert_eq!(hme_page["messages"][0]["id"], "gateway-message-2");
    assert_eq!(
        hme_page["messages"][0]["accountEmail"],
        "private-alias@icloud.com"
    );
    assert!(!hme_page.to_string().contains("owner@example.com"));
    let detail = json(
        router
            .clone()
            .oneshot(gateway_request(
                "/gateway/v1/messages/gateway-message-2".into(),
                &raw,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(detail["message"]["text"], "Gateway body");
    assert_eq!(
        detail["message"]["accountEmail"],
        "private-alias@icloud.com"
    );
    assert!(!detail.to_string().contains("owner@example.com"));
    let invalid_cursor = router
        .clone()
        .oneshot(gateway_request(
            "/gateway/v1/messages?cursor=broken".into(),
            &raw,
        ))
        .await
        .unwrap();
    assert_eq!(invalid_cursor.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json(invalid_cursor).await["error"]["code"],
        "INVALID_CURSOR"
    );
    let denied_send = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/gateway/v1/send")
                .header(HOST, "127.0.0.1:8787")
                .header("authorization", format!("Bearer {raw}"))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"mailbox":"owner@example.com","to":["to@example.com"],"subject":"Hello","text":"Body"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied_send.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(json(denied_send).await["error"]["code"], "UNAUTHORIZED");
    let openapi = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/gateway/openapi.json")
                .header(HOST, "127.0.0.1:8787")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(openapi.status(), StatusCode::OK);
    assert_eq!(openapi.headers()[CACHE_CONTROL], "no-store");
    let openapi = json(openapi).await;
    assert_eq!(openapi["openapi"], "3.1.0");
    assert_eq!(openapi["servers"][0]["url"], "/gateway/v1");
    assert_eq!(
        openapi["components"]["securitySchemes"]["bearerAuth"]["scheme"],
        "bearer"
    );
    for path in [
        "/health",
        "/mailboxes",
        "/messages",
        "/mailboxes/{mailbox}/messages",
        "/messages/{messageId}",
        "/messages/{messageId}/attachments/{index}",
        "/send",
    ] {
        assert!(openapi["paths"].get(path).is_some(), "missing {path}");
    }
    assert_eq!(openapi["x-websocket"]["url"], "/gateway/v1/events");
    assert_eq!(
        openapi["x-websocket"]["authentication"]["requiredScope"],
        "messages:read"
    );
    let docs = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/gateway/docs")
                .header(HOST, "127.0.0.1:8787")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(docs.status(), StatusCode::OK);
    assert_eq!(docs.headers()[CACHE_CONTROL], "no-store");
    assert!(text_body(docs).await.contains("/gateway/openapi.json"));

    let mcp = json(
        router
            .clone()
            .oneshot(request(
                Method::POST,
                "/api/developer-tokens",
                &owner_session,
                Body::from(r#"{"name":"MCP","scopes":["messages:read","mcp:full"],"mailboxes":[],"ttlSeconds":3600}"#),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert!(mcp["token"].as_str().unwrap().starts_with("imail_mcp_"));
    assert_eq!(mcp["detail"]["scopes"], json!(["mcp:full"]));
    assert_eq!(mcp["detail"]["mailboxes"], json!(["owner@example.com"]));

    let listing = json(
        router
            .clone()
            .oneshot(request(
                Method::GET,
                "/api/developer-tokens",
                &owner_session,
                Body::empty(),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(listing["tokens"].as_array().unwrap().len(), 2);
    assert!(!listing.to_string().contains(&raw));
    assert_eq!(
        SqliteAuthStore::open_database(&database)
            .unwrap()
            .authenticate_developer_token(&raw, "messages:read")
            .unwrap()
            .unwrap()
            .account_ids,
        vec![account_id]
    );
    assert_eq!(
        router
            .clone()
            .oneshot(request(
                Method::DELETE,
                &format!("/api/developer-tokens/{token_id}"),
                &owner_session,
                Body::empty(),
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    assert!(SqliteAuthStore::open_database(&database)
        .unwrap()
        .authenticate_developer_token(&raw, "messages:read")
        .unwrap()
        .is_none());
    fs::remove_dir_all(directory).unwrap();
}
