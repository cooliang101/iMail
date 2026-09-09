use super::*;

#[tokio::test]
async fn protects_and_isolates_preferences_with_the_session_user_context() {
    let directory = authentication_directory("preferences");
    let mut store = SqliteAuthStore::open_database(directory.join("imail.sqlite")).unwrap();
    let owner = store
        .create_user("preferences-owner", "Owner", "owner-password-123")
        .unwrap();
    let other = store
        .create_user("preferences-other", "Other", "other-password-123")
        .unwrap();
    let owner_session = store.create_session(&owner.id).unwrap();
    let other_session = store.create_session(&other.id).unwrap();
    drop(store);
    let router = build_router(HttpAdapterConfig::new(&directory)).unwrap();

    let unauthorized = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/preferences")
                .header(HOST, "127.0.0.1:8787")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(json(unauthorized).await["error"], "登录已过期，请重新登录");

    let updated = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PATCH)
                .uri("/api/preferences")
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::from(format!(
                    r##"{{"userId":"{}","theme":"constructivist-red","customTheme":{{"name":"服务端构成","canvas":"#d8d0be","surface":"#f5eedb","surfaceSubtle":"#eee5d1","rail":"#24201e","text":"#24201e","textSecondary":"#5e5751","border":"#b9ae9d","accent":"#c42a22","accentSubtle":"#e9c9bf","radius":"compact","shadow":"offset","typography":"technical"}},"startupView":"starred","defaultMessageView":"rendered","notificationKinds":{{"snooze":false}}}}"##,
                    other.id
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(updated.status(), StatusCode::OK);
    let updated = json(updated).await;
    assert_eq!(updated["preferences"]["theme"], "constructivist-red");
    assert_eq!(updated["preferences"]["customTheme"]["name"], "服务端构成");
    assert_eq!(updated["preferences"]["customTheme"]["accent"], "#c42a22");
    assert_eq!(updated["preferences"]["startupView"], "starred");
    assert_eq!(updated["preferences"]["defaultMessageView"], "rendered");
    assert_eq!(updated["preferences"]["notificationKinds"]["unread"], true);
    assert_eq!(updated["preferences"]["notificationKinds"]["snooze"], false);
    assert_eq!(
        updated["preferences"]["shortcutBindings"]["focusSearch"],
        "Mod+K"
    );

    let isolated = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/preferences")
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={other_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let isolated = json(isolated).await;
    assert_eq!(isolated["preferences"]["theme"], "mint-fresh");
    assert_eq!(isolated["preferences"]["startupView"], "inbox");

    let invalid = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PATCH)
                .uri("/api/preferences")
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::from(r#"{"startupView":"invalid"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

    let invalid_custom_theme = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PATCH)
                .uri("/api/preferences")
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::from(
                    r#"{"customTheme":{"name":"unsafe","canvas":"red"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalid_custom_theme.status(), StatusCode::BAD_REQUEST);

    let persisted = router
        .oneshot(
            Request::builder()
                .uri("/api/preferences")
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(json(persisted).await, updated);
    fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn exposes_provider_catalog_and_enforces_private_export_and_clear_contracts() {
    let directory = authentication_directory("security-http");
    let key_hex = "67".repeat(32);
    fs::write(directory.join("master.key"), &key_hex).unwrap();
    let key = MasterKey::from_hex(&key_hex).unwrap();
    let mut store = SqliteAuthStore::open_database(directory.join("imail.sqlite")).unwrap();
    let owner = store
        .create_user("privacy-owner", "Owner", "owner-password-123")
        .unwrap();
    let other = store
        .create_user("privacy-other", "Other", "other-password-123")
        .unwrap();
    let owner_session = store.create_session(&owner.id).unwrap();
    let other_session = store.create_session(&other.id).unwrap();
    for (id, user_id, email, password) in [
        (
            "owner-account",
            &owner.id,
            "owner@example.com",
            "owner-mail-secret",
        ),
        (
            "other-account",
            &other.id,
            "other@example.com",
            "other-mail-secret",
        ),
    ] {
        store
            .upsert_account(&AccountRecord {
                id: id.into(),
                owner_id: user_id.clone(),
                provider: "gmail".into(),
                email: email.into(),
                display_name: email.into(),
                group: "个人".into(),
                group_icon: "folder".into(),
                color: "#168f78".into(),
                settings: json!({
                    "imapHost": "imap.gmail.com", "imapPort": 993, "imapSecure": true,
                    "smtpHost": "smtp.gmail.com", "smtpPort": 465, "smtpSecure": true
                }),
                proxy: None,
                encrypted_secret: key.encrypt_json(&json!({ "password": password })).unwrap(),
                auth_method: Some("app-password".into()),
                created_at: "2026-08-10T00:00:00.000Z".into(),
                last_sync_at: None,
                status: "connected".into(),
                last_error: None,
                mailboxes: json!([]),
            })
            .unwrap();
    }
    drop(store);
    let router = build_router(HttpAdapterConfig::new(&directory)).unwrap();

    let unauthorized_providers = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/providers")
                .header(HOST, "127.0.0.1:8787")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized_providers.status(), StatusCode::UNAUTHORIZED);
    let providers = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/providers")
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(providers.status(), StatusCode::OK);
    let providers = json(providers).await;
    assert_eq!(
        providers["providers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["outlook", "gmail", "qq", "yahoo", "hotmail", "icloud", "custom"]
    );
    assert_eq!(providers["oauth"][0]["id"], "google");
    assert!(providers.to_string().find("clientSecret").is_none());

    let wrong_password = router
        .clone()
        .oneshot(Request::builder().method(Method::POST).uri("/api/security/mail-authorization-exports").header(HOST, "127.0.0.1:8787").header(CONTENT_TYPE, "application/json").header("cookie", format!("imail_session={owner_session}")).body(Body::from(r#"{"currentPassword":"wrong-password","exportPassword":"portable-password-123"}"#)).unwrap())
        .await
        .unwrap();
    assert_eq!(wrong_password.status(), StatusCode::FORBIDDEN);

    let prepared = router
        .clone()
        .oneshot(Request::builder().method(Method::POST).uri("/api/security/mail-authorization-exports").header(HOST, "127.0.0.1:8787").header(CONTENT_TYPE, "application/json").header("cookie", format!("imail_session={owner_session}")).body(Body::from(r#"{"currentPassword":"owner-password-123","exportPassword":"portable-password-123"}"#)).unwrap())
        .await
        .unwrap();
    assert_eq!(prepared.status(), StatusCode::OK);
    assert_eq!(
        prepared.headers()[CACHE_CONTROL],
        "private, no-store, max-age=0"
    );
    let prepared = json(prepared).await;
    assert_eq!(prepared["accountCount"], 1);
    let download_path = prepared["downloadPath"].as_str().unwrap().to_string();

    let isolated = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(&download_path)
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={other_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(isolated.status(), StatusCode::NOT_FOUND);
    let download = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(&download_path)
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(download.status(), StatusCode::OK);
    assert_eq!(
        download.headers()[CONTENT_TYPE],
        "application/vnd.imail.mail-authorization-export+json; charset=utf-8"
    );
    assert!(download.headers()[CONTENT_DISPOSITION]
        .to_str()
        .unwrap()
        .contains(".imailauth"));
    let envelope: imail_protocol::MailAuthorizationExportEnvelope =
        serde_json::from_slice(&to_bytes(download.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    let plaintext = decrypt_portable_export(
        &PortableEncryptedPayload {
            salt: envelope.kdf.salt,
            iv: envelope.cipher.iv,
            auth_tag: envelope.cipher.auth_tag,
            ciphertext: envelope.ciphertext,
        },
        "portable-password-123",
        b"imail-mail-authorizations:v1",
    )
    .unwrap();
    let payload: imail_protocol::MailAuthorizationExportPayload =
        serde_json::from_slice(&plaintext).unwrap();
    assert_eq!(payload.accounts.len(), 1);
    assert_eq!(payload.accounts[0].email, "owner@example.com");
    assert_eq!(
        payload.accounts[0].authorization.password.as_deref(),
        Some("owner-mail-secret")
    );
    assert!(!String::from_utf8_lossy(&plaintext).contains("other-mail-secret"));
    let consumed = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(&download_path)
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(consumed.status(), StatusCode::NOT_FOUND);

    let pending = router
        .clone()
        .oneshot(Request::builder().method(Method::POST).uri("/api/security/mail-authorization-exports").header(HOST, "127.0.0.1:8787").header(CONTENT_TYPE, "application/json").header("cookie", format!("imail_session={owner_session}")).body(Body::from(r#"{"currentPassword":"owner-password-123","exportPassword":"portable-password-456"}"#)).unwrap())
        .await
        .unwrap();
    let pending_path = json(pending).await["downloadPath"]
        .as_str()
        .unwrap()
        .to_string();
    let invalid_confirmation = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/security/clear-user-data")
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::from(
                    r#"{"currentPassword":"owner-password-123","confirmation":"clear"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalid_confirmation.status(), StatusCode::BAD_REQUEST);
    let cleared = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/security/clear-user-data")
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::from(
                    r#"{"currentPassword":"owner-password-123","confirmation":"清除我的邮箱数据"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cleared.status(), StatusCode::NO_CONTENT);
    let invalidated = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(&pending_path)
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalidated.status(), StatusCode::NOT_FOUND);

    for (session, expected_count) in [(&owner_session, 0), (&other_session, 1)] {
        let accounts = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/accounts")
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            json(accounts).await["accounts"].as_array().unwrap().len(),
            expected_count
        );
    }
    let session_still_valid = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/auth/session")
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(session_still_valid.status(), StatusCode::OK);
    let audit = router
        .oneshot(
            Request::builder()
                .uri("/api/security/audit-events?limit=20")
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let audit = json(audit).await;
    let event_types = audit["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["eventType"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(event_types.contains(&"sensitive-action.reauthentication-failed"));
    assert!(event_types.contains(&"privacy.mail-authorization-export.prepared"));
    assert!(event_types.contains(&"privacy.mail-authorization-export.downloaded"));
    assert!(event_types.contains(&"privacy.user-data-cleared"));
    assert!(!audit.to_string().contains("owner-password-123"));
    assert!(!audit.to_string().contains("owner-mail-secret"));

    let mut store = SqliteAuthStore::open_database(directory.join("imail.sqlite")).unwrap();
    let rate_key = format!("sensitive-action:clear-user-data:user:{}", other.id);
    for _ in 0..5 {
        assert!(
            store
                .consume_attempt(&rate_key, 5, 15 * 60_000)
                .unwrap()
                .allowed
        );
    }
    drop(store);
    let limited = build_router(HttpAdapterConfig::new(&directory))
        .unwrap()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/security/clear-user-data")
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={other_session}"))
                .body(Body::from(
                    r#"{"currentPassword":"other-password-123","confirmation":"清除我的邮箱数据"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(limited.headers().contains_key("retry-after"));
    assert_eq!(
        SqliteAuthStore::open_database(directory.join("imail.sqlite"))
            .unwrap()
            .list_accounts(&other.id)
            .unwrap()
            .len(),
        1
    );
    fs::remove_dir_all(directory).unwrap();
}
