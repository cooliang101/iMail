use super::*;

#[tokio::test]
async fn embedded_host_runs_account_applications_without_the_router() {
    let directory = authentication_directory("embedded-account-applications");
    fs::write(directory.join("master.key"), "52".repeat(32)).unwrap();
    let user = SqliteAuthStore::open_database(directory.join("imail.sqlite"))
        .unwrap()
        .create_user("owner", "Owner", "correct horse battery staple")
        .unwrap();
    let config = HttpAdapterConfig::new(&directory)
        .with_connection_probe(Arc::new(|_: &MailConnectionConfig| Ok(())));
    let host = EmbeddedServiceHost::start(config).unwrap();
    let created = host
        .create_account(
            user.id.clone(),
            "embedded-test".into(),
            json!({
                "provider":"gmail",
                "email":"owner@gmail.example",
                "displayName":"Mailbox",
                "password":"initial-secret"
            }),
        )
        .await
        .unwrap();
    assert_eq!(created.status, "connected");
    let updated = host
        .update_account_credential(
            user.id.clone(),
            "embedded-test".into(),
            created.id.clone(),
            "replacement-secret".into(),
        )
        .await
        .unwrap();
    assert_eq!(updated.status, "connected");
    let proxied = host
        .update_account_proxy(
            user.id.clone(),
            "embedded-test".into(),
            created.id.clone(),
            imail_protocol::AccountProxyUpdate::Explicit {
                protocol: imail_protocol::ProxyProtocol::Socks5,
                host: "proxy.example.test".into(),
                port: 1080,
                username: Some("owner".into()),
                password: Some("proxy-secret".into()),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        proxied.proxy.as_ref().unwrap()["host"],
        "proxy.example.test"
    );
    let tested = host
        .test_account_connection(user.id.clone(), created.id.clone())
        .await
        .unwrap();
    assert_eq!(tested.status, "connected");
    assert!(tested.last_error.is_none());

    let store = SqliteAuthStore::open_database(directory.join("imail.sqlite")).unwrap();
    let stored = store.account(&user.id, &created.id).unwrap().unwrap();
    assert!(!stored.encrypted_secret.contains("replacement-secret"));
    assert!(!stored.encrypted_secret.contains("proxy-secret"));
    for event_type in [
        "account.created",
        "account.credential-updated",
        "account.proxy-updated",
    ] {
        assert_eq!(
            store
                .security_audit_details(&user.id, event_type, 10)
                .unwrap()
                .len(),
            1,
            "{event_type}"
        );
    }
    drop(store);
    host.shutdown(Duration::from_secs(2)).unwrap();
    drop(host);
    fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn lists_and_updates_only_owned_accounts_without_exposing_internal_fields() {
    let directory = authentication_directory("accounts");
    let mut store = SqliteAuthStore::open_database(directory.join("imail.sqlite")).unwrap();
    let owner = store
        .create_user("account-owner", "Owner", "owner-password-123")
        .unwrap();
    let other = store
        .create_user("account-other", "Other", "other-password-123")
        .unwrap();
    let owner_session = store.create_session(&owner.id).unwrap();
    let other_session = store.create_session(&other.id).unwrap();
    let account_id = Uuid::new_v4().to_string();
    store
        .upsert_account(&AccountRecord {
            id: account_id.clone(),
            owner_id: owner.id.clone(),
            provider: "gmail".into(),
            email: "owner@example.com".into(),
            display_name: "Owner Mail".into(),
            group: "个人".into(),
            group_icon: "folder".into(),
            color: "#168f78".into(),
            settings: json!({
                "imapHost": "imap.gmail.com", "imapPort": 993, "imapSecure": true,
                "smtpHost": "smtp.gmail.com", "smtpPort": 465, "smtpSecure": true,
                "password": "settings-plaintext-must-never-leak"
            }),
            proxy: Some(json!({
                "protocol": "socks5", "host": "127.0.0.1", "port": 1080,
                "username": "proxy-user", "password": "proxy-plaintext-must-never-leak",
                "proxyPassword": "proxy-secret-must-never-leak"
            })),
            encrypted_secret: "encrypted-must-never-leak".into(),
            auth_method: Some("oauth2".into()),
            created_at: "2026-08-10T00:00:00.000Z".into(),
            last_sync_at: None,
            status: "connected".into(),
            last_error: None,
            mailboxes: json!([]),
        })
        .unwrap();
    drop(store);
    let router = build_router(HttpAdapterConfig::new(&directory)).unwrap();

    let owner_list = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/accounts")
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(owner_list.status(), StatusCode::OK);
    let owner_list = json(owner_list).await;
    assert_eq!(owner_list["accounts"].as_array().unwrap().len(), 1);
    let account = &owner_list["accounts"][0];
    assert_eq!(account["id"], account_id);
    assert_eq!(account["authMethod"], "oauth2");
    assert_eq!(account["groupIcon"], "folder");
    assert!(account.get("ownerId").is_none());
    assert!(account.get("encryptedSecret").is_none());
    assert!(account.get("lastError").is_none());
    assert!(!owner_list.to_string().contains("encrypted-must-never-leak"));
    assert!(!owner_list.to_string().contains("proxyPassword"));
    assert!(!owner_list
        .to_string()
        .contains("settings-plaintext-must-never-leak"));
    assert!(!owner_list
        .to_string()
        .contains("proxy-plaintext-must-never-leak"));
    assert!(!owner_list
        .to_string()
        .contains("proxy-secret-must-never-leak"));

    let isolated = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/accounts")
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={other_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(json(isolated).await["accounts"], json!([]));

    let forbidden = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PATCH)
                .uri(format!("/api/accounts/{account_id}"))
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={other_session}"))
                .body(Body::from(r#"{"displayName":"Stolen"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(forbidden.status(), StatusCode::NOT_FOUND);
    assert_eq!(json(forbidden).await["error"], "邮箱账户不存在");

    let updated = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PATCH)
                .uri(format!("/api/accounts/{account_id}"))
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::from(format!(
                    r##"{{"ownerId":"{}","displayName":" Updated Mail ","group":" Work ","groupIcon":"briefcase","color":"#123aBc"}}"##,
                    other.id
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(updated.status(), StatusCode::OK);
    let updated = json(updated).await;
    assert_eq!(updated["account"]["displayName"], "Updated Mail");
    assert_eq!(updated["account"]["group"], "Work");
    assert_eq!(updated["account"]["groupIcon"], "briefcase");
    assert_eq!(updated["account"]["color"], "#123aBc");
    assert!(updated["account"].get("ownerId").is_none());

    let invalid = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PATCH)
                .uri(format!("/api/accounts/{account_id}"))
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json(invalid).await["error"], "至少提供一个要更新的字段");

    let forbidden_sync = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/accounts/{account_id}/sync"))
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={other_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(forbidden_sync.status(), StatusCode::NOT_FOUND);

    let queued = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/accounts/{account_id}/sync"))
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(queued.status(), StatusCode::OK);
    let queued = json(queued).await;
    assert_eq!(queued["synced"], 0);
    assert_eq!(queued["queued"], true);
    let inbox_job_id = queued["jobId"].as_str().unwrap().to_string();

    let duplicate = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/accounts/{account_id}/mailboxes/sync"))
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::from(r#"{"mailbox":"INBOX"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(json(duplicate).await["jobId"], inbox_job_id);

    let all = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/sync")
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let all = json(all).await;
    assert_eq!(all["results"].as_array().unwrap().len(), 1);
    assert_eq!(all["results"][0]["accountId"], account_id);
    assert_eq!(all["results"][0]["status"], "fulfilled");
    assert_eq!(all["results"][0]["jobId"], inbox_job_id);

    let forbidden_delete = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri(format!("/api/accounts/{account_id}"))
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={other_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(forbidden_delete.status(), StatusCode::NOT_FOUND);

    let removed = router
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri(format!("/api/accounts/{account_id}"))
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(removed.status(), StatusCode::NO_CONTENT);
    assert!(
        SyncRuntimeStore::open_database(directory.join("imail.sqlite"))
            .unwrap()
            .job(&inbox_job_id)
            .unwrap()
            .is_none()
    );
    assert!(
        SqliteAuthStore::open_database(directory.join("imail.sqlite"))
            .unwrap()
            .list_accounts(&owner.id)
            .unwrap()
            .is_empty()
    );
    fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn validates_sensitive_account_changes_before_persisting_and_redacts_failures() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let directory = authentication_directory("account-sensitive");
    let database = directory.join("imail.sqlite");
    let key_hex = "42".repeat(32);
    fs::write(directory.join("master.key"), &key_hex).unwrap();
    let key = MasterKey::from_hex(&key_hex).unwrap();
    let mut store = SqliteAuthStore::open_database(&database).unwrap();
    let owner = store
        .create_user("sensitive-owner", "Owner", "owner-password-123")
        .unwrap();
    let other = store
        .create_user("sensitive-other", "Other", "other-password-123")
        .unwrap();
    let owner_session = store.create_session(&owner.id).unwrap();
    let other_session = store.create_session(&other.id).unwrap();
    let target_id = Uuid::new_v4().to_string();
    let source_id = Uuid::new_v4().to_string();
    let oauth_id = Uuid::new_v4().to_string();
    let settings = json!({
        "imapHost":"imap.example.com","imapPort":993,"imapSecure":true,
        "smtpHost":"smtp.example.com","smtpPort":465,"smtpSecure":true
    });
    let account = |id: String,
                   email: &str,
                   encrypted_secret: String,
                   auth_method: &str,
                   proxy: Option<Value>| AccountRecord {
        id,
        owner_id: owner.id.clone(),
        provider: "custom".into(),
        email: email.into(),
        display_name: email.into(),
        group: "个人".into(),
        group_icon: "folder".into(),
        color: "#168f78".into(),
        settings: settings.clone(),
        proxy,
        encrypted_secret,
        auth_method: Some(auth_method.into()),
        created_at: "2026-08-10T00:00:00.000Z".into(),
        last_sync_at: None,
        status: "connected".into(),
        last_error: None,
        mailboxes: json!([]),
    };
    store
        .upsert_account(&account(
            target_id.clone(),
            "target@example.com",
            key.encrypt_json(&json!({
                "authType":"app-password","password":"old-password",
                "proxyPassword":"old-proxy-secret"
            }))
            .unwrap(),
            "app-password",
            None,
        ))
        .unwrap();
    store
        .upsert_account(&account(
            source_id.clone(),
            "source@example.com",
            key.encrypt_json(&json!({
                "authType":"app-password","password":"source-password",
                "proxyPassword":"source-proxy-secret"
            }))
            .unwrap(),
            "app-password",
            Some(json!({
                "protocol":"socks5","host":"proxy.example.com","port":1080,
                "username":"proxy-user"
            })),
        ))
        .unwrap();
    store
        .upsert_account(&account(
            oauth_id.clone(),
            "oauth@example.com",
            key.encrypt_json(&json!({
                "authType":"oauth2","accessToken":"oauth-access-secret"
            }))
            .unwrap(),
            "oauth2",
            None,
        ))
        .unwrap();
    drop(store);

    let force_failure = Arc::new(AtomicBool::new(false));
    let probe_failure = Arc::clone(&force_failure);
    let probe = move |config: &MailConnectionConfig| {
        if probe_failure.load(Ordering::SeqCst) {
            return Err(
                "IMAP authentication failed password=connection-secret authorization=token-secret"
                    .into(),
            );
        }
        if matches!(&config.authentication, MailAuthentication::Password(password) if password == "reject-password")
        {
            return Err("IMAP authentication failed password=reject-password".into());
        }
        if config
            .proxy
            .as_ref()
            .is_some_and(|proxy| proxy.host == "reject.example.com")
        {
            return Err("proxy password=proxy-leak".into());
        }
        Ok(())
    };
    let router =
        build_router(HttpAdapterConfig::new(&directory).with_connection_probe(Arc::new(probe)))
            .unwrap();

    let invalid_custom = router.clone().oneshot(Request::builder().method(Method::POST).uri("/api/accounts").header(HOST, "127.0.0.1:8787").header(CONTENT_TYPE, "application/json").header("cookie", format!("imail_session={owner_session}")).body(Body::from(r#"{"provider":"custom","email":"missing-settings@example.com","displayName":"Missing","password":"password"}"#)).unwrap()).await.unwrap();
    assert_eq!(invalid_custom.status(), StatusCode::BAD_REQUEST);

    let rejected_create = router.clone().oneshot(Request::builder().method(Method::POST).uri("/api/accounts").header(HOST, "127.0.0.1:8787").header(CONTENT_TYPE, "application/json").header("cookie", format!("imail_session={owner_session}")).body(Body::from(r#"{"provider":"gmail","email":"rejected@example.com","displayName":"Rejected","password":"reject-password"}"#)).unwrap()).await.unwrap();
    assert_eq!(rejected_create.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let rejected_create = json(rejected_create).await;
    assert!(rejected_create["error"]
        .as_str()
        .unwrap()
        .contains("IMAP 认证失败"));
    assert!(!rejected_create.to_string().contains("reject-password"));
    assert!(SqliteAuthStore::open_database(&database)
        .unwrap()
        .list_accounts(&owner.id)
        .unwrap()
        .iter()
        .all(|account| account.email != "rejected@example.com"));

    let created = router.clone().oneshot(Request::builder().method(Method::POST).uri("/api/accounts").header(HOST, "127.0.0.1:8787").header(CONTENT_TYPE, "application/json").header("cookie", format!("imail_session={owner_session}")).body(Body::from(r#"{"provider":"custom","email":"Created@Example.com","displayName":" Created Account ","password":"created-password","settings":{"imapHost":"imap.created.example","imapPort":993,"imapSecure":true,"smtpHost":"smtp.created.example","smtpPort":465,"smtpSecure":true},"proxy":{"protocol":"socks5","host":"proxy.created.example","port":1080,"password":"created-proxy-secret"}}"#)).unwrap()).await.unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    let created = json(created).await;
    let created_id = created["account"]["id"].as_str().unwrap().to_string();
    assert_eq!(created["account"]["email"], "created@example.com");
    assert_eq!(created["account"]["displayName"], "Created Account");
    assert!(!created.to_string().contains("created-password"));
    assert!(!created.to_string().contains("created-proxy-secret"));
    let created_record = SqliteAuthStore::open_database(&database)
        .unwrap()
        .account(&owner.id, &created_id)
        .unwrap()
        .unwrap();
    let created_secret: Value = key.decrypt_json(&created_record.encrypted_secret).unwrap();
    assert_eq!(created_secret["password"], "created-password");
    assert_eq!(created_secret["proxyPassword"], "created-proxy-secret");
    assert!(SyncRuntimeStore::open_database(&database)
        .unwrap()
        .policy(&created_id)
        .unwrap()
        .is_some());

    let token_account = router.clone().oneshot(Request::builder().method(Method::POST).uri("/api/accounts").header(HOST, "127.0.0.1:8787").header(CONTENT_TYPE, "application/json").header("cookie", format!("imail_session={owner_session}")).body(Body::from(r#"{"provider":"gmail","email":"direct-token@example.com","displayName":"Direct Token","accessToken":"direct-access-secret"}"#)).unwrap()).await.unwrap();
    assert_eq!(token_account.status(), StatusCode::CREATED);
    let token_account = json(token_account).await;
    assert_eq!(token_account["account"]["authMethod"], "oauth2");
    let token_account_id = token_account["account"]["id"].as_str().unwrap();
    let token_test = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/accounts/{token_account_id}/connection-test"))
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(json(token_test).await["account"]["status"], "connected");

    let concurrent_body = r#"{"provider":"gmail","email":"race-create@example.com","displayName":"Race","password":"race-password"}"#;
    let create_once = || {
        router.clone().oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/accounts")
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::from(concurrent_body))
                .unwrap(),
        )
    };
    let (first, second) = tokio::join!(create_once(), create_once());
    let statuses = [first.unwrap().status(), second.unwrap().status()];
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == StatusCode::CREATED)
            .count(),
        1
    );
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == StatusCode::CONFLICT)
            .count(),
        1
    );

    let before = SqliteAuthStore::open_database(&database)
        .unwrap()
        .account(&owner.id, &target_id)
        .unwrap()
        .unwrap();
    let forbidden = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri(format!("/api/accounts/{target_id}/credential"))
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={other_session}"))
                .body(Body::from(r#"{"password":"stolen"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(forbidden.status(), StatusCode::NOT_FOUND);

    let rejected = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri(format!("/api/accounts/{target_id}/credential"))
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::from(r#"{"password":"reject-password"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let after_rejected = SqliteAuthStore::open_database(&database)
        .unwrap()
        .account(&owner.id, &target_id)
        .unwrap()
        .unwrap();
    assert!(after_rejected == before);

    let oauth_rejected = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri(format!("/api/accounts/{oauth_id}/credential"))
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::from(r#"{"password":"must-not-replace"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(oauth_rejected.status(), StatusCode::CONFLICT);

    let credential = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri(format!("/api/accounts/{target_id}/credential"))
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::from(r#"{"password":"new-password"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(credential.status(), StatusCode::OK);
    let credential = json(credential).await;
    assert_eq!(credential["account"]["status"], "connected");
    assert!(!credential.to_string().contains("new-password"));
    assert!(!credential.to_string().contains("old-proxy-secret"));
    let stored = SqliteAuthStore::open_database(&database)
        .unwrap()
        .account(&owner.id, &target_id)
        .unwrap()
        .unwrap();
    let secret: Value = key.decrypt_json(&stored.encrypted_secret).unwrap();
    assert_eq!(secret["password"], "new-password");
    assert_eq!(secret["proxyPassword"], "old-proxy-secret");

    let before_proxy = stored.clone();
    let rejected_proxy = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri(format!("/api/accounts/{target_id}/proxy"))
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::from(r#"{"enabled":true,"protocol":"http","host":"reject.example.com","port":8080,"password":"proxy-new-secret"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rejected_proxy.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        SqliteAuthStore::open_database(&database)
            .unwrap()
            .account(&owner.id, &target_id)
            .unwrap()
            .unwrap()
            == before_proxy
    );

    let copied_proxy = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri(format!("/api/accounts/{target_id}/proxy"))
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::from(format!(
                    r#"{{"enabled":true,"sourceAccountId":"{source_id}"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(copied_proxy.status(), StatusCode::OK);
    let copied_proxy = json(copied_proxy).await;
    assert_eq!(
        copied_proxy["account"]["proxy"]["host"],
        "proxy.example.com"
    );
    assert!(!copied_proxy.to_string().contains("source-proxy-secret"));
    let stored = SqliteAuthStore::open_database(&database)
        .unwrap()
        .account(&owner.id, &target_id)
        .unwrap()
        .unwrap();
    let secret: Value = key.decrypt_json(&stored.encrypted_secret).unwrap();
    assert_eq!(secret["proxyPassword"], "source-proxy-secret");

    force_failure.store(true, Ordering::SeqCst);
    let failed_test = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/accounts/{target_id}/connection-test"))
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(failed_test.status(), StatusCode::OK);
    let failed_test = json(failed_test).await;
    assert_eq!(failed_test["account"]["status"], "error");
    assert!(failed_test["account"]["lastError"]
        .as_str()
        .unwrap()
        .contains("[redacted]"));
    assert!(!failed_test.to_string().contains("connection-secret"));
    assert!(!failed_test.to_string().contains("token-secret"));

    force_failure.store(false, Ordering::SeqCst);
    let recovered = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/accounts/{target_id}/connection-test"))
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let recovered = json(recovered).await;
    assert_eq!(recovered["account"]["status"], "connected");
    assert!(recovered["account"].get("lastError").is_none());

    let foreign_test = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/accounts/{target_id}/connection-test"))
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={other_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(foreign_test.status(), StatusCode::NOT_FOUND);
    fs::remove_dir_all(directory).unwrap();
}
