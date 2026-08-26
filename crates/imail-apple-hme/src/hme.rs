use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use url::Url;

use crate::{
    error::{AppleHmeError, AppleHmeErrorCode, Result},
    model::{AppleSession, HmeAddress, LoginState, LoginStateKind},
    protocol::{
        decode_json, execute, header_map, json_bytes, response_text, APPLE_ACCOUNT_USER_AGENT,
        ICLOUD_WEB_USER_AGENT,
    },
    transport::{HttpMethod, HttpResponse, HttpTransport, UreqTransport},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateChannel {
    Auto,
    AppleAccount,
    ICloudWeb,
}

#[derive(Debug, Clone)]
pub struct CreateHmeRequest {
    pub label: String,
    pub note: String,
    pub channel: CreateChannel,
}

#[derive(Debug, Clone)]
pub struct CreateHmeResult {
    pub address: HmeAddress,
    pub channel: CreateChannel,
}

pub struct AppleHmeClient<T = UreqTransport> {
    transport: T,
}

impl Default for AppleHmeClient<UreqTransport> {
    fn default() -> Self {
        Self::new(UreqTransport::default())
    }
}

impl<T: HttpTransport> AppleHmeClient<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    pub fn create(
        &self,
        session: &mut AppleSession,
        request: CreateHmeRequest,
    ) -> Result<CreateHmeResult> {
        let label = if request.label.trim().is_empty() {
            format!("iMail-{}", Utc::now().format("%m%d-%H%M%S"))
        } else {
            request.label.trim().to_string()
        };
        let note = request.note.trim().to_string();
        if matches!(
            request.channel,
            CreateChannel::Auto | CreateChannel::AppleAccount
        ) && session.state(LoginStateKind::AppleAccount).is_some()
        {
            match self.create_with_apple_account(session, &label, &note) {
                Ok(address) => {
                    return Ok(CreateHmeResult {
                        address,
                        channel: CreateChannel::AppleAccount,
                    })
                }
                Err(error)
                    if request.channel == CreateChannel::Auto
                        && error.code != AppleHmeErrorCode::AddressLimitReached
                        && session.state(LoginStateKind::ICloudWeb).is_some() => {}
                Err(error) => return Err(error),
            }
        } else if request.channel == CreateChannel::AppleAccount {
            return Err(session_missing("缺少 Apple Account 管理会话"));
        }
        if matches!(
            request.channel,
            CreateChannel::Auto | CreateChannel::ICloudWeb
        ) {
            let address = self.create_with_icloud_web(session, &label, &note)?;
            return Ok(CreateHmeResult {
                address,
                channel: CreateChannel::ICloudWeb,
            });
        }
        Err(session_missing("缺少可用的 Apple HME 会话"))
    }

    pub fn list(&self, session: &mut AppleSession) -> Result<Vec<HmeAddress>> {
        let response = self.call_icloud_web(session, HttpMethod::Get, "/v2/hme/list", None)?;
        let result: WebListResult = decode_web_result(&response)?;
        Ok(result
            .hme_emails
            .into_iter()
            .filter_map(WebAddress::into_address)
            .collect())
    }

    pub fn deactivate(&self, session: &mut AppleSession, anonymous_id: &str) -> Result<()> {
        let anonymous_id = required_id(anonymous_id)?;
        let response = self.call_icloud_web(
            session,
            HttpMethod::Post,
            "/v1/hme/deactivate",
            Some(json!({"anonymousId": anonymous_id})),
        )?;
        decode_web_empty(&response)
    }

    pub fn delete_inactive(&self, session: &mut AppleSession, anonymous_id: &str) -> Result<()> {
        let anonymous_id = required_id(anonymous_id)?;
        let addresses = self.list(session)?;
        let address = addresses
            .iter()
            .find(|address| address.anonymous_id == anonymous_id)
            .ok_or_else(|| {
                AppleHmeError::new(
                    AppleHmeErrorCode::AddressNotFound,
                    "Apple Hide My Email 地址不存在",
                    false,
                )
            })?;
        if address.active {
            return Err(AppleHmeError::new(
                AppleHmeErrorCode::InvalidInput,
                "必须先停用 Hide My Email 地址，才能永久删除",
                false,
            ));
        }
        let response = self.call_icloud_web(
            session,
            HttpMethod::Post,
            "/v1/hme/delete",
            Some(json!({"anonymousId": anonymous_id})),
        )?;
        decode_web_empty(&response)
    }

    pub fn refresh_apple_account(&self, session: &mut AppleSession) -> Result<()> {
        let state = session
            .state_mut(LoginStateKind::AppleAccount)
            .ok_or_else(|| session_missing("缺少 Apple Account 管理会话"))?;
        self.refresh_apple_account_state(state)
    }

    /// Performs the lightweight requests used by the Apple Account management
    /// page to keep its authenticated session active.
    pub fn keep_alive_apple_account(&self, session: &mut AppleSession) -> Result<()> {
        let result: Result<()> = (|| {
            let state = session
                .state_mut(LoginStateKind::AppleAccount)
                .ok_or_else(|| session_missing("缺少 Apple Account 管理会话"))?;
            self.refresh_apple_account_state(state)?;
            self.call_apple_account_raw(
                state,
                HttpMethod::Get,
                "/account/manage/forwardemail",
                None,
                true,
            )?;
            state.saved_at = Utc::now();
            Ok(())
        })();
        if let Some(state) = session.state_mut(LoginStateKind::AppleAccount) {
            let checked_at = Utc::now();
            state.last_checked_at = Some(checked_at);
            state.last_check_ok = result.is_ok();
            if result.is_ok() {
                state.last_successful_keepalive_at = Some(checked_at);
            }
            state.last_status_message = Some(match &result {
                Ok(()) => "Apple Account 会话保活正常".into(),
                Err(error) => error.message.clone(),
            });
        }
        result
    }

    /// Verifies the iCloud Web HME endpoint and refreshes its cookies.
    pub fn keep_alive_icloud_web(&self, session: &mut AppleSession) -> Result<()> {
        let result: Result<()> = (|| {
            let response = self.call_icloud_web(session, HttpMethod::Get, "/v2/hme/list", None)?;
            let _: WebListResult = decode_web_result(&response)?;
            Ok(())
        })();
        if let Some(state) = session.state_mut(LoginStateKind::ICloudWeb) {
            let checked_at = Utc::now();
            state.last_checked_at = Some(checked_at);
            state.last_check_ok = result.is_ok();
            if result.is_ok() {
                state.last_successful_keepalive_at = Some(checked_at);
            }
            state.last_status_message = Some(match &result {
                Ok(()) => "iCloud Web 会话保活正常".into(),
                Err(error) => error.message.clone(),
            });
        }
        result
    }

    fn create_with_icloud_web(
        &self,
        session: &mut AppleSession,
        label: &str,
        note: &str,
    ) -> Result<HmeAddress> {
        if !session.is_icloud_plus || !session.can_create_hme {
            return Err(AppleHmeError::new(
                AppleHmeErrorCode::SubscriptionRequired,
                "当前 Apple 账户没有可用的 iCloud+ Hide My Email 权限",
                false,
            ));
        }
        let response = self.call_icloud_web(
            session,
            HttpMethod::Post,
            "/v1/hme/generate",
            Some(json!({"langCode": "zh-cn"})),
        )?;
        let generated: WebGenerateResult = decode_web_result(&response)?;
        if generated.hme.trim().is_empty() {
            return Err(AppleHmeError::bad_response(
                "iCloud 未返回候选 Hide My Email 地址",
            ));
        }
        let response = self.call_icloud_web(
            session,
            HttpMethod::Post,
            "/v1/hme/reserve",
            Some(json!({"hme": generated.hme, "label": label, "note": note})),
        )?;
        let reserved: WebReserveResult = decode_web_result(&response)?;
        reserved
            .hme
            .into_address()
            .ok_or_else(|| AppleHmeError::bad_response("iCloud 创建后未返回 Hide My Email 地址"))
    }

    fn create_with_apple_account(
        &self,
        session: &mut AppleSession,
        label: &str,
        note: &str,
    ) -> Result<HmeAddress> {
        let state = session
            .state_mut(LoginStateKind::AppleAccount)
            .ok_or_else(|| session_missing("缺少 Apple Account 管理会话"))?;
        if !apple_account_state_usable(state) {
            self.refresh_apple_account_state(state)?;
        }
        let generated: AccountGenerated = self.call_apple_account(
            state,
            HttpMethod::Post,
            "/account/manage/email/private/add",
            Some(json!({})),
        )?;
        if generated.email_address.trim().is_empty() {
            return Err(AppleHmeError::bad_response(
                "Apple Account 未返回候选 Hide My Email 地址",
            ));
        }
        let completed: AccountCompleted = self.call_apple_account(
            state,
            HttpMethod::Put,
            "/account/manage/email/private/add/complete",
            Some(json!({
                "emailAddress": generated.email_address,
                "label": label,
                "note": note
            })),
        )?;
        let mut address = HmeAddress {
            anonymous_id: completed.id.trim().to_string(),
            email: first_non_empty(&[&completed.email_address, &generated.email_address])
                .to_ascii_lowercase(),
            label: first_non_empty(&[&completed.label, label]),
            note: first_non_empty(&[&completed.note, note]),
            forward_to_email: String::new(),
            active: completed.active,
            origin: "APPLE_ACCOUNT".into(),
            created_at: None,
        };
        if !address.anonymous_id.is_empty() {
            let path = format!(
                "/account/manage/email/private/{}.em",
                url::form_urlencoded::byte_serialize(address.anonymous_id.as_bytes())
                    .collect::<String>()
            );
            if let Ok(confirmed) =
                self.call_apple_account::<AccountCompleted>(state, HttpMethod::Get, &path, None)
            {
                address.email = first_non_empty(&[&confirmed.email_address, &address.email])
                    .to_ascii_lowercase();
                address.label = first_non_empty(&[&confirmed.label, &address.label]);
                address.note = first_non_empty(&[&confirmed.note, &address.note]);
                address.forward_to_email = confirmed.forward_to_email;
                address.active = confirmed.active;
            }
        }
        if address.email.is_empty() {
            return Err(AppleHmeError::bad_response(
                "Apple Account 创建后未返回 Hide My Email 地址",
            ));
        }
        state.last_checked_at = Some(Utc::now());
        state.last_check_ok = true;
        Ok(address)
    }

    fn call_icloud_web(
        &self,
        session: &mut AppleSession,
        method: HttpMethod,
        path: &str,
        body: Option<Value>,
    ) -> Result<HttpResponse> {
        let base_url = session
            .premium_mail_base_url
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| session_missing("iCloud Web 会话缺少 HME 服务地址"))?;
        let dsid = session
            .dsid
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| session_missing("iCloud Web 会话缺少 DSID"))?;
        let mut url = Url::parse(&format!("{}/", base_url.trim_end_matches('/')))
            .map_err(|_| AppleHmeError::invalid("iCloud HME 服务地址无效"))?
            .join(path.trim_start_matches('/'))
            .map_err(|_| AppleHmeError::invalid("iCloud HME 请求地址无效"))?;
        url.query_pairs_mut()
            .append_pair(
                "clientBuildNumber",
                session
                    .client_build_number
                    .as_deref()
                    .unwrap_or("2622Build20"),
            )
            .append_pair(
                "clientMasteringNumber",
                session
                    .client_mastering_number
                    .as_deref()
                    .or(session.client_build_number.as_deref())
                    .unwrap_or("2622Build20"),
            )
            .append_pair(
                "clientId",
                session.client_id.as_deref().unwrap_or("imail-local"),
            )
            .append_pair("dsid", dsid);
        let host = session.host.clone();
        let state = session
            .state_mut(LoginStateKind::ICloudWeb)
            .ok_or_else(|| session_missing("缺少 iCloud Web 登录会话"))?;
        let origin = if host.contains("icloud.com.cn") {
            "https://www.icloud.com.cn"
        } else {
            "https://www.icloud.com"
        };
        let headers = icloud_headers(origin, body.is_some());
        let response = match execute(
            &self.transport,
            method,
            url.to_string(),
            headers,
            body.as_ref().map(json_bytes).transpose()?,
            &mut state.cookies,
        ) {
            Ok(response) => response,
            Err(error) => {
                state.last_checked_at = Some(Utc::now());
                state.last_check_ok = false;
                state.last_status_message = Some(error.message.clone());
                return Err(error);
            }
        };
        if !(200..300).contains(&response.status) {
            let error = response_error(&response, false);
            state.last_checked_at = Some(Utc::now());
            state.last_check_ok = false;
            state.last_status_message = Some(error.message.clone());
            return Err(error);
        }
        state.saved_at = Utc::now();
        state.last_checked_at = Some(Utc::now());
        state.last_check_ok = true;
        state.last_status_message = Some("iCloud Web 登录态正常".into());
        Ok(response)
    }

    fn refresh_apple_account_state(&self, state: &mut LoginState) -> Result<()> {
        if state.cookies.is_empty() {
            return Err(session_missing("Apple Account 管理会话缺少 Cookie"));
        }
        let response = self.call_apple_account_raw(
            state,
            HttpMethod::Get,
            "/account/manage/gs/ws/token",
            None,
            false,
        )?;
        if let Ok(token) = serde_json::from_slice::<ManageToken>(&response.body) {
            if token.time_out_interval > 0 {
                state.manage_expires_at =
                    Some(Utc::now() + Duration::minutes(token.time_out_interval));
            }
        }
        let manage: ManageResponse =
            self.call_apple_account(state, HttpMethod::Get, "/account/manage", None)?;
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

    fn call_apple_account<R: for<'de> Deserialize<'de>>(
        &self,
        state: &mut LoginState,
        method: HttpMethod,
        path: &str,
        body: Option<Value>,
    ) -> Result<R> {
        let response = self.call_apple_account_raw(state, method, path, body, true)?;
        decode_json(&response)
    }

    fn call_apple_account_raw(
        &self,
        state: &mut LoginState,
        method: HttpMethod,
        path: &str,
        body: Option<Value>,
        include_api_key: bool,
    ) -> Result<HttpResponse> {
        let host = if state.host.trim().is_empty() {
            "appleid.apple.com"
        } else {
            state.host.trim()
        };
        let url = format!("https://{}{}", host, path);
        let headers = apple_account_headers(
            state.scnt.as_deref(),
            include_api_key
                .then_some(state.api_key.as_deref())
                .flatten(),
        );
        let response = match execute(
            &self.transport,
            method,
            url,
            headers,
            body.as_ref().map(json_bytes).transpose()?,
            &mut state.cookies,
        ) {
            Ok(response) => response,
            Err(error) => {
                state.last_checked_at = Some(Utc::now());
                state.last_check_ok = false;
                state.last_status_message = Some(error.message.clone());
                return Err(error);
            }
        };
        update_state_headers(state, &response);
        if !(200..300).contains(&response.status) {
            let error = response_error(&response, true);
            state.last_checked_at = Some(Utc::now());
            state.last_check_ok = false;
            state.last_status_message = Some(error.message.clone());
            return Err(error);
        }
        state.last_checked_at = Some(Utc::now());
        state.last_check_ok = true;
        Ok(response)
    }
}

fn apple_account_state_usable(state: &LoginState) -> bool {
    state
        .scnt
        .as_ref()
        .is_some_and(|value| !value.trim().is_empty())
        && state
            .api_key
            .as_ref()
            .is_some_and(|value| !value.trim().is_empty())
        && state
            .manage_expires_at
            .is_some_and(|expires| expires > Utc::now())
}

fn icloud_headers(origin: &str, has_body: bool) -> BTreeMap<String, String> {
    let mut headers = header_map(&[
        ("Accept", "application/json".into()),
        ("Origin", origin.into()),
        ("Referer", format!("{origin}/")),
        ("Sec-Fetch-Site", "same-site".into()),
        ("Sec-Fetch-Mode", "cors".into()),
        ("Sec-Fetch-Dest", "empty".into()),
        ("Accept-Language", "zh-CN,zh;q=0.9".into()),
        ("User-Agent", ICLOUD_WEB_USER_AGENT.into()),
    ]);
    if has_body {
        headers.insert("Content-Type".into(), "text/plain;charset=UTF-8".into());
    }
    headers
}

fn apple_account_headers(scnt: Option<&str>, api_key: Option<&str>) -> BTreeMap<String, String> {
    let mut headers = header_map(&[
        ("Accept", "application/json, text/plain, */*".into()),
        ("Content-Type", "application/json".into()),
        ("Origin", "https://account.apple.com".into()),
        ("Referer", "https://account.apple.com/".into()),
        ("User-Agent", APPLE_ACCOUNT_USER_AGENT.into()),
        ("Accept-Language", "zh,en;q=0.9".into()),
        ("Sec-Fetch-Site", "same-site".into()),
        ("Sec-Fetch-Mode", "cors".into()),
        ("Sec-Fetch-Dest", "empty".into()),
        ("X-Apple-I-Request-Context", "ca".into()),
        ("X-Apple-I-TimeZone", "Asia/Shanghai".into()),
        (
            "X-Apple-I-FD-Client-Info",
            json!({
                "U": APPLE_ACCOUNT_USER_AGENT,
                "L": "zh-CN",
                "Z": "GMT+08:00",
                "V": "1.1",
                "F": ""
            })
            .to_string(),
        ),
    ]);
    if let Some(value) = scnt.filter(|value| !value.trim().is_empty()) {
        headers.insert("scnt".into(), value.into());
    }
    if let Some(value) = api_key.filter(|value| !value.trim().is_empty()) {
        headers.insert("X-Apple-Api-Key".into(), value.into());
    }
    headers
}

fn update_state_headers(state: &mut LoginState, response: &HttpResponse) {
    if let Some(value) = response.header("scnt") {
        state.scnt = Some(value.into());
    }
    if let Some(value) = response.header("X-Apple-ID-Session-Id") {
        state.session_id = Some(value.into());
    }
    if let Some(value) = response
        .header("X-Apple-I-DA-Token")
        .or_else(|| response.header("X-Apple-I-Cont-X-Apple-I-DA-Token"))
    {
        state.data_access_token = Some(value.into());
    }
    state.saved_at = Utc::now();
}

fn decode_web_result<R: for<'de> Deserialize<'de>>(response: &HttpResponse) -> Result<R> {
    let envelope: WebEnvelope<R> = decode_json(response)?;
    if !envelope.success {
        let message = envelope
            .error
            .and_then(|error| error.error_message)
            .unwrap_or_else(|| "iCloud HME 请求失败".into());
        return Err(web_error(&message));
    }
    envelope
        .result
        .ok_or_else(|| AppleHmeError::bad_response("iCloud HME 返回缺少 result"))
}

fn decode_web_empty(response: &HttpResponse) -> Result<()> {
    let envelope: WebEnvelope<Value> = decode_json(response)?;
    if envelope.success {
        Ok(())
    } else {
        Err(web_error(
            envelope
                .error
                .and_then(|error| error.error_message)
                .as_deref()
                .unwrap_or("iCloud HME 请求失败"),
        ))
    }
}

fn web_error(message: &str) -> AppleHmeError {
    let lower = message.to_ascii_lowercase();
    if lower.contains("limit") || lower.contains("too many") {
        AppleHmeError::new(
            AppleHmeErrorCode::AddressLimitReached,
            "iCloud Hide My Email 已达到当前创建限制",
            true,
        )
    } else {
        AppleHmeError::new(AppleHmeErrorCode::Protocol, message, true)
    }
}

fn response_error(response: &HttpResponse, account: bool) -> AppleHmeError {
    let detail = response_text(response).to_ascii_lowercase();
    if response.status == 401 || response.status == 419 || response.status == 403 {
        return AppleHmeError::new(
            AppleHmeErrorCode::SessionExpired,
            if account {
                "Apple Account 管理会话已失效，请重新登录"
            } else {
                "iCloud Web 会话已失效，请重新登录"
            },
            true,
        );
    }
    if detail.contains("limit") || detail.contains("too many") {
        return AppleHmeError::new(
            AppleHmeErrorCode::AddressLimitReached,
            "Apple Hide My Email 已达到当前创建限制",
            true,
        );
    }
    AppleHmeError::new(
        AppleHmeErrorCode::Protocol,
        format!("Apple HME 请求失败，HTTP {}", response.status),
        response.status >= 500,
    )
}

fn session_missing(message: &str) -> AppleHmeError {
    AppleHmeError::new(AppleHmeErrorCode::SessionMissing, message, true)
}

fn required_id(value: &str) -> Result<&str> {
    let value = value.trim();
    if value.is_empty() || value.len() > 512 {
        return Err(AppleHmeError::invalid("Hide My Email anonymousId 无效"));
    }
    Ok(value)
}

fn first_non_empty(values: &[&str]) -> String {
    values
        .iter()
        .find(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string())
        .unwrap_or_default()
}

fn parse_created_at(value: &Value) -> Option<DateTime<Utc>> {
    if let Some(milliseconds) = value.as_i64() {
        return DateTime::from_timestamp_millis(milliseconds);
    }
    value
        .as_str()
        .and_then(|value| value.parse::<DateTime<Utc>>().ok())
}

#[derive(Deserialize)]
struct WebEnvelope<R> {
    success: bool,
    result: Option<R>,
    error: Option<WebEnvelopeError>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebEnvelopeError {
    error_message: Option<String>,
}

#[derive(Deserialize)]
struct WebGenerateResult {
    #[serde(default)]
    hme: String,
}

#[derive(Deserialize)]
struct WebReserveResult {
    hme: WebAddress,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebListResult {
    #[serde(default)]
    hme_emails: Vec<WebAddress>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebAddress {
    #[serde(default)]
    anonymous_id: String,
    #[serde(default)]
    hme: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    note: String,
    #[serde(default)]
    forward_to_email: String,
    #[serde(default)]
    is_active: bool,
    #[serde(default)]
    origin: String,
    #[serde(default)]
    create_timestamp: Value,
}

impl WebAddress {
    fn into_address(self) -> Option<HmeAddress> {
        let email = self.hme.trim().to_ascii_lowercase();
        if email.is_empty() {
            return None;
        }
        Some(HmeAddress {
            anonymous_id: self.anonymous_id.trim().into(),
            email,
            label: self.label.trim().into(),
            note: self.note.trim().into(),
            forward_to_email: self.forward_to_email.trim().into(),
            active: self.is_active,
            origin: self.origin.trim().into(),
            created_at: parse_created_at(&self.create_timestamp),
        })
    }
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountGenerated {
    #[serde(default)]
    email_address: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountCompleted {
    #[serde(default)]
    email_address: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    note: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    forward_to_email: String,
    #[serde(default)]
    active: bool,
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::{
        model::AppleCookie,
        transport::{HttpRequest, HttpTransport},
    };

    #[derive(Clone)]
    struct ScriptedTransport {
        responses: Arc<Mutex<Vec<HttpResponse>>>,
        paths: Arc<Mutex<Vec<String>>>,
    }

    impl HttpTransport for ScriptedTransport {
        fn execute(&self, request: HttpRequest) -> Result<HttpResponse> {
            self.paths
                .lock()
                .unwrap()
                .push(Url::parse(&request.url).unwrap().path().to_string());
            Ok(self.responses.lock().unwrap().remove(0))
        }
    }

    fn response(body: Value) -> HttpResponse {
        HttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: serde_json::to_vec(&body).unwrap(),
        }
    }

    fn web_session() -> AppleSession {
        let now = Utc::now();
        AppleSession {
            apple_id: "owner@icloud.com".into(),
            dsid: Some("123".into()),
            client_id: Some("client".into()),
            client_build_number: Some("build".into()),
            client_mastering_number: Some("build".into()),
            premium_mail_base_url: Some("https://premium.example".into()),
            mail_gateway_base_url: None,
            mail_base_url: None,
            host: "www.icloud.com".into(),
            is_icloud_plus: true,
            can_create_hme: true,
            login_states: vec![LoginState {
                kind: LoginStateKind::ICloudWeb,
                host: "www.icloud.com".into(),
                origin: "https://www.icloud.com".into(),
                cookies: vec![AppleCookie {
                    name: "session".into(),
                    value: "secret".into(),
                    domain: "premium.example".into(),
                    path: "/".into(),
                    expires_at: None,
                    secure: true,
                    http_only: true,
                }],
                scnt: None,
                session_id: None,
                api_key: None,
                data_access_token: None,
                user_agent: ICLOUD_WEB_USER_AGENT.into(),
                saved_at: now,
                manage_expires_at: None,
                last_checked_at: Some(now),
                last_check_ok: true,
                last_status_message: None,
                last_successful_keepalive_at: None,
            }],
            saved_at: now,
        }
    }

    #[test]
    fn creates_lists_and_deactivates_with_icloud_web_session() {
        let paths = Arc::new(Mutex::new(Vec::new()));
        let transport = ScriptedTransport {
            responses: Arc::new(Mutex::new(vec![
                response(json!({"success":true,"result":{"hme":"alias@icloud.com"}})),
                response(
                    json!({"success":true,"result":{"hme":{"anonymousId":"a1","hme":"alias@icloud.com","label":"service","note":"note","forwardToEmail":"owner@icloud.com","isActive":true,"origin":"WEB"}}}),
                ),
                response(
                    json!({"success":true,"result":{"hmeEmails":[{"anonymousId":"a1","hme":"alias@icloud.com","label":"service","note":"note","forwardToEmail":"owner@icloud.com","isActive":true,"origin":"WEB","createTimestamp":1723852800000_i64}]}}),
                ),
                response(json!({"success":true,"result":{}})),
            ])),
            paths: Arc::clone(&paths),
        };
        let client = AppleHmeClient::new(transport);
        let mut session = web_session();
        let created = client
            .create(
                &mut session,
                CreateHmeRequest {
                    label: "service".into(),
                    note: "note".into(),
                    channel: CreateChannel::ICloudWeb,
                },
            )
            .unwrap();
        assert_eq!(created.address.email, "alias@icloud.com");
        let listed = client.list(&mut session).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].forward_to_email, "owner@icloud.com");
        client.deactivate(&mut session, "a1").unwrap();
        assert_eq!(
            *paths.lock().unwrap(),
            vec![
                "/v1/hme/generate",
                "/v1/hme/reserve",
                "/v2/hme/list",
                "/v1/hme/deactivate"
            ]
        );
    }

    #[test]
    fn refuses_to_permanently_delete_an_active_address() {
        let transport = ScriptedTransport {
            responses: Arc::new(Mutex::new(vec![response(json!({
                "success":true,
                "result":{"hmeEmails":[{"anonymousId":"a1","hme":"alias@icloud.com","isActive":true}]}
            }))])),
            paths: Arc::new(Mutex::new(Vec::new())),
        };
        let client = AppleHmeClient::new(transport);
        let error = client
            .delete_inactive(&mut web_session(), "a1")
            .unwrap_err();
        assert_eq!(error.code, AppleHmeErrorCode::InvalidInput);
    }

    #[test]
    fn keeps_apple_account_manage_session_alive() {
        let paths = Arc::new(Mutex::new(Vec::new()));
        let client = AppleHmeClient::new(ScriptedTransport {
            responses: Arc::new(Mutex::new(vec![
                response(json!({"timeOutInterval":5})),
                response(json!({"apiKey":"rotated-key"})),
                response(json!({"forwardToEmail":"owner@icloud.com"})),
            ])),
            paths: Arc::clone(&paths),
        });
        let now = Utc::now();
        let mut session = AppleSession::empty("owner@icloud.com");
        session.put_state(LoginState {
            kind: LoginStateKind::AppleAccount,
            host: "appleid.apple.com".into(),
            origin: "https://account.apple.com".into(),
            cookies: vec![AppleCookie {
                name: "account".into(),
                value: "secret".into(),
                domain: ".appleid.apple.com".into(),
                path: "/".into(),
                expires_at: None,
                secure: true,
                http_only: true,
            }],
            scnt: Some("scnt".into()),
            session_id: Some("session".into()),
            api_key: Some("old-key".into()),
            data_access_token: None,
            user_agent: APPLE_ACCOUNT_USER_AGENT.into(),
            saved_at: now,
            manage_expires_at: Some(now),
            last_checked_at: Some(now),
            last_check_ok: true,
            last_status_message: None,
            last_successful_keepalive_at: None,
        });

        client.keep_alive_apple_account(&mut session).unwrap();

        let state = session.state(LoginStateKind::AppleAccount).unwrap();
        assert_eq!(state.api_key.as_deref(), Some("rotated-key"));
        assert_eq!(
            state.last_status_message.as_deref(),
            Some("Apple Account 会话保活正常")
        );
        assert!(state.last_successful_keepalive_at.is_some());
        assert_eq!(
            *paths.lock().unwrap(),
            vec![
                "/account/manage/gs/ws/token",
                "/account/manage",
                "/account/manage/forwardemail"
            ]
        );
    }

    #[test]
    fn marks_icloud_web_session_disconnected_when_keepalive_is_rejected() {
        let client = AppleHmeClient::new(ScriptedTransport {
            responses: Arc::new(Mutex::new(vec![HttpResponse {
                status: 401,
                headers: BTreeMap::new(),
                body: serde_json::to_vec(&json!({"error":"session expired"})).unwrap(),
            }])),
            paths: Arc::new(Mutex::new(Vec::new())),
        });
        let mut session = web_session();

        client.keep_alive_icloud_web(&mut session).unwrap_err();

        let state = session.state(LoginStateKind::ICloudWeb).unwrap();
        assert!(!state.last_check_ok);
        assert!(state.last_checked_at.is_some());
        assert!(state.last_status_message.is_some());
        assert!(state.last_successful_keepalive_at.is_none());
    }

    #[test]
    fn creates_with_apple_account_and_persists_rotated_state() {
        #[derive(Clone)]
        struct AccountTransport {
            responses: Arc<Mutex<Vec<HttpResponse>>>,
            requests: Arc<Mutex<Vec<HttpRequest>>>,
        }
        impl HttpTransport for AccountTransport {
            fn execute(&self, request: HttpRequest) -> Result<HttpResponse> {
                self.requests.lock().unwrap().push(request);
                Ok(self.responses.lock().unwrap().remove(0))
            }
        }

        let response_with_state = |body: Value| HttpResponse {
            status: 200,
            headers: BTreeMap::from([
                ("scnt".into(), vec!["rotated-scnt".into()]),
                (
                    "Set-Cookie".into(),
                    vec!["account=rotated; Domain=.appleid.apple.com; Path=/; Secure".into()],
                ),
            ]),
            body: serde_json::to_vec(&body).unwrap(),
        };
        let requests = Arc::new(Mutex::new(Vec::new()));
        let client = AppleHmeClient::new(AccountTransport {
            responses: Arc::new(Mutex::new(vec![
                response_with_state(json!({"emailAddress":"private@icloud.com"})),
                response_with_state(
                    json!({"emailAddress":"private@icloud.com","label":"service","note":"created","id":"id1","active":true}),
                ),
                response_with_state(
                    json!({"emailAddress":"private@icloud.com","label":"service","note":"created","id":"id1","forwardToEmail":"owner@icloud.com","active":true}),
                ),
            ])),
            requests: Arc::clone(&requests),
        });
        let now = Utc::now();
        let mut session = AppleSession::empty("owner@icloud.com");
        session.put_state(LoginState {
            kind: LoginStateKind::AppleAccount,
            host: "appleid.apple.com".into(),
            origin: "https://account.apple.com".into(),
            cookies: vec![AppleCookie {
                name: "account".into(),
                value: "old".into(),
                domain: ".appleid.apple.com".into(),
                path: "/".into(),
                expires_at: None,
                secure: true,
                http_only: true,
            }],
            scnt: Some("old-scnt".into()),
            session_id: Some("session".into()),
            api_key: Some("dynamic-key".into()),
            data_access_token: None,
            user_agent: APPLE_ACCOUNT_USER_AGENT.into(),
            saved_at: now,
            manage_expires_at: Some(now + Duration::hours(1)),
            last_checked_at: Some(now),
            last_check_ok: true,
            last_status_message: None,
            last_successful_keepalive_at: None,
        });
        let result = client
            .create(
                &mut session,
                CreateHmeRequest {
                    label: "service".into(),
                    note: "created".into(),
                    channel: CreateChannel::AppleAccount,
                },
            )
            .unwrap();
        assert_eq!(result.address.email, "private@icloud.com");
        assert_eq!(result.address.forward_to_email, "owner@icloud.com");
        let state = session.state(LoginStateKind::AppleAccount).unwrap();
        assert_eq!(state.scnt.as_deref(), Some("rotated-scnt"));
        assert_eq!(state.cookies[0].value, "rotated");
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests.iter().all(|request| request
            .headers
            .get("X-Apple-Api-Key")
            .map(String::as_str)
            == Some("dynamic-key")));
    }
}
