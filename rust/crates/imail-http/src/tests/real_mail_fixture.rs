use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use imail_mail_network::{NetworkCancellation, NetworkMailAdapter};
use imail_runtime::{SyncImapPort, SyncMailTransportFactory};
use imail_storage_sqlite::MasterKeyCredentialCodec;
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::{
    env,
    fs::OpenOptions,
    io::{Read, Write},
    net::TcpListener as StdTcpListener,
    path::{Component, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    thread::{self, JoinHandle},
    time::Instant,
};
use sysinfo::{get_current_pid, System};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};
use tokio_rustls::{server::TlsStream as ServerTlsStream, TlsAcceptor, TlsConnector};

#[derive(Clone)]
struct RealTlsTransportFactory {
    connector: TlsConnector,
}

#[derive(Clone)]
struct FixtureConnectionProbe {
    transport: RealTlsTransportFactory,
    imap_port: u16,
    smtp_port: u16,
}

impl MailConnectionProbe for FixtureConnectionProbe {
    fn verify(&self, config: &MailConnectionConfig) -> Result<(), String> {
        let mut local = config.clone();
        local.imap_host = "localhost".into();
        local.imap_port = self.imap_port;
        local.imap_secure = true;
        local.smtp_host = "localhost".into();
        local.smtp_port = self.smtp_port;
        local.smtp_secure = true;
        self.transport.verify(&local)
    }
}

struct FixtureOAuthConfigResolver {
    base_url: String,
}

impl imail_oauth::OAuthConfigResolver for FixtureOAuthConfigResolver {
    fn resolve(
        &self,
        environment: &OAuthEnvironment,
        key: imail_oauth::OAuthProviderKey,
        account_provider: Option<&str>,
    ) -> imail_oauth::OAuthConfig {
        let mut config = imail_oauth::provider_config(environment, key, account_provider);
        config.authorization_endpoint = format!("{}/authorize", self.base_url);
        config.token_endpoint = format!("{}/token", self.base_url);
        config.user_info_endpoint = Some(format!("{}/userinfo", self.base_url));
        config
    }
}

impl MailConnectionProbe for RealTlsTransportFactory {
    fn verify(&self, config: &MailConnectionConfig) -> Result<(), String> {
        let mut imap = NetworkMailAdapter::with_tls_connector(self.connector.clone())
            .map_err(|error| error.to_string())?;
        ImapPort::verify(&mut imap, config).map_err(|error| error.to_string())?;
        let mut smtp = NetworkMailAdapter::with_tls_connector(self.connector.clone())
            .map_err(|error| error.to_string())?;
        SmtpPort::verify(&mut smtp, config).map_err(|error| error.to_string())
    }
}

impl MailTransportFactory for RealTlsTransportFactory {
    fn create_imap(&self) -> Result<Box<dyn ImapPort>, String> {
        NetworkMailAdapter::with_tls_connector(self.connector.clone())
            .map(|adapter| Box::new(adapter) as Box<dyn ImapPort>)
            .map_err(|error| error.to_string())
    }

    fn create_smtp(&self) -> Result<Box<dyn SmtpPort>, String> {
        NetworkMailAdapter::with_tls_connector(self.connector.clone())
            .map(|adapter| Box::new(adapter) as Box<dyn SmtpPort>)
            .map_err(|error| error.to_string())
    }
}

impl SyncMailTransportFactory for RealTlsTransportFactory {
    fn create(
        &self,
        cancellation: Arc<dyn NetworkCancellation>,
    ) -> Result<Box<dyn SyncImapPort>, String> {
        NetworkMailAdapter::with_cancellation_and_tls_connector(
            cancellation,
            self.connector.clone(),
        )
        .map(|adapter| Box::new(adapter) as Box<dyn SyncImapPort>)
        .map_err(|error| error.to_string())
    }
}

fn fixture_tls() -> (TlsConnector, Arc<rustls::ServerConfig>) {
    let certificate = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let certificate_der = CertificateDer::from(certificate.serialize_der().unwrap());
    let private_key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        certificate.serialize_private_key_der(),
    ));
    let server = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![certificate_der.clone()], private_key)
        .unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(certificate_der).unwrap();
    let client = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    (TlsConnector::from(Arc::new(client)), Arc::new(server))
}

fn fixture_listener() -> (StdTcpListener, u16) {
    let listener = StdTcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    (listener, port)
}

fn spawn_imap_fixture(
    listener: StdTcpListener,
    tls: Arc<rustls::ServerConfig>,
    connections: usize,
    source: Vec<u8>,
) -> JoinHandle<Vec<String>> {
    thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                let listener = TcpListener::from_std(listener).unwrap();
                let acceptor = TlsAcceptor::from(tls);
                let mut transcript = Vec::new();
                for _ in 0..connections {
                    let (stream, _) = listener.accept().await.unwrap();
                    let stream = acceptor.accept(stream).await.unwrap();
                    transcript.extend(serve_imap(stream, &source).await);
                }
                transcript
            })
    })
}

fn spawn_flaky_imap_fixture(
    listener: StdTcpListener,
    tls: Arc<rustls::ServerConfig>,
    source: Vec<u8>,
) -> JoinHandle<Vec<Vec<String>>> {
    thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                let listener = TcpListener::from_std(listener).unwrap();
                let acceptor = TlsAcceptor::from(tls);
                let mut transcripts = Vec::new();
                for connection in 0..4 {
                    let (stream, _) = listener.accept().await.unwrap();
                    let stream = acceptor.accept(stream).await.unwrap();
                    if connection == 1 {
                        transcripts.push(drop_imap_after_first_command(stream).await);
                    } else {
                        transcripts.push(serve_imap(stream, &source).await);
                    }
                }
                transcripts
            })
    })
}

async fn drop_imap_after_first_command(mut stream: ServerTlsStream<TcpStream>) -> Vec<String> {
    stream
        .write_all(b"* OK iMail HTTP fixture ready then disconnect\r\n")
        .await
        .unwrap();
    let (read, _) = tokio::io::split(stream);
    let mut lines = BufReader::new(read).lines();
    lines.next_line().await.unwrap().into_iter().collect()
}

fn spawn_idle_recovery_imap_fixture(
    listener: StdTcpListener,
    tls: Arc<rustls::ServerConfig>,
    source: Vec<u8>,
) -> (Arc<AtomicBool>, JoinHandle<Vec<Vec<String>>>) {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_server = Arc::clone(&stop);
    let handle = thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                let listener = TcpListener::from_std(listener).unwrap();
                let acceptor = TlsAcceptor::from(tls);
                let source = Arc::new(source);
                let mut tasks = tokio::task::JoinSet::new();
                let mut connection = 0_usize;
                while !stop_server.load(Ordering::SeqCst) {
                    let accepted =
                        tokio::time::timeout(Duration::from_millis(50), listener.accept()).await;
                    let Ok(Ok((stream, _))) = accepted else {
                        continue;
                    };
                    let stream = acceptor.accept(stream).await.unwrap();
                    let index = connection;
                    connection += 1;
                    let source = Arc::clone(&source);
                    tasks.spawn(async move {
                        let transcript = if index == 0 {
                            serve_imap(stream, &source).await
                        } else if index == 1 {
                            drop_imap_after_first_command(stream).await
                        } else {
                            serve_idle_recovery_imap(stream, &source, index == 2).await
                        };
                        (index, transcript)
                    });
                }
                let mut transcripts = Vec::new();
                while let Some(result) = tasks.join_next().await {
                    transcripts.push(result.unwrap());
                }
                transcripts.sort_by_key(|(index, _)| *index);
                transcripts
                    .into_iter()
                    .map(|(_, transcript)| transcript)
                    .collect()
            })
    });
    (stop, handle)
}

async fn serve_idle_recovery_imap(
    mut stream: ServerTlsStream<TcpStream>,
    source: &[u8],
    emit_change: bool,
) -> Vec<String> {
    stream
        .write_all(b"* OK iMail IDLE recovery fixture ready\r\n")
        .await
        .unwrap();
    let (read, mut write) = tokio::io::split(stream);
    let mut lines = BufReader::new(read).lines();
    let mut transcript = Vec::new();
    let mut idle_tag = None;
    loop {
        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) | Err(_) => break,
        };
        transcript.push(line.clone());
        if line.eq_ignore_ascii_case("DONE") {
            if let Some(tag) = idle_tag.take() {
                write
                    .write_all(format!("{tag} OK IDLE completed\r\n").as_bytes())
                    .await
                    .unwrap();
            }
            continue;
        }
        let tag = line.split_whitespace().next().unwrap_or("A0");
        let command = line.to_ascii_uppercase();
        if command.contains(" IDLE") {
            idle_tag = Some(tag.to_string());
            write.write_all(b"+ idling\r\n").await.unwrap();
            if emit_change {
                write.write_all(b"* 2 EXISTS\r\n").await.unwrap();
            }
            continue;
        }
        let response = if command.contains(" LOGIN ") {
            format!("{tag} OK LOGIN completed\r\n").into_bytes()
        } else if command.contains(" CAPABILITY") {
            format!("* CAPABILITY IMAP4rev1 IDLE SPECIAL-USE\r\n{tag} OK CAPABILITY completed\r\n")
                .into_bytes()
        } else if command.contains(" LIST ") {
            format!("* LIST (\\Inbox) \"/\" \"INBOX\"\r\n{tag} OK LIST completed\r\n").into_bytes()
        } else if command.contains(" STATUS ") {
            format!(
                "* STATUS \"INBOX\" (MESSAGES 1 UNSEEN 1 UIDNEXT 2 UIDVALIDITY 72)\r\n{tag} OK STATUS completed\r\n"
            )
            .into_bytes()
        } else if command.contains(" EXAMINE ") || command.contains(" SELECT ") {
            format!(
                "* FLAGS (\\Seen \\Flagged)\r\n* 1 EXISTS\r\n* OK [UIDVALIDITY 72] UIDs valid\r\n* OK [UIDNEXT 2] next UID\r\n{tag} OK [READ-ONLY] mailbox selected\r\n"
            )
            .into_bytes()
        } else if command.contains(" FETCH ") {
            let mut response = format!(
                "* 1 FETCH (UID 1 FLAGS () INTERNALDATE \"10-Aug-2026 12:00:00 +0000\" BODY[] {{{}}}\r\n",
                source.len()
            )
            .into_bytes();
            response.extend_from_slice(source);
            response.extend_from_slice(format!(")\r\n{tag} OK FETCH completed\r\n").as_bytes());
            response
        } else if command.contains(" LOGOUT") {
            let response =
                format!("* BYE fixture closing\r\n{tag} OK LOGOUT completed\r\n").into_bytes();
            write.write_all(&response).await.unwrap();
            break;
        } else {
            format!("{tag} BAD unsupported fixture command\r\n").into_bytes()
        };
        write.write_all(&response).await.unwrap();
    }
    transcript
}

async fn serve_imap(mut stream: ServerTlsStream<TcpStream>, source: &[u8]) -> Vec<String> {
    stream
        .write_all(b"* OK iMail HTTP fixture ready\r\n")
        .await
        .unwrap();
    let (read, mut write) = tokio::io::split(stream);
    let mut lines = BufReader::new(read).lines();
    let mut transcript = Vec::new();
    let mut oauth_tag = None;
    while let Some(line) = lines.next_line().await.unwrap() {
        transcript.push(line.clone());
        if let Some(tag) = oauth_tag.take() {
            write
                .write_all(format!("{tag} OK AUTHENTICATE completed\r\n").as_bytes())
                .await
                .unwrap();
            continue;
        }
        let tag = line.split_whitespace().next().unwrap_or("A0");
        let command = line.to_ascii_uppercase();
        let response = if command.contains(" LOGIN ") {
            format!("{tag} OK LOGIN completed\r\n").into_bytes()
        } else if command.contains(" AUTHENTICATE XOAUTH2") {
            if line.split_whitespace().count() > 3 {
                format!("{tag} OK AUTHENTICATE completed\r\n").into_bytes()
            } else {
                oauth_tag = Some(tag.to_string());
                b"+ \r\n".to_vec()
            }
        } else if command.contains(" CAPABILITY") {
            format!("* CAPABILITY IMAP4rev1 SPECIAL-USE\r\n{tag} OK CAPABILITY completed\r\n")
                .into_bytes()
        } else if command.contains(" LIST ") {
            format!("* LIST (\\Inbox) \"/\" \"INBOX\"\r\n{tag} OK LIST completed\r\n").into_bytes()
        } else if command.contains(" STATUS ") {
            format!(
                "* STATUS \"INBOX\" (MESSAGES 1 UNSEEN 1 UIDNEXT 2 UIDVALIDITY 71)\r\n{tag} OK STATUS completed\r\n"
            )
            .into_bytes()
        } else if command.contains(" EXAMINE ") || command.contains(" SELECT ") {
            format!(
                "* FLAGS (\\Seen \\Flagged)\r\n* 1 EXISTS\r\n* OK [UIDVALIDITY 71] UIDs valid\r\n* OK [UIDNEXT 2] next UID\r\n{tag} OK [READ-ONLY] mailbox selected\r\n"
            )
            .into_bytes()
        } else if command.contains(" FETCH ") {
            let mut response = format!(
                "* 1 FETCH (UID 1 FLAGS () INTERNALDATE \"10-Aug-2026 12:00:00 +0000\" BODY[] {{{}}}\r\n",
                source.len()
            )
            .into_bytes();
            response.extend_from_slice(source);
            response.extend_from_slice(format!(")\r\n{tag} OK FETCH completed\r\n").as_bytes());
            response
        } else if command.contains(" LOGOUT") {
            let response =
                format!("* BYE fixture closing\r\n{tag} OK LOGOUT completed\r\n").into_bytes();
            write.write_all(&response).await.unwrap();
            break;
        } else {
            format!("{tag} BAD unsupported fixture command\r\n").into_bytes()
        };
        write.write_all(&response).await.unwrap();
    }
    transcript
}

fn spawn_oauth_fixture(listener: StdTcpListener) -> JoinHandle<Vec<String>> {
    listener.set_nonblocking(false).unwrap();
    thread::spawn(move || {
        let mut transcript = Vec::new();
        for _ in 0..3 {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 4096];
            let header_end = loop {
                let read = stream.read(&mut chunk).unwrap();
                assert!(read > 0, "OAuth fixture connection closed before headers");
                request.extend_from_slice(&chunk[..read]);
                if let Some(position) = request.windows(4).position(|value| value == b"\r\n\r\n") {
                    break position + 4;
                }
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.split_once(':').and_then(|(name, value)| {
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                })
                .unwrap_or(0);
            while request.len() < header_end + content_length {
                let read = stream.read(&mut chunk).unwrap();
                assert!(read > 0, "OAuth fixture connection closed before body");
                request.extend_from_slice(&chunk[..read]);
            }
            let request_text = String::from_utf8(request).unwrap();
            transcript.push(request_text.clone());
            let first_line = request_text.lines().next().unwrap();
            let body = if first_line.starts_with("GET /userinfo ") {
                r#"{"email":"fixture@example.test","name":"OAuth Fixture"}"#
            } else if request_text.contains("grant_type=refresh_token") {
                r#"{"access_token":"fresh-access-token","expires_in":3600,"token_type":"Bearer","scope":"openid email https://mail.google.com/"}"#
            } else {
                r#"{"access_token":"expired-access-token","refresh_token":"fixture-refresh-token","expires_in":0,"token_type":"Bearer","scope":"openid email https://mail.google.com/"}"#
            };
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        }
        transcript
    })
}

fn spawn_smtp_fixture(
    listener: StdTcpListener,
    tls: Arc<rustls::ServerConfig>,
    connections: usize,
) -> JoinHandle<Vec<Vec<u8>>> {
    thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                let listener = TcpListener::from_std(listener).unwrap();
                let acceptor = TlsAcceptor::from(tls);
                let mut messages = Vec::new();
                for _ in 0..connections {
                    let (stream, _) = listener.accept().await.unwrap();
                    let stream = acceptor.accept(stream).await.unwrap();
                    if let Some(message) = serve_smtp(stream).await {
                        messages.push(message);
                    }
                }
                messages
            })
    })
}

async fn serve_smtp(mut stream: ServerTlsStream<TcpStream>) -> Option<Vec<u8>> {
    stream
        .write_all(b"220 localhost iMail HTTP fixture\r\n")
        .await
        .unwrap();
    let (read, mut write) = tokio::io::split(stream);
    let mut lines = BufReader::new(read).lines();
    let mut login_step = 0_u8;
    let mut message: Option<Vec<u8>> = None;
    let mut collecting_data = false;
    while let Some(line) = lines.next_line().await.unwrap() {
        let upper = line.to_ascii_uppercase();
        if collecting_data {
            if line == "." {
                collecting_data = false;
                write.write_all(b"250 2.0.0 queued\r\n").await.unwrap();
            } else {
                let payload = message.as_mut().unwrap();
                payload.extend_from_slice(line.as_bytes());
                payload.extend_from_slice(b"\r\n");
                continue;
            }
        } else if login_step == 1 {
            login_step = 2;
            write.write_all(b"334 UGFzc3dvcmQ6\r\n").await.unwrap();
        } else if login_step == 2 {
            login_step = 0;
            write
                .write_all(b"235 2.7.0 authentication successful\r\n")
                .await
                .unwrap();
        } else if upper.starts_with("EHLO ") {
            write
                .write_all(b"250-localhost\r\n250-AUTH PLAIN LOGIN XOAUTH2\r\n250 8BITMIME\r\n")
                .await
                .unwrap();
        } else if upper == "AUTH LOGIN" {
            login_step = 1;
            write.write_all(b"334 VXNlcm5hbWU6\r\n").await.unwrap();
        } else if upper.starts_with("AUTH PLAIN ") || upper.starts_with("AUTH XOAUTH2 ") {
            write
                .write_all(b"235 2.7.0 authentication successful\r\n")
                .await
                .unwrap();
        } else if upper.starts_with("MAIL FROM:") || upper.starts_with("RCPT TO:") {
            write.write_all(b"250 2.1.0 accepted\r\n").await.unwrap();
        } else if upper == "DATA" {
            message = Some(Vec::new());
            collecting_data = true;
            write
                .write_all(b"354 End data with <CR><LF>.<CR><LF>\r\n")
                .await
                .unwrap();
        } else if upper == "QUIT" {
            write.write_all(b"221 2.0.0 bye\r\n").await.unwrap();
            break;
        } else {
            write.write_all(b"500 unsupported\r\n").await.unwrap();
        }
    }
    message
}

fn request(
    method: Method,
    uri: impl AsRef<str>,
    cookie: Option<&str>,
    body: Body,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri.as_ref())
        .header(HOST, "127.0.0.1:8787");
    if let Some(cookie) = cookie {
        builder = builder.header("cookie", cookie);
    }
    builder
        .header(CONTENT_TYPE, "application/json")
        .body(body)
        .unwrap()
}

#[tokio::test]
async fn http_account_sync_query_and_send_use_the_real_tls_protocol_stack() {
    let directory = authentication_directory("real-mail-network");
    fs::write(directory.join("master.key"), "64".repeat(32)).unwrap();
    let database = directory.join("imail.sqlite");
    let (connector, tls_server) = fixture_tls();
    let (imap_listener, imap_port) = fixture_listener();
    let (smtp_listener, smtp_port) = fixture_listener();
    let inbound = concat!(
        "From: Network Sender <sender@example.test>\r\n",
        "To: Fixture <fixture@example.test>\r\n",
        "Subject: HTTP network inbound\r\n",
        "Message-ID: <http-network-inbound@example.test>\r\n",
        "Content-Type: text/plain; charset=utf-8\r\n",
        "\r\n",
        "network body\r\n"
    )
    .as_bytes()
    .to_vec();
    let imap_fixture = spawn_imap_fixture(imap_listener, Arc::clone(&tls_server), 2, inbound);
    let smtp_fixture = spawn_smtp_fixture(smtp_listener, tls_server, 2);
    let factory = Arc::new(RealTlsTransportFactory { connector });
    let options = SyncRuntimeOptions {
        worker_count: 1,
        poll_interval: Duration::from_millis(20),
        lease_duration: Duration::from_secs(5),
        idle_enabled: false,
        scheduler_startup_delay: Duration::from_secs(60),
        ..SyncRuntimeOptions::default()
    };
    let mut config = HttpAdapterConfig::new(&directory)
        .with_connection_probe(factory.clone())
        .with_mail_transport_factory(factory.clone())
        .with_sync_mail_transport_factory(factory)
        .with_sync_worker(true)
        .with_sync_worker_options(options);
    config.registration_open = true;
    let mut runtime = start_sync_runtime(&config, None).unwrap().unwrap();
    let router = build_router(config).unwrap();

    let registered = router
        .clone()
        .oneshot(request(
            Method::POST,
            "/api/auth/register",
            None,
            Body::from(
                r#"{"login":"network-owner","displayName":"Network Owner","password":"network-owner-password-123"}"#,
            ),
        ))
        .await
        .unwrap();
    assert_eq!(registered.status(), StatusCode::CREATED);
    let cookie = registered.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();

    let created = router
        .clone()
        .oneshot(request(
            Method::POST,
            "/api/accounts",
            Some(&cookie),
            Body::from(format!(
                r#"{{"provider":"custom","email":"fixture@example.test","displayName":"Fixture Mail","password":"fixture-password","settings":{{"imapHost":"localhost","imapPort":{imap_port},"imapSecure":true,"smtpHost":"localhost","smtpPort":{smtp_port},"smtpSecure":true}}}}"#
            )),
        ))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    let created = json(created).await;
    assert!(!created.to_string().contains("fixture-password"));
    assert!(!created.to_string().contains("encryptedSecret"));
    let account_id = created["account"]["id"].as_str().unwrap().to_string();

    let queued = router
        .clone()
        .oneshot(request(
            Method::POST,
            format!("/api/accounts/{account_id}/sync"),
            Some(&cookie),
            Body::empty(),
        ))
        .await
        .unwrap();
    assert_eq!(queued.status(), StatusCode::OK);

    let mut synchronized = None;
    for _ in 0..100 {
        let listed = router
            .clone()
            .oneshot(request(
                Method::GET,
                "/api/messages?limit=10&offset=0",
                Some(&cookie),
                Body::empty(),
            ))
            .await
            .unwrap();
        assert_eq!(listed.status(), StatusCode::OK);
        let listed = json(listed).await;
        if listed["total"] == 1 {
            synchronized = Some(listed);
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let synchronized = synchronized.expect("real IMAP message was not committed by the worker");
    assert_eq!(
        synchronized["messages"][0]["subject"],
        "HTTP network inbound"
    );
    assert_eq!(synchronized["messages"][0]["uid"], 1);
    assert!(synchronized["messages"][0].get("ownerId").is_none());
    assert!(synchronized["messages"][0].get("text").is_none());
    let message_id = synchronized["messages"][0]["id"].as_str().unwrap();
    let detail = router
        .clone()
        .oneshot(request(
            Method::GET,
            format!("/api/messages/{message_id}"),
            Some(&cookie),
            Body::empty(),
        ))
        .await
        .unwrap();
    assert_eq!(detail.status(), StatusCode::OK);
    assert_eq!(json(detail).await["message"]["text"], "network body");
    let sync_store = SyncRuntimeStore::open_database(&database).unwrap();
    let job = sync_store
        .account_jobs(&account_id, 10)
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(job.status, "succeeded");
    drop(sync_store);

    let sent = router
        .clone()
        .oneshot(request(
            Method::POST,
            "/api/send",
            Some(&cookie),
            Body::from(format!(
                r#"{{"accountId":"{account_id}","to":["recipient@example.test"],"subject":"HTTP network outbound","text":"outbound body"}}"#
            )),
        ))
        .await
        .unwrap();
    assert_eq!(sent.status(), StatusCode::CREATED);
    assert_eq!(
        json(sent).await["accepted"],
        json!(["recipient@example.test"])
    );

    let report = runtime.shutdown(Duration::from_secs(5));
    assert!(report.graceful(), "timed out tasks: {:?}", report.timed_out);
    assert!(SyncRuntimeStore::open_database(&database)
        .unwrap()
        .worker_health(chrono::Utc::now())
        .unwrap()
        .workers
        .is_empty());
    let imap_transcript = imap_fixture.join().unwrap();
    assert!(imap_transcript
        .iter()
        .any(|line| line.to_ascii_uppercase().contains(" FETCH ")));
    let smtp_messages = smtp_fixture.join().unwrap();
    assert_eq!(smtp_messages.len(), 1);
    let outbound = imail_mail::parse_rfc822(&smtp_messages[0]).unwrap();
    assert_eq!(outbound.subject, "HTTP network outbound");
    assert_eq!(outbound.text, "outbound body");
    fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn persistent_worker_recovers_after_tls_disconnect_and_round_trips_large_attachments() {
    let directory = authentication_directory("real-mail-recovery");
    fs::write(directory.join("master.key"), "66".repeat(32)).unwrap();
    let database = directory.join("imail.sqlite");
    let (connector, tls_server) = fixture_tls();
    let (imap_listener, imap_port) = fixture_listener();
    let (smtp_listener, smtp_port) = fixture_listener();
    let attachment = vec![b'Z'; 2 * 1024 * 1024];
    let inbound = format!(
        concat!(
            "From: Recovery Sender <sender@example.test>\r\n",
            "To: Fixture <fixture@example.test>\r\n",
            "Subject: Recovery attachment\r\n",
            "Message-ID: <recovery-attachment@example.test>\r\n",
            "MIME-Version: 1.0\r\n",
            "Content-Type: multipart/mixed; boundary=imail-recovery\r\n",
            "\r\n",
            "--imail-recovery\r\n",
            "Content-Type: text/plain; charset=utf-8\r\n",
            "\r\n",
            "recovered after disconnect\r\n",
            "--imail-recovery\r\n",
            "Content-Type: application/octet-stream; name=large.bin\r\n",
            "Content-Disposition: attachment; filename=large.bin\r\n",
            "Content-Transfer-Encoding: base64\r\n",
            "\r\n",
            "{}\r\n",
            "--imail-recovery--\r\n"
        ),
        STANDARD.encode(&attachment)
    )
    .into_bytes();
    let imap_fixture = spawn_flaky_imap_fixture(imap_listener, Arc::clone(&tls_server), inbound);
    let smtp_fixture = spawn_smtp_fixture(smtp_listener, tls_server, 2);
    let factory = Arc::new(RealTlsTransportFactory { connector });
    let options = SyncRuntimeOptions {
        worker_count: 1,
        poll_interval: Duration::from_millis(20),
        lease_duration: Duration::from_secs(5),
        idle_enabled: false,
        scheduler_startup_delay: Duration::from_secs(60),
        ..SyncRuntimeOptions::default()
    };
    let mut config = HttpAdapterConfig::new(&directory)
        .with_connection_probe(factory.clone())
        .with_mail_transport_factory(factory.clone())
        .with_sync_mail_transport_factory(factory)
        .with_sync_worker(true)
        .with_sync_worker_options(options);
    config.registration_open = true;
    let mut runtime = start_sync_runtime(&config, None).unwrap().unwrap();
    let router = build_router(config).unwrap();

    let registered = router
        .clone()
        .oneshot(request(
            Method::POST,
            "/api/auth/register",
            None,
            Body::from(
                r#"{"login":"recovery-owner","displayName":"Recovery Owner","password":"recovery-owner-password-123"}"#,
            ),
        ))
        .await
        .unwrap();
    assert_eq!(registered.status(), StatusCode::CREATED);
    let cookie = registered.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();

    let created = router
        .clone()
        .oneshot(request(
            Method::POST,
            "/api/accounts",
            Some(&cookie),
            Body::from(format!(
                r#"{{"provider":"custom","email":"fixture@example.test","displayName":"Recovery Mail","password":"fixture-password","settings":{{"imapHost":"localhost","imapPort":{imap_port},"imapSecure":true,"smtpHost":"localhost","smtpPort":{smtp_port},"smtpSecure":true}}}}"#
            )),
        ))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    let account_id = json(created).await["account"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let first = router
        .clone()
        .oneshot(request(
            Method::POST,
            format!("/api/accounts/{account_id}/sync"),
            Some(&cookie),
            Body::empty(),
        ))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let mut failed_job = None;
    for _ in 0..120 {
        let jobs = SyncRuntimeStore::open_database(&database)
            .unwrap()
            .account_jobs(&account_id, 10)
            .unwrap();
        if let Some(job) = jobs.into_iter().find(|job| job.status == "failed") {
            failed_job = Some(job);
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let failed_job =
        failed_job.expect("the forced TLS disconnect was not persisted as a failed job");
    assert_eq!(failed_job.error_code.as_deref(), Some("IMAP_UNAVAILABLE"));
    let safe_failure = failed_job.error_message.unwrap();
    assert!(!safe_failure.contains("fixture-password"));
    assert!(!safe_failure.contains("recovery-owner-password"));

    let empty = router
        .clone()
        .oneshot(request(
            Method::GET,
            "/api/messages?limit=10&offset=0",
            Some(&cookie),
            Body::empty(),
        ))
        .await
        .unwrap();
    assert_eq!(json(empty).await["total"], 0);

    let retry = router
        .clone()
        .oneshot(request(
            Method::POST,
            format!("/api/accounts/{account_id}/sync"),
            Some(&cookie),
            Body::empty(),
        ))
        .await
        .unwrap();
    assert_eq!(retry.status(), StatusCode::OK);
    let mut synchronized = None;
    for _ in 0..160 {
        let listed = router
            .clone()
            .oneshot(request(
                Method::GET,
                "/api/messages?limit=10&offset=0",
                Some(&cookie),
                Body::empty(),
            ))
            .await
            .unwrap();
        let listed = json(listed).await;
        if listed["total"] == 1 {
            synchronized = Some(listed);
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let synchronized = synchronized.expect("retry did not recover the disconnected IMAP sync");
    let message = &synchronized["messages"][0];
    assert_eq!(message["subject"], "Recovery attachment");
    assert_eq!(message["hasAttachments"], true);
    let message_id = message["id"].as_str().unwrap();

    let downloaded = router
        .clone()
        .oneshot(request(
            Method::GET,
            format!("/api/messages/{message_id}/attachments/0"),
            Some(&cookie),
            Body::empty(),
        ))
        .await
        .unwrap();
    assert_eq!(downloaded.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(downloaded.into_body(), attachment.len() + 1)
            .await
            .unwrap(),
        attachment
    );

    let outgoing = json!({
        "accountId": account_id,
        "to": ["recipient@example.test"],
        "subject": "Recovery outbound attachment",
        "text": "large attachment after recovery",
        "attachments": [{
            "id": "large-attachment",
            "filename": "large.bin",
            "contentType": "application/octet-stream",
            "size": attachment.len(),
            "data": STANDARD.encode(&attachment)
        }]
    });
    let sent = router
        .clone()
        .oneshot(request(
            Method::POST,
            "/api/send",
            Some(&cookie),
            Body::from(outgoing.to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(sent.status(), StatusCode::CREATED);

    let health = runtime.health();
    assert_eq!(health.jobs_failed, 1);
    assert_eq!(health.jobs_succeeded, 1);
    let report = runtime.shutdown(Duration::from_secs(5));
    assert!(report.graceful(), "timed out tasks: {:?}", report.timed_out);

    let transcripts = imap_fixture.join().unwrap();
    assert_eq!(transcripts.len(), 4);
    assert_eq!(transcripts[1].len(), 1);
    assert!(transcripts[2]
        .iter()
        .any(|line| line.to_ascii_uppercase().contains(" FETCH ")));
    assert!(transcripts[3]
        .iter()
        .any(|line| line.to_ascii_uppercase().contains(" FETCH ")));
    let smtp_messages = smtp_fixture.join().unwrap();
    assert_eq!(smtp_messages.len(), 1);
    let outbound = imail_mail::parse_rfc822(&smtp_messages[0]).unwrap();
    assert_eq!(outbound.subject, "Recovery outbound attachment");
    assert_eq!(
        imail_mail::attachment_content(&smtp_messages[0], 0).unwrap(),
        attachment
    );
    fs::remove_dir_all(directory).unwrap();
}

#[derive(Debug)]
struct RealTlsSoakOptions {
    duration: Duration,
    maximum_growth_bytes: u64,
    report: PathBuf,
}

fn parse_real_tls_soak_options(
    read: impl Fn(&str) -> Option<String>,
) -> Result<RealTlsSoakOptions, String> {
    if read("IMAIL_RUST_TLS_SOAK_ALLOW").as_deref() != Some("isolated-loopback") {
        return Err(
            "真实 TLS 长稳验收默认拒绝；设置 IMAIL_RUST_TLS_SOAK_ALLOW=isolated-loopback".into(),
        );
    }
    let duration_seconds = read("IMAIL_RUST_TLS_SOAK_DURATION_SECONDS")
        .ok_or("缺少 IMAIL_RUST_TLS_SOAK_DURATION_SECONDS")?
        .parse::<u64>()
        .map_err(|_| "IMAIL_RUST_TLS_SOAK_DURATION_SECONDS 必须是整数")?;
    if !(30..=86_400).contains(&duration_seconds) {
        return Err("真实 TLS 长稳时长必须为 30..86400 秒".into());
    }
    let maximum_growth_mib = read("IMAIL_RUST_TLS_SOAK_MAX_GROWTH_MIB")
        .unwrap_or_else(|| "32".into())
        .parse::<u64>()
        .map_err(|_| "IMAIL_RUST_TLS_SOAK_MAX_GROWTH_MIB 必须是整数")?;
    if maximum_growth_mib == 0 {
        return Err("真实 TLS 长稳内存增长预算必须大于 0 MiB".into());
    }
    let report =
        PathBuf::from(read("IMAIL_RUST_TLS_SOAK_REPORT").ok_or("缺少 IMAIL_RUST_TLS_SOAK_REPORT")?);
    if report.extension().and_then(|value| value.to_str()) != Some("json") {
        return Err("真实 TLS 长稳报告必须使用 .json 后缀".into());
    }
    if report.components().any(|component| {
        matches!(
            component,
            Component::Normal(value) if value.to_string_lossy().eq_ignore_ascii_case(".data")
        )
    }) {
        return Err("真实 TLS 长稳报告不得写入 .data".into());
    }
    Ok(RealTlsSoakOptions {
        duration: Duration::from_secs(duration_seconds),
        maximum_growth_bytes: maximum_growth_mib.saturating_mul(1024 * 1024),
        report,
    })
}

#[test]
fn real_tls_soak_requires_explicit_isolated_loopback_guard() {
    let error = parse_real_tls_soak_options(|_| None).unwrap_err();
    assert!(error.contains("默认拒绝"));
}

async fn exercise_real_tls_idle_recovery(soak: Option<&RealTlsSoakOptions>) -> Value {
    let directory = authentication_directory("real-idle-recovery");
    fs::write(directory.join("master.key"), "67".repeat(32)).unwrap();
    let database = directory.join("imail.sqlite");
    let (connector, tls_server) = fixture_tls();
    let (imap_listener, imap_port) = fixture_listener();
    let (smtp_listener, smtp_port) = fixture_listener();
    let inbound = concat!(
        "From: IDLE Sender <sender@example.test>\r\n",
        "To: Fixture <fixture@example.test>\r\n",
        "Subject: IDLE reconnect recovery\r\n",
        "Message-ID: <idle-reconnect@example.test>\r\n",
        "Content-Type: text/plain; charset=utf-8\r\n",
        "\r\n",
        "woken by a real IDLE EXISTS event\r\n"
    )
    .as_bytes()
    .to_vec();
    let (stop_imap, imap_fixture) =
        spawn_idle_recovery_imap_fixture(imap_listener, Arc::clone(&tls_server), inbound);
    let smtp_fixture = spawn_smtp_fixture(smtp_listener, tls_server, 1);
    let factory = Arc::new(RealTlsTransportFactory { connector });
    let options = SyncRuntimeOptions {
        worker_count: 1,
        poll_interval: Duration::from_millis(20),
        lease_duration: Duration::from_secs(5),
        idle_enabled: true,
        watcher_reconcile_interval: Duration::from_millis(20),
        watcher_maximum_wait: Duration::from_secs(15),
        scheduler_startup_delay: Duration::from_secs(60),
        ..SyncRuntimeOptions::default()
    };
    let mut config = HttpAdapterConfig::new(&directory)
        .with_connection_probe(factory.clone())
        .with_mail_transport_factory(factory.clone())
        .with_sync_mail_transport_factory(factory)
        .with_sync_worker(true)
        .with_sync_worker_options(options);
    config.registration_open = true;
    let mut runtime = start_sync_runtime(&config, None).unwrap().unwrap();
    let router = build_router(config).unwrap();

    let registered = router
        .clone()
        .oneshot(request(
            Method::POST,
            "/api/auth/register",
            None,
            Body::from(
                r#"{"login":"idle-owner","displayName":"IDLE Owner","password":"idle-owner-password-123"}"#,
            ),
        ))
        .await
        .unwrap();
    assert_eq!(registered.status(), StatusCode::CREATED);
    let cookie = registered.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let created = router
        .clone()
        .oneshot(request(
            Method::POST,
            "/api/accounts",
            Some(&cookie),
            Body::from(format!(
                r#"{{"provider":"custom","email":"fixture@example.test","displayName":"IDLE Mail","password":"fixture-password","settings":{{"imapHost":"localhost","imapPort":{imap_port},"imapSecure":true,"smtpHost":"localhost","smtpPort":{smtp_port},"smtpSecure":true}}}}"#
            )),
        ))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    let account_id = json(created).await["account"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let synchronized = loop {
        let listed = router
            .clone()
            .oneshot(request(
                Method::GET,
                "/api/messages?limit=10&offset=0",
                Some(&cookie),
                Body::empty(),
            ))
            .await
            .unwrap();
        let listed = json(listed).await;
        if listed["total"] == 1 {
            break listed;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "real TLS IDLE reconnect did not commit a recovery sync"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    assert_eq!(
        synchronized["messages"][0]["subject"],
        "IDLE reconnect recovery"
    );

    let health = runtime.health();
    assert_eq!(health.active_watchers, 1);
    assert_eq!(health.watcher_disconnects, 1);
    assert_eq!(health.watcher_reconnects, 1);
    assert_eq!(health.jobs_failed, 0);
    assert_eq!(health.jobs_succeeded, 1);
    let jobs = SyncRuntimeStore::open_database(&database)
        .unwrap()
        .account_jobs(&account_id, 10)
        .unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].reason, "recovery");
    assert_eq!(jobs[0].status, "succeeded");

    let mut samples = Vec::new();
    if let Some(options) = soak {
        let process_id = get_current_pid().unwrap();
        let mut system = System::new();
        let started = Instant::now();
        while started.elapsed() < options.duration {
            system.refresh_process(process_id);
            let process = system.process(process_id).unwrap();
            let worker_health = SyncRuntimeStore::open_database(&database)
                .unwrap()
                .worker_health(chrono::Utc::now())
                .unwrap();
            let runtime_health = runtime.health();
            samples.push(serde_json::json!({
                "elapsedMs": started.elapsed().as_millis() as u64,
                "residentBytes": process.memory(),
                "virtualBytes": process.virtual_memory(),
                "cpuPercent": process.cpu_usage(),
                "threadCount": process.tasks().map(|tasks| tasks.len()),
                "activeWatchers": runtime_health.active_watchers,
                "watcherDisconnects": runtime_health.watcher_disconnects,
                "watcherReconnects": runtime_health.watcher_reconnects,
                "queuedJobs": worker_health.queued_jobs,
                "workerHeartbeats": worker_health.workers.len(),
            }));
            if samples.len() == 1 || samples.len() % 30 == 0 {
                println!(
                    "real TLS soak progress: {} / {} seconds",
                    started.elapsed().as_secs(),
                    options.duration.as_secs()
                );
            }
            let remaining = options.duration.saturating_sub(started.elapsed());
            tokio::time::sleep(remaining.min(Duration::from_secs(1))).await;
        }
    }

    let health = runtime.health();
    let report = runtime.shutdown(Duration::from_secs(5));
    let graceful = report.graceful();
    stop_imap.store(true, Ordering::SeqCst);
    let transcripts = imap_fixture.join().unwrap();
    let idle_seen = transcripts
        .iter()
        .flatten()
        .any(|line| line.to_ascii_uppercase().contains(" IDLE"));
    let fetch_seen = transcripts
        .iter()
        .flatten()
        .any(|line| line.to_ascii_uppercase().contains(" FETCH "));
    let smtp_messages = smtp_fixture.join().unwrap();
    let final_store = SyncRuntimeStore::open_database(&database).unwrap();
    let final_worker_health = final_store.worker_health(chrono::Utc::now()).unwrap();
    let final_jobs = final_store.account_jobs(&account_id, 100).unwrap();
    let recovery_jobs = final_jobs
        .iter()
        .filter(|job| job.reason == "recovery")
        .count();
    let scheduled_jobs = final_jobs
        .iter()
        .filter(|job| job.reason == "scheduled")
        .count();
    let all_jobs_succeeded = final_jobs.iter().all(|job| job.status == "succeeded");
    let first_memory = samples
        .first()
        .and_then(|sample| sample["residentBytes"].as_u64())
        .unwrap_or(0);
    let last_memory = samples
        .last()
        .and_then(|sample| sample["residentBytes"].as_u64())
        .unwrap_or(first_memory);
    let peak_memory = samples
        .iter()
        .filter_map(|sample| sample["residentBytes"].as_u64())
        .max()
        .unwrap_or(first_memory);
    let growth = last_memory.saturating_sub(first_memory);
    let maximum_growth_bytes = soak
        .map(|options| options.maximum_growth_bytes)
        .unwrap_or(u64::MAX);
    let maximum_queued_jobs = samples
        .iter()
        .filter_map(|sample| sample["queuedJobs"].as_u64())
        .max()
        .unwrap_or(0);
    let ok = graceful
        && health.active_watchers == 1
        && health.watcher_disconnects == 1
        && health.watcher_reconnects == 1
        && health.jobs_failed == 0
        && health.jobs_cancelled == 0
        && health.jobs_started == health.jobs_succeeded
        && health.jobs_succeeded >= 1
        && all_jobs_succeeded
        && final_worker_health.workers.is_empty()
        && final_worker_health.queued_jobs == 0
        && maximum_queued_jobs == 0
        && transcripts.len() >= 4
        && transcripts.get(1).is_some_and(|lines| lines.len() == 1)
        && idle_seen
        && fetch_seen
        && smtp_messages.is_empty()
        && growth <= maximum_growth_bytes;
    let evidence = serde_json::json!({
        "schemaVersion": 1,
        "ok": ok,
        "generatedAt": chrono::Utc::now().to_rfc3339(),
        "fixture": "isolated-loopback-tls",
        "durationSeconds": soak.map(|options| options.duration.as_secs()).unwrap_or(0),
        "sampleCount": samples.len(),
        "memory": {
            "firstResidentBytes": first_memory,
            "lastResidentBytes": last_memory,
            "peakResidentBytes": peak_memory,
            "growthBytes": growth,
            "maximumGrowthBytes": maximum_growth_bytes,
            "withinBudget": growth <= maximum_growth_bytes,
        },
        "runtime": {
            "activeWatchersBeforeShutdown": health.active_watchers,
            "watcherDisconnects": health.watcher_disconnects,
            "watcherReconnects": health.watcher_reconnects,
            "jobsSucceeded": health.jobs_succeeded,
            "jobsFailed": health.jobs_failed,
            "jobsCancelled": health.jobs_cancelled,
            "finalJobCount": final_jobs.len(),
            "recoveryJobs": recovery_jobs,
            "scheduledJobs": scheduled_jobs,
            "allJobsSucceeded": all_jobs_succeeded,
            "maximumQueuedJobs": maximum_queued_jobs,
            "remainingQueuedJobs": final_worker_health.queued_jobs,
            "remainingWorkerHeartbeats": final_worker_health.workers.len(),
            "shutdownGraceful": graceful,
        },
        "protocol": {
            "imapConnections": transcripts.len(),
            "initialDisconnectObserved": transcripts.get(1).is_some_and(|lines| lines.len() == 1),
            "idleObserved": idle_seen,
            "fetchObserved": fetch_seen,
            "smtpMessages": smtp_messages.len(),
        },
        "data": {
            "temporaryDirectory": true,
            "activeRepositoryDataOpened": false,
        },
        "samples": samples,
    });
    drop(final_store);
    fs::remove_dir_all(directory).unwrap();
    evidence
}

#[tokio::test]
async fn real_tls_idle_watcher_reconnects_enqueues_and_commits_a_recovery_sync() {
    let evidence = exercise_real_tls_idle_recovery(None).await;
    assert_eq!(evidence["ok"], true, "{evidence}");
}

#[tokio::test]
#[ignore = "requires explicit isolated-loopback soak guard, duration and report path"]
async fn real_tls_idle_runtime_soak_acceptance() {
    let options = parse_real_tls_soak_options(|key| env::var(key).ok()).unwrap();
    assert!(!options.report.exists(), "长稳报告目标已存在，拒绝覆盖");
    assert!(
        options
            .report
            .parent()
            .is_some_and(|parent| parent.is_dir()),
        "长稳报告父目录必须已存在"
    );
    let evidence = exercise_real_tls_idle_recovery(Some(&options)).await;
    let serialized = format!("{}\n", serde_json::to_string_pretty(&evidence).unwrap());
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&options.report)
        .unwrap();
    output.write_all(serialized.as_bytes()).unwrap();
    assert_eq!(evidence["ok"], true, "{serialized}");
}

#[tokio::test]
async fn oauth_callback_refresh_sync_and_send_share_real_network_adapters() {
    let directory = authentication_directory("real-oauth-mail-network");
    fs::write(directory.join("master.key"), "65".repeat(32)).unwrap();
    let database = directory.join("imail.sqlite");
    let mut store = SqliteAuthStore::open_database(&database).unwrap();
    let owner = store
        .create_user(
            "oauth-network-owner",
            "OAuth Network",
            "oauth-network-password-123",
        )
        .unwrap();
    let session = store.create_session(&owner.id).unwrap();
    drop(store);

    let (connector, tls_server) = fixture_tls();
    let (imap_listener, imap_port) = fixture_listener();
    let (smtp_listener, smtp_port) = fixture_listener();
    let (oauth_listener, oauth_port) = fixture_listener();
    let inbound = concat!(
        "From: OAuth Sender <sender@example.test>\r\n",
        "To: OAuth Fixture <fixture@example.test>\r\n",
        "Subject: OAuth network inbound\r\n",
        "Message-ID: <oauth-network-inbound@example.test>\r\n",
        "Content-Type: text/plain; charset=utf-8\r\n",
        "\r\n",
        "oauth network body\r\n"
    )
    .as_bytes()
    .to_vec();
    let imap_fixture = spawn_imap_fixture(imap_listener, Arc::clone(&tls_server), 2, inbound);
    let smtp_fixture = spawn_smtp_fixture(smtp_listener, tls_server, 2);
    let oauth_fixture = spawn_oauth_fixture(oauth_listener);
    let transport = RealTlsTransportFactory { connector };
    let probe = Arc::new(FixtureConnectionProbe {
        transport: transport.clone(),
        imap_port,
        smtp_port,
    });
    let oauth_base_url = format!("http://127.0.0.1:{oauth_port}");
    let resolver = Arc::new(FixtureOAuthConfigResolver {
        base_url: oauth_base_url,
    });
    let options = SyncRuntimeOptions {
        worker_count: 1,
        poll_interval: Duration::from_millis(20),
        lease_duration: Duration::from_secs(5),
        idle_enabled: false,
        scheduler_startup_delay: Duration::from_secs(60),
        ..SyncRuntimeOptions::default()
    };
    let environment = OAuthEnvironment {
        callback_base_url: "http://127.0.0.1:8787/api/oauth".into(),
        google_client_id: Some("fixture-client-id".into()),
        google_client_secret: Some("fixture-client-secret".into()),
        ..OAuthEnvironment::default()
    };
    let config = HttpAdapterConfig::new(&directory)
        .with_oauth_environment(environment)
        .with_oauth_config_resolver(resolver)
        .with_connection_probe(probe)
        .with_mail_transport_factory(Arc::new(transport.clone()))
        .with_sync_mail_transport_factory(Arc::new(transport))
        .with_sync_worker(true)
        .with_sync_worker_options(options);
    let mut runtime = start_sync_runtime(&config, None).unwrap().unwrap();
    let router = build_router(config).unwrap();
    let cookie = format!("imail_session={session}");

    let started = router
        .clone()
        .oneshot(request(
            Method::POST,
            "/api/oauth/start",
            Some(&cookie),
            Body::from(
                r##"{"provider":"gmail","displayName":"OAuth Fixture","group":"个人","color":"#168f78"}"##,
            ),
        ))
        .await
        .unwrap();
    assert_eq!(started.status(), StatusCode::OK);
    let started = json(started).await;
    let state = started["state"].as_str().unwrap();
    assert!(started["authorizationUrl"]
        .as_str()
        .unwrap()
        .starts_with(&format!("http://127.0.0.1:{oauth_port}/authorize?")));

    let callback_uri = format!(
        "/api/oauth/google/callback?{}",
        url::form_urlencoded::Serializer::new(String::new())
            .append_pair("state", state)
            .append_pair("code", "fixture-authorization-code")
            .finish()
    );
    let callback = router
        .clone()
        .oneshot(request(Method::GET, callback_uri, None, Body::empty()))
        .await
        .unwrap();
    assert_eq!(callback.status(), StatusCode::OK);
    let callback_body = text_body(callback).await;
    assert!(!callback_body.contains("expired-access-token"));
    assert!(!callback_body.contains("fixture-refresh-token"));

    let status = router
        .clone()
        .oneshot(request(
            Method::POST,
            "/api/oauth/status",
            Some(&cookie),
            Body::from(json!({ "state": state }).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(status.status(), StatusCode::OK);
    let status = json(status).await;
    assert_eq!(status["completed"], true);
    assert!(!status.to_string().contains("access-token"));
    assert!(!status.to_string().contains("refresh-token"));
    let account_id = status["account"]["id"].as_str().unwrap().to_string();

    let mut store = SqliteAuthStore::open_database(&database).unwrap();
    let mut account = store.account(&owner.id, &account_id).unwrap().unwrap();
    account.settings = json!({
        "imapHost": "localhost", "imapPort": imap_port, "imapSecure": true,
        "smtpHost": "localhost", "smtpPort": smtp_port, "smtpSecure": true
    });
    store.upsert_account(&account).unwrap();
    drop(store);

    let queued = router
        .clone()
        .oneshot(request(
            Method::POST,
            format!("/api/accounts/{account_id}/sync"),
            Some(&cookie),
            Body::empty(),
        ))
        .await
        .unwrap();
    assert_eq!(queued.status(), StatusCode::OK);
    let mut synchronized = false;
    for _ in 0..100 {
        let messages = router
            .clone()
            .oneshot(request(
                Method::GET,
                "/api/messages?limit=10&offset=0",
                Some(&cookie),
                Body::empty(),
            ))
            .await
            .unwrap();
        let messages = json(messages).await;
        if messages["total"] == 1 {
            assert_eq!(messages["messages"][0]["subject"], "OAuth network inbound");
            synchronized = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(synchronized, "OAuth IMAP message was not committed");

    let key = MasterKey::from_file(directory.join("master.key")).unwrap();
    let store = SqliteAuthStore::open_database(&database).unwrap();
    let account = store.account(&owner.id, &account_id).unwrap().unwrap();
    let secret = imail_core::accounts::AccountSecretCodec::decrypt(
        &MasterKeyCredentialCodec::new(&key),
        &account.encrypted_secret,
    )
    .unwrap();
    assert_eq!(secret["accessToken"], "fresh-access-token");
    assert_eq!(secret["refreshToken"], "fixture-refresh-token");
    drop(store);

    let sent = router
        .clone()
        .oneshot(request(
            Method::POST,
            "/api/send",
            Some(&cookie),
            Body::from(format!(
                r#"{{"accountId":"{account_id}","to":["recipient@example.test"],"subject":"OAuth network outbound","text":"oauth outbound body"}}"#
            )),
        ))
        .await
        .unwrap();
    assert_eq!(sent.status(), StatusCode::CREATED);

    let report = runtime.shutdown(Duration::from_secs(5));
    assert!(report.graceful(), "timed out tasks: {:?}", report.timed_out);
    let oauth_requests = oauth_fixture.join().unwrap();
    assert_eq!(oauth_requests.len(), 3);
    assert!(oauth_requests[0].contains("grant_type=authorization_code"));
    assert!(oauth_requests[0].contains("code_verifier="));
    assert!(oauth_requests[0].contains("client_secret=fixture-client-secret"));
    assert!(oauth_requests[1].starts_with("GET /userinfo "));
    assert!(oauth_requests[1].contains("Authorization: Bearer expired-access-token"));
    assert!(oauth_requests[2].contains("grant_type=refresh_token"));
    assert!(oauth_requests[2].contains("refresh_token=fixture-refresh-token"));
    let imap_transcript = imap_fixture.join().unwrap();
    assert!(imap_transcript
        .iter()
        .any(|line| line.to_ascii_uppercase().contains("AUTHENTICATE XOAUTH2")));
    let smtp_messages = smtp_fixture.join().unwrap();
    assert_eq!(smtp_messages.len(), 1);
    let outbound = imail_mail::parse_rfc822(&smtp_messages[0]).unwrap();
    assert_eq!(outbound.subject, "OAuth network outbound");
    assert_eq!(outbound.text, "oauth outbound body");
    fs::remove_dir_all(directory).unwrap();
}
