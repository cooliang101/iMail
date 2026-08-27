use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha1::{Digest as Sha1Digest, Sha1};
use url::Url;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{
    error::{AppleHmeError, AppleHmeErrorCode, Result},
    model::{AppleCookie, AppleSession, LoginState, LoginStateKind, TwoFactorMethod},
    protocol::{
        decode_json, execute, header_map, json_bytes, response_text, APPLE_ACCOUNT_USER_AGENT,
        ICLOUD_WEB_USER_AGENT,
    },
    srp::SrpClient,
    transport::{HttpMethod, HttpResponse, HttpTransport, UreqTransport},
    DEFAULT_ICLOUD_WEB_CLIENT_ID,
};

const APPLE_ACCOUNT_CLIENT_ID: &str =
    "af1139274f266b22b68c2a3e7ad932cb3c0bbe854e13a79af78dcc73136882c3";
const ICLOUD_BUILD_NUMBER: &str = "2622Build20";
const PENDING_TTL_MINUTES: i64 = 10;

#[derive(Clone)]
pub struct LoginRequest {
    pub kind: LoginStateKind,
    pub apple_id: String,
    pub password: String,
    pub two_factor_method: TwoFactorMethod,
    pub phone_number: Option<Value>,
    pub icloud_host: Option<String>,
    pub client_id: Option<String>,
}

impl LoginRequest {
    pub fn icloud_web(apple_id: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            kind: LoginStateKind::ICloudWeb,
            apple_id: apple_id.into(),
            password: password.into(),
            two_factor_method: TwoFactorMethod::TrustedDevice,
            phone_number: None,
            icloud_host: None,
            client_id: None,
        }
    }

    pub fn apple_account(apple_id: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            kind: LoginStateKind::AppleAccount,
            apple_id: apple_id.into(),
            password: password.into(),
            two_factor_method: TwoFactorMethod::TrustedDevice,
            phone_number: None,
            icloud_host: None,
            client_id: None,
        }
    }
}

#[derive(Clone)]
pub struct LoginResult {
    pub session: Option<AppleSession>,
    pub pending_id: Option<String>,
    pub needs_two_factor: bool,
    pub expires_at: Option<DateTime<Utc>>,
    pub message: String,
}

pub trait PendingLoginStore: Send + Sync + 'static {
    fn put(&self, payload: Vec<u8>, expires_at: DateTime<Utc>) -> Result<String>;
    fn get(&self, id: &str) -> Result<Option<Vec<u8>>>;
    fn remove(&self, id: &str) -> Result<()>;
}

impl<S: PendingLoginStore> PendingLoginStore for Arc<S> {
    fn put(&self, payload: Vec<u8>, expires_at: DateTime<Utc>) -> Result<String> {
        self.as_ref().put(payload, expires_at)
    }

    fn get(&self, id: &str) -> Result<Option<Vec<u8>>> {
        self.as_ref().get(id)
    }

    fn remove(&self, id: &str) -> Result<()> {
        self.as_ref().remove(id)
    }
}

#[derive(Default)]
pub struct MemoryPendingLoginStore {
    entries: Mutex<PendingEntries>,
}

type PendingEntries = HashMap<String, PendingEntry>;

struct PendingEntry {
    expires_at: DateTime<Utc>,
    payload: Zeroizing<Vec<u8>>,
}

impl PendingLoginStore for MemoryPendingLoginStore {
    fn put(&self, payload: Vec<u8>, expires_at: DateTime<Utc>) -> Result<String> {
        let id = random_token();
        let mut entries = self.entries.lock().map_err(|_| {
            AppleHmeError::new(
                AppleHmeErrorCode::Protocol,
                "Apple 待验证登录状态不可用",
                true,
            )
        })?;
        entries.retain(|_, entry| entry.expires_at > Utc::now());
        entries.insert(
            id.clone(),
            PendingEntry {
                expires_at,
                payload: Zeroizing::new(payload),
            },
        );
        Ok(id)
    }

    fn get(&self, id: &str) -> Result<Option<Vec<u8>>> {
        let mut entries = self.entries.lock().map_err(|_| {
            AppleHmeError::new(
                AppleHmeErrorCode::Protocol,
                "Apple 待验证登录状态不可用",
                true,
            )
        })?;
        entries.retain(|_, entry| entry.expires_at > Utc::now());
        Ok(entries
            .get(id.trim())
            .map(|entry| entry.payload.as_slice().to_vec()))
    }

    fn remove(&self, id: &str) -> Result<()> {
        self.entries
            .lock()
            .map_err(|_| {
                AppleHmeError::new(
                    AppleHmeErrorCode::Protocol,
                    "Apple 待验证登录状态不可用",
                    true,
                )
            })?
            .remove(id.trim());
        Ok(())
    }
}

pub struct AppleAuthClient<T = UreqTransport, S = MemoryPendingLoginStore> {
    transport: T,
    pending: S,
}

impl Default for AppleAuthClient<UreqTransport, MemoryPendingLoginStore> {
    fn default() -> Self {
        Self::new(UreqTransport::default(), MemoryPendingLoginStore::default())
    }
}

impl<T: HttpTransport, S: PendingLoginStore> AppleAuthClient<T, S> {
    pub fn new(transport: T, pending: S) -> Self {
        Self { transport, pending }
    }

    pub fn start_login(&self, mut request: LoginRequest) -> Result<LoginResult> {
        request.apple_id = request.apple_id.trim().to_ascii_lowercase();
        if request.apple_id.is_empty() || request.password.is_empty() {
            return Err(AppleHmeError::invalid("缺少 Apple 账户或密码"));
        }
        let password = Zeroizing::new(std::mem::take(&mut request.password));
        for attempt in 0..2 {
            let mut state = AuthState::new(&request);
            if state.flow == LoginStateKind::AppleAccount {
                self.prime_apple_account(&mut state)
                    .map_err(|error| login_stage(error, "Apple Account 初始化"))?;
            }
            self.authorize(&mut state)
                .map_err(|error| login_stage(error, "Apple 授权初始化"))?;
            if state.flow == LoginStateKind::AppleAccount {
                self.device_key_challenge(&mut state)
                    .map_err(|error| login_stage(error, "Apple 设备挑战"))?;
            }
            self.federate(&mut state)
                .map_err(|error| login_stage(error, "Apple 账户识别"))?;
            let needs_two_factor = self
                .srp_sign_in(&mut state, password.as_str())
                .map_err(|error| login_stage(error, "Apple 密码验证"))?;

            if let Some(host) = state.account_country_redirect_host() {
                if attempt == 0 {
                    // Apple authentication state is scoped to the issuing domain. Do not
                    // carry cookies/scnt/session-id across the .com and .com.cn boundary;
                    // restart the entire SRP flow on the account's actual regional host.
                    request.icloud_host = Some(host.into());
                    continue;
                }
                return Err(AppleHmeError::new(
                    AppleHmeErrorCode::Protocol,
                    "Apple 登录区域切换后仍未进入正确域名，请重新登录",
                    true,
                ));
            }

            if needs_two_factor {
                let message = self.prepare_two_factor(&mut state, request.phone_number.as_ref())?;
                let expires_at = Utc::now() + Duration::minutes(PENDING_TTL_MINUTES);
                let payload = serde_json::to_vec(&state).map_err(|_| {
                    AppleHmeError::new(
                        AppleHmeErrorCode::Protocol,
                        "Apple 待验证登录状态无法保存",
                        true,
                    )
                })?;
                let pending_id = self.pending.put(payload, expires_at)?;
                return Ok(LoginResult {
                    session: None,
                    pending_id: Some(pending_id),
                    needs_two_factor: true,
                    expires_at: Some(expires_at),
                    message: message.into(),
                });
            }
            let session = self
                .finish_login(&mut state)
                .map_err(|error| login_stage(error, "Apple 会话建立"))?;
            return Ok(LoginResult {
                session: Some(session),
                pending_id: None,
                needs_two_factor: false,
                expires_at: None,
                message: "Apple 登录成功".into(),
            });
        }
        unreachable!("Apple regional login retries are bounded")
    }

    pub fn submit_two_factor(
        &self,
        pending_id: &str,
        code: &str,
        phone_number: Option<Value>,
    ) -> Result<AppleSession> {
        let code = code.trim();
        if code.len() != 6 || !code.bytes().all(|value| value.is_ascii_digit()) {
            return Err(AppleHmeError::invalid(
                "Apple 双重认证验证码必须是 6 位数字",
            ));
        }
        let payload = self.pending.get(pending_id)?.ok_or_else(|| {
            AppleHmeError::new(
                AppleHmeErrorCode::PendingLoginExpired,
                "Apple 登录验证已过期，请重新登录",
                true,
            )
        })?;
        let mut state: AuthState = serde_json::from_slice(&payload).map_err(|_| {
            AppleHmeError::new(
                AppleHmeErrorCode::PendingLoginExpired,
                "Apple 登录验证状态无效，请重新登录",
                true,
            )
        })?;
        match state.two_factor_method {
            TwoFactorMethod::TrustedDevice => self
                .verify_trusted_device_code(&mut state, code)
                .map_err(|error| login_stage(error, "双重认证验证码"))?,
            TwoFactorMethod::Phone => {
                let phone = phone_number.or(state.phone_number.clone()).ok_or_else(|| {
                    AppleHmeError::invalid("短信验证需要 Apple 返回的 phoneNumber")
                })?;
                self.verify_phone_code(&mut state, code, &phone)
                    .map_err(|error| login_stage(error, "短信验证码"))?;
            }
        }
        let trusted = self.trust_session(&mut state);
        if state.flow == LoginStateKind::ICloudWeb {
            trusted.map_err(|error| login_stage(error, "信任浏览器"))?;
        }
        let finish_stage = if state.flow == LoginStateKind::ICloudWeb {
            "iCloud 会话建立"
        } else {
            "Apple Account 会话建立"
        };
        let session = self
            .finish_login(&mut state)
            .map_err(|error| login_stage(error, finish_stage))?;
        self.pending.remove(pending_id)?;
        Ok(session)
    }

    fn authorize(&self, state: &mut AuthState) -> Result<()> {
        let frame = format!("auth-{}", state.frame_id);
        let mut url = Url::parse(&format!("{}/authorize/signin", state.auth_base))
            .map_err(|_| AppleHmeError::invalid("Apple 授权地址无效"))?;
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("frame_id", &frame);
            query.append_pair("skVersion", "7");
            query.append_pair("iframeId", &frame);
            query.append_pair("client_id", &state.client_id);
            query.append_pair("redirect_uri", &state.home);
            query.append_pair("response_type", "code");
            query.append_pair("response_mode", "web_message");
            query.append_pair("state", &frame);
            if state.flow == LoginStateKind::AppleAccount {
                query.append_pair("authVersion", "8.0.2");
            } else {
                query.append_pair("language", "zh_CN");
                query.append_pair("authVersion", "latest");
            }
        }
        let mut extra = BTreeMap::new();
        if state.flow == LoginStateKind::AppleAccount {
            extra.insert(
                "Accept".into(),
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8".into(),
            );
            extra.insert("Sec-Fetch-Dest".into(), "iframe".into());
            extra.insert("Sec-Fetch-Mode".into(), "navigate".into());
            extra.insert("Sec-Fetch-Site".into(), "same-site".into());
        } else {
            extra.insert("Accept".into(), "*/*".into());
        }
        let response =
            self.request_with_extra(state, HttpMethod::Get, url.to_string(), false, None, extra)?;
        self.require_success(&response, false)?;
        state.complete_hashcash_bits = state.hashcash_bits;
        state.complete_hashcash_challenge = state.hashcash_challenge.clone();
        Ok(())
    }

    fn device_key_challenge(&self, state: &mut AuthState) -> Result<()> {
        let url = format!("{}/verify/device/key/challenge", state.auth_base);
        let previous_scnt = state.scnt.take();
        let previous_session_id = state.session_id.take();
        let response = self.request(
            state,
            HttpMethod::Post,
            url,
            true,
            Some(json!({"passkeyAutofill": false})),
        )?;
        if state.scnt.is_none() {
            state.scnt = previous_scnt;
        }
        if state.session_id.is_none() {
            state.session_id = previous_session_id;
        }
        self.require_success(&response, false)
    }

    fn federate(&self, state: &mut AuthState) -> Result<()> {
        let url = format!("{}/federate?isRememberMeEnabled=true", state.auth_base);
        let apple_id = state.apple_id.clone();
        let response = self.request(
            state,
            HttpMethod::Post,
            url,
            true,
            Some(json!({"accountName": apple_id, "rememberMe": true})),
        )?;
        self.require_success(&response, false)
    }

    fn srp_sign_in(&self, state: &mut AuthState, password: &str) -> Result<bool> {
        let srp = SrpClient::random()?;
        let apple_id = state.apple_id.clone();
        let response = self.request(
            state,
            HttpMethod::Post,
            format!("{}/signin/init", state.auth_base),
            true,
            Some(json!({
                "a": BASE64.encode(srp.public_a()),
                "accountName": apple_id,
                "protocols": ["s2k", "s2k_fo"]
            })),
        )?;
        self.require_success(&response, false)?;
        let challenge: SrpChallenge = decode_json(&response)?;
        let salt = BASE64
            .decode(challenge.salt)
            .map_err(|_| AppleHmeError::bad_response("Apple SRP salt 无法解析"))?;
        let server_b = BASE64
            .decode(challenge.b)
            .map_err(|_| AppleHmeError::bad_response("Apple SRP B 无法解析"))?;
        let proof = srp.prove(
            state.apple_id.as_bytes(),
            password,
            &challenge.protocol,
            challenge.iteration,
            &salt,
            &server_b,
        )?;
        let mut body = json!({
            "accountName": state.apple_id,
            "m1": BASE64.encode(proof.m1),
            "m2": BASE64.encode(proof.m2),
            "c": challenge.c,
            "rememberMe": true
        });
        if state.flow == LoginStateKind::ICloudWeb {
            body["trustTokens"] = json!(state
                .trust_token
                .as_ref()
                .map(|value| vec![value.clone()])
                .unwrap_or_default());
        }
        let mut extra = BTreeMap::new();
        if state.flow == LoginStateKind::AppleAccount {
            extra.insert("X-Apple-HC".into(), hashcash(state)?);
        }
        let response = self.request_with_extra(
            state,
            HttpMethod::Post,
            format!(
                "{}/signin/complete?isRememberMeEnabled=true",
                state.auth_base
            ),
            true,
            Some(body),
            extra,
        )?;
        if response.status == 401 {
            return Err(AppleHmeError::new(
                AppleHmeErrorCode::CredentialsInvalid,
                "Apple 账户或密码错误",
                false,
            ));
        }
        if response.status == 409 {
            return Ok(true);
        }
        self.require_success(&response, false)?;
        Ok(false)
    }

    fn request_phone_code(&self, state: &mut AuthState, phone: &Value) -> Result<()> {
        validate_phone_number(phone)?;
        state.phone_number = Some(phone.clone());
        let response = self.request(
            state,
            HttpMethod::Put,
            format!("{}/verify/phone", state.auth_base),
            true,
            Some(json!({"phoneNumber": phone, "mode": "sms"})),
        )?;
        self.require_success(&response, false)
    }

    fn prepare_two_factor(
        &self,
        state: &mut AuthState,
        phone_number: Option<&Value>,
    ) -> Result<&'static str> {
        match state.two_factor_method {
            TwoFactorMethod::TrustedDevice => {
                if state.flow == LoginStateKind::AppleAccount {
                    return Ok("Apple Account 已向受信任设备发送验证码，请提交 6 位验证码");
                }
                // signin/complete has already moved the Apple session into HSA2 and
                // frequently pushes the code before this best-effort request returns.
                // Apple may answer this extra request with 401/409 even though the
                // challenge is usable, so it must never prevent us from preserving the
                // pending state and showing the code form.
                Ok(if self.request_trusted_device_code(state).is_ok() {
                    "Apple 已向受信任设备发送验证码，请提交 6 位验证码"
                } else {
                    "Apple 已要求双重认证；请查看受信任设备并提交 6 位验证码"
                })
            }
            TwoFactorMethod::Phone => {
                let phone = phone_number.ok_or_else(|| {
                    AppleHmeError::new(
                        AppleHmeErrorCode::InvalidInput,
                        "短信验证需要 Apple 返回的 phoneNumber",
                        false,
                    )
                })?;
                self.request_phone_code(state, phone)?;
                Ok("Apple 已向受信任手机号发送短信验证码，请提交 6 位验证码")
            }
        }
    }

    fn request_trusted_device_code(&self, state: &mut AuthState) -> Result<()> {
        let response = self.request(
            state,
            HttpMethod::Put,
            format!("{}/verify/trusteddevice/securitycode", state.auth_base),
            true,
            None,
        )?;
        self.require_success(&response, false)
    }

    fn verify_trusted_device_code(&self, state: &mut AuthState, code: &str) -> Result<()> {
        let response = self.request(
            state,
            HttpMethod::Post,
            format!("{}/verify/trusteddevice/securitycode", state.auth_base),
            true,
            Some(json!({"securityCode": {"code": code}})),
        )?;
        if approved_apple_redirect(&response) {
            if let Some(host) = redirect_icloud_host(&response) {
                state.switch_icloud_host(&host);
            }
            return Ok(());
        }
        self.require_two_factor_success(&response)
    }

    fn verify_phone_code(&self, state: &mut AuthState, code: &str, phone: &Value) -> Result<()> {
        validate_phone_number(phone)?;
        let response = self.request(
            state,
            HttpMethod::Post,
            format!("{}/verify/phone/securitycode", state.auth_base),
            true,
            Some(json!({
                "phoneNumber": phone,
                "securityCode": {"code": code},
                "mode": "sms"
            })),
        )?;
        if approved_apple_redirect(&response) {
            if let Some(host) = redirect_icloud_host(&response) {
                state.switch_icloud_host(&host);
            }
            return Ok(());
        }
        self.require_two_factor_success(&response)
    }

    fn trust_session(&self, state: &mut AuthState) -> Result<()> {
        let response = self.request(
            state,
            HttpMethod::Get,
            format!("{}/2sv/trust", state.auth_base),
            true,
            None,
        )?;
        if approved_apple_redirect(&response) {
            if let Some(host) = redirect_icloud_host(&response) {
                state.switch_icloud_host(&host);
            }
            return Ok(());
        }
        self.require_success(&response, false)
    }

    fn finish_login(&self, state: &mut AuthState) -> Result<AppleSession> {
        match state.flow {
            LoginStateKind::ICloudWeb => self.finish_icloud_web(state),
            LoginStateKind::AppleAccount => self.finish_apple_account(state),
        }
    }

    fn finish_icloud_web(&self, state: &mut AuthState) -> Result<AppleSession> {
        let session_token = state.session_token.clone().ok_or_else(|| {
            AppleHmeError::new(
                AppleHmeErrorCode::SessionMissing,
                "Apple 登录未返回 Session Token",
                true,
            )
        })?;
        let mut response = self.account_login(state, &session_token)?;
        if let Some(host) = redirect_icloud_host(&response) {
            if state.switch_icloud_host(&host) {
                response = self.account_login(state, &session_token)?;
            }
        }
        if !approved_apple_redirect(&response) {
            self.require_success(&response, false)?;
        }
        self.validate_icloud_web(state)
    }

    fn account_login(&self, state: &mut AuthState, session_token: &str) -> Result<HttpResponse> {
        let mut headers = common_browser_headers(ICLOUD_WEB_USER_AGENT, &state.home);
        headers.insert("Content-Type".into(), "application/json".into());
        execute(
            &self.transport,
            HttpMethod::Post,
            format!("{}/accountLogin", state.setup_base),
            headers,
            Some(json_bytes(&json!({
                "accountCountryCode": state.account_country,
                "dsWebAuthToken": session_token,
                "extended_login": true,
                "trustToken": state.trust_token
            }))?),
            &mut state.cookies,
        )
    }

    fn validate_icloud_web(&self, state: &mut AuthState) -> Result<AppleSession> {
        let client_id = Uuid::new_v4().to_string();
        let request_id = Uuid::new_v4().to_string();
        let setup_host = if state.host.ends_with("icloud.com.cn") {
            "setup.icloud.com.cn"
        } else {
            "setup.icloud.com"
        };
        let mut url = Url::parse(&format!("https://{setup_host}/setup/ws/1/validate"))
            .map_err(|_| AppleHmeError::invalid("iCloud validate 地址无效"))?;
        url.query_pairs_mut()
            .append_pair("clientBuildNumber", ICLOUD_BUILD_NUMBER)
            .append_pair("clientMasteringNumber", ICLOUD_BUILD_NUMBER)
            .append_pair("clientId", &client_id)
            .append_pair("requestId", &request_id);
        let mut headers = common_browser_headers(ICLOUD_WEB_USER_AGENT, &state.home);
        headers.insert("Content-Type".into(), "text/plain;charset=UTF-8".into());
        let response = execute(
            &self.transport,
            HttpMethod::Post,
            url.to_string(),
            headers,
            None,
            &mut state.cookies,
        )?;
        self.require_success(&response, false)?;
        let account: ICloudValidate = decode_json(&response)?;
        let premium = account.service_url("premiummailsettings");
        let now = Utc::now();
        let login_state = LoginState {
            kind: LoginStateKind::ICloudWeb,
            host: state.host.clone(),
            origin: state.home.clone(),
            cookies: state.cookies.clone(),
            scnt: None,
            session_id: None,
            api_key: None,
            data_access_token: None,
            user_agent: ICLOUD_WEB_USER_AGENT.into(),
            saved_at: now,
            manage_expires_at: None,
            last_checked_at: Some(now),
            last_check_ok: true,
            last_status_message: Some("iCloud Web 登录态正常".into()),
            last_successful_keepalive_at: None,
        };
        Ok(AppleSession {
            apple_id: first_non_empty(&[
                &account.ds_info.apple_id,
                &account.ds_info.primary_email,
                &state.apple_id,
            ]),
            dsid: nonempty(account.ds_info.dsid.clone()),
            client_id: Some(client_id),
            client_build_number: Some(ICLOUD_BUILD_NUMBER.into()),
            client_mastering_number: Some(ICLOUD_BUILD_NUMBER.into()),
            premium_mail_base_url: nonempty(premium),
            mail_gateway_base_url: nonempty(account.service_url("mccgateway")),
            mail_base_url: nonempty(account.service_url("mail")),
            host: state.host.clone(),
            is_icloud_plus: account.ds_info.is_hide_my_email_subscription_active,
            can_create_hme: account.ds_info.is_hide_my_email_feature_available,
            login_states: vec![login_state],
            saved_at: now,
        })
    }

    fn finish_apple_account(&self, state: &mut AuthState) -> Result<AppleSession> {
        let now = Utc::now();
        let mut login_state = LoginState {
            kind: LoginStateKind::AppleAccount,
            host: "appleid.apple.com".into(),
            origin: "https://account.apple.com".into(),
            cookies: state.cookies.clone(),
            scnt: state.scnt.clone().or_else(|| state.manage_scnt.clone()),
            session_id: state.session_id.clone(),
            api_key: None,
            data_access_token: None,
            user_agent: APPLE_ACCOUNT_USER_AGENT.into(),
            saved_at: now,
            manage_expires_at: None,
            last_checked_at: None,
            last_check_ok: false,
            last_status_message: None,
            last_successful_keepalive_at: None,
        };
        self.refresh_apple_account_state(&mut login_state)?;
        Ok(AppleSession {
            apple_id: state.apple_id.clone(),
            dsid: None,
            client_id: None,
            client_build_number: None,
            client_mastering_number: None,
            premium_mail_base_url: None,
            mail_gateway_base_url: None,
            mail_base_url: None,
            host: "appleid.apple.com".into(),
            is_icloud_plus: false,
            can_create_hme: true,
            login_states: vec![login_state],
            saved_at: now,
        })
    }

    fn prime_apple_account(&self, state: &mut AuthState) -> Result<()> {
        let mut cookies = Vec::new();
        for path in ["/account/manage/section/privacy", "/bootstrap/portal"] {
            let response = execute(
                &self.transport,
                HttpMethod::Get,
                format!("https://account.apple.com{path}"),
                apple_account_portal_headers(path),
                None,
                &mut cookies,
            )?;
            if !(200..300).contains(&response.status) && !is_login_challenge(&response) {
                self.require_success(&response, false)?;
            }
        }
        let response = execute(
            &self.transport,
            HttpMethod::Get,
            "https://appleid.apple.com/account/manage/gs/ws/token".into(),
            apple_account_api_headers(None, None),
            None,
            &mut cookies,
        )?;
        let challenge_scnt = response
            .header("scnt")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        // Apple may intentionally answer unauthenticated priming requests with a
        // redirect/401 plus a fresh scnt challenge. That is the start of a login,
        // not an expired persisted session. Match the reference Go flow and continue.
        if !(200..300).contains(&response.status) && !is_login_challenge(&response) {
            self.require_success(&response, false)?;
        }
        state.manage_scnt = challenge_scnt;
        state.cookies = cookies;
        Ok(())
    }

    fn refresh_apple_account_state(&self, state: &mut LoginState) -> Result<()> {
        let response = execute(
            &self.transport,
            HttpMethod::Get,
            "https://appleid.apple.com/account/manage/gs/ws/token".into(),
            apple_account_api_headers(state.scnt.as_deref(), None),
            None,
            &mut state.cookies,
        )?;
        self.require_success(&response, true)?;
        if let Some(scnt) = response.header("scnt") {
            state.scnt = Some(scnt.into());
        }
        if let Ok(token) = serde_json::from_slice::<ManageToken>(&response.body) {
            if token.time_out_interval > 0 {
                state.manage_expires_at =
                    Some(Utc::now() + Duration::minutes(token.time_out_interval));
            }
        }
        let response = execute(
            &self.transport,
            HttpMethod::Get,
            "https://appleid.apple.com/account/manage".into(),
            apple_account_api_headers(state.scnt.as_deref(), None),
            None,
            &mut state.cookies,
        )?;
        self.require_success(&response, true)?;
        let manage: ManageResponse = decode_json(&response)?;
        if manage.api_key.trim().is_empty() {
            return Err(AppleHmeError::bad_response(
                "Apple Account 管理接口未返回 API Key",
            ));
        }
        state.api_key = Some(manage.api_key);
        state.saved_at = Utc::now();
        state.last_checked_at = Some(Utc::now());
        state.last_check_ok = true;
        state.last_status_message = Some("Apple Account 管理态正常".into());
        Ok(())
    }

    fn request(
        &self,
        state: &mut AuthState,
        method: HttpMethod,
        url: String,
        json_content: bool,
        body: Option<Value>,
    ) -> Result<HttpResponse> {
        self.request_with_extra(state, method, url, json_content, body, BTreeMap::new())
    }

    fn request_with_extra(
        &self,
        state: &mut AuthState,
        method: HttpMethod,
        url: String,
        json_content: bool,
        body: Option<Value>,
        extra: BTreeMap<String, String>,
    ) -> Result<HttpResponse> {
        let mut headers = self.auth_headers(state, json_content);
        headers.extend(extra);
        if state.flow == LoginStateKind::AppleAccount && url.contains("/verify/") {
            headers.remove("X-Requested-With");
            if !url.ends_with("/verify/device/key/challenge") {
                headers.insert("X-Apple-App-Id".into(), state.client_id.clone());
            }
        }
        let response = execute(
            &self.transport,
            method,
            url,
            headers,
            body.as_ref().map(json_bytes).transpose()?,
            &mut state.cookies,
        )?;
        state.extract(&response);
        Ok(response)
    }

    fn auth_headers(&self, state: &AuthState, json_content: bool) -> BTreeMap<String, String> {
        let auth_origin = state
            .auth_base
            .trim_end_matches("/appleauth/auth")
            .to_string();
        let frame = format!("auth-{}", state.frame_id);
        let mut headers = header_map(&[
            ("Accept", "application/json".into()),
            ("Origin", auth_origin.clone()),
            ("Referer", format!("{auth_origin}/")),
            ("User-Agent", state.user_agent.clone()),
            ("X-Apple-Widget-Key", state.client_id.clone()),
            ("X-Apple-OAuth-Client-Id", state.client_id.clone()),
            ("X-Apple-OAuth-Client-Type", "firstPartyAuth".into()),
            ("X-Apple-OAuth-Redirect-URI", state.home.clone()),
            ("X-Apple-OAuth-Require-Grant-Code", "true".into()),
            ("X-Apple-OAuth-Response-Mode", "web_message".into()),
            ("X-Apple-OAuth-Response-Type", "code".into()),
            ("X-Apple-OAuth-State", frame.clone()),
            ("X-Apple-Frame-Id", frame),
            ("X-Requested-With", "XMLHttpRequest".into()),
            (
                "X-Apple-I-FD-Client-Info",
                fd_client_info(&state.user_agent),
            ),
            ("X-Apple-Mandate-Security-Upgrade", "0".into()),
            ("X-Apple-I-Require-UE", "true".into()),
        ]);
        if json_content {
            headers.insert("Content-Type".into(), "application/json".into());
        }
        if let Some(value) = &state.auth_attributes {
            headers.insert("X-Apple-Auth-Attributes".into(), value.clone());
        }
        if let Some(value) = &state.scnt {
            headers.insert("scnt".into(), value.clone());
        }
        if let Some(value) = &state.session_id {
            headers.insert("X-Apple-ID-Session-Id".into(), value.clone());
        }
        if let Some(value) = &state.session_token {
            headers.insert("X-Apple-Session-Token".into(), value.clone());
        }
        if state.flow == LoginStateKind::AppleAccount {
            headers.remove("X-Apple-OAuth-Require-Grant-Code");
            headers.remove("X-Apple-Mandate-Security-Upgrade");
            headers.remove("X-Apple-I-Require-UE");
            headers.insert("X-Apple-Domain-Id".into(), "11".into());
            headers.insert("X-Apple-Privacy-Consent".into(), "true".into());
            headers.insert("X-Apple-Privacy-Consent-Accepted".into(), "true".into());
            headers.insert("Accept-Language".into(), "zh,en;q=0.9".into());
            headers.insert("Sec-Fetch-Dest".into(), "empty".into());
            headers.insert("Sec-Fetch-Mode".into(), "cors".into());
            headers.insert("Sec-Fetch-Site".into(), "same-origin".into());
        }
        headers
    }

    fn require_success(&self, response: &HttpResponse, account_management: bool) -> Result<()> {
        if (200..300).contains(&response.status) {
            return Ok(());
        }
        let detail = response_text(response).to_ascii_lowercase();
        if response.status == 401 || response.status == 419 {
            return Err(AppleHmeError::new(
                AppleHmeErrorCode::SessionExpired,
                "Apple 拒绝了当前登录步骤，请重新提交账户和密码",
                true,
            ));
        }
        if response.status == 403 && !account_management {
            return Err(AppleHmeError::new(
                AppleHmeErrorCode::CredentialsInvalid,
                "Apple 拒绝登录，请检查账户密码或安全状态",
                false,
            ));
        }
        if detail.contains("limit") || detail.contains("too many") {
            return Err(AppleHmeError::new(
                AppleHmeErrorCode::AddressLimitReached,
                "Apple Hide My Email 已达到当前创建限制",
                true,
            ));
        }
        Err(AppleHmeError::new(
            AppleHmeErrorCode::Protocol,
            format!("Apple 协议请求失败，HTTP {}", response.status),
            response.status >= 500,
        ))
    }

    fn require_two_factor_success(&self, response: &HttpResponse) -> Result<()> {
        if (200..300).contains(&response.status)
            || (response.status == 409 && two_factor_code_was_accepted(response))
        {
            return Ok(());
        }
        Err(AppleHmeError::new(
            AppleHmeErrorCode::TwoFactorInvalid,
            format!("Apple 双重认证失败，HTTP {}", response.status),
            response.status >= 500,
        ))
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthState {
    flow: LoginStateKind,
    apple_id: String,
    client_id: String,
    frame_id: String,
    host: String,
    home: String,
    setup_base: String,
    auth_base: String,
    user_agent: String,
    two_factor_method: TwoFactorMethod,
    phone_number: Option<Value>,
    cookies: Vec<AppleCookie>,
    session_token: Option<String>,
    scnt: Option<String>,
    manage_scnt: Option<String>,
    session_id: Option<String>,
    account_country: Option<String>,
    trust_token: Option<String>,
    auth_attributes: Option<String>,
    hashcash_bits: Option<u32>,
    hashcash_challenge: Option<String>,
    complete_hashcash_bits: Option<u32>,
    complete_hashcash_challenge: Option<String>,
}

impl AuthState {
    fn new(request: &LoginRequest) -> Self {
        let is_account = request.kind == LoginStateKind::AppleAccount;
        let china = request
            .icloud_host
            .as_deref()
            .unwrap_or("www.icloud.com.cn")
            .contains("icloud.com.cn");
        let host = if china {
            "www.icloud.com.cn"
        } else {
            "www.icloud.com"
        };
        Self {
            flow: request.kind,
            apple_id: request.apple_id.clone(),
            client_id: if is_account {
                APPLE_ACCOUNT_CLIENT_ID.into()
            } else {
                request
                    .client_id
                    .clone()
                    .unwrap_or_else(|| DEFAULT_ICLOUD_WEB_CLIENT_ID.into())
            },
            frame_id: Uuid::new_v4().to_string().to_ascii_lowercase(),
            host: if is_account {
                "appleid.apple.com".into()
            } else {
                host.into()
            },
            home: if is_account {
                "https://account.apple.com".into()
            } else if china {
                "https://www.icloud.com.cn".into()
            } else {
                "https://www.icloud.com".into()
            },
            setup_base: if china {
                "https://setup.icloud.com.cn/setup/ws/1".into()
            } else {
                "https://setup.icloud.com/setup/ws/1".into()
            },
            auth_base: if is_account || !china {
                "https://idmsa.apple.com/appleauth/auth".into()
            } else {
                "https://idmsa.apple.com.cn/appleauth/auth".into()
            },
            user_agent: if is_account {
                APPLE_ACCOUNT_USER_AGENT.into()
            } else {
                ICLOUD_WEB_USER_AGENT.into()
            },
            two_factor_method: request.two_factor_method,
            phone_number: request.phone_number.clone(),
            cookies: Vec::new(),
            session_token: None,
            scnt: None,
            manage_scnt: None,
            session_id: None,
            account_country: None,
            trust_token: None,
            auth_attributes: None,
            hashcash_bits: None,
            hashcash_challenge: None,
            complete_hashcash_bits: None,
            complete_hashcash_challenge: None,
        }
    }

    fn account_country_redirect_host(&self) -> Option<&'static str> {
        if self.flow != LoginStateKind::ICloudWeb {
            return None;
        }
        let Some(country) = self.account_country.as_deref() else {
            return None;
        };
        let host = if matches!(country.trim().to_ascii_uppercase().as_str(), "CN" | "CHN") {
            "www.icloud.com.cn"
        } else {
            "www.icloud.com"
        };
        (!self.host.eq_ignore_ascii_case(host)).then_some(host)
    }

    fn switch_icloud_host(&mut self, host: &str) -> bool {
        if self.flow != LoginStateKind::ICloudWeb {
            return false;
        }
        let china = host.to_ascii_lowercase().contains("icloud.com.cn");
        let next_host = if china {
            "www.icloud.com.cn"
        } else {
            "www.icloud.com"
        };
        if self.host.eq_ignore_ascii_case(next_host) {
            return false;
        }
        self.host = next_host.into();
        self.home = if china {
            "https://www.icloud.com.cn".into()
        } else {
            "https://www.icloud.com".into()
        };
        self.setup_base = if china {
            "https://setup.icloud.com.cn/setup/ws/1".into()
        } else {
            "https://setup.icloud.com/setup/ws/1".into()
        };
        self.auth_base = if china {
            "https://idmsa.apple.com.cn/appleauth/auth".into()
        } else {
            "https://idmsa.apple.com/appleauth/auth".into()
        };
        true
    }

    fn extract(&mut self, response: &HttpResponse) {
        macro_rules! set_header {
            ($header:literal, $field:ident) => {
                if let Some(value) = response.header($header) {
                    if !value.trim().is_empty() {
                        self.$field = Some(value.trim().to_string());
                    }
                }
            };
        }
        set_header!("X-Apple-ID-Account-Country", account_country);
        set_header!("X-Apple-ID-Session-Id", session_id);
        set_header!("X-Apple-Session-Token", session_token);
        set_header!("X-Apple-TwoSV-Trust-Token", trust_token);
        set_header!("scnt", scnt);
        set_header!("X-Apple-Auth-Attributes", auth_attributes);
        set_header!("X-Apple-HC-Challenge", hashcash_challenge);
        if let Some(value) = response.header("X-Apple-HC-Bits") {
            self.hashcash_bits = value.parse().ok();
        }
    }
}

#[derive(Deserialize)]
struct SrpChallenge {
    iteration: u32,
    salt: String,
    protocol: String,
    b: String,
    c: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ICloudValidate {
    #[serde(rename = "dsInfo")]
    ds_info: ValidateDsInfo,
    #[serde(default)]
    webservices: BTreeMap<String, ValidateService>,
}

impl ICloudValidate {
    fn service_url(&self, name: &str) -> String {
        self.webservices
            .get(name)
            .map(|service| service.url.clone())
            .unwrap_or_default()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ValidateDsInfo {
    #[serde(default)]
    dsid: String,
    #[serde(default)]
    apple_id: String,
    #[serde(default)]
    primary_email: String,
    #[serde(default)]
    is_hide_my_email_subscription_active: bool,
    #[serde(default)]
    is_hide_my_email_feature_available: bool,
}

#[derive(Deserialize)]
struct ValidateService {
    #[serde(default)]
    url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManageToken {
    #[serde(default)]
    time_out_interval: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManageResponse {
    #[serde(default)]
    api_key: String,
}

fn common_browser_headers(user_agent: &str, origin: &str) -> BTreeMap<String, String> {
    header_map(&[
        ("Accept", "application/json, text/plain, */*".into()),
        ("Origin", origin.into()),
        ("Referer", format!("{}/", origin.trim_end_matches('/'))),
        ("User-Agent", user_agent.into()),
        ("Accept-Language", "zh-CN,zh;q=0.9".into()),
        ("Sec-Fetch-Site", "same-site".into()),
        ("Sec-Fetch-Mode", "cors".into()),
        ("Sec-Fetch-Dest", "empty".into()),
    ])
}

fn apple_account_api_headers(
    scnt: Option<&str>,
    api_key: Option<&str>,
) -> BTreeMap<String, String> {
    let mut headers = common_browser_headers(APPLE_ACCOUNT_USER_AGENT, "https://account.apple.com");
    headers.insert("Content-Type".into(), "application/json".into());
    headers.insert(
        "X-Apple-I-FD-Client-Info".into(),
        fd_client_info(APPLE_ACCOUNT_USER_AGENT),
    );
    headers.insert("X-Apple-I-Request-Context".into(), "ca".into());
    headers.insert("X-Apple-I-TimeZone".into(), "Asia/Shanghai".into());
    if let Some(scnt) = scnt.filter(|value| !value.trim().is_empty()) {
        headers.insert("scnt".into(), scnt.into());
    }
    if let Some(api_key) = api_key.filter(|value| !value.trim().is_empty()) {
        headers.insert("X-Apple-Api-Key".into(), api_key.into());
    }
    headers
}

fn apple_account_portal_headers(path: &str) -> BTreeMap<String, String> {
    let mut headers = common_browser_headers(APPLE_ACCOUNT_USER_AGENT, "https://account.apple.com");
    headers.remove("Origin");
    headers.insert("Sec-Fetch-Site".into(), "same-origin".into());
    if path == "/account/manage/section/privacy" {
        headers.insert(
            "Accept".into(),
            "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7".into(),
        );
        headers.insert("Sec-Fetch-Dest".into(), "document".into());
        headers.insert("Sec-Fetch-Mode".into(), "navigate".into());
    } else {
        headers.insert("Content-Type".into(), "application/json".into());
        headers.insert("Sec-Fetch-Dest".into(), "empty".into());
        headers.insert("Sec-Fetch-Mode".into(), "cors".into());
        headers.insert("X-Apple-I-Request-Context".into(), "ca".into());
        headers.insert("X-Apple-I-TimeZone".into(), "Asia/Shanghai".into());
        headers.insert(
            "X-Apple-I-FD-Client-Info".into(),
            fd_client_info(APPLE_ACCOUNT_USER_AGENT),
        );
    }
    headers
}

fn is_login_challenge(response: &HttpResponse) -> bool {
    matches!(response.status, 300..=399 | 401 | 419)
        && response
            .header("scnt")
            .is_some_and(|value| !value.trim().is_empty())
}

fn redirect_icloud_host(response: &HttpResponse) -> Option<String> {
    if !(300..400).contains(&response.status) {
        return None;
    }
    let domain = serde_json::from_slice::<Value>(&response.body)
        .ok()
        .and_then(|value| value.get("domainToUse")?.as_str().map(str::to_string))
        .or_else(|| response.header("Location").map(str::to_string))?;
    let normalized = domain.to_ascii_lowercase();
    if normalized.contains("icloud.com.cn") {
        Some("www.icloud.com.cn".into())
    } else if normalized.contains("icloud.com") {
        Some("www.icloud.com".into())
    } else {
        None
    }
}

fn approved_apple_redirect(response: &HttpResponse) -> bool {
    if !(300..400).contains(&response.status) {
        return false;
    }
    if redirect_icloud_host(response).is_some() {
        return true;
    }
    response.header("Location").is_some_and(|location| {
        if location.starts_with('/') {
            return true;
        }
        Url::parse(location)
            .ok()
            .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
            .is_some_and(|host| {
                host == "apple.com"
                    || host.ends_with(".apple.com")
                    || host == "apple.com.cn"
                    || host.ends_with(".apple.com.cn")
                    || host == "icloud.com"
                    || host.ends_with(".icloud.com")
                    || host == "icloud.com.cn"
                    || host.ends_with(".icloud.com.cn")
            })
    })
}

fn login_stage(mut error: AppleHmeError, stage: &str) -> AppleHmeError {
    error.message = format!("{}（阶段：{stage}）", error.message);
    error
}

fn fd_client_info(user_agent: &str) -> String {
    json!({
        "U": user_agent,
        "L": "zh-CN",
        "Z": "GMT+08:00",
        "V": "1.1",
        "F": ""
    })
    .to_string()
}

fn hashcash(state: &AuthState) -> Result<String> {
    let bits = state
        .complete_hashcash_bits
        .or(state.hashcash_bits)
        .ok_or_else(|| {
            AppleHmeError::new(
                AppleHmeErrorCode::Protocol,
                "Apple Account 缺少 Hashcash 难度",
                true,
            )
        })?;
    let challenge = state
        .complete_hashcash_challenge
        .as_deref()
        .or(state.hashcash_challenge.as_deref())
        .ok_or_else(|| {
            AppleHmeError::new(
                AppleHmeErrorCode::Protocol,
                "Apple Account 缺少 Hashcash challenge",
                true,
            )
        })?;
    if bits > 24 {
        return Err(AppleHmeError::new(
            AppleHmeErrorCode::Protocol,
            "Apple Account Hashcash 难度超出安全上限",
            true,
        ));
    }
    let timestamp = Utc::now().format("%Y%m%d%H%M%S");
    let prefix = format!("1:{bits}:{timestamp}:{challenge}::");
    for counter in 0_u64..(1_u64 << 24) {
        let value = format!("{prefix}{}", radix36(counter));
        let digest = Sha1::digest(value.as_bytes());
        if leading_zero_bits(&digest) >= bits {
            return Ok(value);
        }
    }
    Err(AppleHmeError::new(
        AppleHmeErrorCode::Protocol,
        "Apple Account Hashcash 计算失败",
        true,
    ))
}

fn leading_zero_bits(value: &[u8]) -> u32 {
    let mut total = 0;
    for byte in value {
        let zeros = byte.leading_zeros();
        total += zeros;
        if zeros != 8 {
            break;
        }
    }
    total
}

fn radix36(mut value: u64) -> String {
    if value == 0 {
        return "0".into();
    }
    let mut output = Vec::new();
    while value > 0 {
        let digit = (value % 36) as u8;
        output.push(if digit < 10 {
            b'0' + digit
        } else {
            b'a' + digit - 10
        });
        value /= 36;
    }
    output.reverse();
    String::from_utf8(output).unwrap_or_default()
}

fn validate_phone_number(phone: &Value) -> Result<()> {
    if phone.get("id").is_none() {
        return Err(AppleHmeError::invalid("Apple 短信验证 phoneNumber 缺少 id"));
    }
    Ok(())
}

fn two_factor_code_was_accepted(response: &HttpResponse) -> bool {
    serde_json::from_slice::<Value>(&response.body)
        .ok()
        .and_then(|body| {
            body.pointer("/securityCode/valid")
                .and_then(Value::as_bool)
                .or_else(|| {
                    body.pointer("/security_code/valid")
                        .and_then(Value::as_bool)
                })
        })
        .unwrap_or(false)
}

fn nonempty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then(|| value.trim().to_string())
}

fn first_non_empty(values: &[&str]) -> String {
    values
        .iter()
        .find(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string())
        .unwrap_or_default()
}

fn random_token() -> String {
    Uuid::new_v4().simple().to_string()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::transport::{HttpRequest, HttpTransport};

    #[derive(Clone)]
    struct ScriptedTransport {
        responses: Arc<Mutex<Vec<HttpResponse>>>,
        requests: Arc<Mutex<Vec<HttpRequest>>>,
    }

    impl HttpTransport for ScriptedTransport {
        fn execute(&self, request: HttpRequest) -> Result<HttpResponse> {
            self.requests.lock().unwrap().push(request);
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                return Err(AppleHmeError::bad_response("mock 响应不足"));
            }
            Ok(responses.remove(0))
        }
    }

    #[test]
    fn memory_pending_store_expires_and_removes_values() {
        let store = MemoryPendingLoginStore::default();
        let id = store
            .put(vec![1, 2], Utc::now() + Duration::minutes(1))
            .unwrap();
        assert_eq!(store.get(&id).unwrap(), Some(vec![1, 2]));
        store.remove(&id).unwrap();
        assert_eq!(store.get(&id).unwrap(), None);
        let expired = store
            .put(vec![3], Utc::now() - Duration::seconds(1))
            .unwrap();
        assert_eq!(store.get(&expired).unwrap(), None);
    }

    #[test]
    fn validates_phone_payload_and_hashcash_helpers() {
        assert!(validate_phone_number(&json!({"id": 1})).is_ok());
        assert!(validate_phone_number(&json!({"number": "+1"})).is_err());
        assert_eq!(radix36(35), "z");
        assert_eq!(radix36(36), "10");
        assert_eq!(leading_zero_bits(&[0, 0b0001_0000]), 11);
    }

    #[test]
    fn regional_icloud_redirect_requires_a_fresh_auth_state() {
        let initial = LoginRequest::icloud_web("owner@example.test", "secret");
        let mut state = AuthState::new(&initial);
        state.account_country = Some("USA".into());
        state.scnt = Some("old-domain-scnt".into());
        state.session_id = Some("old-domain-session".into());
        state.cookies.push(AppleCookie {
            name: "session".into(),
            value: "old-domain-cookie".into(),
            domain: "idmsa.apple.com.cn".into(),
            path: "/".into(),
            secure: true,
            http_only: true,
            expires_at: None,
        });

        let redirect = state.account_country_redirect_host().unwrap();
        let mut redirected = initial;
        redirected.icloud_host = Some(redirect.into());
        let fresh = AuthState::new(&redirected);

        assert_eq!(fresh.host, "www.icloud.com");
        assert!(fresh.scnt.is_none());
        assert!(fresh.session_id.is_none());
        assert!(fresh.cookies.is_empty());
    }

    #[test]
    fn icloud_auth_headers_match_reference_security_contract() {
        let client = AppleAuthClient::default();
        let state = AuthState::new(&LoginRequest::icloud_web("owner@example.test", "secret"));

        let headers = client.auth_headers(&state, true);

        assert_eq!(
            headers
                .get("X-Apple-OAuth-Require-Grant-Code")
                .map(String::as_str),
            Some("true")
        );
        assert_eq!(
            headers
                .get("X-Apple-Mandate-Security-Upgrade")
                .map(String::as_str),
            Some("0")
        );
        assert_eq!(
            headers.get("X-Apple-I-Require-UE").map(String::as_str),
            Some("true")
        );
    }

    #[test]
    fn accepts_conflict_when_apple_marks_two_factor_code_valid() {
        let accepted = HttpResponse {
            status: 409,
            headers: BTreeMap::new(),
            body: br#"{"securityCode":{"code":"******","valid":true}}"#.to_vec(),
        };
        let rejected = HttpResponse {
            status: 409,
            headers: BTreeMap::new(),
            body: br#"{"securityCode":{"code":"******","valid":false}}"#.to_vec(),
        };
        assert!(two_factor_code_was_accepted(&accepted));
        assert!(!two_factor_code_was_accepted(&rejected));
    }

    #[test]
    fn login_rejection_does_not_claim_an_established_session_expired() {
        let client = AppleAuthClient::default();
        let response = HttpResponse {
            status: 401,
            headers: BTreeMap::new(),
            body: Vec::new(),
        };

        let error = client.require_success(&response, false).unwrap_err();

        assert!(error.message.contains("当前登录步骤"));
        assert!(!error.message.contains("登录态"));
    }

    #[test]
    fn trusted_device_code_request_matches_reference_protocol() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let transport = ScriptedTransport {
            responses: Arc::new(Mutex::new(vec![HttpResponse {
                status: 202,
                headers: BTreeMap::new(),
                body: Vec::new(),
            }])),
            requests: Arc::clone(&requests),
        };
        let client = AppleAuthClient::new(transport, MemoryPendingLoginStore::default());
        let mut state = AuthState::new(&LoginRequest::icloud_web("owner@example.test", "secret"));

        client.request_trusted_device_code(&mut state).unwrap();

        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, HttpMethod::Put);
        assert!(requests[0]
            .url
            .ends_with("/verify/trusteddevice/securitycode"));
    }

    #[test]
    fn trusted_device_trigger_rejection_does_not_block_two_factor_form() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let transport = ScriptedTransport {
            responses: Arc::new(Mutex::new(vec![HttpResponse {
                status: 401,
                headers: BTreeMap::new(),
                body: Vec::new(),
            }])),
            requests,
        };
        let client = AppleAuthClient::new(transport, MemoryPendingLoginStore::default());
        let mut state = AuthState::new(&LoginRequest::icloud_web("owner@example.test", "secret"));

        let message = client.prepare_two_factor(&mut state, None).unwrap();

        assert!(message.contains("提交 6 位验证码"));
    }

    #[test]
    fn apple_account_two_factor_does_not_send_icloud_trigger_request() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let transport = ScriptedTransport {
            responses: Arc::new(Mutex::new(Vec::new())),
            requests: Arc::clone(&requests),
        };
        let client = AppleAuthClient::new(transport, MemoryPendingLoginStore::default());
        let mut state =
            AuthState::new(&LoginRequest::apple_account("owner@example.test", "secret"));

        let message = client.prepare_two_factor(&mut state, None).unwrap();

        assert!(message.contains("提交 6 位验证码"));
        assert!(requests.lock().unwrap().is_empty());
    }

    #[test]
    fn scripted_transport_is_injectable() {
        let transport = ScriptedTransport {
            responses: Arc::new(Mutex::new(vec![HttpResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: Vec::new(),
            }])),
            requests: Arc::new(Mutex::new(Vec::new())),
        };
        let response = transport
            .execute(crate::transport::HttpRequest {
                method: HttpMethod::Get,
                url: "https://example.test".into(),
                headers: BTreeMap::new(),
                body: None,
            })
            .unwrap();
        assert_eq!(response.status, 200);
    }

    #[test]
    fn apple_account_priming_accepts_unauthorized_scnt_challenge() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let transport = ScriptedTransport {
            responses: Arc::new(Mutex::new(vec![
                HttpResponse {
                    status: 401,
                    headers: BTreeMap::from([("scnt".into(), vec!["portal-challenge".into()])]),
                    body: Vec::new(),
                },
                HttpResponse {
                    status: 200,
                    headers: BTreeMap::new(),
                    body: Vec::new(),
                },
                HttpResponse {
                    status: 401,
                    headers: BTreeMap::from([("scnt".into(), vec!["fresh-challenge".into()])]),
                    body: br#"{"serviceErrors":[{"code":"UNAUTHORIZED"}]}"#.to_vec(),
                },
            ])),
            requests: Arc::clone(&requests),
        };
        let client = AppleAuthClient::new(transport, MemoryPendingLoginStore::default());
        let mut state =
            AuthState::new(&LoginRequest::apple_account("owner@example.test", "secret"));

        client.prime_apple_account(&mut state).unwrap();

        assert_eq!(state.manage_scnt.as_deref(), Some("fresh-challenge"));
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(!requests[0].headers.contains_key("Origin"));
        assert_eq!(
            requests[0]
                .headers
                .get("Sec-Fetch-Dest")
                .map(String::as_str),
            Some("document")
        );
        assert_eq!(
            requests[1]
                .headers
                .get("Sec-Fetch-Mode")
                .map(String::as_str),
            Some("cors")
        );
    }

    #[test]
    fn apple_account_device_challenge_omits_session_and_two_factor_headers() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let transport = ScriptedTransport {
            responses: Arc::new(Mutex::new(vec![HttpResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: Vec::new(),
            }])),
            requests: Arc::clone(&requests),
        };
        let client = AppleAuthClient::new(transport, MemoryPendingLoginStore::default());
        let mut state =
            AuthState::new(&LoginRequest::apple_account("owner@example.test", "secret"));
        state.scnt = Some("previous-scnt".into());
        state.session_id = Some("previous-session".into());

        client.device_key_challenge(&mut state).unwrap();

        let requests = requests.lock().unwrap();
        let headers = &requests[0].headers;
        assert!(!headers.contains_key("scnt"));
        assert!(!headers.contains_key("X-Apple-ID-Session-Id"));
        assert!(!headers.contains_key("X-Apple-App-Id"));
        assert!(!headers.contains_key("X-Requested-With"));
        assert_eq!(state.scnt.as_deref(), Some("previous-scnt"));
        assert_eq!(state.session_id.as_deref(), Some("previous-session"));
    }

    #[test]
    fn accepts_only_apple_owned_redirects_after_two_factor() {
        let approved = HttpResponse {
            status: 302,
            headers: BTreeMap::from([("Location".into(), vec!["https://www.icloud.com/".into()])]),
            body: Vec::new(),
        };
        let rejected = HttpResponse {
            status: 302,
            headers: BTreeMap::from([(
                "Location".into(),
                vec!["https://example.test/steal".into()],
            )]),
            body: Vec::new(),
        };
        let relative = HttpResponse {
            status: 302,
            headers: BTreeMap::from([("Location".into(), vec!["/".into()])]),
            body: Vec::new(),
        };
        assert!(approved_apple_redirect(&approved));
        assert!(approved_apple_redirect(&relative));
        assert!(!approved_apple_redirect(&rejected));
    }

    #[test]
    fn retries_icloud_account_login_on_region_redirect() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let transport = ScriptedTransport {
            responses: Arc::new(Mutex::new(vec![
                HttpResponse {
                    status: 302,
                    headers: BTreeMap::new(),
                    body: br#"{"domainToUse":"icloud.com"}"#.to_vec(),
                },
                HttpResponse {
                    status: 200,
                    headers: BTreeMap::new(),
                    body: br#"{}"#.to_vec(),
                },
                HttpResponse {
                    status: 200,
                    headers: BTreeMap::new(),
                    body: br#"{"dsInfo":{"dsid":"1","appleId":"owner@icloud.com","isHideMyEmailSubscriptionActive":true,"isHideMyEmailFeatureAvailable":true},"webservices":{"premiummailsettings":{"url":"https://premium.example"}}}"#.to_vec(),
                },
            ])),
            requests: Arc::clone(&requests),
        };
        let client = AppleAuthClient::new(transport, MemoryPendingLoginStore::default());
        let mut state = AuthState::new(&LoginRequest::icloud_web("owner@icloud.com", "secret"));
        state.session_token = Some("token".into());

        let session = client.finish_icloud_web(&mut state).unwrap();

        assert_eq!(session.host, "www.icloud.com");
        let requests = requests.lock().unwrap();
        assert!(requests[0].url.starts_with("https://setup.icloud.com.cn/"));
        assert!(requests[1].url.starts_with("https://setup.icloud.com/"));
        assert!(requests[2].url.starts_with("https://setup.icloud.com/"));
    }
}
