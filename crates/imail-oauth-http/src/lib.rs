use std::{collections::HashMap, time::Duration};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use imail_oauth::{
    provider_error, OAuthConfig, OAuthError, OAuthGrant, OAuthIdentity, OAuthProviderKey,
    OAuthProviderPort, OAuthTokenResponse,
};
use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use url::form_urlencoded;

pub mod loopback;

const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

pub struct OAuthHttpAdapter {
    agent: ureq::Agent,
    jwks: HashMap<String, JwkSet>,
}

impl Default for OAuthHttpAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl OAuthHttpAdapter {
    pub fn new() -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .timeout_connect(HTTP_TIMEOUT)
                .timeout_read(HTTP_TIMEOUT)
                .timeout_write(HTTP_TIMEOUT)
                .build(),
            jwks: HashMap::new(),
        }
    }

    fn microsoft_identity(
        &mut self,
        config: &OAuthConfig,
        token: &OAuthTokenResponse,
        nonce: &str,
    ) -> Result<OAuthIdentity, OAuthError> {
        let id_token = token
            .id_token
            .as_deref()
            .ok_or_else(|| provider_error("Microsoft 未返回 ID Token"))?;
        let jwks_uri = config
            .jwks_uri
            .as_deref()
            .ok_or_else(|| provider_error("Microsoft JWKS 地址未配置"))?;
        if !self.jwks.contains_key(jwks_uri) {
            let response = self
                .agent
                .get(jwks_uri)
                .set("Accept", "application/json")
                .call()
                .map_err(http_error)?;
            let set: JwkSet = response
                .into_json()
                .map_err(|_| provider_error("Microsoft JWKS 响应无效"))?;
            self.jwks.insert(jwks_uri.to_string(), set);
        }
        let header =
            decode_header(id_token).map_err(|_| provider_error("Microsoft ID Token 头部无效"))?;
        if header.alg != Algorithm::RS256 {
            return Err(provider_error("Microsoft ID Token 签名算法无效"));
        }
        let kid = header
            .kid
            .as_deref()
            .ok_or_else(|| provider_error("Microsoft ID Token 缺少 kid"))?;
        let jwk = self
            .jwks
            .get(jwks_uri)
            .and_then(|set| set.find(kid))
            .ok_or_else(|| provider_error("Microsoft ID Token 签名密钥不存在"))?;
        let key = DecodingKey::from_jwk(jwk)
            .map_err(|_| provider_error("Microsoft ID Token 签名密钥无效"))?;
        let client_id = config
            .client_id
            .as_deref()
            .ok_or_else(|| OAuthError::Configuration(config.configuration_hint.clone()))?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&[client_id]);
        let claims = decode::<MicrosoftClaims>(id_token, &key, &validation)
            .map_err(|_| provider_error("Microsoft ID Token 校验失败"))?
            .claims;
        if claims.nonce.as_deref() != Some(nonce) {
            return Err(provider_error("OAuth nonce 校验失败"));
        }
        let valid_issuer = claims.iss.as_deref().is_some_and(|issuer| {
            issuer.starts_with("https://login.microsoftonline.com/") && issuer.ends_with("/v2.0")
        });
        if !valid_issuer {
            return Err(provider_error("Microsoft Token 签发方无效"));
        }
        let email = claims
            .preferred_username
            .or(claims.email)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| provider_error("Microsoft 账户没有可用邮箱地址"))?;
        Ok(OAuthIdentity {
            email: email.to_lowercase(),
            name: claims.name,
        })
    }
}

impl OAuthProviderPort for OAuthHttpAdapter {
    fn token_request(
        &mut self,
        config: &OAuthConfig,
        grant: OAuthGrant<'_>,
    ) -> Result<OAuthTokenResponse, OAuthError> {
        let client_id = config
            .client_id
            .as_deref()
            .ok_or_else(|| OAuthError::Configuration(config.configuration_hint.clone()))?;
        let mut form = form_urlencoded::Serializer::new(String::new());
        match grant {
            OAuthGrant::AuthorizationCode {
                code,
                redirect_uri,
                code_verifier,
            } => {
                form.append_pair("grant_type", "authorization_code")
                    .append_pair("code", code)
                    .append_pair("redirect_uri", redirect_uri)
                    .append_pair("code_verifier", code_verifier);
            }
            OAuthGrant::RefreshToken { refresh_token } => {
                form.append_pair("grant_type", "refresh_token")
                    .append_pair("refresh_token", refresh_token);
            }
        }
        form.append_pair("client_id", client_id);
        if config.key != OAuthProviderKey::Yahoo {
            if let Some(secret) = config.client_secret.as_deref() {
                form.append_pair("client_secret", secret);
            }
        }
        let body = form.finish();
        let mut request = self
            .agent
            .post(&config.token_endpoint)
            .set("Content-Type", "application/x-www-form-urlencoded")
            .set("Accept", "application/json");
        if config.key == OAuthProviderKey::Yahoo {
            if let Some(secret) = config.client_secret.as_deref() {
                request = request.set(
                    "Authorization",
                    &format!("Basic {}", BASE64.encode(format!("{client_id}:{secret}"))),
                );
            }
        }
        let response = match request.send_string(&body) {
            Ok(response) => response,
            Err(ureq::Error::Status(_, response)) => {
                return Err(token_response_error(response));
            }
            Err(error) => return Err(http_error(error)),
        };
        let wire: TokenResponse = response
            .into_json()
            .map_err(|_| provider_error("OAuth Token 响应不是有效 JSON"))?;
        if let Some(error) = wire.error.as_deref() {
            return Err(provider_error(
                wire.error_description.as_deref().unwrap_or(error),
            ));
        }
        let access_token = wire
            .access_token
            .filter(|value| !value.is_empty())
            .ok_or_else(|| provider_error("OAuth Token 响应缺少 access_token"))?;
        Ok(OAuthTokenResponse {
            access_token,
            refresh_token: wire.refresh_token,
            expires_in: wire.expires_in,
            token_type: wire.token_type,
            scope: wire.scope,
            id_token: wire.id_token,
        })
    }

    fn fetch_identity(
        &mut self,
        config: &OAuthConfig,
        token: &OAuthTokenResponse,
        nonce: &str,
    ) -> Result<OAuthIdentity, OAuthError> {
        if config.key == OAuthProviderKey::Microsoft {
            return self.microsoft_identity(config, token, nonce);
        }
        let endpoint = config
            .user_info_endpoint
            .as_deref()
            .ok_or_else(|| provider_error("OAuth 身份端点未配置"))?;
        let response = self
            .agent
            .get(endpoint)
            .set("Authorization", &format!("Bearer {}", token.access_token))
            .set("Accept", "application/json")
            .call()
            .map_err(http_error)?;
        let profile: UserProfile = response
            .into_json()
            .map_err(|_| provider_error("OAuth 身份响应不是有效 JSON"))?;
        let email = profile
            .email
            .filter(|value| !value.is_empty())
            .ok_or_else(|| provider_error("OAuth 登录成功，但未能读取邮箱地址"))?;
        Ok(OAuthIdentity {
            email: email.to_lowercase(),
            name: profile.name,
        })
    }
}

fn token_response_error(response: ureq::Response) -> OAuthError {
    let status = response.status();
    match response.into_json::<TokenResponse>() {
        Ok(wire) => provider_error(
            wire.error_description
                .as_deref()
                .or(wire.error.as_deref())
                .unwrap_or("OAuth Token 交换失败"),
        ),
        Err(_) => provider_error(&format!("OAuth Token 交换失败 ({status})")),
    }
}

fn http_error(error: ureq::Error) -> OAuthError {
    match error {
        ureq::Error::Status(status, _) => {
            provider_error(&format!("OAuth HTTP 请求失败 ({status})"))
        }
        ureq::Error::Transport(error) => provider_error(&format!("OAuth HTTP 请求失败：{error}")),
    }
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    token_type: Option<String>,
    scope: Option<String>,
    id_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UserProfile {
    email: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MicrosoftClaims {
    iss: Option<String>,
    nonce: Option<String>,
    preferred_username: Option<String>,
    email: Option<String>,
    name: Option<String>,
    #[allow(dead_code)]
    exp: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    #[test]
    fn exchanges_authorization_code_and_reads_google_identity() {
        let (base, server) = fake_server(vec![
            (
                "/token",
                "{\"access_token\":\"access-1\",\"refresh_token\":\"refresh-1\",\"expires_in\":3600,\"scope\":\"openid email\"}",
            ),
            (
                "/userinfo",
                "{\"email\":\"OWNER@EXAMPLE.COM\",\"name\":\"Owner\"}",
            ),
        ]);
        let config = OAuthConfig {
            key: OAuthProviderKey::Google,
            client_id: Some("client-1".into()),
            client_secret: Some("client-secret".into()),
            redirect_uri: "http://localhost/callback".into(),
            authorization_endpoint: format!("{base}/authorize"),
            token_endpoint: format!("{base}/token"),
            user_info_endpoint: Some(format!("{base}/userinfo")),
            jwks_uri: None,
            scopes: vec!["openid".into(), "email".into()],
            configured: true,
            configuration_hint: "configure".into(),
        };
        let mut adapter = OAuthHttpAdapter::new();
        let token = adapter
            .token_request(
                &config,
                OAuthGrant::AuthorizationCode {
                    code: "code-1",
                    redirect_uri: &config.redirect_uri,
                    code_verifier: "verifier-1",
                },
            )
            .unwrap();
        let identity = adapter.fetch_identity(&config, &token, "nonce-1").unwrap();

        assert_eq!(token.refresh_token.as_deref(), Some("refresh-1"));
        assert_eq!(identity.email, "owner@example.com");
        let requests = server.join().unwrap();
        assert!(requests[0].contains("code_verifier=verifier-1"));
        assert!(requests[0].contains("client_secret=client-secret"));
        assert!(requests[1].contains("Authorization: Bearer access-1"));
    }

    #[test]
    fn provider_errors_are_redacted() {
        let error = provider_error(
            "Bearer access-secret access_token=token-secret refresh_token=refresh-secret",
        );
        let rendered = error.to_string();
        assert!(!rendered.contains("access-secret"));
        assert!(!rendered.contains("token-secret"));
        assert!(!rendered.contains("refresh-secret"));
    }

    fn fake_server(
        responses: Vec<(&'static str, &'static str)>,
    ) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let mut requests = Vec::new();
            for (expected_path, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut data = Vec::new();
                let mut buffer = [0_u8; 1024];
                loop {
                    let count = stream.read(&mut buffer).unwrap();
                    data.extend_from_slice(&buffer[..count]);
                    let header_end = data.windows(4).position(|value| value == b"\r\n\r\n");
                    if let Some(header_end) = header_end {
                        let headers = String::from_utf8_lossy(&data[..header_end + 4]);
                        let content_length = headers
                            .lines()
                            .find_map(|line| {
                                line.strip_prefix("Content-Length: ")
                                    .and_then(|value| value.parse::<usize>().ok())
                            })
                            .unwrap_or_default();
                        if data.len() >= header_end + 4 + content_length {
                            break;
                        }
                    }
                }
                let request = String::from_utf8(data).unwrap();
                assert!(request.lines().next().unwrap().contains(expected_path));
                requests.push(request);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).unwrap();
            }
            requests
        });
        (format!("http://{address}"), handle)
    }
}
