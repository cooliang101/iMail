//! TLS trust roots and handshakes for mail servers and HTTPS proxies.
use crate::{error::NetworkError, TunnelStream};
use rustls_pki_types::ServerName;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::{client::TlsStream, TlsConnector};
pub(super) type SecureStream = TlsStream<TunnelStream>;

pub(super) async fn connect_tls(
    stream: TunnelStream,
    host: &str,
    tls_connector: &TlsConnector,
) -> Result<SecureStream, NetworkError> {
    tls_connector
        .connect(server_name(host)?, stream)
        .await
        .map_err(|error| NetworkError::Provider(format!("TLS 握手失败：{error}")))
}

pub(super) async fn connect_tls_tcp(
    stream: TcpStream,
    host: &str,
    tls_connector: &TlsConnector,
) -> Result<TlsStream<TcpStream>, NetworkError> {
    tls_connector
        .connect(server_name(host)?, stream)
        .await
        .map_err(|error| NetworkError::Provider(format!("代理 TLS 握手失败：{error}")))
}

pub(super) fn tls_connector() -> TlsConnector {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if let Ok(native) = rustls_native_certs::load_native_certs() {
        for certificate in native {
            let _ = roots.add(certificate);
        }
    }
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    TlsConnector::from(Arc::new(config))
}

fn server_name(host: &str) -> Result<ServerName<'static>, NetworkError> {
    ServerName::try_from(host.to_string())
        .map_err(|_| NetworkError::Provider("TLS 服务器名称无效".into()))
}
