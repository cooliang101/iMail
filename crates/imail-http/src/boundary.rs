//! HTTP host, proxy, CORS and response-header boundary.
use crate::{config::normalize_origin, AppState};
use axum::{
    body::Body,
    extract::{Request, State},
    http::{
        header::{
            ACCESS_CONTROL_ALLOW_CREDENTIALS, ACCESS_CONTROL_ALLOW_HEADERS,
            ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN, HOST, ORIGIN, VARY,
        },
        HeaderName, HeaderValue, Method, StatusCode,
    },
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use std::{collections::BTreeSet, net::IpAddr, sync::Arc};

#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
}

pub(super) async fn security_boundary(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    if state.config.trust_proxy_one_hop && !valid_forwarded_headers(request.headers()) {
        return secured_response(
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorBody {
                    error: "代理转发头无效",
                }),
            )
                .into_response(),
            false,
        );
    }
    let forwarded_https = state.config.trust_proxy_one_hop
        && request
            .headers()
            .get("x-forwarded-proto")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .is_some_and(|value| value.trim() == "https");
    let host = request
        .headers()
        .get(HOST)
        .and_then(|value| value.to_str().ok());
    if state.config.production && !host_allowed(host, &state.config.allowed_hosts) {
        return secured_response(
            (
                StatusCode::MISDIRECTED_REQUEST,
                Json(ErrorBody {
                    error: "请求主机名不在服务允许列表中",
                }),
            )
                .into_response(),
            forwarded_https,
        );
    }

    let origin = request
        .headers()
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok());
    let allowed_origin = origin.and_then(|value| {
        normalize_origin(value)
            .ok()
            .filter(|normalized| state.config.cors_origins.contains(normalized))
    });
    if request.method() == Method::OPTIONS && allowed_origin.is_some() {
        return with_cors(
            secured_response(StatusCode::NO_CONTENT.into_response(), forwarded_https),
            allowed_origin,
        );
    }
    with_cors(
        secured_response(next.run(request).await, forwarded_https),
        allowed_origin,
    )
}

fn valid_forwarded_headers(headers: &axum::http::HeaderMap) -> bool {
    let forwarded_for = headers
        .get("x-forwarded-for")
        .map(|value| {
            value
                .to_str()
                .ok()
                .and_then(|value| value.rsplit(',').next())
                .map(str::trim)
                .is_some_and(|value| !value.is_empty() && value.parse::<IpAddr>().is_ok())
        })
        .unwrap_or(true);
    let forwarded_proto = headers
        .get("x-forwarded-proto")
        .map(|value| {
            value
                .to_str()
                .ok()
                .and_then(|value| value.split(',').next())
                .map(str::trim)
                .is_some_and(|value| matches!(value, "http" | "https"))
        })
        .unwrap_or(true);
    forwarded_for && forwarded_proto
}

fn secured_response(mut response: Response<Body>, secure: bool) -> Response<Body> {
    let headers = response.headers_mut();
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    if secure {
        headers.insert(
            HeaderName::from_static("strict-transport-security"),
            HeaderValue::from_static("max-age=31536000"),
        );
    }
    headers.insert(
        HeaderName::from_static("x-frame-options"),
        HeaderValue::from_static("DENY"),
    );
    headers.insert(
        HeaderName::from_static("referrer-policy"),
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    response
}

fn with_cors(mut response: Response<Body>, origin: Option<String>) -> Response<Body> {
    if let Some(origin) = origin.and_then(|value| HeaderValue::from_str(&value).ok()) {
        let headers = response.headers_mut();
        headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin);
        headers.insert(
            ACCESS_CONTROL_ALLOW_CREDENTIALS,
            HeaderValue::from_static("true"),
        );
        headers.insert(VARY, HeaderValue::from_static("Origin"));
        headers.insert(
            ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET,POST,PUT,PATCH,DELETE,OPTIONS"),
        );
        headers.insert(
            ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("Authorization,Content-Type"),
        );
    }
    response
}

pub(super) fn host_allowed(value: Option<&str>, allowed: &BTreeSet<String>) -> bool {
    value
        .and_then(|value| value.parse::<axum::http::uri::Authority>().ok())
        .map(|authority| {
            authority
                .host()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_ascii_lowercase()
        })
        .is_some_and(|host| allowed.contains(&host))
}
