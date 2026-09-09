//! Direct, SOCKS5 and HTTP CONNECT byte streams; independent of mail protocols.
use crate::{error::NetworkError, tls::connect_tls_tcp};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use imail_mail::MailProxy;
use std::{
    fmt, io,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf},
    net::TcpStream,
    time::timeout,
};
use tokio_rustls::{client::TlsStream, TlsConnector};
use tokio_socks::tcp::Socks5Stream;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_PROXY_RESPONSE_BYTES: usize = 16 * 1024;

pub(super) async fn connect_tunnel(
    proxy: Option<&MailProxy>,
    target_host: &str,
    target_port: u16,
    tls_connector: &TlsConnector,
) -> Result<TunnelStream, NetworkError> {
    timeout(CONNECT_TIMEOUT, async {
        match proxy {
            None => TcpStream::connect((target_host, target_port))
                .await
                .map(TunnelStream::Direct)
                .map_err(NetworkError::io),
            Some(proxy) if proxy.protocol == "socks5" => {
                let proxy_address = (proxy.host.as_str(), proxy.port);
                let target = (target_host, target_port);
                let stream = if let Some(username) = proxy.username.as_deref() {
                    Socks5Stream::connect_with_password(
                        proxy_address,
                        target,
                        username,
                        proxy.password.as_deref().unwrap_or_default(),
                    )
                    .await
                } else {
                    Socks5Stream::connect(proxy_address, target).await
                }
                .map_err(|error| NetworkError::Provider(format!("SOCKS5 代理连接失败：{error}")))?;
                Ok(TunnelStream::Socks(stream))
            }
            Some(proxy) if proxy.protocol == "http" || proxy.protocol == "https" => {
                let stream = TcpStream::connect((proxy.host.as_str(), proxy.port))
                    .await
                    .map_err(NetworkError::io)?;
                let mut stream = if proxy.protocol == "https" {
                    ProxyConnection::Tls(Box::new(
                        connect_tls_tcp(stream, &proxy.host, tls_connector).await?,
                    ))
                } else {
                    ProxyConnection::Plain(stream)
                };
                establish_http_connect(&mut stream, proxy, target_host, target_port).await?;
                Ok(match stream {
                    ProxyConnection::Plain(stream) => TunnelStream::Http(stream),
                    ProxyConnection::Tls(stream) => TunnelStream::Https(stream),
                })
            }
            Some(_) => Err(NetworkError::Provider("不支持的代理协议".into())),
        }
    })
    .await
    .map_err(|_| NetworkError::Provider("代理或服务器连接超时".into()))?
}

async fn establish_http_connect<T: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut T,
    proxy: &MailProxy,
    target_host: &str,
    target_port: u16,
) -> Result<(), NetworkError> {
    let authority = if target_host.contains(':') {
        format!("[{target_host}]:{target_port}")
    } else {
        format!("{target_host}:{target_port}")
    };
    let authorization = proxy.username.as_deref().map(|username| {
        let credentials = format!(
            "{}:{}",
            username,
            proxy.password.as_deref().unwrap_or_default()
        );
        format!(
            "Proxy-Authorization: Basic {}\r\n",
            BASE64.encode(credentials)
        )
    });
    let request = format!(
        "CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\nProxy-Connection: Keep-Alive\r\n{}\r\n",
        authorization.as_deref().unwrap_or_default()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(NetworkError::io)?;
    stream.flush().await.map_err(NetworkError::io)?;
    let mut response = Vec::new();
    let mut byte = [0_u8; 1];
    while response.len() < MAX_PROXY_RESPONSE_BYTES {
        stream
            .read_exact(&mut byte)
            .await
            .map_err(NetworkError::io)?;
        response.push(byte[0]);
        if response.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    if !response.ends_with(b"\r\n\r\n") {
        return Err(NetworkError::Provider("HTTP 代理响应过大或不完整".into()));
    }
    let status_line = String::from_utf8_lossy(&response)
        .lines()
        .next()
        .unwrap_or_default()
        .to_string();
    let accepted = status_line
        .split_whitespace()
        .nth(1)
        .is_some_and(|status| status == "200");
    if !accepted {
        return Err(NetworkError::Provider(format!(
            "HTTP 代理拒绝 CONNECT：{status_line}"
        )));
    }
    Ok(())
}

enum ProxyConnection {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl AsyncRead for ProxyConnection {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match &mut *self {
            Self::Plain(stream) => Pin::new(stream).poll_read(context, buffer),
            Self::Tls(stream) => Pin::new(stream).poll_read(context, buffer),
        }
    }
}

impl AsyncWrite for ProxyConnection {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        match &mut *self {
            Self::Plain(stream) => Pin::new(stream).poll_write(context, buffer),
            Self::Tls(stream) => Pin::new(stream).poll_write(context, buffer),
        }
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        match &mut *self {
            Self::Plain(stream) => Pin::new(stream).poll_flush(context),
            Self::Tls(stream) => Pin::new(stream).poll_flush(context),
        }
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        match &mut *self {
            Self::Plain(stream) => Pin::new(stream).poll_shutdown(context),
            Self::Tls(stream) => Pin::new(stream).poll_shutdown(context),
        }
    }
}

pub enum TunnelStream {
    Direct(TcpStream),
    Socks(Socks5Stream<TcpStream>),
    Http(TcpStream),
    Https(Box<TlsStream<TcpStream>>),
}

impl fmt::Debug for TunnelStream {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Direct(_) => "TunnelStream::Direct",
            Self::Socks(_) => "TunnelStream::Socks",
            Self::Http(_) => "TunnelStream::Http",
            Self::Https(_) => "TunnelStream::Https",
        })
    }
}

impl AsyncRead for TunnelStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match &mut *self {
            Self::Direct(stream) | Self::Http(stream) => {
                Pin::new(stream).poll_read(context, buffer)
            }
            Self::Socks(stream) => Pin::new(stream).poll_read(context, buffer),
            Self::Https(stream) => Pin::new(stream).poll_read(context, buffer),
        }
    }
}

impl AsyncWrite for TunnelStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        match &mut *self {
            Self::Direct(stream) | Self::Http(stream) => {
                Pin::new(stream).poll_write(context, buffer)
            }
            Self::Socks(stream) => Pin::new(stream).poll_write(context, buffer),
            Self::Https(stream) => Pin::new(stream).poll_write(context, buffer),
        }
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        match &mut *self {
            Self::Direct(stream) | Self::Http(stream) => Pin::new(stream).poll_flush(context),
            Self::Socks(stream) => Pin::new(stream).poll_flush(context),
            Self::Https(stream) => Pin::new(stream).poll_flush(context),
        }
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        match &mut *self {
            Self::Direct(stream) | Self::Http(stream) => Pin::new(stream).poll_shutdown(context),
            Self::Socks(stream) => Pin::new(stream).poll_shutdown(context),
            Self::Https(stream) => Pin::new(stream).poll_shutdown(context),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::runtime::Builder as RuntimeBuilder;
    #[test]
    fn http_connect_uses_basic_proxy_auth_without_exposing_it_in_errors() {
        let runtime = RuntimeBuilder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let proxy = MailProxy {
                protocol: "http".into(),
                host: "proxy.example.com".into(),
                port: 3128,
                username: Some("mail user".into()),
                password: Some("proxy-secret".into()),
            };
            let (mut client, mut server) = tokio::io::duplex(4_096);
            let server_task = tokio::spawn(async move {
                let mut request = Vec::new();
                let mut byte = [0_u8; 1];
                while !request.ends_with(b"\r\n\r\n") {
                    server.read_exact(&mut byte).await.unwrap();
                    request.push(byte[0]);
                }
                server
                    .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                    .await
                    .unwrap();
                String::from_utf8(request).unwrap()
            });

            establish_http_connect(&mut client, &proxy, "imap.example.com", 993)
                .await
                .unwrap();
            let request = server_task.await.unwrap();
            assert!(request.starts_with("CONNECT imap.example.com:993 HTTP/1.1\r\n"));
            assert!(request.contains("Proxy-Authorization: Basic bWFpbCB1c2VyOnByb3h5LXNlY3JldA=="));

            let (mut client, mut server) = tokio::io::duplex(4_096);
            let rejection = tokio::spawn(async move {
                let mut request = [0_u8; 256];
                let _ = server.read(&mut request).await.unwrap();
                server
                    .write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\n\r\n")
                    .await
                    .unwrap();
            });
            let error = establish_http_connect(&mut client, &proxy, "imap.example.com", 993)
                .await
                .unwrap_err();
            rejection.await.unwrap();
            let NetworkError::Provider(detail) = error;
            assert!(detail.contains("407"));
            assert!(!detail.contains("proxy-secret"));
            assert!(!detail.contains("bWFpbCB1c2Vy"));
        });
    }

    #[test]
    fn rejects_disconnected_and_oversized_http_proxy_responses() {
        let runtime = RuntimeBuilder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let proxy = MailProxy {
                protocol: "http".into(),
                host: "proxy.example.com".into(),
                port: 3128,
                username: None,
                password: None,
            };
            let (mut client, server) = tokio::io::duplex(128);
            drop(server);
            assert!(
                establish_http_connect(&mut client, &proxy, "imap.example.com", 993)
                    .await
                    .is_err()
            );

            let (mut client, mut server) = tokio::io::duplex(256);
            let server_task = tokio::spawn(async move {
                let mut request = [0_u8; 256];
                let _ = server.read(&mut request).await.unwrap();
                server
                    .write_all(&vec![b'x'; MAX_PROXY_RESPONSE_BYTES + 1])
                    .await
                    .unwrap();
            });
            let error = establish_http_connect(&mut client, &proxy, "imap.example.com", 993)
                .await
                .unwrap_err();
            server_task.await.unwrap();
            let NetworkError::Provider(detail) = error;
            assert_eq!(detail, "HTTP 代理响应过大或不完整");
        });
    }

    #[tokio::test]
    async fn ipv6_connect_preserves_the_first_tunneled_bytes() {
        let proxy = MailProxy {
            protocol: "http".into(),
            host: "unused.example".into(),
            port: 3128,
            username: None,
            password: None,
        };
        let (mut client, mut server) = tokio::io::duplex(1024);
        let peer = tokio::spawn(async move {
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(server.read_u8().await.unwrap());
            }
            // Proxy headers and the server greeting arrive in the same write.
            server
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n* OK ready\r\n")
                .await
                .unwrap();
            String::from_utf8(request).unwrap()
        });
        timeout(Duration::from_secs(5), async {
            establish_http_connect(&mut client, &proxy, "::1", 993)
                .await
                .unwrap();
            let mut greeting = [0; 12];
            client.read_exact(&mut greeting).await.unwrap();
            assert_eq!(&greeting, b"* OK ready\r\n");
            let request = peer.await.unwrap();
            assert!(request.starts_with("CONNECT [::1]:993 HTTP/1.1\r\nHost: [::1]:993\r\n"));
            assert!(!request.contains("Proxy-Authorization"));
        })
        .await
        .expect("local CONNECT fixture must complete");
    }

    #[tokio::test]
    async fn connect_accepts_the_header_size_limit_and_rejects_truncated_headers() {
        let proxy = MailProxy {
            protocol: "http".into(),
            host: "unused.example".into(),
            port: 3128,
            username: None,
            password: None,
        };
        let prefix = b"HTTP/1.1 200 OK\r\nX-Padding: ";
        let mut at_limit = prefix.to_vec();
        at_limit.resize(MAX_PROXY_RESPONSE_BYTES - 4, b'x');
        at_limit.extend_from_slice(b"\r\n\r\n");
        for (response, accepted) in [(at_limit, true), (b"HTTP/1.1 200 OK\r\n".to_vec(), false)] {
            let (mut client, mut server) = tokio::io::duplex(MAX_PROXY_RESPONSE_BYTES + 256);
            let peer = tokio::spawn(async move {
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    request.push(server.read_u8().await.unwrap());
                }
                server.write_all(&response).await.unwrap();
            });
            timeout(Duration::from_secs(5), async {
                let result = establish_http_connect(&mut client, &proxy, "localhost", 993).await;
                assert_eq!(result.is_ok(), accepted, "{result:?}");
                peer.await.unwrap();
            })
            .await
            .expect("local CONNECT fixture must complete");
        }
    }
}
