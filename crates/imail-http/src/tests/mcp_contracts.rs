use super::*;

#[tokio::test]
async fn mcp_requires_full_scope_and_user_switch_then_serves_protocol_and_tools() {
    use imail_core::external_access::{ExternalAccessChanges, ExternalAccessService};

    struct McpOAuthProvider;
    impl OAuthProviderPort for McpOAuthProvider {
        fn token_request(
            &mut self,
            _: &OAuthConfig,
            _: OAuthGrant<'_>,
        ) -> Result<OAuthTokenResponse, OAuthError> {
            unreachable!("MCP OAuth begin must not exchange a token")
        }
        fn fetch_identity(
            &mut self,
            _: &OAuthConfig,
            _: &OAuthTokenResponse,
            _: &str,
        ) -> Result<OAuthIdentity, OAuthError> {
            unreachable!("MCP OAuth begin must not fetch identity")
        }
    }

    let directory = authentication_directory("mcp-transport");
    let database = directory.join("imail.sqlite");
    fs::write(directory.join("master.key"), "52".repeat(32)).unwrap();
    let (token, owner_id) = {
        let mut store = SqliteAuthStore::open_database(&database).unwrap();
        let user = store
            .create_user("mcp@example.com", "MCP", "Owner password 123!")
            .unwrap();
        store
            .insert_account_if_email_available(&AccountRecord {
                id: "mcp-account".into(), owner_id: user.id.clone(), provider: "custom".into(), email: "mailbox@example.com".into(),
                display_name: "Mailbox".into(), group: "个人".into(), group_icon: "folder".into(), color: "#168aad".into(),
                settings: json!({"imapHost":"imap.example.com","imapPort":993,"imapSecure":true,"smtpHost":"smtp.example.com","smtpPort":465,"smtpSecure":true,"password":"settings-must-not-leak"}),
                proxy: Some(json!({"protocol":"http","host":"proxy.example.com","port":8080,"password":"proxy-must-not-leak"})),
                encrypted_secret: "mail-secret-must-not-leak".into(), auth_method: Some("app-password".into()),
                created_at: "2026-08-10T00:00:00.000Z".into(), last_sync_at: None, status: "connected".into(), last_error: None,
                mailboxes: json!([{"path":"INBOX","specialUse":"\\Inbox"},{"path":"Projects","specialUse":null}]),
            })
            .unwrap();
        let issued = store
            .issue_developer_token(&user.id, "MCP", &["mcp:full".into()], &[], 3_600)
            .unwrap();
        (issued.raw, user.id)
    };
    let mut config = HttpAdapterConfig::new(&directory);
    config.mcp = true;
    config.connection_probe = Arc::new(|_: &MailConnectionConfig| Ok(()));
    config.oauth_environment = OAuthEnvironment {
        callback_base_url: "http://127.0.0.1:8787/api/oauth".into(),
        google_client_id: Some("mcp-client-id".into()),
        google_client_secret: Some("mcp-client-secret".into()),
        ..OAuthEnvironment::default()
    };
    config.oauth_provider_factory =
        Arc::new(|| Box::new(McpOAuthProvider) as Box<dyn OAuthProviderPort>);
    let mail_state = Arc::new(Mutex::new(MailTestState::default()));
    config.mail_transport_factory = Arc::new(TestMailTransportFactory {
        state: Arc::clone(&mail_state),
        source: b"From: sender@example.org\r\nTo: future@example.net\r\nSubject: MCP attachment\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=part\r\n\r\n--part\r\nContent-Type: text/plain\r\n\r\nBody\r\n--part\r\nContent-Type: text/plain; name=report.txt\r\nContent-Disposition: attachment; filename=report.txt\r\nContent-Transfer-Encoding: base64\r\n\r\naGVsbG8=\r\n--part--\r\n".to_vec(),
    });
    let router = build_router(config).unwrap();
    let rpc = |body: &str, raw: &str| {
        Request::builder()
            .method(Method::POST)
            .uri("/mcp")
            .header(HOST, "127.0.0.1:8787")
            .header(CONTENT_TYPE, "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("authorization", format!("Bearer {raw}"))
            .body(Body::from(body.to_owned()))
            .unwrap()
    };
    let hostile = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/mcp")
                .header(HOST, "attacker.example")
                .header(CONTENT_TYPE, "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(hostile.status(), StatusCode::FORBIDDEN);
    let bad_accept = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/mcp")
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("accept", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bad_accept.status(), StatusCode::NOT_ACCEPTABLE);
    let bad_content = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/mcp")
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "text/plain")
                .header("accept", "application/json, text/event-stream")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bad_content.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let disabled = router
        .clone()
        .oneshot(rpc(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}"#,
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(disabled.status(), StatusCode::FORBIDDEN);
    {
        let mut store = SqliteAuthStore::open_database(&database).unwrap();
        ExternalAccessService::new(&mut store)
            .update(
                &owner_id,
                ExternalAccessChanges {
                    mcp_enabled: Some(true),
                    gateway_enabled: None,
                },
            )
            .unwrap();
    }
    let initialized = router
        .clone()
        .oneshot(rpc(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}"#,
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(initialized.status(), StatusCode::OK);
    assert_eq!(
        json(initialized).await["result"]["serverInfo"]["name"],
        "imail"
    );
    let legacy_header = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/mcp")
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("accept", "application/json, text/event-stream")
                .header("authorization", format!("Bearer {token}"))
                .header("mcp-protocol-version", "2025-06-18")
                .body(Body::from(r#"{"jsonrpc":"2.0","id":100,"method":"ping"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(legacy_header.status(), StatusCode::OK);
    assert_eq!(json(legacy_header).await["id"], 100);
    let modern_rpc = |body: &'static str, method: &'static str, name: Option<&'static str>| {
        let mut request = Request::builder()
            .method(Method::POST)
            .uri("/mcp")
            .header(HOST, "127.0.0.1:8787")
            .header(CONTENT_TYPE, "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("authorization", format!("Bearer {token}"))
            .header("mcp-protocol-version", "2026-07-28")
            .header("mcp-method", method);
        if let Some(name) = name {
            request = request.header("mcp-name", name);
        }
        request.body(Body::from(body)).unwrap()
    };
    let discovered = json(
        router
            .clone()
            .oneshot(modern_rpc(
                r#"{"jsonrpc":"2.0","id":1001,"method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{},"io.modelcontextprotocol/clientInfo":{"name":"rust-test","version":"1.0.0"}}}}"#,
                "server/discover",
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        discovered["result"]["supportedVersions"],
        json!(["2026-07-28"])
    );
    assert_eq!(
        discovered["result"]["capabilities"]["tools"]["listChanged"],
        true
    );
    assert_eq!(discovered["result"]["resultType"], "complete");
    assert_eq!(discovered["result"]["ttlMs"], 0);
    assert_eq!(discovered["result"]["cacheScope"], "private");
    let modern_list = router
        .clone()
        .oneshot(modern_rpc(
            r#"{"jsonrpc":"2.0","id":101,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#,
            "tools/list",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(modern_list.status(), StatusCode::OK);
    let modern_list = json(modern_list).await;
    assert_eq!(modern_list["result"]["resultType"], "complete");
    assert_eq!(modern_list["result"]["ttlMs"], 0);
    assert_eq!(modern_list["result"]["cacheScope"], "private");
    assert_eq!(
        modern_list["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "imail"
    );
    let tool_names = modern_list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<Vec<_>>();
    assert!(tool_names.contains(&"translation_profiles_list"));
    assert!(tool_names.contains(&"message_translate"));
    let translation_profiles = json(
        router
            .clone()
            .oneshot(modern_rpc(
                r#"{"jsonrpc":"2.0","id":108,"method":"tools/call","params":{"name":"translation_profiles_list","arguments":{},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#,
                "tools/call",
                Some("translation_profiles_list"),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        translation_profiles["result"]["structuredContent"]["profiles"],
        json!([])
    );
    let modern_call = json(
        router
            .clone()
            .oneshot(modern_rpc(
                r#"{"jsonrpc":"2.0","id":102,"method":"tools/call","params":{"name":"accounts_list","arguments":{},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#,
                "tools/call",
                Some("accounts_list"),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(modern_call["result"]["resultType"], "complete");
    assert_eq!(
        modern_call["result"]["structuredContent"]["accounts"][0]["email"],
        "mailbox@example.com"
    );
    let missing_envelope = router
        .clone()
        .oneshot(modern_rpc(
            r#"{"jsonrpc":"2.0","id":103,"method":"tools/list","params":{}}"#,
            "tools/list",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(missing_envelope.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json(missing_envelope).await["error"]["code"], -32602);
    let mismatched_method = router
        .clone()
        .oneshot(modern_rpc(
            r#"{"jsonrpc":"2.0","id":104,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#,
            "ping",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(mismatched_method.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json(mismatched_method).await["error"]["code"], -32020);
    let modern_batch = router
        .clone()
        .oneshot(modern_rpc(
            r#"[{"jsonrpc":"2.0","id":105,"method":"ping","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}]"#,
            "ping",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(modern_batch.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json(modern_batch).await["error"]["code"], -32600);
    let legacy_batch = json(
        router
            .clone()
            .oneshot(rpc(
                r#"[{"jsonrpc":"2.0","id":106,"method":"ping"},{"jsonrpc":"2.0","method":"notifications/initialized"},{"jsonrpc":"2.0","id":107,"method":"tools/list","params":{}}]"#,
                &token,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(legacy_batch.as_array().unwrap().len(), 2);
    assert_eq!(legacy_batch[0]["id"], 106);
    assert_eq!(legacy_batch[1]["id"], 107);
    let listed = json(
        router
            .clone()
            .oneshot(rpc(
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
                &token,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 63);
    assert!(listed.to_string().contains("accounts_list"));
    let tools = listed["result"]["tools"].as_array().unwrap();
    let shared_contract: Value =
        serde_json::from_str(include_str!("../../../../contracts/mcp-tools.json")).unwrap();
    assert_eq!(listed["result"]["tools"], shared_contract["tools"]);
    let move_tool = tools
        .iter()
        .find(|tool| tool["name"] == "message_move")
        .unwrap();
    assert_eq!(move_tool["annotations"]["destructiveHint"], true);
    assert_eq!(move_tool["annotations"]["idempotentHint"], false);
    let reply_draft_tool = tools
        .iter()
        .find(|tool| tool["name"] == "mail_reply_draft_create")
        .unwrap();
    assert_eq!(reply_draft_tool["annotations"]["destructiveHint"], false);
    let draft_schedule_tool = tools
        .iter()
        .find(|tool| tool["name"] == "mail_draft_schedule")
        .unwrap();
    assert_eq!(
        draft_schedule_tool["inputSchema"]["properties"]["confirmed"]["const"],
        true
    );
    let send_tool = tools
        .iter()
        .find(|tool| tool["name"] == "message_send")
        .unwrap();
    assert_eq!(
        send_tool["inputSchema"]["required"],
        json!(["accountEmail", "to", "subject", "text"])
    );
    let add_tool = tools
        .iter()
        .find(|tool| tool["name"] == "account_add_with_code")
        .unwrap();
    assert!(add_tool["inputSchema"]["properties"]["groupIcon"]["enum"]
        .as_array()
        .unwrap()
        .contains(&json!("star")));
    let accounts = json(
        router
            .clone()
            .oneshot(rpc(
                r#"{"jsonrpc":"2.0","id":21,"method":"tools/call","params":{"name":"accounts_list","arguments":{}}}"#,
                &token,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        accounts["result"]["structuredContent"]["accounts"][0]["email"],
        "mailbox@example.com"
    );
    assert!(!accounts.to_string().contains("mail-secret-must-not-leak"));
    assert!(!accounts.to_string().contains("proxy-must-not-leak"));
    assert!(!accounts.to_string().contains("settings-must-not-leak"));
    let added = json(
        router
            .clone()
            .oneshot(rpc(
                r##"{"jsonrpc":"2.0","id":22,"method":"tools/call","params":{"name":"account_add_with_code","arguments":{"provider":"custom","email":"future@example.net","displayName":"Future","authorizationCode":"future-secret-must-not-leak","group":"个人","groupIcon":"folder","color":"#168aad","settings":{"imapHost":"imap.example.net","imapPort":993,"imapSecure":true,"smtpHost":"smtp.example.net","smtpPort":465,"smtpSecure":true}}}}"##,
                &token,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        added["result"]["structuredContent"]["account"]["email"],
        "future@example.net"
    );
    assert!(!added.to_string().contains("future-secret-must-not-leak"));
    let proxied = json(
        router
            .clone()
            .oneshot(rpc(
                r#"{"jsonrpc":"2.0","id":23,"method":"tools/call","params":{"name":"account_proxy_update","arguments":{"email":"future@example.net","enabled":true,"protocol":"http","host":"127.0.0.1","port":8080,"username":"agent","password":"proxy-new-must-not-leak"}}}"#,
                &token,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        proxied["result"]["structuredContent"]["account"]["proxy"]["host"],
        "127.0.0.1"
    );
    assert!(!proxied.to_string().contains("proxy-new-must-not-leak"));
    let bad_draft = json(router.clone().oneshot(rpc(
        r#"{"jsonrpc":"2.0","id":230,"method":"tools/call","params":{"name":"draft_save","arguments":{"accountEmail":"future@example.net","to":["friend@example.com"],"attachments":[{"filename":"bad.bin","contentType":"application/octet-stream","data":"not-base64"}]}}}"#,&token)).await.unwrap()).await;
    assert_eq!(bad_draft["result"]["isError"], true);
    assert!(SqliteAuthStore::open_database(&database)
        .unwrap()
        .list_drafts(&owner_id)
        .unwrap()
        .is_empty());
    let good_draft = json(router.clone().oneshot(rpc(
        r#"{"jsonrpc":"2.0","id":2301,"method":"tools/call","params":{"name":"draft_save","arguments":{"accountEmail":"future@example.net","to":["friend@example.com"],"attachments":[{"filename":"hello.txt","contentType":"text/plain","data":"aGVsbG8="}]}}}"#,&token)).await.unwrap()).await;
    assert_eq!(
        good_draft["result"]["structuredContent"]["draft"]["attachments"][0]["size"],
        5
    );
    assert!(good_draft["result"]["structuredContent"]["draft"]["attachments"][0]["id"].is_string());
    assert!(!good_draft.to_string().contains("aGVsbG8="));
    let oauth = json(router.clone().oneshot(rpc(
        r##"{"jsonrpc":"2.0","id":231,"method":"tools/call","params":{"name":"account_start_oauth","arguments":{"provider":"gmail","displayName":"OAuth via MCP","group":"个人","color":"#168aad"}}}"##,&token)).await.unwrap()).await;
    assert!(oauth["result"]["structuredContent"]["authorizationUrl"]
        .as_str()
        .is_some_and(|value| value.starts_with("https://")));
    assert!(oauth["result"]["structuredContent"]["state"].is_string());
    assert!(!oauth.to_string().contains("mcp-client-secret"));
    {
        let mut store = SqliteAuthStore::open_database(&database).unwrap();
        let future = store
            .list_accounts(&owner_id)
            .unwrap()
            .into_iter()
            .find(|account| account.email == "future@example.net")
            .unwrap();
        store
            .upsert_message(
                &owner_id,
                &imail_protocol::MessageReadModel {
                    headers: Default::default(),
                    id: "mcp-side-effect".into(), account_id: future.id, mailbox: "INBOX".into(), mailbox_role: "inbox".into(), uid: 7,
                    message_id: Some("<mcp@example.net>".into()), from: json!({"name":"Sender","address":"sender@example.org"}), to: json!([{"address":"future@example.net"}]),
                    subject: "MCP side effects".into(), preview: "Body".into(), text: "Body".into(), html: None, date: "2026-08-10T08:00:00.000Z".into(),
                    unread: true, flagged: false, has_attachments: true, attachments: json!([{"filename":"report.txt","contentType":"text/plain","size":5,"index":0}]), labels: json!([]), snoozed_until: None,
                },
            )
            .unwrap();
    }
    let updated = json(router.clone().oneshot(rpc(
        r#"{"jsonrpc":"2.0","id":24,"method":"tools/call","params":{"name":"message_update","arguments":{"messageId":"mcp-side-effect","unread":false,"labels":["MCP"]}}}"#,&token)).await.unwrap()).await;
    assert_eq!(
        updated["result"]["structuredContent"]["message"]["unread"],
        false
    );
    let queued = json(router.clone().oneshot(rpc(
        r#"{"jsonrpc":"2.0","id":242,"method":"tools/call","params":{"name":"mail_work_item_set","arguments":{"messageId":"mcp-side-effect","status":"needsReply","note":"Agent queue"}}}"#,&token)).await.unwrap()).await;
    assert_eq!(
        queued["result"]["structuredContent"]["item"]["status"],
        "needsReply"
    );
    let queue = json(router.clone().oneshot(rpc(
        r#"{"jsonrpc":"2.0","id":243,"method":"tools/call","params":{"name":"mail_work_items_list","arguments":{"status":"needsReply"}}}"#,&token)).await.unwrap()).await;
    assert_eq!(
        queue["result"]["structuredContent"]["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let reply = json(router.clone().oneshot(rpc(
        r#"{"jsonrpc":"2.0","id":244,"method":"tools/call","params":{"name":"mail_reply_draft_create","arguments":{"messageId":"mcp-side-effect","mode":"reply","text":"Agent reply"}}}"#,&token)).await.unwrap()).await;
    assert_eq!(
        reply["result"]["structuredContent"]["item"]["status"],
        "needsReview"
    );
    assert_eq!(
        reply["result"]["structuredContent"]["draft"]["to"],
        json!(["sender@example.org"])
    );
    let reply_draft_id = reply["result"]["structuredContent"]["draft"]["id"]
        .as_str()
        .unwrap();
    let reply_send_at = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
    let reply_schedule = format!(
        r#"{{"jsonrpc":"2.0","id":245,"method":"tools/call","params":{{"name":"mail_draft_schedule","arguments":{{"draftId":"{reply_draft_id}","requestId":"00000000-0000-4000-8000-000000000245","sendAt":"{reply_send_at}","confirmed":true}}}}}}"#
    );
    let reply_scheduled = json(
        router
            .clone()
            .oneshot(rpc(&reply_schedule, &token))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        reply_scheduled["result"]["structuredContent"]["item"]["status"],
        "scheduled"
    );
    mail_state.lock().unwrap().reject_flags = true;
    let rejected = json(router.clone().oneshot(rpc(
        r#"{"jsonrpc":"2.0","id":241,"method":"tools/call","params":{"name":"message_update","arguments":{"messageId":"mcp-side-effect","unread":true}}}"#,&token)).await.unwrap()).await;
    assert_eq!(rejected["result"]["isError"], true);
    assert!(
        !SqliteAuthStore::open_database(&database)
            .unwrap()
            .list_messages(&owner_id)
            .unwrap()
            .into_iter()
            .find(|message| message.id == "mcp-side-effect")
            .unwrap()
            .unread
    );
    mail_state.lock().unwrap().reject_flags = false;
    let attachment = json(router.clone().oneshot(rpc(
        r#"{"jsonrpc":"2.0","id":25,"method":"tools/call","params":{"name":"attachment_download","arguments":{"messageId":"mcp-side-effect","index":0}}}"#,&token)).await.unwrap()).await;
    assert_eq!(
        attachment["result"]["structuredContent"]["data"],
        "aGVsbG8="
    );
    let moved = json(router.clone().oneshot(rpc(
        r#"{"jsonrpc":"2.0","id":26,"method":"tools/call","params":{"name":"message_move","arguments":{"messageId":"mcp-side-effect","destination":"archive"}}}"#,&token)).await.unwrap()).await;
    assert_eq!(moved["result"]["structuredContent"]["mailbox"], "Archive");
    let sent = json(router.clone().oneshot(rpc(
        r#"{"jsonrpc":"2.0","id":27,"method":"tools/call","params":{"name":"message_send","arguments":{"accountEmail":"future@example.net","to":["friend@example.com"],"subject":"MCP send","text":"Hello"}}}"#,&token)).await.unwrap()).await;
    assert_eq!(
        sent["result"]["structuredContent"]["delivery"]["messageId"],
        "<sent@example.com>"
    );
    let send_at = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
    let schedule_request = format!(
        r#"{{"jsonrpc":"2.0","id":271,"method":"tools/call","params":{{"name":"outbox_schedule","arguments":{{"accountEmail":"future@example.net","to":["later@example.com"],"subject":"MCP scheduled","text":"Later","attachments":[{{"filename":"note.txt","contentType":"text/plain","data":"aGVsbG8="}}],"requestId":"00000000-0000-4000-8000-000000000271","sendAt":"{send_at}"}}}}}}"#
    );
    let scheduled = json(
        router
            .clone()
            .oneshot(rpc(&schedule_request, &token))
            .await
            .unwrap(),
    )
    .await;
    let outbox_id = scheduled["result"]["structuredContent"]["item"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let repeated = json(
        router
            .clone()
            .oneshot(rpc(&schedule_request, &token))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        repeated["result"]["structuredContent"]["item"]["id"],
        outbox_id
    );
    let outbox = json(router.clone().oneshot(rpc(
        r#"{"jsonrpc":"2.0","id":272,"method":"tools/call","params":{"name":"outbox_list","arguments":{}}}"#,
        &token,
    )).await.unwrap()).await;
    assert_eq!(
        outbox["result"]["structuredContent"]["items"][0]["subject"],
        "MCP scheduled"
    );
    let cancel_request = format!(
        r#"{{"jsonrpc":"2.0","id":273,"method":"tools/call","params":{{"name":"outbox_cancel","arguments":{{"outboxId":"{outbox_id}"}}}}}}"#
    );
    let cancelled = json(
        router
            .clone()
            .oneshot(rpc(&cancel_request, &token))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(cancelled["result"]["structuredContent"]["cancelled"], true);
    {
        let mail = mail_state.lock().unwrap();
        assert_eq!(mail.flag_updates, 2);
        assert_eq!(mail.fetches, 1);
        assert_eq!(mail.moves, 1);
        assert_eq!(mail.sends, 1);
    }
    let settings = json(
        router
            .clone()
            .oneshot(rpc(
                r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"settings_update","arguments":{"startupView":"inbox"}}}"#,
                &token,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_ne!(settings["result"]["isError"], true);
    let policy = json(
        router
            .clone()
            .oneshot(rpc(
                r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"sync_policy_update","arguments":{"email":"mailbox@example.com","folderMode":"selected","selectedMailboxes":["Projects"],"notifyOnError":false}}}"#,
                &token,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        policy["result"]["structuredContent"]["policy"]["folderMode"],
        "selected"
    );
    assert_eq!(
        policy["result"]["structuredContent"]["policy"]["selectedMailboxes"],
        json!(["Projects"])
    );
    let queued = json(
        router
            .clone()
            .oneshot(rpc(
                r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"mailbox_sync","arguments":{"email":"mailbox@example.com","mailboxRole":"custom","mailboxPath":"Projects"}}}"#,
                &token,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        queued["result"]["structuredContent"]["results"][0]["status"],
        "queued"
    );
    let sync = SyncRuntimeStore::open_database(&database).unwrap();
    let jobs = sync.account_jobs("mcp-account", 10).unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].mailbox.as_deref(), Some("Projects"));
    drop(sync);
    let audits = SqliteAuthStore::open_database(&database)
        .unwrap()
        .security_audit_details(&owner_id, "mcp.management-tool-called", 50)
        .unwrap();
    assert!(audits.iter().any(|detail| {
        detail.get("tool").map(String::as_str) == Some("settings_update")
            && detail
                .get("authorizationCodeId")
                .is_some_and(|value| !value.is_empty())
    }));
    assert!(!serde_json::to_string(&audits).unwrap().contains(&token));
    fs::remove_dir_all(directory).unwrap();
}
