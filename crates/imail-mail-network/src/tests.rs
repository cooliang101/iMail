//! Local TLS protocol fixtures and adapter cancellation/timeout contracts.
use imail_mail::{
    ImapPort, ImapSyncPort, MailAuthentication, MailConnectionConfig, RemoteMessageLocator,
    RemoteSyncRequest, SmtpPort,
};

use super::*;
use imail_mail::OutgoingMessage;
use imail_protocol::MailAddressView;
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::{
    net::TcpListener as StdTcpListener,
    thread::{self, JoinHandle},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};
use tokio_rustls::{server::TlsStream as ServerTlsStream, TlsAcceptor};

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
        RuntimeBuilder::new_current_thread()
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
                    transcript.extend(serve_imap_connection(stream, &source).await);
                }
                transcript
            })
    })
}

async fn serve_imap_connection(
    mut stream: ServerTlsStream<TcpStream>,
    source: &[u8],
) -> Vec<String> {
    stream
        .write_all(b"* OK iMail fixture ready\r\n")
        .await
        .unwrap();
    let (read, mut write) = tokio::io::split(stream);
    let mut lines = BufReader::new(read).lines();
    let mut transcript = Vec::new();
    while let Some(line) = lines.next_line().await.unwrap() {
        transcript.push(line.clone());
        let tag = line.split_whitespace().next().unwrap_or("A0");
        let command = line.to_ascii_uppercase();
        let response = if command.contains(" LOGIN ") {
            format!("{tag} OK LOGIN completed\r\n").into_bytes()
        } else if command.contains(" CAPABILITY") {
            format!("* CAPABILITY IMAP4rev1 SPECIAL-USE\r\n{tag} OK CAPABILITY completed\r\n")
                .into_bytes()
        } else if command.contains(" LIST ") {
            format!("* LIST (\\Inbox) \"/\" \"INBOX\"\r\n{tag} OK LIST completed\r\n").into_bytes()
        } else if command.contains(" STATUS ") {
            format!(
                "* STATUS \"INBOX\" (MESSAGES 1 UNSEEN 1 UIDNEXT 2 UIDVALIDITY 7)\r\n{tag} OK STATUS completed\r\n"
            )
            .into_bytes()
        } else if command.contains(" EXAMINE ") || command.contains(" SELECT ") {
            format!(
                "* FLAGS (\\Seen \\Flagged)\r\n* 1 EXISTS\r\n* OK [UIDVALIDITY 7] UIDs valid\r\n* OK [UIDNEXT 2] next UID\r\n{tag} OK [READ-ONLY] mailbox selected\r\n"
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

fn spawn_icloud_fallback_fixture(
    listener: StdTcpListener,
    tls: Arc<rustls::ServerConfig>,
    source: Vec<u8>,
) -> JoinHandle<Vec<String>> {
    thread::spawn(move || {
        RuntimeBuilder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                let listener = TcpListener::from_std(listener).unwrap();
                let acceptor = TlsAcceptor::from(tls);
                let (stream, _) = listener.accept().await.unwrap();
                let mut stream = acceptor.accept(stream).await.unwrap();
                stream.write_all(b"* OK iCloud fixture ready\r\n").await.unwrap();
                let (read, mut write) = tokio::io::split(stream);
                let mut lines = BufReader::new(read).lines();
                let mut transcript = Vec::new();
                while let Some(line) = lines.next_line().await.unwrap() {
                    transcript.push(line.clone());
                    let tag = line.split_whitespace().next().unwrap_or("A0");
                    let command = line.to_ascii_uppercase();
                    let response = if command.contains(" LOGIN ") {
                        format!("{tag} OK LOGIN completed\r\n").into_bytes()
                    } else if command.contains(" CAPABILITY") {
                        format!("* CAPABILITY IMAP4rev1\r\n{tag} OK CAPABILITY completed\r\n")
                            .into_bytes()
                    } else if command.contains(" EXAMINE ") {
                        format!("* 1 EXISTS\r\n* OK [UIDVALIDITY 8] UIDs valid\r\n* OK [UIDNEXT 2] next UID\r\n{tag} OK [READ-ONLY] selected\r\n").into_bytes()
                    } else if command.contains("UID SEARCH HEADER MESSAGE-ID") {
                        format!("* SEARCH\r\n{tag} OK SEARCH completed\r\n").into_bytes()
                    } else if command.contains("UID SEARCH ALL") {
                        format!("* SEARCH 1\r\n{tag} OK SEARCH completed\r\n").into_bytes()
                    } else if command.contains("UID FETCH 1 ")
                        && command.contains("HEADER.FIELDS")
                    {
                        let header = b"Message-ID: <icloud-fallback@example.test>\r\n\r\n";
                        let mut response = format!(
                            "* 1 FETCH (UID 1 BODY[HEADER.FIELDS (MESSAGE-ID)] {{{}}}\r\n",
                            header.len()
                        )
                        .into_bytes();
                        response.extend_from_slice(header);
                        response.extend_from_slice(
                            format!(")\r\n{tag} OK FETCH completed\r\n").as_bytes(),
                        );
                        response
                    } else if command.contains("UID FETCH 1 ")
                        && command.contains("(UID BODY.PEEK[])")
                    {
                        let mut response = format!(
                            "* 1 FETCH (UID 1 BODY[] {{{}}}\r\n",
                            source.len()
                        )
                        .into_bytes();
                        response.extend_from_slice(&source);
                        response.extend_from_slice(
                            format!(")\r\n{tag} OK FETCH completed\r\n").as_bytes(),
                        );
                        response
                    } else if command.contains("UID FETCH 1 ")
                        || command.contains("UID FETCH 99 ")
                    {
                        format!("{tag} OK FETCH completed\r\n").into_bytes()
                    } else if command.contains(" LOGOUT") {
                        let response = format!(
                            "* BYE fixture closing\r\n{tag} OK LOGOUT completed\r\n"
                        )
                        .into_bytes();
                        write.write_all(&response).await.unwrap();
                        break;
                    } else {
                        format!("{tag} BAD unsupported fixture command\r\n").into_bytes()
                    };
                    write.write_all(&response).await.unwrap();
                }
                transcript
            })
    })
}

struct SmtpFixtureMessage {
    source: Vec<u8>,
    recipients: Vec<String>,
}

fn spawn_smtp_fixture(
    listener: StdTcpListener,
    tls: Arc<rustls::ServerConfig>,
    connections: usize,
) -> JoinHandle<Vec<SmtpFixtureMessage>> {
    thread::spawn(move || {
        RuntimeBuilder::new_current_thread()
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
                    if let Some(message) = serve_smtp_connection(stream).await {
                        messages.push(message);
                    }
                }
                messages
            })
    })
}

async fn serve_smtp_connection(
    mut stream: ServerTlsStream<TcpStream>,
) -> Option<SmtpFixtureMessage> {
    stream
        .write_all(b"220 localhost iMail fixture\r\n")
        .await
        .unwrap();
    let (read, mut write) = tokio::io::split(stream);
    let mut lines = BufReader::new(read).lines();
    let mut login_step = 0_u8;
    let mut data: Option<Vec<u8>> = None;
    let mut collecting_data = false;
    let mut recipients = Vec::new();
    while let Some(line) = lines.next_line().await.unwrap() {
        let upper = line.to_ascii_uppercase();
        if collecting_data {
            if line == "." {
                collecting_data = false;
                write.write_all(b"250 2.0.0 queued\r\n").await.unwrap();
            } else {
                let payload = data.as_mut().unwrap();
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
            if upper.starts_with("RCPT TO:") {
                recipients.push(line.clone());
            }
            write.write_all(b"250 2.1.0 accepted\r\n").await.unwrap();
        } else if upper == "DATA" {
            data = Some(Vec::new());
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
    data.map(|source| SmtpFixtureMessage { source, recipients })
}

fn fixture_config(imap_port: u16, smtp_port: u16) -> MailConnectionConfig {
    MailConnectionConfig {
        email: "fixture@example.test".into(),
        display_name: "Fixture Owner".into(),
        imap_host: "localhost".into(),
        imap_port,
        imap_secure: true,
        smtp_host: "localhost".into(),
        smtp_port,
        smtp_secure: true,
        authentication: MailAuthentication::Password("fixture-password".into()),
        proxy: None,
    }
}

#[test]
fn real_tls_imap_and_smtp_fixture_covers_verify_sync_mime_and_send() {
    let (tls_connector, tls_server) = fixture_tls();
    let (imap_listener, imap_port) = fixture_listener();
    let (smtp_listener, smtp_port) = fixture_listener();
    let source = concat!(
        "From: Sender <sender@example.test>\r\n",
        "To: Fixture <fixture@example.test>\r\n",
        "Subject: fixture inbound\r\n",
        "Message-ID: <fixture-inbound@example.test>\r\n",
        "Content-Type: text/plain; charset=utf-8\r\n",
        "\r\n",
        "fixture body\r\n"
    )
    .as_bytes()
    .to_vec();
    let imap = spawn_imap_fixture(imap_listener, Arc::clone(&tls_server), 2, source);
    let smtp = spawn_smtp_fixture(smtp_listener, tls_server, 2);
    let config = fixture_config(imap_port, smtp_port);
    let mut adapter = NetworkMailAdapter::with_tls_connector(tls_connector).unwrap();

    ImapPort::verify(&mut adapter, &config).unwrap();
    let batch = ImapSyncPort::fetch_incremental(
        &mut adapter,
        &config,
        &RemoteSyncRequest {
            target: imail_mail::SyncTarget {
                mailbox_role: "inbox".into(),
                requested_mailbox: None,
            },
            cursor: Default::default(),
            cached_uids: vec![],
        },
    )
    .unwrap();
    assert_eq!(batch.uid_validity.as_deref(), Some("7"));
    assert_eq!(batch.last_seen_uid, 1);
    assert_eq!(batch.incoming.len(), 1);
    let parsed = imail_mail::parse_rfc822(&batch.incoming[0].source).unwrap();
    assert_eq!(parsed.subject, "fixture inbound");
    assert_eq!(parsed.text, "fixture body");

    SmtpPort::verify(&mut adapter, &config).unwrap();
    let sent = SmtpPort::send(
        &mut adapter,
        &config,
        &OutgoingMessage {
            envelope: imail_protocol::ComposeEnvelope {
                bcc: vec![
                    "hidden@example.test".into(),
                    "RECIPIENT@example.test".into(),
                ],
                reply: imail_protocol::ReplyHeaders {
                    in_reply_to: vec!["<parent@example.test>".into()],
                    references: vec!["<root@example.test>".into(), "<parent@example.test>".into()],
                },
            },
            from: MailAddressView {
                name: "Fixture Owner".into(),
                address: "fixture@example.test".into(),
            },
            to: vec!["recipient@example.test".into()],
            cc: None,
            subject: "fixture outbound".into(),
            text: "fixture send body".into(),
            html: None,
            attachments: vec![],
        },
    )
    .unwrap();
    assert_eq!(
        sent.accepted,
        vec!["recipient@example.test", "hidden@example.test"]
    );

    let imap_transcript = imap.join().unwrap();
    assert!(imap_transcript
        .iter()
        .any(|line| line.to_ascii_uppercase().contains(" LOGIN ")));
    assert!(imap_transcript
        .iter()
        .any(|line| line.to_ascii_uppercase().contains(" FETCH ")));
    let smtp_messages = smtp.join().unwrap();
    assert_eq!(smtp_messages.len(), 1);
    let outbound = imail_mail::parse_rfc822(&smtp_messages[0].source).unwrap();
    assert_eq!(outbound.subject, "fixture outbound");
    assert_eq!(outbound.text, "fixture send body");
    assert_eq!(
        outbound.headers.reply.in_reply_to,
        ["<parent@example.test>"]
    );
    assert_eq!(
        outbound.headers.reply.references,
        ["<root@example.test>", "<parent@example.test>"]
    );
    let delivered = String::from_utf8_lossy(&smtp_messages[0].source).to_ascii_lowercase();
    assert!(!delivered.contains("bcc:"));
    assert!(!delivered.contains("hidden@example.test"));
    let recipients = &smtp_messages[0].recipients;
    assert_eq!(recipients.len(), 2);
    assert!(recipients
        .iter()
        .any(|line| line.contains("hidden@example.test")));
}

#[test]
fn icloud_fallback_matches_headers_locally_when_uid_and_server_search_are_stale() {
    let (tls_connector, tls_server) = fixture_tls();
    let (imap_listener, imap_port) = fixture_listener();
    let source = concat!(
        "From: Sender <sender@example.test>\r\n",
        "To: Fixture <fixture@example.test>\r\n",
        "Subject: iCloud attachment\r\n",
        "Message-ID: <icloud-fallback@example.test>\r\n",
        "Content-Type: text/plain; charset=utf-8\r\n",
        "\r\n",
        "attachment source\r\n"
    )
    .as_bytes()
    .to_vec();
    let fixture = spawn_icloud_fallback_fixture(imap_listener, tls_server, source.clone());
    let mut adapter = NetworkMailAdapter::with_tls_connector(tls_connector).unwrap();
    let fetched = ImapPort::fetch_source(
        &mut adapter,
        &fixture_config(imap_port, 465),
        &RemoteMessageLocator {
            mailbox: "INBOX".into(),
            uid: 99,
            message_id: Some("<icloud-fallback@example.test>".into()),
        },
    )
    .unwrap();
    assert_eq!(fetched, source);
    let transcript = fixture.join().unwrap().join("\n").to_ascii_uppercase();
    assert!(transcript.contains("UID SEARCH HEADER MESSAGE-ID"));
    assert!(transcript.contains("UID SEARCH ALL"));
    assert!(transcript.contains("HEADER.FIELDS (MESSAGE-ID)"));
}

#[test]
fn adapter_timeout_is_stable_and_does_not_wait_for_the_real_network_limit() {
    let adapter = NetworkMailAdapter::new().unwrap();
    let failure = adapter
        .run_with_timeout::<()>(
            ProtocolStage::Imap,
            Duration::from_millis(1),
            std::future::pending(),
        )
        .unwrap_err();
    assert_eq!(failure.status.as_deref(), Some("TIMEOUT"));
    assert_eq!(failure.message, "IMAP 验证失败 (TIMEOUT)：网络操作超时");
}

struct Cancelled;

impl NetworkCancellation for Cancelled {
    fn is_cancelled(&self) -> bool {
        true
    }
}

#[test]
fn injected_cancellation_interrupts_an_in_flight_network_future() {
    let adapter = NetworkMailAdapter::with_cancellation(Arc::new(Cancelled)).unwrap();
    let started = std::time::Instant::now();
    let failure = adapter
        .run_with_timeout::<()>(
            ProtocolStage::Imap,
            Duration::from_secs(120),
            std::future::pending(),
        )
        .unwrap_err();
    assert_eq!(failure.status.as_deref(), Some("CANCELLED"));
    assert!(started.elapsed() < Duration::from_millis(250));
}

#[test]
fn cancellation_after_operation_start_drops_the_in_flight_future() {
    use std::sync::atomic::{AtomicBool, Ordering};
    struct Signal(AtomicBool);
    impl NetworkCancellation for Signal {
        fn is_cancelled(&self) -> bool {
            self.0.load(Ordering::SeqCst)
        }
    }
    struct Dropped(Arc<AtomicBool>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let signal = Arc::new(Signal(AtomicBool::new(false)));
    let dropped = Arc::new(AtomicBool::new(false));
    let adapter = NetworkMailAdapter::with_cancellation(signal.clone()).unwrap();
    let failure = adapter
        .run_with_timeout::<()>(ProtocolStage::Smtp, Duration::from_secs(5), async {
            let _guard = Dropped(dropped.clone());
            // Flip only when the operation is polled: cancellation must work after startup.
            signal.0.store(true, Ordering::SeqCst);
            std::future::pending().await
        })
        .unwrap_err();
    assert_eq!(failure.stage, ProtocolStage::Smtp);
    assert_eq!(failure.status.as_deref(), Some("CANCELLED"));
    assert!(dropped.load(Ordering::SeqCst));
}
