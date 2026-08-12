use futures_util::StreamExt;
use reqwest::{Client, Method, Url};
use reqwest_cookie_store::{CookieStore, CookieStoreMutex};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    io::{BufReader, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::{AppHandle, Emitter, State};

#[derive(Clone)]
struct ServiceClient {
    client: Client,
    cookies: Arc<CookieStoreMutex>,
    cookie_path: Arc<PathBuf>,
    persist_lock: Arc<Mutex<()>>,
}

pub struct HttpBridgeState {
    clients: Mutex<HashMap<String, ServiceClient>>,
    cookie_root: PathBuf,
    event_task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

impl HttpBridgeState {
    pub fn new(cookie_root: PathBuf) -> Self {
        Self {
            clients: Mutex::new(HashMap::new()),
            cookie_root,
            event_task: Mutex::new(None),
        }
    }

    fn client(&self, base_url: &str) -> Result<(String, ServiceClient), String> {
        let normalized = normalize_base_url(base_url)?;
        let mut clients = self
            .clients
            .lock()
            .map_err(|_| "桌面网络状态不可用".to_string())?;
        let client = match clients.get(&normalized) {
            Some(client) => client.clone(),
            None => {
                let cookie_path = self.cookie_root.join(cookie_file_name(&normalized));
                let cookies = Arc::new(CookieStoreMutex::new(load_cookie_store(&cookie_path)));
                let client = Client::builder()
                    .cookie_provider(cookies.clone())
                    .redirect(reqwest::redirect::Policy::none())
                    .connect_timeout(Duration::from_secs(8))
                    .timeout(Duration::from_secs(120))
                    .build()
                    .map_err(|error| format!("桌面网络客户端初始化失败：{error}"))?;
                let client = ServiceClient {
                    client,
                    cookies,
                    cookie_path: Arc::new(cookie_path),
                    persist_lock: Arc::new(Mutex::new(())),
                };
                clients.insert(normalized.clone(), client.clone());
                client
            }
        };
        Ok((normalized, client))
    }

    fn replace_event_task(
        &self,
        task: Option<tauri::async_runtime::JoinHandle<()>>,
    ) -> Result<(), String> {
        let mut current = self
            .event_task
            .lock()
            .map_err(|_| "桌面事件状态不可用".to_string())?;
        if let Some(previous) = current.take() {
            previous.abort();
        }
        *current = task;
        Ok(())
    }
}

fn cookie_file_name(base_url: &str) -> String {
    let digest = Sha256::digest(base_url.as_bytes());
    format!("{digest:x}.json")
}

fn load_cookie_store(path: &Path) -> CookieStore {
    let Ok(file) = fs::File::open(path) else {
        return CookieStore::default();
    };
    cookie_store::serde::json::load(BufReader::new(file)).unwrap_or_default()
}

pub(crate) fn persisted_session_token(
    cookie_root: &Path,
    service_base: &str,
) -> Result<Option<String>, String> {
    let normalized = normalize_base_url(service_base)?;
    let url = Url::parse(&normalized).map_err(|_| "本地服务地址无效".to_string())?;
    let store = load_cookie_store(&cookie_root.join(cookie_file_name(&normalized)));
    let session = store
        .get_request_values(&url)
        .find(|(name, value)| *name == "imail_session" && !value.is_empty())
        .map(|(_, value)| value.to_string());
    Ok(session)
}

fn write_private_file(path: &Path, contents: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "会话存储目录无效".to_string())?;
    fs::create_dir_all(parent).map_err(|error| format!("创建会话目录失败：{error}"))?;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut options = fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| format!("写入会话文件失败：{error}"))?;
    file.write_all(contents)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("写入会话文件失败：{error}"))?;
    #[cfg(windows)]
    if path.exists() {
        fs::remove_file(path).map_err(|error| format!("更新会话文件失败：{error}"))?;
    }
    fs::rename(&temporary, path).map_err(|error| format!("更新会话文件失败：{error}"))
}

impl ServiceClient {
    fn persist_cookies(&self) -> Result<(), String> {
        let _persist_guard = self
            .persist_lock
            .lock()
            .map_err(|_| "会话存储状态不可用".to_string())?;
        let mut contents = Vec::new();
        let store = self
            .cookies
            .lock()
            .map_err(|_| "会话 Cookie 状态不可用".to_string())?;
        cookie_store::serde::json::save(&store, &mut contents)
            .map_err(|error| format!("序列化会话失败：{error}"))?;
        drop(store);
        write_private_file(&self.cookie_path, &contents)
    }

    fn persist_cookies_best_effort(&self) {
        if let Err(error) = self.persist_cookies() {
            log::warn!("{error}");
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpRequest {
    base_url: String,
    path: String,
    method: Option<String>,
    body: Option<String>,
    timeout_ms: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpResponse {
    status: u16,
    body: String,
}

#[derive(Clone, Serialize)]
struct SyncEvent {
    event: String,
    data: String,
}

fn normalize_base_url(value: &str) -> Result<String, String> {
    let mut url = Url::parse(value.trim()).map_err(|_| "服务地址无效".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("服务地址只支持 HTTP 或 HTTPS".into());
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("服务地址不能包含凭据、查询参数或片段".into());
    }
    if url.scheme() == "http" {
        let host = url
            .host_str()
            .ok_or_else(|| "服务地址缺少主机名".to_string())?;
        let normalized_host = host.trim_start_matches('[').trim_end_matches(']');
        let loopback = normalized_host.eq_ignore_ascii_case("localhost")
            || normalized_host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback());
        if !loopback {
            return Err("远程服务必须使用 HTTPS；HTTP 仅允许本机回环地址".into());
        }
    }
    let path = url.path().trim_end_matches('/').to_string();
    url.set_path(&path);
    Ok(url.to_string().trim_end_matches('/').to_string())
}

fn request_url(base_url: &str, path: &str) -> Result<String, String> {
    if !path.starts_with('/') || path.starts_with("//") {
        return Err("服务请求路径无效".into());
    }
    let target = format!("{base_url}{path}");
    Url::parse(&target).map_err(|_| "服务请求地址无效".to_string())?;
    Ok(target)
}

fn request_method(value: Option<&str>) -> Result<Method, String> {
    match value.unwrap_or("GET").to_ascii_uppercase().as_str() {
        "GET" => Ok(Method::GET),
        "POST" => Ok(Method::POST),
        "PUT" => Ok(Method::PUT),
        "PATCH" => Ok(Method::PATCH),
        "DELETE" => Ok(Method::DELETE),
        _ => Err("桌面端不允许该请求方法".into()),
    }
}

#[tauri::command]
pub async fn desktop_http_request(
    state: State<'_, HttpBridgeState>,
    request: HttpRequest,
) -> Result<HttpResponse, String> {
    let (base_url, client) = state.client(&request.base_url)?;
    let method = request_method(request.method.as_deref())?;
    let url = request_url(&base_url, &request.path)?;
    let timeout = Duration::from_millis(request.timeout_ms.unwrap_or(30_000).clamp(1_000, 120_000));
    let mut builder = client
        .client
        .request(method, url)
        .header("Accept", "application/json")
        .timeout(timeout);
    if let Some(body) = request.body {
        builder = builder
            .header("Content-Type", "application/json")
            .body(body);
    }
    let response = builder
        .send()
        .await
        .map_err(|error| format!("无法连接服务：{error}"))?;
    client.persist_cookies_best_effort();
    let status = response.status().as_u16();
    let body = response
        .text()
        .await
        .map_err(|error| format!("读取服务响应失败：{error}"))?;
    Ok(HttpResponse { status, body })
}

#[tauri::command]
pub async fn desktop_download(
    state: State<'_, HttpBridgeState>,
    base_url: String,
    path: String,
    target: PathBuf,
) -> Result<(), String> {
    let (base_url, client) = state.client(&base_url)?;
    let response = client
        .client
        .get(request_url(&base_url, &path)?)
        .send()
        .await
        .map_err(|error| format!("附件下载失败：{error}"))?;
    client.persist_cookies_best_effort();
    if !response.status().is_success() {
        return Err(format!("附件下载失败：{}", response.status().as_u16()));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("读取附件失败：{error}"))?;
    std::fs::write(target, bytes).map_err(|error| format!("保存附件失败：{error}"))
}

#[tauri::command]
pub async fn desktop_read_binary(
    state: State<'_, HttpBridgeState>,
    base_url: String,
    path: String,
) -> Result<tauri::ipc::Response, String> {
    let (base_url, client) = state.client(&base_url)?;
    let response = client
        .client
        .get(request_url(&base_url, &path)?)
        .send()
        .await
        .map_err(|error| format!("二进制资源加载失败：{error}"))?;
    client.persist_cookies_best_effort();
    if !response.status().is_success() {
        return Err(format!(
            "二进制资源加载失败：{}",
            response.status().as_u16()
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("读取二进制资源失败：{error}"))?;
    Ok(tauri::ipc::Response::new(bytes.to_vec()))
}

fn take_sse_block(buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
    let (index, width) = buffer
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .map(|index| (index, 4))
        .or_else(|| {
            buffer
                .windows(2)
                .position(|part| part == b"\n\n")
                .map(|index| (index, 2))
        })?;
    let block = buffer[..index].to_vec();
    buffer.drain(..index + width);
    Some(block)
}

fn parse_sse_block(block: &[u8]) -> Option<(SyncEvent, Option<String>)> {
    let text = String::from_utf8_lossy(block).replace("\r\n", "\n");
    let mut event = "message".to_string();
    let mut data = Vec::new();
    let mut id = None;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("event:") {
            event = value.trim().to_string();
        } else if let Some(value) = line.strip_prefix("data:") {
            data.push(value.trim_start().to_string());
        } else if let Some(value) = line.strip_prefix("id:") {
            id = Some(value.trim().to_string());
        }
    }
    if data.is_empty() {
        None
    } else {
        Some((
            SyncEvent {
                event,
                data: data.join("\n"),
            },
            id,
        ))
    }
}

#[tauri::command]
pub async fn desktop_start_events(
    app: AppHandle,
    state: State<'_, HttpBridgeState>,
    base_url: String,
) -> Result<(), String> {
    let (base_url, client) = state.client(&base_url)?;
    let task = tauri::async_runtime::spawn(async move {
        let mut cursor: Option<String> = None;
        loop {
            let suffix = cursor
                .as_ref()
                .map(|id| format!("?after={id}"))
                .unwrap_or_default();
            let url = match request_url(&base_url, &format!("/api/events{suffix}")) {
                Ok(url) => url,
                Err(_) => return,
            };
            if let Ok(response) = client
                .client
                .get(url)
                .header("Accept", "text/event-stream")
                .send()
                .await
            {
                client.persist_cookies_best_effort();
                if response.status().is_success() {
                    let mut stream = response.bytes_stream();
                    let mut buffer = Vec::new();
                    while let Some(chunk) = stream.next().await {
                        let Ok(chunk) = chunk else { break };
                        buffer.extend_from_slice(&chunk);
                        while let Some(block) = take_sse_block(&mut buffer) {
                            if let Some((event, id)) = parse_sse_block(&block) {
                                if let Some(id) = id {
                                    cursor = Some(id);
                                }
                                let _ = app.emit("imail-sync-event", event);
                            }
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
    state.replace_event_task(Some(task))
}

#[tauri::command]
pub fn desktop_stop_events(state: State<'_, HttpBridgeState>) -> Result<(), String> {
    state.replace_event_task(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_service_urls_and_request_paths() {
        assert_eq!(
            normalize_base_url(" http://127.0.0.1:8787/ ").unwrap(),
            "http://127.0.0.1:8787"
        );
        assert_eq!(
            request_url("https://mail.example.com/root", "/api/auth/status").unwrap(),
            "https://mail.example.com/root/api/auth/status"
        );
        assert!(normalize_base_url("file:///tmp/imail").is_err());
        assert!(normalize_base_url("https://user:secret@mail.example.com").is_err());
        assert!(normalize_base_url("http://192.168.1.20:8787").is_err());
        assert!(normalize_base_url("http://mail.example.com").is_err());
        assert_eq!(
            normalize_base_url("http://[::1]:8787").unwrap(),
            "http://[::1]:8787"
        );
        assert_eq!(
            normalize_base_url("https://192.168.1.20:8787").unwrap(),
            "https://192.168.1.20:8787"
        );
        assert!(request_url("https://mail.example.com", "//evil.example.com").is_err());
        assert!(request_method(Some("CONNECT")).is_err());
    }

    #[test]
    fn does_not_follow_service_redirects() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                let address = listener.local_addr().unwrap();
                let server = std::thread::spawn(move || {
                    let (mut stream, _) = listener.accept().unwrap();
                    let mut request = [0_u8; 2048];
                    let _ = stream.read(&mut request).unwrap();
                    stream
                        .write_all(
                            b"HTTP/1.1 307 Temporary Redirect\r\nLocation: http://127.0.0.1:9/plaintext\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .unwrap();
                });
                let root = std::env::temp_dir().join(format!(
                    "imail-http-redirect-test-{}",
                    uuid::Uuid::new_v4()
                ));
                let state = HttpBridgeState::new(root.clone());
                let (base_url, client) = state.client(&format!("http://{address}")).unwrap();
                let response = client
                    .client
                    .post(request_url(&base_url, "/api/auth/login").unwrap())
                    .body("must-not-be-forwarded")
                    .send()
                    .await
                    .unwrap();
                assert_eq!(response.status().as_u16(), 307);
                server.join().unwrap();
                let _ = fs::remove_dir_all(root);
            });
    }

    #[test]
    fn parses_complete_sse_blocks_and_preserves_cursor() {
        let mut buffer = b"id: 42\nevent: sync.completed\ndata: {\"ok\":true}\n\npartial".to_vec();
        let block = take_sse_block(&mut buffer).unwrap();
        let (event, cursor) = parse_sse_block(&block).unwrap();
        assert_eq!(event.event, "sync.completed");
        assert_eq!(event.data, "{\"ok\":true}");
        assert_eq!(cursor.as_deref(), Some("42"));
        assert_eq!(buffer, b"partial");
    }

    #[test]
    fn isolates_cookie_clients_by_normalized_service_base() {
        let root =
            std::env::temp_dir().join(format!("imail-http-bridge-test-{}", uuid::Uuid::new_v4()));
        let state = HttpBridgeState::new(root.clone());
        state.client("https://mail-a.example.test/").unwrap();
        state.client("https://mail-a.example.test").unwrap();
        assert_eq!(state.clients.lock().unwrap().len(), 1);
        state.client("https://mail-b.example.test").unwrap();
        assert_eq!(state.clients.lock().unwrap().len(), 2);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn persists_sessions_across_bridge_restarts_and_keeps_services_isolated() {
        let root =
            std::env::temp_dir().join(format!("imail-http-session-test-{}", uuid::Uuid::new_v4()));
        let service_a = "https://mail-a.example.test";
        let service_b = "https://mail-b.example.test";
        let url_a = Url::parse(service_a).unwrap();

        let first_state = HttpBridgeState::new(root.clone());
        let (_, first_client) = first_state.client(service_a).unwrap();
        first_client
            .cookies
            .lock()
            .unwrap()
            .parse(
                "imail_session=secret-a; Path=/; Max-Age=2592000; HttpOnly; Secure; SameSite=Lax",
                &url_a,
            )
            .unwrap();
        first_client.persist_cookies().unwrap();
        drop(first_state);
        assert_eq!(
            persisted_session_token(&root, service_a)
                .unwrap()
                .as_deref(),
            Some("secret-a")
        );

        let restarted_state = HttpBridgeState::new(root.clone());
        let (_, restored_client) = restarted_state.client(service_a).unwrap();
        let restored =
            reqwest::cookie::CookieStore::cookies(restored_client.cookies.as_ref(), &url_a)
                .unwrap();
        assert_eq!(restored.to_str().unwrap(), "imail_session=secret-a");

        let (_, isolated_client) = restarted_state.client(service_b).unwrap();
        assert!(
            reqwest::cookie::CookieStore::cookies(isolated_client.cookies.as_ref(), &url_a)
                .is_none()
        );
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);

        restored_client
            .cookies
            .lock()
            .unwrap()
            .parse(
                "imail_session=; Path=/; Max-Age=0; HttpOnly; Secure; SameSite=Lax",
                &url_a,
            )
            .unwrap();
        restored_client.persist_cookies().unwrap();
        drop(restarted_state);
        assert!(persisted_session_token(&root, service_a).unwrap().is_none());

        let logged_out_state = HttpBridgeState::new(root.clone());
        let (_, logged_out_client) = logged_out_state.client(service_a).unwrap();
        assert!(
            reqwest::cookie::CookieStore::cookies(logged_out_client.cookies.as_ref(), &url_a)
                .is_none()
        );
        fs::remove_dir_all(root).unwrap();
    }
}
