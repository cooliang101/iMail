use std::{path::Path, sync::Arc};

use axum::{
    body::Body,
    extract::{Request, State},
    http::{header, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
};

use crate::AppState;

const WEB_CONTENT_SECURITY_POLICY: &str = "default-src 'self'; base-uri 'self'; frame-ancestors 'none'; form-action 'self'; object-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self' ws: wss:";

pub(crate) async fn serve(State(state): State<Arc<AppState>>, request: Request) -> Response {
    let Some(root) = state.config.web_client_root.as_deref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !matches!(*request.method(), Method::GET | Method::HEAD) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let path = request.uri().path();
    if reserved(path) {
        return StatusCode::NOT_FOUND.into_response();
    }

    if let Some(file) = resolve_static_file(root, path).await {
        return file_response(file, request.method() == Method::HEAD, path).await;
    }
    let accepts_html = request
        .headers()
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.split(',').any(|part| {
                part.trim()
                    .split(';')
                    .next()
                    .is_some_and(|kind| matches!(kind.trim(), "text/html" | "*/*"))
            })
        });
    if !accepts_html {
        return StatusCode::NOT_FOUND.into_response();
    }
    file_response(
        root.join("index.html"),
        request.method() == Method::HEAD,
        "/index.html",
    )
    .await
}

async fn resolve_static_file(root: &Path, request_path: &str) -> Option<std::path::PathBuf> {
    let relative = request_path.trim_start_matches('/').to_string();
    if relative.is_empty() || relative.contains('\0') {
        return None;
    }
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let candidate = root.join(relative).canonicalize().ok()?;
        (candidate.starts_with(&root) && candidate.is_file()).then_some(candidate)
    })
    .await
    .ok()
    .flatten()
}

async fn file_response(path: std::path::PathBuf, head: bool, request_path: &str) -> Response {
    let content = match tokio::task::spawn_blocking(move || std::fs::read(path)).await {
        Ok(Ok(content)) => content,
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    let mut response = Response::new(if head {
        Body::empty()
    } else {
        Body::from(content)
    });
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(content_type(request_path)),
    );
    if request_path == "/index.html" {
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        response.headers_mut().insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(WEB_CONTENT_SECURITY_POLICY),
        );
    } else if request_path.starts_with("/assets/") {
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        );
    }
    response
}

fn reserved(path: &str) -> bool {
    ["/api", "/gateway", "/mcp"]
        .iter()
        .any(|prefix| path == *prefix || path.starts_with(&format!("{prefix}/")))
}

fn content_type(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}
