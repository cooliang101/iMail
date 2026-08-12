use std::{
    io::{self, Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    thread,
    time::{Duration, Instant},
};

use imail_oauth::{provider_error, OAuthCallback, OAuthError};
use url::Url;

const MAX_REQUEST_BYTES: usize = 16 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(25);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(2);

pub struct LoopbackCallbackServer {
    listener: TcpListener,
    redirect_uri: String,
    callback_path: String,
}

impl LoopbackCallbackServer {
    pub fn bind(redirect_uri: &str) -> Result<Self, OAuthError> {
        let mut url =
            Url::parse(redirect_uri).map_err(|_| provider_error("OAuth loopback 回调地址无效"))?;
        if url.scheme() != "http" {
            return Err(provider_error("OAuth loopback 回调必须使用 HTTP"));
        }
        let host = url
            .host_str()
            .ok_or_else(|| provider_error("OAuth loopback 回调缺少主机"))?;
        let ip = match host {
            "localhost" | "127.0.0.1" => IpAddr::V4(Ipv4Addr::LOCALHOST),
            "::1" | "[::1]" => IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
            _ => return Err(provider_error("OAuth loopback 回调只允许本机地址")),
        };
        let port = url
            .port_or_known_default()
            .ok_or_else(|| provider_error("OAuth loopback 回调缺少端口"))?;
        let listener = TcpListener::bind(SocketAddr::new(ip, port))
            .map_err(|_| provider_error("OAuth loopback 回调端口无法监听"))?;
        listener
            .set_nonblocking(true)
            .map_err(|_| provider_error("OAuth loopback 回调无法设置非阻塞模式"))?;
        let actual_port = listener
            .local_addr()
            .map_err(|_| provider_error("OAuth loopback 回调地址不可用"))?
            .port();
        url.set_port(Some(actual_port))
            .map_err(|_| provider_error("OAuth loopback 回调端口无效"))?;
        let callback_path = url.path().to_string();
        Ok(Self {
            listener,
            redirect_uri: url.into(),
            callback_path,
        })
    }

    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    pub fn wait(
        &self,
        expected_state: &str,
        maximum_wait: Duration,
    ) -> Result<OAuthCallback, OAuthError> {
        self.wait_until(expected_state, maximum_wait, || false)
    }

    pub fn wait_until<F>(
        &self,
        expected_state: &str,
        maximum_wait: Duration,
        mut cancelled: F,
    ) -> Result<OAuthCallback, OAuthError>
    where
        F: FnMut() -> bool,
    {
        let deadline = Instant::now() + maximum_wait;
        loop {
            if cancelled() {
                return Err(provider_error("OAuth loopback 回调等待已取消"));
            }
            if Instant::now() >= deadline {
                return Err(provider_error("OAuth loopback 回调等待超时"));
            }
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).map_err(io_error)?;
                    stream
                        .set_read_timeout(Some(CLIENT_TIMEOUT))
                        .map_err(io_error)?;
                    stream
                        .set_write_timeout(Some(CLIENT_TIMEOUT))
                        .map_err(io_error)?;
                    match read_callback(&mut stream, &self.callback_path, expected_state) {
                        Ok(callback) => {
                            write_response(&mut stream, 200, SUCCESS_HTML)?;
                            return Ok(callback);
                        }
                        Err(CallbackRequestError::Ignore) => {
                            write_response(&mut stream, 404, FAILURE_HTML)?;
                        }
                        Err(CallbackRequestError::Invalid) => {
                            write_response(&mut stream, 400, FAILURE_HTML)?;
                        }
                        Err(CallbackRequestError::Io(error)) => {
                            let _ = write_response(&mut stream, 400, FAILURE_HTML);
                            return Err(io_error(error));
                        }
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(POLL_INTERVAL);
                }
                Err(error) => return Err(io_error(error)),
            }
        }
    }
}

fn read_callback(
    stream: &mut TcpStream,
    callback_path: &str,
    expected_state: &str,
) -> Result<OAuthCallback, CallbackRequestError> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    while request.len() < MAX_REQUEST_BYTES {
        let count = stream.read(&mut buffer).map_err(CallbackRequestError::Io)?;
        if count == 0 {
            return Err(CallbackRequestError::Invalid);
        }
        request.extend_from_slice(&buffer[..count]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    if request.len() >= MAX_REQUEST_BYTES {
        return Err(CallbackRequestError::Invalid);
    }
    let request = std::str::from_utf8(&request).map_err(|_| CallbackRequestError::Invalid)?;
    let first_line = request
        .lines()
        .next()
        .ok_or(CallbackRequestError::Invalid)?;
    let mut parts = first_line.split_whitespace();
    if parts.next() != Some("GET") {
        return Err(CallbackRequestError::Invalid);
    }
    let target = parts.next().ok_or(CallbackRequestError::Invalid)?;
    let url = Url::parse(&format!("http://localhost{target}"))
        .map_err(|_| CallbackRequestError::Invalid)?;
    if url.path() != callback_path {
        return Err(CallbackRequestError::Ignore);
    }
    let query = url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<std::collections::HashMap<_, _>>();
    let state = query.get("state").ok_or(CallbackRequestError::Invalid)?;
    if state != expected_state {
        return Err(CallbackRequestError::Invalid);
    }
    let code = query.get("code").cloned();
    let error = query.get("error").cloned();
    if code.is_none() && error.is_none() {
        return Err(CallbackRequestError::Invalid);
    }
    Ok(OAuthCallback {
        state: state.clone(),
        code,
        error,
        error_description: query.get("error_description").cloned(),
    })
}

fn write_response(stream: &mut TcpStream, status: u16, body: &str) -> Result<(), OAuthError> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        _ => "Not Found",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).map_err(io_error)
}

fn io_error(_error: io::Error) -> OAuthError {
    provider_error("OAuth loopback 回调通信失败")
}

enum CallbackRequestError {
    Ignore,
    Invalid,
    Io(io::Error),
}

const SUCCESS_HTML: &str =
    "<!doctype html><meta charset=utf-8><title>iMail</title><p>授权已完成，可以关闭此窗口。</p>";
const FAILURE_HTML: &str = "<!doctype html><meta charset=utf-8><title>iMail</title><p>授权回调无效，请返回 iMail 重试。</p>";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_the_expected_loopback_state_and_path() {
        let server = LoopbackCallbackServer::bind("http://127.0.0.1:0/oauth/callback").unwrap();
        let redirect = server.redirect_uri().to_string();
        let client = thread::spawn(move || {
            send(&redirect, "/wrong?state=state-1&code=ignored");
            send(&redirect, "/oauth/callback?state=wrong&code=ignored");
            send(&redirect, "/oauth/callback?state=state-1&code=code-1");
        });

        let callback = server.wait("state-1", Duration::from_secs(2)).unwrap();
        client.join().unwrap();
        assert_eq!(callback.code.as_deref(), Some("code-1"));
        assert_eq!(callback.state, "state-1");
    }

    #[test]
    fn rejects_non_loopback_and_https_bindings() {
        assert!(LoopbackCallbackServer::bind("http://example.com:8787/callback").is_err());
        assert!(LoopbackCallbackServer::bind("https://127.0.0.1:8787/callback").is_err());
    }

    #[test]
    fn allows_the_owner_to_cancel_the_transient_listener() {
        let server = LoopbackCallbackServer::bind("http://127.0.0.1:0/oauth/callback").unwrap();
        let mut checks = 0;
        let error = server
            .wait_until("state-1", Duration::from_secs(2), || {
                checks += 1;
                checks > 1
            })
            .unwrap_err();
        assert!(error.to_string().contains("取消"));
    }

    fn send(redirect: &str, target: &str) {
        let url = Url::parse(redirect).unwrap();
        let mut stream =
            TcpStream::connect((url.host_str().unwrap(), url.port().unwrap())).unwrap();
        write!(
            stream,
            "GET {target} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
    }
}
