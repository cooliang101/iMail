use super::*;

#[tokio::test]
async fn rejects_untrusted_production_hosts_and_applies_security_headers() {
    let directory = temporary_directory("host");
    let mut config = HttpAdapterConfig::production(&directory);
    config.allowed_hosts = ["mail.example.com".into()].into_iter().collect();
    let response = build_router(config)
        .unwrap()
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .header(HOST, "attacker.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::MISDIRECTED_REQUEST);
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert_eq!(
        json(response).await["error"],
        "请求主机名不在服务允许列表中"
    );
    fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn trusts_exactly_one_forwarding_hop_only_when_explicitly_enabled() {
    let ignored_directory = temporary_directory("proxy-ignored");
    let ignored = build_router(HttpAdapterConfig::new(&ignored_directory))
        .unwrap()
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .header("x-forwarded-for", "not-an-ip")
                .header("x-forwarded-proto", "gopher")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ignored.status(), StatusCode::OK);

    let trusted_directory = authentication_directory("proxy-trusted");
    let config = HttpAdapterConfig::new(&trusted_directory).with_trusted_proxy_one_hop(true);
    let router = build_router(config).unwrap();
    let malformed = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .header("x-forwarded-for", "203.0.113.4, not-an-ip")
                .header("x-forwarded-proto", "https")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);

    let registration = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/auth/register")
                .header(HOST, "mail.example.test")
                .header(CONTENT_TYPE, "application/json")
                .header("x-forwarded-for", "198.51.100.10, 203.0.113.4")
                .header("x-forwarded-proto", "https, http")
                .body(Body::from(
                    r#"{"login":"proxy.owner","displayName":"Proxy Owner","password":"test-password-123"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(registration.status(), StatusCode::CREATED);
    assert!(registration.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .contains("Secure"));
    assert_eq!(
        registration.headers()["strict-transport-security"],
        "max-age=31536000"
    );
    fs::remove_dir_all(ignored_directory).unwrap();
    fs::remove_dir_all(trusted_directory).unwrap();
}

#[tokio::test]
async fn emits_credentials_cors_only_for_an_exact_valid_origin() {
    let directory = temporary_directory("cors");
    let mut config = HttpAdapterConfig::new(&directory);
    config.cors_origins = ["https://desktop.example.com".into()].into_iter().collect();
    let router = build_router(config).unwrap();
    let allowed = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .header(ORIGIN, "https://desktop.example.com")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        allowed.headers()[ACCESS_CONTROL_ALLOW_ORIGIN],
        "https://desktop.example.com"
    );
    assert_eq!(allowed.headers()[ACCESS_CONTROL_ALLOW_CREDENTIALS], "true");

    let denied = router
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .header(ORIGIN, "https://attacker.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(denied.headers().get(ACCESS_CONTROL_ALLOW_ORIGIN).is_none());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn rejects_unsafe_origin_and_host_configuration() {
    let directory = temporary_directory("config");
    let mut config = HttpAdapterConfig::new(&directory);
    config.cors_origins = ["http://mail.example.com".into()].into_iter().collect();
    assert!(matches!(
        build_router(config),
        Err(HttpAdapterError::Config(ConfigError::InvalidCorsOrigin(_)))
    ));
    fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn preflight_cannot_bypass_host_or_forwarding_checks() {
    let directory = temporary_directory("preflight-boundary");
    let mut config = HttpAdapterConfig::production(&directory).with_trusted_proxy_one_hop(true);
    config.allowed_hosts = ["mail.example".into()].into_iter().collect();
    config.cors_origins = ["https://client.example".into()].into_iter().collect();
    let router = build_router(config).unwrap();
    for (host, proto, origin, expected, cors) in [
        (
            "mail.example",
            "https",
            "https://client.example",
            StatusCode::NO_CONTENT,
            true,
        ),
        (
            "attacker.example",
            "https",
            "https://client.example",
            StatusCode::MISDIRECTED_REQUEST,
            false,
        ),
        (
            "mail.example",
            "gopher",
            "https://client.example",
            StatusCode::BAD_REQUEST,
            false,
        ),
        (
            "mail.example",
            "https",
            "https://client.example.attacker.test",
            StatusCode::UNAUTHORIZED,
            false,
        ),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/api/accounts")
                    .header(HOST, host)
                    .header("x-forwarded-proto", proto)
                    .header(ORIGIN, origin)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected, "{host} {proto} {origin}");
        assert_eq!(
            response.headers().contains_key(ACCESS_CONTROL_ALLOW_ORIGIN),
            cors
        );
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert_eq!(
            response.headers().contains_key("strict-transport-security"),
            proto == "https"
        );
        if cors {
            assert_eq!(response.headers()[ACCESS_CONTROL_ALLOW_ORIGIN], origin);
            assert_eq!(response.headers()[ACCESS_CONTROL_ALLOW_CREDENTIALS], "true");
            assert_eq!(response.headers()["vary"], "Origin");
            assert_eq!(
                response.headers()["access-control-allow-methods"],
                "GET,POST,PUT,PATCH,DELETE,OPTIONS"
            );
        }
    }
    fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn production_host_matching_handles_ports_ipv6_and_missing_headers() {
    let directory = temporary_directory("host-matrix");
    let mut config = HttpAdapterConfig::production(&directory);
    config.allowed_hosts = ["MAIL.Example".into(), "[::1]".into()]
        .into_iter()
        .collect();
    let router = build_router(config).unwrap();
    for (host, expected) in [
        (Some("mail.example:8787"), StatusCode::OK),
        (Some("MAIL.EXAMPLE"), StatusCode::OK),
        (Some("[::1]:8787"), StatusCode::OK),
        (
            Some("mail.example.attacker.test"),
            StatusCode::MISDIRECTED_REQUEST,
        ),
        (Some("mail.example/path"), StatusCode::MISDIRECTED_REQUEST),
        (None, StatusCode::MISDIRECTED_REQUEST),
    ] {
        let mut request = Request::builder().uri("/api/health");
        if let Some(host) = host {
            request = request.header(HOST, host);
        }
        let response = router
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), expected, "{host:?}");
        assert!(!response.headers().contains_key("strict-transport-security"));
    }
    fs::remove_dir_all(directory).unwrap();
}
