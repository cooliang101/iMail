use std::{
    collections::HashMap,
    sync::{Arc, Condvar, Mutex, OnceLock},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, SecondsFormat, Utc};
use imail_security::{MasterKey, SecurityError};
use rand::{rngs::OsRng, RngCore};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use url::Url;

pub const OAUTH_STATE_TTL_MS: i64 = 10 * 60 * 1_000;
pub const TOKEN_EXPIRY_SKEW_MS: i64 = 90 * 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OAuthProviderKey {
    Google,
    Microsoft,
    Yahoo,
}

impl OAuthProviderKey {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Microsoft => "microsoft",
            Self::Yahoo => "yahoo",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthConfig {
    pub key: OAuthProviderKey,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub redirect_uri: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub user_info_endpoint: Option<String>,
    pub jwks_uri: Option<String>,
    pub scopes: Vec<String>,
    pub configured: bool,
    pub configuration_hint: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OAuthEnvironment {
    pub callback_base_url: String,
    pub google_client_id: Option<String>,
    pub google_client_secret: Option<String>,
    pub google_redirect_uri: Option<String>,
    pub microsoft_client_id: Option<String>,
    pub microsoft_client_secret: Option<String>,
    pub microsoft_redirect_uri: Option<String>,
    pub yahoo_client_id: Option<String>,
    pub yahoo_client_secret: Option<String>,
    pub yahoo_redirect_uri: Option<String>,
    pub yahoo_mail_oauth_approved: bool,
}

pub trait OAuthConfigResolver: Send + Sync {
    fn resolve(
        &self,
        environment: &OAuthEnvironment,
        key: OAuthProviderKey,
        account_provider: Option<&str>,
    ) -> OAuthConfig;
}

#[derive(Debug, Default)]
pub struct StandardOAuthConfigResolver;

impl OAuthConfigResolver for StandardOAuthConfigResolver {
    fn resolve(
        &self,
        environment: &OAuthEnvironment,
        key: OAuthProviderKey,
        account_provider: Option<&str>,
    ) -> OAuthConfig {
        provider_config(environment, key, account_provider)
    }
}

pub trait OAuthProviderPortFactory: Send + Sync {
    fn create(&self) -> Box<dyn OAuthProviderPort>;
}

impl<F> OAuthProviderPortFactory for F
where
    F: Fn() -> Box<dyn OAuthProviderPort> + Send + Sync,
{
    fn create(&self) -> Box<dyn OAuthProviderPort> {
        self()
    }
}

pub fn provider_config(
    environment: &OAuthEnvironment,
    key: OAuthProviderKey,
    account_provider: Option<&str>,
) -> OAuthConfig {
    match key {
        OAuthProviderKey::Google => OAuthConfig {
            key,
            client_id: environment.google_client_id.clone(),
            client_secret: environment.google_client_secret.clone(),
            redirect_uri: environment.google_redirect_uri.clone().unwrap_or_else(|| {
                format!("{}/google/callback", environment.callback_base_url)
            }),
            authorization_endpoint: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            token_endpoint: "https://oauth2.googleapis.com/token".into(),
            user_info_endpoint: Some(
                "https://openidconnect.googleapis.com/v1/userinfo".into(),
            ),
            jwks_uri: None,
            scopes: vec![
                "openid".into(),
                "email".into(),
                "profile".into(),
                "https://mail.google.com/".into(),
            ],
            configured: environment.google_client_id.is_some()
                && environment.google_client_secret.is_some(),
            configuration_hint: "配置 Google Desktop App 凭据中的 Client ID 与 Client Secret；桌面公共客户端仍使用 PKCE，Client Secret 不作为可保密凭据。".into(),
        },
        OAuthProviderKey::Microsoft => {
            let tenant = if account_provider == Some("hotmail") {
                "consumers"
            } else {
                "common"
            };
            OAuthConfig {
                key,
                client_id: environment.microsoft_client_id.clone(),
                client_secret: environment.microsoft_client_secret.clone(),
                redirect_uri: environment.microsoft_redirect_uri.clone().unwrap_or_else(|| {
                    format!("{}/microsoft/callback", environment.callback_base_url)
                }),
                authorization_endpoint: format!(
                    "https://login.microsoftonline.com/{tenant}/oauth2/v2.0/authorize"
                ),
                token_endpoint: format!(
                    "https://login.microsoftonline.com/{tenant}/oauth2/v2.0/token"
                ),
                user_info_endpoint: None,
                jwks_uri: Some(
                    "https://login.microsoftonline.com/common/discovery/v2.0/keys".into(),
                ),
                scopes: vec![
                    "openid".into(),
                    "email".into(),
                    "profile".into(),
                    "offline_access".into(),
                    "https://outlook.office.com/IMAP.AccessAsUser.All".into(),
                    "https://outlook.office.com/SMTP.Send".into(),
                ],
                configured: environment.microsoft_client_id.is_some(),
                configuration_hint: "配置 Microsoft Desktop App 的 OAuth Client ID；桌面公共客户端使用 PKCE，不需要 Client Secret。".into(),
            }
        }
        OAuthProviderKey::Yahoo => OAuthConfig {
            key,
            client_id: environment.yahoo_client_id.clone(),
            client_secret: environment.yahoo_client_secret.clone(),
            redirect_uri: environment.yahoo_redirect_uri.clone().unwrap_or_else(|| {
                format!("{}/yahoo/callback", environment.callback_base_url)
            }),
            authorization_endpoint: "https://api.login.yahoo.com/oauth2/request_auth".into(),
            token_endpoint: "https://api.login.yahoo.com/oauth2/get_token".into(),
            user_info_endpoint: Some("https://api.login.yahoo.com/openid/v1/userinfo".into()),
            jwks_uri: None,
            scopes: vec![
                "openid".into(),
                "email".into(),
                "profile".into(),
                "mail-r".into(),
                "mail-w".into(),
            ],
            configured: environment.yahoo_client_id.is_some()
                && environment.yahoo_client_secret.is_some()
                && environment.yahoo_mail_oauth_approved,
            configuration_hint: "Yahoo mail-r/mail-w 是受限权限。审核通过后配置 YAHOO_OAUTH_CLIENT_ID、YAHOO_OAUTH_CLIENT_SECRET 与 YAHOO_MAIL_OAUTH_APPROVED=true。".into(),
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthProxyInput {
    pub protocol: String,
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BeginOAuthInput {
    pub owner_id: String,
    pub account_provider: String,
    pub display_name: Option<String>,
    pub group: Option<String>,
    pub color: Option<String>,
    pub account_id: Option<String>,
    pub expected_email: Option<String>,
    pub proxy: Option<OAuthProxyInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeginOAuthResult {
    pub authorization_url: String,
    pub state: String,
    pub provider: OAuthProviderKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthCallback {
    pub state: String,
    pub code: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingOAuth {
    pub owner_id: String,
    pub provider_key: OAuthProviderKey,
    pub account_provider: String,
    pub code_verifier: String,
    pub nonce: String,
    pub created_at: i64,
    pub display_name: Option<String>,
    pub group: String,
    pub color: String,
    pub account_id: Option<String>,
    pub expected_email: Option<String>,
    pub proxy: Option<OAuthProxyInput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthTokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<i64>,
    pub token_type: Option<String>,
    pub scope: Option<String>,
    pub id_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthAccountSecret {
    pub auth_type: String,
    pub oauth_provider: OAuthProviderKey,
    pub access_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    pub expires_at: String,
    pub scopes: Vec<String>,
    pub token_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_password: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthIdentity {
    pub email: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OAuthGrant<'a> {
    AuthorizationCode {
        code: &'a str,
        redirect_uri: &'a str,
        code_verifier: &'a str,
    },
    RefreshToken {
        refresh_token: &'a str,
    },
}

pub trait OAuthProviderPort {
    fn token_request(
        &mut self,
        config: &OAuthConfig,
        grant: OAuthGrant<'_>,
    ) -> Result<OAuthTokenResponse, OAuthError>;

    fn fetch_identity(
        &mut self,
        config: &OAuthConfig,
        token: &OAuthTokenResponse,
        nonce: &str,
    ) -> Result<OAuthIdentity, OAuthError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedOAuth {
    pub pending: PendingOAuth,
    pub identity: OAuthIdentity,
    pub secret: OAuthAccountSecret,
}

#[derive(Debug, Error)]
pub enum OAuthError {
    #[error("{0}")]
    Configuration(String),
    #[error("OAuth state 无效或已过期，请重新开始")]
    InvalidState,
    #[error("OAuth state 缺少应用账号归属，请重新开始")]
    MissingOwner,
    #[error("OAuth 回调缺少 code 或 state")]
    MissingCallbackParameters,
    #[error("OAuth 服务商返回的数据无效")]
    InvalidProviderResponse,
    #[error("OAuth 授权已过期且没有刷新 Token，请重新连接邮箱")]
    MissingRefreshToken,
    #[error("OAuth 网络请求失败：{0}")]
    Provider(String),
    #[error(transparent)]
    Security(#[from] SecurityError),
}

pub struct OAuthService<'a, P: OAuthProviderPort + ?Sized> {
    master_key: &'a MasterKey,
    provider: &'a mut P,
}

impl<'a, P: OAuthProviderPort + ?Sized> OAuthService<'a, P> {
    pub fn new(master_key: &'a MasterKey, provider: &'a mut P) -> Self {
        Self {
            master_key,
            provider,
        }
    }

    pub fn begin(
        &self,
        config: &OAuthConfig,
        input: BeginOAuthInput,
        now_ms: i64,
    ) -> Result<BeginOAuthResult, OAuthError> {
        let mut nonce = [0_u8; 24];
        let mut verifier = [0_u8; 48];
        OsRng.fill_bytes(&mut nonce);
        OsRng.fill_bytes(&mut verifier);
        self.begin_with_material(config, input, now_ms, nonce, verifier)
    }

    pub fn complete(
        &mut self,
        config: &OAuthConfig,
        provider_key: OAuthProviderKey,
        state: Option<&str>,
        code: Option<&str>,
        now_ms: i64,
    ) -> Result<CompletedOAuth, OAuthError> {
        let (state, code) = match (state, code) {
            (Some(state), Some(code)) => (state, code),
            _ => return Err(OAuthError::MissingCallbackParameters),
        };
        let pending = self.validate_state(provider_key, state, now_ms)?;
        let token = self.provider.token_request(
            config,
            OAuthGrant::AuthorizationCode {
                code,
                redirect_uri: &config.redirect_uri,
                code_verifier: &pending.code_verifier,
            },
        )?;
        let identity = self
            .provider
            .fetch_identity(config, &token, &pending.nonce)?;
        let secret = token_to_secret(config, token, None, None, now_ms)?;
        Ok(CompletedOAuth {
            pending,
            identity: OAuthIdentity {
                email: identity.email.to_lowercase(),
                name: identity.name,
            },
            secret,
        })
    }

    pub fn refresh(
        &mut self,
        config: &OAuthConfig,
        current: &OAuthAccountSecret,
        now_ms: i64,
    ) -> Result<OAuthAccountSecret, OAuthError> {
        refresh_account_secret(self.provider, config, current, now_ms)
    }

    pub fn validate_state(
        &self,
        provider_key: OAuthProviderKey,
        state: &str,
        now_ms: i64,
    ) -> Result<PendingOAuth, OAuthError> {
        let pending = self
            .master_key
            .decrypt_json::<PendingOAuth>(state)
            .map_err(|_| OAuthError::InvalidState)?;
        if pending.provider_key != provider_key
            || now_ms.saturating_sub(pending.created_at) > OAUTH_STATE_TTL_MS
            || now_ms < pending.created_at
        {
            return Err(OAuthError::InvalidState);
        }
        if pending.owner_id.is_empty() {
            return Err(OAuthError::MissingOwner);
        }
        Ok(pending)
    }

    fn begin_with_material(
        &self,
        config: &OAuthConfig,
        input: BeginOAuthInput,
        now_ms: i64,
        nonce_bytes: [u8; 24],
        verifier_bytes: [u8; 48],
    ) -> Result<BeginOAuthResult, OAuthError> {
        if !config.configured {
            return Err(OAuthError::Configuration(config.configuration_hint.clone()));
        }
        if input.owner_id.is_empty() {
            return Err(OAuthError::MissingOwner);
        }
        let nonce = URL_SAFE_NO_PAD.encode(nonce_bytes);
        let code_verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);
        let code_challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(code_verifier.as_bytes()));
        let pending = PendingOAuth {
            owner_id: input.owner_id,
            provider_key: config.key,
            account_provider: input.account_provider,
            code_verifier,
            nonce,
            created_at: now_ms,
            display_name: input.display_name,
            group: input
                .group
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("个人")
                .to_string(),
            color: input.color.unwrap_or_else(|| "#168f78".into()),
            account_id: input.account_id,
            expected_email: input.expected_email.map(|value| value.to_lowercase()),
            proxy: input.proxy,
        };
        let state = self.master_key.encrypt_json(&pending)?;
        let mut url = Url::parse(&config.authorization_endpoint)
            .map_err(|_| OAuthError::Configuration("OAuth 授权地址无效".into()))?;
        {
            let mut query = url.query_pairs_mut();
            query
                .append_pair(
                    "client_id",
                    config.client_id.as_deref().ok_or_else(|| {
                        OAuthError::Configuration(config.configuration_hint.clone())
                    })?,
                )
                .append_pair("redirect_uri", &config.redirect_uri)
                .append_pair("response_type", "code")
                .append_pair("scope", &config.scopes.join(" "))
                .append_pair("state", &state)
                .append_pair("nonce", &pending.nonce)
                .append_pair("code_challenge", &code_challenge)
                .append_pair("code_challenge_method", "S256");
            match config.key {
                OAuthProviderKey::Google => {
                    query
                        .append_pair("access_type", "offline")
                        .append_pair("prompt", "consent select_account")
                        .append_pair("include_granted_scopes", "true");
                }
                OAuthProviderKey::Microsoft => {
                    query.append_pair("prompt", "select_account");
                }
                OAuthProviderKey::Yahoo => {}
            }
        }
        Ok(BeginOAuthResult {
            authorization_url: url.into(),
            state,
            provider: config.key,
        })
    }
}

pub fn refresh_account_secret<P: OAuthProviderPort + ?Sized>(
    provider: &mut P,
    config: &OAuthConfig,
    current: &OAuthAccountSecret,
    now_ms: i64,
) -> Result<OAuthAccountSecret, OAuthError> {
    let refresh_token = current
        .refresh_token
        .as_deref()
        .ok_or(OAuthError::MissingRefreshToken)?;
    let token = provider.token_request(config, OAuthGrant::RefreshToken { refresh_token })?;
    token_to_secret(
        config,
        token,
        Some(refresh_token),
        current.proxy_password.clone(),
        now_ms,
    )
}

pub fn token_needs_refresh(secret: &OAuthAccountSecret, now_ms: i64) -> bool {
    DateTime::parse_from_rfc3339(&secret.expires_at)
        .ok()
        .map(|value| value.timestamp_millis())
        .is_some_and(|expires_at| expires_at <= now_ms.saturating_add(TOKEN_EXPIRY_SKEW_MS))
}

fn token_to_secret(
    config: &OAuthConfig,
    token: OAuthTokenResponse,
    previous_refresh_token: Option<&str>,
    proxy_password: Option<String>,
    now_ms: i64,
) -> Result<OAuthAccountSecret, OAuthError> {
    if token.access_token.is_empty() {
        return Err(OAuthError::InvalidProviderResponse);
    }
    let lifetime_seconds = token.expires_in.unwrap_or(3_600).max(60);
    let expires_at_ms = now_ms.saturating_add(lifetime_seconds.saturating_mul(1_000));
    let expires_at = millis_to_rfc3339(expires_at_ms)?;
    let scopes = token
        .scope
        .unwrap_or_else(|| config.scopes.join(" "))
        .split_whitespace()
        .map(str::to_string)
        .collect();
    Ok(OAuthAccountSecret {
        auth_type: "oauth2".into(),
        oauth_provider: config.key,
        access_token: token.access_token,
        refresh_token: token
            .refresh_token
            .or_else(|| previous_refresh_token.map(str::to_string)),
        expires_at,
        scopes,
        token_type: token.token_type.unwrap_or_else(|| "Bearer".into()),
        proxy_password,
    })
}

fn millis_to_rfc3339(value: i64) -> Result<String, OAuthError> {
    DateTime::<Utc>::from_timestamp_millis(value)
        .map(|date| date.to_rfc3339_opts(SecondsFormat::Millis, true))
        .ok_or(OAuthError::InvalidProviderResponse)
}

pub fn provider_for_account(account_provider: &str) -> Option<OAuthProviderKey> {
    match account_provider {
        "gmail" => Some(OAuthProviderKey::Google),
        "outlook" | "hotmail" => Some(OAuthProviderKey::Microsoft),
        "yahoo" => Some(OAuthProviderKey::Yahoo),
        _ => None,
    }
}

pub fn callback_error(error: &str, description: Option<&str>) -> String {
    if let Some(detail) = description.map(str::trim).filter(|value| !value.is_empty()) {
        return detail.to_string();
    }
    match error {
        "access_denied" => "你已取消或拒绝授权，邮箱没有发生更改".into(),
        "server_error" | "temporarily_unavailable" => {
            "服务商暂时未能完成授权。iMail 会先检查授权是否已经保存；若账户仍未出现，请稍后重试"
                .into()
        }
        "invalid_request" | "unauthorized_client" => {
            "OAuth 应用或回调地址配置不正确，请检查服务商开发者控制台".into()
        }
        _ => format!("授权未完成：{error}"),
    }
}

pub fn provider_error(detail: &str) -> OAuthError {
    static BEARER: OnceLock<Regex> = OnceLock::new();
    static NAMED_SECRET: OnceLock<Regex> = OnceLock::new();
    let collapsed = detail.split_whitespace().collect::<Vec<_>>().join(" ");
    let without_bearer = BEARER
        .get_or_init(|| Regex::new(r"(?i)Bearer\s+[^\s,;]+").unwrap())
        .replace_all(&collapsed, "Bearer [redacted]");
    let safe = NAMED_SECRET
        .get_or_init(|| {
            Regex::new(
                r"(?i)(access[_-]?token|refresh[_-]?token|client[_-]?secret|authorization)(\s*[:=]\s*)[^\s,;]+",
            )
            .unwrap()
        })
        .replace_all(&without_bearer, "$1$2[redacted]");
    OAuthError::Provider(safe.trim().chars().take(500).collect())
}

#[derive(Default)]
pub struct RefreshCoordinator {
    in_flight: Mutex<HashMap<String, Arc<RefreshEntry>>>,
}

#[derive(Default)]
struct RefreshEntry {
    state: Mutex<Option<Result<OAuthAccountSecret, String>>>,
    completed: Condvar,
}

impl RefreshCoordinator {
    pub fn run(
        &self,
        account_id: &str,
        operation: impl FnOnce() -> Result<OAuthAccountSecret, OAuthError>,
    ) -> Result<OAuthAccountSecret, OAuthError> {
        let (entry, leader) = {
            let mut in_flight = self
                .in_flight
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if let Some(entry) = in_flight.get(account_id) {
                (Arc::clone(entry), false)
            } else {
                let entry = Arc::new(RefreshEntry::default());
                in_flight.insert(account_id.to_string(), Arc::clone(&entry));
                (entry, true)
            }
        };
        if leader {
            let result = operation().map_err(|error| error.to_string());
            {
                let mut state = entry
                    .state
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                *state = Some(result.clone());
            }
            entry.completed.notify_all();
            self.in_flight
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(account_id);
            return result.map_err(|detail| provider_error(&detail));
        }
        let mut state = entry
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        while state.is_none() {
            state = entry
                .completed
                .wait(state)
                .unwrap_or_else(|error| error.into_inner());
        }
        state
            .as_ref()
            .expect("refresh state is complete")
            .clone()
            .map_err(|detail| provider_error(&detail))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            Barrier,
        },
        thread,
        time::Duration,
    };

    const NOW: i64 = 1_786_336_496_000;

    #[test]
    fn begin_builds_encrypted_pkce_state_and_google_parameters() {
        let key = test_key();
        let mut port = FakeProvider::default();
        let service = OAuthService::new(&key, &mut port);
        let result = service
            .begin_with_material(
                &config(OAuthProviderKey::Google),
                input(),
                NOW,
                [7; 24],
                [9; 48],
            )
            .unwrap();
        let pending: PendingOAuth = key.decrypt_json(&result.state).unwrap();
        let url = Url::parse(&result.authorization_url).unwrap();
        let query = url
            .query_pairs()
            .collect::<std::collections::BTreeMap<_, _>>();

        assert_eq!(pending.owner_id, "user-1");
        assert_eq!(pending.expected_email.as_deref(), Some("owner@example.com"));
        assert_eq!(query.get("state").unwrap(), &result.state);
        assert_eq!(query.get("code_challenge_method").unwrap(), "S256");
        assert_eq!(query.get("access_type").unwrap(), "offline");
        assert!(!result.state.contains("owner@example.com"));
        assert!(!result.authorization_url.contains("proxy-secret"));
    }

    #[test]
    fn complete_validates_state_and_exchanges_with_the_original_verifier() {
        let key = test_key();
        let mut port = FakeProvider::default();
        let begin = {
            let service = OAuthService::new(&key, &mut port);
            service
                .begin_with_material(
                    &config(OAuthProviderKey::Microsoft),
                    input(),
                    NOW,
                    [1; 24],
                    [2; 48],
                )
                .unwrap()
        };
        let completed = OAuthService::new(&key, &mut port)
            .complete(
                &config(OAuthProviderKey::Microsoft),
                OAuthProviderKey::Microsoft,
                Some(&begin.state),
                Some("authorization-code"),
                NOW + 1_000,
            )
            .unwrap();

        assert_eq!(completed.identity.email, "owner@example.com");
        assert_eq!(completed.secret.expires_at, "2026-08-10T05:34:57.000Z");
        assert_eq!(completed.secret.refresh_token.as_deref(), Some("refresh-1"));
        assert!(port.saw_authorization_code);
        assert_eq!(
            port.saw_nonce.as_deref(),
            Some(completed.pending.nonce.as_str())
        );
    }

    #[test]
    fn state_rejects_wrong_provider_future_and_expired_values() {
        let key = test_key();
        let mut port = FakeProvider::default();
        let service = OAuthService::new(&key, &mut port);
        let begin = service
            .begin_with_material(
                &config(OAuthProviderKey::Google),
                input(),
                NOW,
                [1; 24],
                [2; 48],
            )
            .unwrap();

        for (provider, time) in [
            (OAuthProviderKey::Yahoo, NOW),
            (OAuthProviderKey::Google, NOW - 1),
            (OAuthProviderKey::Google, NOW + OAUTH_STATE_TTL_MS + 1),
        ] {
            assert!(matches!(
                service.validate_state(provider, &begin.state, time),
                Err(OAuthError::InvalidState)
            ));
        }
    }

    #[test]
    fn refresh_preserves_old_refresh_token_and_proxy_password() {
        let key = test_key();
        let mut port = FakeProvider::default();
        let mut service = OAuthService::new(&key, &mut port);
        let current = OAuthAccountSecret {
            auth_type: "oauth2".into(),
            oauth_provider: OAuthProviderKey::Google,
            access_token: "old-access".into(),
            refresh_token: Some("old-refresh".into()),
            expires_at: "2026-08-10T04:34:00.000Z".into(),
            scopes: vec![],
            token_type: "Bearer".into(),
            proxy_password: Some("proxy-secret".into()),
        };

        assert!(token_needs_refresh(&current, NOW));
        let refreshed = service
            .refresh(&config(OAuthProviderKey::Google), &current, NOW)
            .unwrap();
        assert_eq!(refreshed.refresh_token.as_deref(), Some("old-refresh"));
        assert_eq!(refreshed.proxy_password.as_deref(), Some("proxy-secret"));
    }

    #[test]
    fn provider_catalog_matches_the_existing_endpoints_and_approval_rules() {
        let environment = OAuthEnvironment {
            callback_base_url: "http://localhost:8787/api/oauth".into(),
            google_client_id: Some("google-id".into()),
            google_client_secret: Some("google-secret".into()),
            microsoft_client_id: Some("microsoft-id".into()),
            yahoo_client_id: Some("yahoo-id".into()),
            yahoo_client_secret: Some("yahoo-secret".into()),
            ..Default::default()
        };
        let google = provider_config(&environment, OAuthProviderKey::Google, Some("gmail"));
        let microsoft = provider_config(&environment, OAuthProviderKey::Microsoft, Some("hotmail"));
        let yahoo = provider_config(&environment, OAuthProviderKey::Yahoo, Some("yahoo"));

        assert!(google.configured);
        assert!(google.scopes.contains(&"https://mail.google.com/".into()));
        assert!(microsoft.authorization_endpoint.contains("/consumers/"));
        assert!(microsoft.scopes.contains(&"offline_access".into()));
        assert!(!yahoo.configured);
        assert_eq!(
            yahoo.redirect_uri,
            "http://localhost:8787/api/oauth/yahoo/callback"
        );
    }

    #[test]
    fn refresh_coordinator_runs_one_operation_per_account() {
        let coordinator = Arc::new(RefreshCoordinator::default());
        let start = Arc::new(Barrier::new(8));
        let calls = Arc::new(AtomicUsize::new(0));
        let threads = (0..8)
            .map(|_| {
                let coordinator = Arc::clone(&coordinator);
                let start = Arc::clone(&start);
                let calls = Arc::clone(&calls);
                thread::spawn(move || {
                    start.wait();
                    coordinator
                        .run("account-1", || {
                            calls.fetch_add(1, Ordering::SeqCst);
                            thread::sleep(Duration::from_millis(100));
                            Ok(test_secret())
                        })
                        .unwrap()
                })
            })
            .collect::<Vec<_>>();
        let secrets = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(secrets.iter().all(|secret| secret.access_token == "access"));
    }

    fn test_key() -> MasterKey {
        MasterKey::from_hex(&"11".repeat(32)).unwrap()
    }

    fn test_secret() -> OAuthAccountSecret {
        OAuthAccountSecret {
            auth_type: "oauth2".into(),
            oauth_provider: OAuthProviderKey::Google,
            access_token: "access".into(),
            refresh_token: Some("refresh".into()),
            expires_at: "2026-08-10T05:34:56.000Z".into(),
            scopes: vec!["openid".into()],
            token_type: "Bearer".into(),
            proxy_password: None,
        }
    }

    fn input() -> BeginOAuthInput {
        BeginOAuthInput {
            owner_id: "user-1".into(),
            account_provider: "gmail".into(),
            display_name: None,
            group: Some(" ".into()),
            color: None,
            account_id: None,
            expected_email: Some("OWNER@EXAMPLE.COM".into()),
            proxy: Some(OAuthProxyInput {
                protocol: "socks5".into(),
                host: "127.0.0.1".into(),
                port: 1080,
                username: Some("mail".into()),
                password: Some("proxy-secret".into()),
            }),
        }
    }

    fn config(key: OAuthProviderKey) -> OAuthConfig {
        OAuthConfig {
            key,
            client_id: Some("client-1".into()),
            client_secret: None,
            redirect_uri: format!("http://localhost:8787/api/oauth/{}/callback", key.as_str()),
            authorization_endpoint: "https://login.example.com/authorize".into(),
            token_endpoint: "https://login.example.com/token".into(),
            user_info_endpoint: Some("https://login.example.com/userinfo".into()),
            jwks_uri: None,
            scopes: vec!["openid".into(), "email".into()],
            configured: true,
            configuration_hint: "configure OAuth".into(),
        }
    }

    #[derive(Default)]
    struct FakeProvider {
        saw_authorization_code: bool,
        saw_nonce: Option<String>,
    }

    impl OAuthProviderPort for FakeProvider {
        fn token_request(
            &mut self,
            _config: &OAuthConfig,
            grant: OAuthGrant<'_>,
        ) -> Result<OAuthTokenResponse, OAuthError> {
            self.saw_authorization_code = matches!(grant, OAuthGrant::AuthorizationCode { .. });
            Ok(OAuthTokenResponse {
                access_token: "access-1".into(),
                refresh_token: self.saw_authorization_code.then(|| "refresh-1".into()),
                expires_in: Some(3_600),
                token_type: None,
                scope: Some("openid email".into()),
                id_token: None,
            })
        }

        fn fetch_identity(
            &mut self,
            _config: &OAuthConfig,
            _token: &OAuthTokenResponse,
            nonce: &str,
        ) -> Result<OAuthIdentity, OAuthError> {
            self.saw_nonce = Some(nonce.into());
            Ok(OAuthIdentity {
                email: "OWNER@EXAMPLE.COM".into(),
                name: Some("Owner".into()),
            })
        }
    }
}
