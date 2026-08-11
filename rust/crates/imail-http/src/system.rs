use std::sync::Arc;

use axum::{
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use imail_oauth::OAuthProviderKey;
use serde::Serialize;

use crate::{auth::AuthenticatedUser, AppState};

pub(crate) fn protected_routes() -> Router<Arc<AppState>> {
    Router::new().route("/api/providers", get(providers))
}

pub(crate) fn public_routes() -> Router<Arc<AppState>> {
    Router::new().route("/api/system/shutdown", post(shutdown))
}

#[derive(Serialize)]
struct ShutdownBody {
    stopping: bool,
}

#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
}

async fn shutdown(
    State(state): State<Arc<AppState>>,
    connect: Option<ConnectInfo<std::net::SocketAddr>>,
    headers: HeaderMap,
) -> Response {
    let Some(control_file) = state.config.daemon_control_file.as_ref() else {
        return (
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "此服务不由桌面守护程序管理",
            }),
        )
            .into_response();
    };
    let expected = match std::fs::read_to_string(control_file) {
        Ok(value) => value,
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ErrorBody {
                    error: "守护控制信息不可用",
                }),
            )
                .into_response()
        }
    };
    let loopback = connect.is_some_and(|ConnectInfo(address)| address.ip().is_loopback());
    let provided = headers
        .get("x-imail-daemon-token")
        .and_then(|value| value.to_str().ok());
    if !loopback
        || !provided
            .is_some_and(|value| constant_time_equal(value.as_bytes(), expected.trim().as_bytes()))
    {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorBody {
                error: "守护控制授权失败",
            }),
        )
            .into_response();
    }
    let Some(sender) = state.daemon_shutdown.as_ref() else {
        return (
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "此服务不由桌面守护程序管理",
            }),
        )
            .into_response();
    };
    if sender.try_send(()).is_err() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorBody {
                error: "守护控制信息不可用",
            }),
        )
            .into_response();
    }
    (StatusCode::ACCEPTED, Json(ShutdownBody { stopping: true })).into_response()
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Provider {
    id: &'static str,
    name: &'static str,
    hint: &'static str,
    auth_mode: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    oauth_provider: Option<Option<&'static str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    oauth_tenant: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fallback_auth_mode: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    help_url: Option<&'static str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OAuthCatalogEntry {
    id: &'static str,
    configured: bool,
    redirect_uri: String,
    scopes: Vec<String>,
    configuration_hint: String,
}

#[derive(Serialize)]
struct ProvidersBody {
    providers: Vec<Provider>,
    oauth: Vec<OAuthCatalogEntry>,
}

async fn providers(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Extension(_user): Extension<AuthenticatedUser>,
) -> Json<ProvidersBody> {
    let providers = vec![
        Provider { id: "outlook", name: "Outlook / Microsoft 365", hint: "优先使用 Microsoft 安全登录，也可使用账户允许的应用专用密码", auth_mode: "oauth2", oauth_provider: Some(Some("microsoft")), oauth_tenant: None, fallback_auth_mode: Some("app-password"), help_url: Some("https://support.microsoft.com/account-billing/manage-app-passwords-for-two-step-verification-d6dc8c6d-4bf7-4851-ad95-6d07799387e9") },
        Provider { id: "gmail", name: "Gmail", hint: "使用 Google 安全登录", auth_mode: "oauth2", oauth_provider: Some(Some("google")), oauth_tenant: None, fallback_auth_mode: Some("app-password"), help_url: None },
        Provider { id: "qq", name: "QQ 邮箱", hint: "QQ 未公开邮件 OAuth，请使用授权码", auth_mode: "authorization-code", oauth_provider: Some(None), oauth_tenant: None, fallback_auth_mode: None, help_url: Some("https://help.mail.qq.com/detail/106/985") },
        Provider { id: "yahoo", name: "Yahoo", hint: "OAuth 需要 Yahoo Mail 接入审核；未审核可使用第三方应用密码", auth_mode: "oauth2", oauth_provider: Some(Some("yahoo")), oauth_tenant: None, fallback_auth_mode: Some("app-password"), help_url: Some("https://login.yahoo.com/account/security") },
        Provider { id: "hotmail", name: "Hotmail / Outlook.com", hint: "优先使用 Microsoft 个人账户安全登录，也可使用账户允许的应用专用密码", auth_mode: "oauth2", oauth_provider: Some(Some("microsoft")), oauth_tenant: Some("consumers"), fallback_auth_mode: Some("app-password"), help_url: Some("https://support.microsoft.com/account-billing/manage-app-passwords-for-two-step-verification-d6dc8c6d-4bf7-4851-ad95-6d07799387e9") },
        Provider { id: "icloud", name: "iCloud", hint: "普通跨平台客户端使用 Apple 应用专用密码", auth_mode: "app-password", oauth_provider: Some(None), oauth_tenant: None, fallback_auth_mode: None, help_url: Some("https://account.apple.com/account/manage") },
        Provider { id: "custom", name: "其他邮箱", hint: "自定义 IMAP / SMTP", auth_mode: "custom", oauth_provider: None, oauth_tenant: None, fallback_auth_mode: None, help_url: None },
    ];
    let oauth = [
        OAuthProviderKey::Google,
        OAuthProviderKey::Microsoft,
        OAuthProviderKey::Yahoo,
    ]
    .into_iter()
    .map(|key| {
        let config =
            state
                .config
                .oauth_config_resolver
                .resolve(&state.config.oauth_environment, key, None);
        OAuthCatalogEntry {
            id: key.as_str(),
            configured: config.configured,
            redirect_uri: config.redirect_uri,
            scopes: config.scopes,
            configuration_hint: config.configuration_hint,
        }
    })
    .collect();
    Json(ProvidersBody { providers, oauth })
}
