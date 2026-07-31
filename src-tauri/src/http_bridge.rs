use futures_util::StreamExt;
use reqwest::{Client, Method, Url};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf, sync::Mutex, time::Duration};
use tauri::{AppHandle, Emitter, State};

pub struct HttpBridgeState {
    clients: Mutex<HashMap<String, Client>>,
    event_task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

impl HttpBridgeState {
    pub fn new() -> Self {
        Self {
            clients: Mutex::new(HashMap::new()),
            event_task: Mutex::new(None),
        }
    }

    fn client(&self, base_url: &str) -> Result<(String, Client), String> {
        let normalized = normalize_base_url(base_url)?;
        let mut clients = self
            .clients
            .lock()
            .map_err(|_| "桌面网络状态不可用".to_string())?;
        let client = clients
            .entry(normalized.clone())
            .or_insert_with(|| {
                Client::builder()
                    .cookie_store(true)
                    .connect_timeout(Duration::from_secs(8))
                    .timeout(Duration::from_secs(120))
                    .build()
                    .expect("reqwest client configuration must be valid")
            })
            .clone();
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
        .get(request_url(&base_url, &path)?)
        .send()
        .await
        .map_err(|error| format!("附件下载失败：{error}"))?;
    if !response.status().is_success() {
        return Err(format!("附件下载失败：{}", response.status().as_u16()));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("读取附件失败：{error}"))?;
    std::fs::write(target, bytes).map_err(|error| format!("保存附件失败：{error}"))
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
                .get(url)
                .header("Accept", "text/event-stream")
                .send()
                .await
            {
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
        assert!(request_url("https://mail.example.com", "//evil.example.com").is_err());
        assert!(request_method(Some("CONNECT")).is_err());
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
}
