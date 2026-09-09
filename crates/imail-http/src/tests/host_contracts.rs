use super::*;

#[test]
fn defaults_to_embedded_and_requires_explicit_http_bridge() {
    assert_eq!(BridgeMode::parse([]).unwrap(), BridgeMode::Embedded);
    assert_eq!(
        BridgeMode::parse(["--http"]).unwrap(),
        BridgeMode::Http {
            host: IpAddr::from([127, 0, 0, 1]),
            port: 8787
        }
    );
    assert_eq!(
        BridgeMode::parse(["--http", "--host", "0.0.0.0", "--port", "8080"]).unwrap(),
        BridgeMode::Http {
            host: IpAddr::from([0, 0, 0, 0]),
            port: 8080
        }
    );
    assert_eq!(
        BridgeMode::parse(["--port", "8080"]),
        Err(ConfigError::HttpOptionWithoutBridge)
    );
    assert_eq!(
        BridgeMode::parse(["--http", "--port", "0"]),
        Err(ConfigError::InvalidPort)
    );
}

#[tokio::test]
async fn matches_public_health_and_service_info_contract_without_exposing_data() {
    let directory = temporary_directory("identity");
    let router = build_router(HttpAdapterConfig::new(&directory)).unwrap();
    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/system/info")
                .header(HOST, "127.0.0.1:8787")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
    assert_eq!(response.headers()["x-frame-options"], "DENY");
    assert_eq!(response.headers()[CONTENT_TYPE], "application/json");
    let body = json(response).await;
    assert_eq!(body["service"], "imail");
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(body["protocolVersion"], 1);
    assert_eq!(body["capabilities"]["gateway"], false);
    assert_eq!(body["capabilities"]["mcp"], false);
    assert_eq!(body["capabilities"]["syncWorker"], false);
    assert!(Uuid::parse_str(body["instanceId"].as_str().unwrap()).is_ok());
    assert!(!body.to_string().contains("encrypted"));

    let persisted = fs::read_to_string(directory.join("instance-id")).unwrap();
    assert_eq!(persisted.trim(), body["instanceId"]);

    let restarted = build_router(HttpAdapterConfig::new(&directory))
        .unwrap()
        .oneshot(
            Request::builder()
                .uri("/api/system/info")
                .header(HOST, "127.0.0.1:8787")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(json(restarted).await["instanceId"], body["instanceId"]);
    fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn serves_static_web_assets_and_spa_without_swallowing_reserved_routes() {
    let directory = authentication_directory("web-client");
    let web = directory.join("web");
    fs::create_dir_all(web.join("assets")).unwrap();
    fs::write(
        web.join("index.html"),
        "<!doctype html><title>Rust hosted iMail</title>",
    )
    .unwrap();
    fs::write(web.join("asset.txt"), "asset").unwrap();
    fs::write(web.join("assets").join("app.js"), "export {};").unwrap();
    let router =
        build_router(HttpAdapterConfig::new(&directory).with_web_client_root(&web)).unwrap();

    let info = json(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/system/info")
                    .header(HOST, "127.0.0.1:8787")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(info["capabilities"]["webClient"], true);

    let asset = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/asset.txt")
                .header(HOST, "127.0.0.1:8787")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(asset.status(), StatusCode::OK);
    assert_eq!(asset.headers()[CONTENT_TYPE], "text/plain; charset=utf-8");
    assert_eq!(text_body(asset).await, "asset");

    let immutable = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/assets/app.js")
                .header(HOST, "127.0.0.1:8787")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        immutable.headers()[CACHE_CONTROL],
        "public, max-age=31536000, immutable"
    );

    let route = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/settings")
                .header(HOST, "127.0.0.1:8787")
                .header("accept", "text/html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(route.status(), StatusCode::OK);
    assert_eq!(route.headers()[CACHE_CONTROL], "no-cache");
    assert!(route
        .headers()
        .get("content-security-policy")
        .is_some_and(|value| value.to_str().unwrap().contains("frame-ancestors 'none'")));
    assert!(text_body(route).await.contains("Rust hosted iMail"));

    let missing_api = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/not-a-route")
                .header(HOST, "127.0.0.1:8787")
                .header("accept", "text/html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing_api.status(), StatusCode::UNAUTHORIZED);
    assert!(!text_body(missing_api).await.contains("Rust hosted iMail"));

    let traversal = router
        .oneshot(
            Request::builder()
                .uri("/%2e%2e/master.key")
                .header(HOST, "127.0.0.1:8787")
                .header("accept", "application/octet-stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(traversal.status(), StatusCode::NOT_FOUND);
    assert!(!text_body(traversal).await.contains(&"52".repeat(32)));
    fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn standalone_http_host_stops_after_the_shutdown_signal() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let directory = authentication_directory("graceful-shutdown");
    let database = directory.join("imail.sqlite");
    let mut store = SqliteAuthStore::open_database(&database).unwrap();
    let owner = store
        .create_user("shutdown-owner", "Shutdown Owner", "shutdown-password-123")
        .unwrap();
    let session = store.create_session(&owner.id).unwrap();
    drop(store);
    let reserved = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = reserved.local_addr().unwrap().port();
    drop(reserved);
    let (shutdown, wait_for_shutdown) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_with_shutdown(
        HttpAdapterConfig::new(&directory),
        std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        port,
        async move {
            let _ = wait_for_shutdown.await;
        },
    ));
    let connected = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(connected.is_ok(), "HTTP listener did not start");
    let mut event_stream = tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .unwrap();
    event_stream
        .write_all(
            format!(
                "GET /api/events HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nCookie: imail_session={}\r\nConnection: keep-alive\r\n\r\n",
                session
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut received = Vec::new();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let mut chunk = [0_u8; 2048];
            let count = event_stream.read(&mut chunk).await.unwrap();
            assert!(count > 0, "SSE connection closed before its initial event");
            received.extend_from_slice(&chunk[..count]);
            if String::from_utf8_lossy(&received).contains("event: connected") {
                break;
            }
        }
    })
    .await
    .expect("SSE stream did not send its initial event");
    shutdown.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), server)
        .await
        .expect("HTTP host ignored graceful shutdown")
        .unwrap()
        .unwrap();
    fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn daemon_shutdown_requires_loopback_control_token_and_stops_the_host() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let directory = authentication_directory("daemon-shutdown");
    fs::write(directory.join("master.key"), "73".repeat(32)).unwrap();
    let control_file = directory.join("daemon-control-token");
    fs::write(&control_file, "local-control-secret\n").unwrap();
    let unmanaged = build_router(HttpAdapterConfig::new(&directory)).unwrap();
    let unmanaged = unmanaged
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/system/shutdown")
                .header(HOST, "127.0.0.1:8787")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unmanaged.status(), StatusCode::NOT_FOUND);

    let reserved = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = reserved.local_addr().unwrap().port();
    drop(reserved);
    let server = tokio::spawn(serve_with_shutdown(
        HttpAdapterConfig::new(&directory).with_daemon_control_file(&control_file),
        std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        port,
        std::future::pending(),
    ));
    let request = |token: &'static str| async move {
        let mut stream = loop {
            match tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).await {
                Ok(stream) => break stream,
                Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
            }
        };
        stream
            .write_all(
                format!("POST /api/system/shutdown HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-iMail-Daemon-Token: {token}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes(),
            )
            .await
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        response
    };
    let rejected = request("wrong-control-secret").await;
    assert!(rejected.starts_with("HTTP/1.1 403"));
    assert!(!server.is_finished());
    let accepted = request("local-control-secret").await;
    assert!(accepted.starts_with("HTTP/1.1 202"));
    assert!(accepted.contains("\"stopping\":true"));
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("daemon shutdown must stop the host")
        .unwrap()
        .unwrap();
    fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn standalone_host_reports_only_a_started_sync_runtime_and_removes_heartbeats() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let directory = authentication_directory("host-sync-runtime");
    fs::write(directory.join("master.key"), "53".repeat(32)).unwrap();
    let database = directory.join("imail.sqlite");
    let mut auth = SqliteAuthStore::open_database(&database).unwrap();
    let owner = auth
        .create_user("runtime-owner", "Runtime Owner", "runtime-password-123")
        .unwrap();
    let account_id = Uuid::new_v4().to_string();
    auth.upsert_account(&AccountRecord {
        id: account_id.clone(),
        owner_id: owner.id,
        provider: "custom".into(),
        email: "runtime@example.com".into(),
        display_name: "Runtime".into(),
        group: "Contract".into(),
        group_icon: "folder".into(),
        color: "#168f78".into(),
        settings: json!({
            "imapHost": "imap.example.com", "imapPort": 993, "imapSecure": true,
            "smtpHost": "smtp.example.com", "smtpPort": 465, "smtpSecure": true
        }),
        proxy: None,
        encrypted_secret: "invalid-contract-ciphertext".into(),
        auth_method: Some("app-password".into()),
        created_at: "2026-08-10T00:00:00.000Z".into(),
        last_sync_at: None,
        status: "connected".into(),
        last_error: None,
        mailboxes: json!([]),
    })
    .unwrap();
    drop(auth);
    let mut sync = SyncRuntimeStore::open_database(&database).unwrap();
    let job = sync
        .enqueue(
            &SyncEnqueue {
                account_id,
                mailbox: None,
                mailbox_role: "inbox".into(),
                reason: "manual".into(),
                priority: 100,
                not_before: None,
            },
            chrono::Utc::now(),
        )
        .unwrap();
    drop(sync);
    let reserved = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = reserved.local_addr().unwrap().port();
    drop(reserved);
    let (shutdown, wait_for_shutdown) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_with_shutdown(
        HttpAdapterConfig::new(&directory)
            .with_sync_worker(true)
            .with_sync_worker_options(SyncRuntimeOptions {
                worker_count: 2,
                poll_interval: Duration::from_millis(25),
                host_name: "contract-host".into(),
                idle_enabled: false,
                ..SyncRuntimeOptions::default()
            }),
        std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        port,
        async move {
            let _ = wait_for_shutdown.await;
        },
    ));
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let workers = SyncRuntimeStore::open_database(&database)
                .and_then(|store| store.worker_health(chrono::Utc::now()))
                .map(|health| health.workers)
                .unwrap_or_default();
            let job_failed = SyncRuntimeStore::open_database(&database)
                .and_then(|store| store.job(&job.id))
                .ok()
                .flatten()
                .is_some_and(|job| job.status == "failed");
            if job_failed
                && workers.len() == 2
                && workers
                    .iter()
                    .all(|worker| worker.host_name == "contract-host")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("sync runtime did not publish heartbeats and execute the queued job");

    let mut connection = tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .unwrap();
    connection
        .write_all(b"GET /api/system/info HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = String::new();
    connection.read_to_string(&mut response).await.unwrap();
    assert!(response.contains("\"syncWorker\":true"));

    shutdown.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("HTTP host or sync runtime ignored graceful shutdown")
        .unwrap()
        .unwrap();
    let health = SyncRuntimeStore::open_database(&database)
        .unwrap()
        .worker_health(chrono::Utc::now())
        .unwrap();
    assert!(health.workers.is_empty());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn rejects_an_invalid_existing_instance_identity_without_replacing_it() {
    let directory = temporary_directory("invalid-identity");
    let identity = directory.join("instance-id");
    fs::write(&identity, "not-a-valid-instance\n").unwrap();
    let error = build_router(HttpAdapterConfig::new(&directory));
    assert!(matches!(error, Err(HttpAdapterError::InvalidInstanceId)));
    assert_eq!(
        fs::read_to_string(identity).unwrap(),
        "not-a-valid-instance\n"
    );
    fs::remove_dir_all(directory).unwrap();
}
