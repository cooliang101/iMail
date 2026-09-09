use super::*;

#[tokio::test]
async fn registers_resolves_and_revokes_a_persistent_application_session() {
    let directory = authentication_directory("auth-session");
    let router = build_router(HttpAdapterConfig::new(&directory)).unwrap();
    let registration = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/auth/register")
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"login":" Test.Owner ","displayName":" Test Owner ","password":"test-password-123"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(registration.status(), StatusCode::CREATED);
    let set_cookie = registration.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .to_string();
    assert!(set_cookie.starts_with("imail_session="));
    assert!(set_cookie.contains("HttpOnly"));
    assert!(set_cookie.contains("SameSite=Lax"));
    assert!(!set_cookie.contains("Secure"));
    let cookie = set_cookie.split(';').next().unwrap().to_string();
    let registered = json(registration).await;
    assert_eq!(registered["user"]["login"], "test.owner");
    assert_eq!(registered["user"]["displayName"], "Test Owner");
    assert!(registered["user"].get("createdAt").is_none());

    let status = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/auth/status")
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = json(status).await;
    assert_eq!(status["setupRequired"], false);
    assert_eq!(status["registrationOpen"], true);
    assert_eq!(status["user"]["id"], registered["user"]["id"]);

    let session = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/auth/session")
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(session.status(), StatusCode::OK);

    let logout = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/auth/logout")
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(logout.status(), StatusCode::NO_CONTENT);
    assert!(logout.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .contains("Max-Age=0"));
    let expired = router
        .oneshot(
            Request::builder()
                .uri("/api/auth/session")
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(expired.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(json(expired).await["error"], "请先登录");
    fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn allows_only_one_concurrent_production_setup_and_uses_secure_cookies() {
    let directory = authentication_directory("auth-production");
    let router = build_router(HttpAdapterConfig::production(&directory)).unwrap();
    let register = |login: &'static str| {
        router.clone().oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/auth/register")
                .header(HOST, "localhost:443")
                .header(ORIGIN, "https://localhost")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(format!(
                    r#"{{"login":"{login}","displayName":"Owner","password":"test-password-123"}}"#
                )))
                .unwrap(),
        )
    };
    let (first, second) = tokio::join!(register("production-owner"), register("production-other"));
    let responses = [first.unwrap(), second.unwrap()];
    assert_eq!(
        responses
            .iter()
            .filter(|response| response.status() == StatusCode::CREATED)
            .count(),
        1
    );
    assert_eq!(
        responses
            .iter()
            .filter(|response| response.status() == StatusCode::FORBIDDEN)
            .count(),
        1
    );
    let created = responses
        .iter()
        .find(|response| response.status() == StatusCode::CREATED)
        .unwrap();
    assert!(created.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .contains("Secure"));
    assert!(created.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .contains("SameSite=Lax"));
    let denied = responses
        .into_iter()
        .find(|response| response.status() == StatusCode::FORBIDDEN)
        .unwrap();
    assert_eq!(json(denied).await["error"], "此服务已关闭新用户注册");
    fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn enforces_persistent_login_limits_before_password_verification() {
    let directory = authentication_directory("auth-limit");
    let mut store = SqliteAuthStore::open_database(directory.join("imail.sqlite")).unwrap();
    store
        .create_user("limited-owner", "Limited", "test-password-123")
        .unwrap();
    for _ in 0..10 {
        assert!(
            store
                .consume_attempt("login-account:limited-owner", 10, 15 * 60_000)
                .unwrap()
                .allowed
        );
    }
    drop(store);
    let response = build_router(HttpAdapterConfig::new(&directory))
        .unwrap()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/auth/login")
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"login":"limited-owner","password":"wrong-password"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(response.headers().get("retry-after").is_some());
    assert_eq!(json(response).await["error"], "尝试过多，请稍后再试");
    fs::remove_dir_all(directory).unwrap();
}
