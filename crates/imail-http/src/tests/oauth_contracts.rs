use super::*;

#[tokio::test]
async fn embedded_oauth_uses_transient_loopback_callback_without_business_listener() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct EmbeddedOAuthProvider {
        exchanges: Arc<AtomicUsize>,
    }

    impl OAuthProviderPort for EmbeddedOAuthProvider {
        fn token_request(
            &mut self,
            _config: &OAuthConfig,
            grant: OAuthGrant<'_>,
        ) -> Result<OAuthTokenResponse, OAuthError> {
            assert!(matches!(grant, OAuthGrant::AuthorizationCode { .. }));
            self.exchanges.fetch_add(1, Ordering::SeqCst);
            Ok(OAuthTokenResponse {
                access_token: "embedded-access-token-must-never-leak".into(),
                refresh_token: Some("embedded-refresh-token-must-never-leak".into()),
                expires_in: Some(3600),
                token_type: Some("Bearer".into()),
                scope: Some("openid email".into()),
                id_token: None,
            })
        }

        fn fetch_identity(
            &mut self,
            _config: &OAuthConfig,
            _token: &OAuthTokenResponse,
            _nonce: &str,
        ) -> Result<OAuthIdentity, OAuthError> {
            Ok(OAuthIdentity {
                email: "embedded-oauth@example.com".into(),
                name: Some("Embedded OAuth".into()),
            })
        }
    }

    let directory = authentication_directory("embedded-oauth-loopback");
    fs::write(directory.join("master.key"), "74".repeat(32)).unwrap();
    let database = directory.join("imail.sqlite");
    let mut store = SqliteAuthStore::open_database(&database).unwrap();
    let owner = store
        .create_user("embedded-oauth-owner", "Owner", "owner-password-123")
        .unwrap();
    drop(store);

    let exchanges = Arc::new(AtomicUsize::new(0));
    let factory_exchanges = Arc::clone(&exchanges);
    let factory: Arc<dyn OAuthProviderPortFactory> = Arc::new(move || {
        Box::new(EmbeddedOAuthProvider {
            exchanges: Arc::clone(&factory_exchanges),
        }) as Box<dyn OAuthProviderPort>
    });
    let host = EmbeddedServiceHost::start(
        HttpAdapterConfig::new(&directory)
            .with_oauth_environment(OAuthEnvironment {
                callback_base_url: "http://127.0.0.1:0/api/oauth".into(),
                google_client_id: Some("embedded-client-id".into()),
                google_client_secret: Some("embedded-client-secret".into()),
                ..OAuthEnvironment::default()
            })
            .with_oauth_provider_factory(factory)
            .with_connection_probe(Arc::new(|_: &MailConnectionConfig| Ok(()))),
    )
    .unwrap();

    let started = host
        .start_oauth(
            owner.id.clone(),
            "embedded-test".into(),
            serde_json::json!({
                "provider":"gmail",
                "displayName":"Embedded OAuth",
                "group":"个人",
                "color":"#168f78"
            }),
        )
        .await
        .unwrap();
    let state = started["state"].as_str().unwrap().to_string();
    let authorization_url = url::Url::parse(started["authorizationUrl"].as_str().unwrap())
        .expect("authorization URL is valid");
    let redirect_uri = authorization_url
        .query_pairs()
        .find(|(key, _)| key == "redirect_uri")
        .map(|(_, value)| value.into_owned())
        .expect("authorization URL contains redirect URI");
    assert!(redirect_uri.starts_with("http://127.0.0.1:"));
    assert!(!redirect_uri.contains(":0/"));
    assert!(authorization_url.as_str().contains("code_challenge="));

    let callback_url = format!(
        "{redirect_uri}?{}",
        url::form_urlencoded::Serializer::new(String::new())
            .append_pair("state", &state)
            .append_pair("code", "embedded-one-time-code")
            .finish()
    );
    let callback_status = tokio::task::spawn_blocking(move || {
        ureq::get(&callback_url)
            .call()
            .map(|response| response.status())
            .map_err(|error| error.to_string())
    })
    .await
    .unwrap()
    .unwrap();
    assert!((200..300).contains(&callback_status));

    let mut completed = None;
    for _ in 0..50 {
        let status = host
            .oauth_status(owner.id.clone(), serde_json::json!({"state":state}))
            .await
            .unwrap();
        if status["completed"] == true {
            completed = Some(status);
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let completed = completed.expect("loopback callback completes");
    assert_eq!(completed["account"]["email"], "embedded-oauth@example.com");
    assert_eq!(exchanges.load(Ordering::SeqCst), 1);
    let response_text = completed.to_string();
    assert!(!response_text.contains("embedded-access-token-must-never-leak"));
    assert!(!response_text.contains("embedded-refresh-token-must-never-leak"));

    let store = SqliteAuthStore::open_database(&database).unwrap();
    let account = store
        .list_accounts(&owner.id)
        .unwrap()
        .into_iter()
        .find(|account| account.email == "embedded-oauth@example.com")
        .unwrap();
    assert!(!account
        .encrypted_secret
        .contains("embedded-access-token-must-never-leak"));
    assert!(!account
        .encrypted_secret
        .contains("embedded-refresh-token-must-never-leak"));
    drop(store);
    drop(host);
    fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn completes_oauth_once_and_isolates_status_and_reconnection_by_owner() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FakeOAuthProvider {
        exchanges: Arc<AtomicUsize>,
    }

    impl OAuthProviderPort for FakeOAuthProvider {
        fn token_request(
            &mut self,
            _config: &OAuthConfig,
            grant: OAuthGrant<'_>,
        ) -> Result<OAuthTokenResponse, OAuthError> {
            assert!(matches!(grant, OAuthGrant::AuthorizationCode { .. }));
            self.exchanges.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(100));
            Ok(OAuthTokenResponse {
                access_token: "access-token-must-never-leak".into(),
                refresh_token: Some("refresh-token-must-never-leak".into()),
                expires_in: Some(3600),
                token_type: Some("Bearer".into()),
                scope: Some("openid email".into()),
                id_token: None,
            })
        }

        fn fetch_identity(
            &mut self,
            _config: &OAuthConfig,
            _token: &OAuthTokenResponse,
            _nonce: &str,
        ) -> Result<OAuthIdentity, OAuthError> {
            Ok(OAuthIdentity {
                email: "oauth-owner@example.com".into(),
                name: Some("OAuth Owner".into()),
            })
        }
    }

    let directory = authentication_directory("oauth-loop");
    let database = directory.join("imail.sqlite");
    let key_hex = "73".repeat(32);
    fs::write(directory.join("master.key"), &key_hex).unwrap();
    let mut store = SqliteAuthStore::open_database(&database).unwrap();
    let owner = store
        .create_user("oauth-owner", "Owner", "owner-password-123")
        .unwrap();
    let other = store
        .create_user("oauth-other", "Other", "other-password-123")
        .unwrap();
    let owner_session = store.create_session(&owner.id).unwrap();
    let other_session = store.create_session(&other.id).unwrap();
    drop(store);

    let exchanges = Arc::new(AtomicUsize::new(0));
    let factory_exchanges = Arc::clone(&exchanges);
    let factory: Arc<dyn OAuthProviderPortFactory> = Arc::new(move || {
        Box::new(FakeOAuthProvider {
            exchanges: Arc::clone(&factory_exchanges),
        }) as Box<dyn OAuthProviderPort>
    });
    let environment = OAuthEnvironment {
        callback_base_url: "http://127.0.0.1:8787/api/oauth".into(),
        google_client_id: Some("test-client-id".into()),
        google_client_secret: Some("test-client-secret".into()),
        ..OAuthEnvironment::default()
    };
    let router = build_router(
        HttpAdapterConfig::new(&directory)
            .with_oauth_environment(environment)
            .with_oauth_provider_factory(factory)
            .with_connection_probe(Arc::new(|_: &MailConnectionConfig| Ok(())))
            .with_oauth_frontend_origin("http://localhost:5173"),
    )
    .unwrap();

    let start = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/oauth/start")
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::from(r##"{"provider":"gmail","displayName":"OAuth Owner","group":"个人","color":"#168f78"}"##))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start.status(), StatusCode::OK);
    let start = json(start).await;
    let oauth_state = start["state"].as_str().unwrap().to_string();
    assert_eq!(start["provider"], "google");
    assert!(start["authorizationUrl"]
        .as_str()
        .unwrap()
        .contains("code_challenge="));

    let callback_uri = format!(
        "/api/oauth/google/callback?{}",
        url::form_urlencoded::Serializer::new(String::new())
            .append_pair("state", &oauth_state)
            .append_pair("code", "one-time-code")
            .finish()
    );
    let callback_request = || {
        router.clone().oneshot(
            Request::builder()
                .uri(&callback_uri)
                .header(HOST, "127.0.0.1:8787")
                .body(Body::empty())
                .unwrap(),
        )
    };
    let (callback, concurrent_duplicate) = tokio::join!(callback_request(), callback_request());
    let callback = callback.unwrap();
    let concurrent_duplicate = concurrent_duplicate.unwrap();
    assert_eq!(callback.status(), StatusCode::OK);
    assert_eq!(concurrent_duplicate.status(), StatusCode::ACCEPTED);
    let callback_html = text_body(callback).await;
    assert!(callback_html.contains("邮箱授权成功"));
    assert!(callback_html.contains("http://localhost:5173"));
    assert!(!callback_html.contains("access-token-must-never-leak"));
    assert!(!callback_html.contains("refresh-token-must-never-leak"));
    assert_eq!(exchanges.load(Ordering::SeqCst), 1);

    let duplicate = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(&callback_uri)
                .header(HOST, "127.0.0.1:8787")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(duplicate.status(), StatusCode::OK);
    assert_eq!(exchanges.load(Ordering::SeqCst), 1);

    let status_request = |session: &str| {
        Request::builder()
            .method(Method::POST)
            .uri("/api/oauth/status")
            .header(HOST, "127.0.0.1:8787")
            .header(CONTENT_TYPE, "application/json")
            .header("cookie", format!("imail_session={session}"))
            .body(Body::from(json!({ "state": oauth_state }).to_string()))
            .unwrap()
    };
    let owner_status = json(
        router
            .clone()
            .oneshot(status_request(&owner_session))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(owner_status["completed"], true);
    assert_eq!(owner_status["account"]["email"], "oauth-owner@example.com");
    assert!(!owner_status.to_string().contains("token-must-never-leak"));
    let account_id = owner_status["account"]["id"].as_str().unwrap().to_string();
    let other_status = json(
        router
            .clone()
            .oneshot(status_request(&other_session))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(other_status, json!({ "completed": false }));

    let reconnect = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/accounts/{account_id}/oauth/reconnect"))
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={owner_session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(reconnect.status(), StatusCode::OK);
    let reconnect_state = json(reconnect).await["state"].as_str().unwrap().to_string();
    let reconnect_uri = format!(
        "/api/oauth/google/callback?{}",
        url::form_urlencoded::Serializer::new(String::new())
            .append_pair("state", &reconnect_state)
            .append_pair("code", "reconnect-code")
            .finish()
    );
    assert_eq!(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(reconnect_uri)
                    .header(HOST, "127.0.0.1:8787")
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(exchanges.load(Ordering::SeqCst), 2);
    let accounts = SqliteAuthStore::open_database(&database)
        .unwrap()
        .list_accounts(&owner.id)
        .unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].id, account_id);
    assert!(SyncRuntimeStore::open_database(&database)
        .unwrap()
        .policy(&account_id)
        .unwrap()
        .is_some());
    fs::remove_dir_all(directory).unwrap();
}
