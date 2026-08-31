use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::{Arc, OnceLock},
};

use axum::{
    extract::{Request, State},
    http::{header, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::any,
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::{SecondsFormat, Utc};
use imail_core::{
    accounts::AccountService,
    drafts::DraftService,
    external_access::ExternalAccessService,
    messages::{MessageQuery, MessageQueryService},
    notifications::MailOverviewService,
    preferences::PreferencesService,
    theme::CustomThemeService,
    AccountRepository, AuthRepository, ContentRepository, DeveloperTokenRepository,
};
use imail_protocol::{
    AccountMetadataPatch, AccountProxyUpdate, AppPreferencesPatch, CustomTheme, DraftInput,
    MessageMoveDestination, ProxyProtocol, SendAttachmentInput, SendMessageInput,
};
use imail_storage_sqlite::{SqliteAuthStore, SyncEnqueue, SyncPolicySettings, SyncRuntimeStore};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::AppState;

const MANAGEMENT_TOOLS: [&str; 18] = [
    "settings_update",
    "theme_custom_update",
    "account_add_with_code",
    "account_start_oauth",
    "account_reconnect_oauth",
    "account_update",
    "account_update_authorization_code",
    "account_proxy_update",
    "account_remove",
    "sync_policy_update",
    "message_translate",
    "apple_hme_start_login",
    "apple_hme_submit_two_factor",
    "apple_hme_sync",
    "apple_hme_create",
    "apple_hme_deactivate",
    "apple_hme_delete",
    "apple_hme_disconnect",
];
const MODERN_PROTOCOL: &str = "2026-07-28";
const LEGACY_PROTOCOLS: [&str; 5] = [
    "2025-11-25",
    "2025-06-18",
    "2025-03-26",
    "2024-11-05",
    "2024-10-07",
];
const PROTOCOL_META: &str = "io.modelcontextprotocol/protocolVersion";
const CLIENT_CAPABILITIES_META: &str = "io.modelcontextprotocol/clientCapabilities";
const SERVER_INFO_META: &str = "io.modelcontextprotocol/serverInfo";

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/mcp", any(handle))
}

#[derive(Deserialize)]
struct RpcRequest {
    #[serde(default)]
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SyncPolicyPatch {
    enabled: Option<bool>,
    folder_mode: Option<String>,
    selected_mailboxes: Option<Vec<String>>,
    notify_on_error: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct McpProxyInput {
    email: String,
    enabled: bool,
    source_email: Option<String>,
    protocol: Option<ProxyProtocol>,
    host: Option<String>,
    port: Option<u16>,
    username: Option<String>,
    password: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct McpMessageUpdate {
    message_id: String,
    unread: Option<bool>,
    flagged: Option<bool>,
    labels: Option<Vec<String>>,
    snoozed_until: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct McpSendInput {
    #[serde(default)]
    bcc: Vec<String>,
    #[serde(default)]
    in_reply_to: Vec<String>,
    #[serde(default)]
    references: Vec<String>,
    account_email: String,
    to: Vec<String>,
    cc: Option<Vec<String>>,
    subject: String,
    text: String,
    html: Option<String>,
    attachments: Option<Vec<SendAttachmentInput>>,
}

impl McpSendInput {
    fn envelope(&self) -> imail_protocol::ComposeEnvelope {
        imail_protocol::ComposeEnvelope {
            bcc: self.bcc.clone(),
            reply: imail_protocol::ReplyHeaders {
                in_reply_to: self.in_reply_to.clone(),
                references: self.references.clone(),
            },
        }
    }
}

async fn handle(State(state): State<Arc<AppState>>, request: Request) -> Response {
    if !crate::host_allowed(
        request
            .headers()
            .get(header::HOST)
            .and_then(|value| value.to_str().ok()),
        &state.config.allowed_hosts,
    ) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"MCP Host 不在允许列表"})),
        )
            .into_response();
    }
    if request.headers().get(header::ORIGIN).is_some_and(|value| {
        match value
            .to_str()
            .ok()
            .and_then(|origin| url::Url::parse(origin).ok())
            .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
        {
            Some(host) => !state.config.allowed_hosts.contains(&host),
            None => true,
        }
    }) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"MCP Origin 不在允许列表"})),
        )
            .into_response();
    }
    if request.method() != Method::POST {
        return (
            StatusCode::METHOD_NOT_ALLOWED,
            Json(json!({
                "error": "Rust MCP 当前使用无状态 Streamable HTTP，仅接受 POST"
            })),
        )
            .into_response();
    }
    let accept = request
        .headers()
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !accept.contains("application/json") || !accept.contains("text/event-stream") {
        return rpc_http_error(
            StatusCode::NOT_ACCEPTABLE,
            Value::Null,
            -32000,
            "Not Acceptable: Client must accept both application/json and text/event-stream",
        );
    }
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !content_type
        .split(';')
        .next()
        .is_some_and(|value| value.trim() == "application/json")
    {
        return rpc_http_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Value::Null,
            -32000,
            "Unsupported Media Type: Content-Type must be application/json",
        );
    }
    let protocol_header = request
        .headers()
        .get("mcp-protocol-version")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    if protocol_header
        .as_deref()
        .is_some_and(|value| value != MODERN_PROTOCOL && !LEGACY_PROTOCOLS.contains(&value))
    {
        return rpc_http_error(
            StatusCode::BAD_REQUEST,
            Value::Null,
            -32000,
            "Bad Request: Unsupported protocol version",
        );
    }
    let method_header = request
        .headers()
        .get("mcp-method")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let name_header = request
        .headers()
        .get("mcp-name")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let raw = match bearer(request.headers().get(header::AUTHORIZATION)) {
        Some(raw) => raw.to_string(),
        None => return unauthorized(),
    };
    let body = match axum::body::to_bytes(request.into_body(), 25 * 1024 * 1024).await {
        Ok(body) => body,
        Err(_) => {
            return rpc_http_error(StatusCode::BAD_REQUEST, Value::Null, -32700, "请求体无效")
        }
    };
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return rpc_http_error(
                StatusCode::BAD_REQUEST,
                Value::Null,
                -32700,
                "JSON 请求体无效",
            )
        }
    };
    let is_batch = payload.is_array();
    if is_batch
        && (protocol_header.as_deref() == Some(MODERN_PROTOCOL)
            || payload.as_array().is_some_and(|items| {
                items.iter().any(|item| {
                    item.pointer("/params/_meta/io.modelcontextprotocol~1protocolVersion")
                        .and_then(Value::as_str)
                        == Some(MODERN_PROTOCOL)
                })
            }))
    {
        return rpc_http_error(
            StatusCode::BAD_REQUEST,
            Value::Null,
            -32600,
            "JSON-RPC batches are not supported by protocol revision 2026-07-28",
        );
    }
    let mut requests = if let Some(items) = payload.as_array() {
        if items.is_empty() {
            return rpc_http_error(
                StatusCode::BAD_REQUEST,
                Value::Null,
                -32600,
                "JSON-RPC 批处理不能为空",
            );
        }
        let mut requests = Vec::with_capacity(items.len());
        for item in items {
            let rpc = match serde_json::from_value::<RpcRequest>(item.clone()) {
                Ok(rpc) if rpc.jsonrpc == "2.0" => rpc,
                _ => {
                    return rpc_http_error(
                        StatusCode::BAD_REQUEST,
                        Value::Null,
                        -32600,
                        "JSON-RPC 批处理包含无效请求",
                    )
                }
            };
            requests.push(rpc);
        }
        requests
    } else {
        match serde_json::from_value::<RpcRequest>(payload) {
            Ok(rpc) if rpc.jsonrpc == "2.0" => vec![rpc],
            _ => {
                return rpc_http_error(
                    StatusCode::BAD_REQUEST,
                    Value::Null,
                    -32600,
                    "JSON-RPC 请求无效",
                )
            }
        }
    };
    let modern = if is_batch {
        false
    } else {
        let rpc = &requests[0];
        match modern_request(
            rpc,
            protocol_header.as_deref(),
            method_header.as_deref(),
            name_header.as_deref(),
        ) {
            Ok(value) => value,
            Err((code, message)) => {
                return rpc_http_error(
                    StatusCode::BAD_REQUEST,
                    rpc.id.clone().unwrap_or(Value::Null),
                    code,
                    &message,
                )
            }
        }
    };
    let request_id = requests[0].id.clone().unwrap_or(Value::Null);
    let database = state.config.data_dir.join("imail.sqlite");
    let authenticated = run(database.clone(), move |store| {
        let token = store
            .authenticate_developer_token(&raw, "mcp:full")?
            .ok_or(McpError::Unauthorized)?;
        if token.owner_id.is_empty() || token.owner_id == "__legacy__" {
            return Err(McpError::Unauthorized);
        }
        if !ExternalAccessService::new(store)
            .get(&token.owner_id)?
            .mcp_enabled
        {
            return Err(McpError::Disabled);
        }
        Ok((token.owner_id, token.id))
    })
    .await;
    let (owner_id, token_id) = match authenticated {
        Ok(value) => value,
        Err(McpError::Unauthorized) => return unauthorized(),
        Err(McpError::Disabled) => {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({"error":"MCP 尚未在 iMail 界面中启用"})),
            )
                .into_response()
        }
        Err(_) => {
            return rpc_http_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                request_id,
                -32603,
                "服务暂时不可用",
            )
        }
    };
    if is_batch {
        let mut responses = Vec::new();
        for rpc in requests {
            if let Some(response) = execute_rpc(
                Arc::clone(&state),
                database.clone(),
                &owner_id,
                &token_id,
                rpc,
                false,
            )
            .await
            {
                responses.push(response);
            }
        }
        return if responses.is_empty() {
            StatusCode::ACCEPTED.into_response()
        } else {
            Json(Value::Array(responses)).into_response()
        };
    }
    match execute_rpc(
        state,
        database,
        &owner_id,
        &token_id,
        requests.remove(0),
        modern,
    )
    .await
    {
        Some(response) => Json(response).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

async fn execute_rpc(
    state: Arc<AppState>,
    database: PathBuf,
    owner_id: &str,
    token_id: &str,
    rpc: RpcRequest,
    modern: bool,
) -> Option<Value> {
    let id = rpc.id.clone();
    let response_id = id.clone().unwrap_or(Value::Null);
    let result = match rpc.method.as_str() {
        "initialize" if !modern => Ok(json!({
            "protocolVersion": negotiated_protocol(&rpc.params),
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": "imail", "version": "1.0.0" },
            "instructions": "iMail 是本地邮箱控制面。执行移动、删除账户、更新凭据或发送邮件前，先读取目标并确认邮箱地址与邮件 ID。授权码和邮箱凭据属于敏感信息，不得回显、记录或写入邮件内容。"
        })),
        "server/discover" if modern => Ok(json!({
            "supportedVersions": [MODERN_PROTOCOL],
            "capabilities": { "tools": { "listChanged": true } },
            "instructions": "iMail 是本地邮箱控制面。执行移动、删除账户、更新凭据或发送邮件前，先读取目标并确认邮箱地址与邮件 ID。授权码和邮箱凭据属于敏感信息，不得回显、记录或写入邮件内容。"
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tools()})),
        "tools/call" => {
            let name = rpc
                .params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let arguments = rpc
                .params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            if MANAGEMENT_TOOLS.contains(&name.as_str()) {
                let audit_owner = owner_id.to_string();
                let audit_token = token_id.to_string();
                let audit_name = name.clone();
                let _ = run(database.clone(), move |store| {
                    store.record_security_event(
                        "mcp.management-tool-called",
                        "mcp",
                        Some(&audit_owner),
                        &BTreeMap::from([
                            ("tool".into(), audit_name),
                            ("authorizationCodeId".into(), audit_token),
                        ]),
                    )?;
                    Ok(())
                })
                .await;
            }
            call_tool(
                Arc::clone(&state),
                database,
                owner_id.to_string(),
                &name,
                arguments,
            )
            .await
        }
        "notifications/initialized" | "notifications/cancelled" => return None,
        _ => Err(McpError::MethodNotFound),
    };
    id.as_ref()?;
    match result {
        Ok(result) => {
            let result = if modern {
                modern_result(&rpc.method, result)
            } else {
                result
            };
            Some(json!({"jsonrpc":"2.0", "id": response_id, "result": result}))
        }
        Err(McpError::MethodNotFound) => Some(json!({
            "jsonrpc":"2.0", "id":response_id,
            "error":{"code":-32601,"message":"方法或工具不存在"}
        })),
        Err(McpError::Invalid(message)) => Some(tool_error_value(response_id, message)),
        Err(McpError::Application(message)) => Some(tool_error_value(response_id, &message)),
        Err(_) => Some(tool_error_value(response_id, "工具执行失败")),
    }
}

async fn call_tool(
    state: Arc<AppState>,
    database: PathBuf,
    owner_id: String,
    name: &str,
    arguments: Value,
) -> Result<Value, McpError> {
    if name == "translation_profiles_list" {
        let data_dir = database.parent().ok_or(McpError::Storage)?.to_path_buf();
        let response = crate::translation_settings::embedded_call(
            data_dir,
            owner_id,
            crate::translation_settings::TranslationSettingsApplicationCall::Read,
        )
        .await;
        if response.status != 200 {
            return Err(McpError::Application(safe_application_error(
                &response.body,
            )));
        }
        return Ok(tool_output(json!({
            "defaultProfileId": response.body.pointer("/environment/defaultProfileId").cloned().unwrap_or(Value::Null),
            "profiles": response.body.get("profiles").cloned().unwrap_or_else(|| json!([]))
        })));
    }
    if name == "message_translate" {
        let message_id = required_string(&arguments, "messageId")?.to_string();
        let profile_id = required_string(&arguments, "profileId")?.to_string();
        let target_language = required_string(&arguments, "targetLanguage")?.to_string();
        let source_language = optional_string(&arguments, "sourceLanguage").map(str::to_string);
        let data_dir = database.parent().ok_or(McpError::Storage)?.to_path_buf();
        let response = crate::translations::embedded_call(
            data_dir,
            owner_id,
            crate::translations::TranslationApplicationCall::Execute {
                message_id,
                input: imail_protocol::TranslationExecutionRequest {
                    profile_id,
                    source_language,
                    target_language,
                },
            },
        )
        .await;
        if response.status != 200 {
            return Err(McpError::Application(safe_application_error(
                &response.body,
            )));
        }
        return Ok(tool_output(json!({
            "profileId": response.body.pointer("/key/profileId").cloned().unwrap_or(Value::Null),
            "sourceLanguage": response.body.pointer("/key/sourceLanguage").cloned().unwrap_or(Value::Null),
            "targetLanguage": response.body.pointer("/key/targetLanguage").cloned().unwrap_or(Value::Null),
            "segments": response.body.get("segments").cloned().unwrap_or_else(|| json!([])),
            "createdAt": response.body.get("createdAt").cloned().unwrap_or(Value::Null),
            "updatedAt": response.body.get("updatedAt").cloned().unwrap_or(Value::Null)
        })));
    }
    if name.starts_with("apple_hme_") {
        let email = required_string(&arguments, "email")?.to_string();
        let lookup_owner = owner_id.clone();
        let account_id = run(database.clone(), move |store| {
            Ok(account_by_email(store, &lookup_owner, &email)?.id)
        })
        .await?;
        let result = match name {
            "apple_hme_status" => crate::apple_hme::status_service(state, owner_id, account_id)
                .await
                .and_then(|value| {
                    serde_json::to_value(value)
                        .map_err(|_| crate::apple_hme::IntegrationError::Internal)
                }),
            "apple_hme_start_login" => {
                let kind = parse(
                    arguments
                        .get("kind")
                        .cloned()
                        .ok_or(McpError::Invalid("缺少必填参数"))?,
                )?;
                let password = required_string(&arguments, "password")?.to_string();
                let apple_id = optional_string(&arguments, "appleId").map(str::to_string);
                let method = arguments
                    .get("twoFactorMethod")
                    .cloned()
                    .map(parse)
                    .transpose()?;
                let phone = arguments.get("phoneNumber").cloned();
                let host = optional_string(&arguments, "icloudHost").map(str::to_string);
                crate::apple_hme::start_login_service(
                    state,
                    owner_id,
                    "mcp".into(),
                    account_id,
                    kind,
                    password,
                    apple_id,
                    method,
                    phone,
                    host,
                )
                .await
                .and_then(|value| {
                    serde_json::to_value(value)
                        .map_err(|_| crate::apple_hme::IntegrationError::Internal)
                })
            }
            "apple_hme_submit_two_factor" => {
                let pending_id = required_string(&arguments, "pendingId")?.to_string();
                let code = required_string(&arguments, "code")?.to_string();
                let phone = arguments.get("phoneNumber").cloned();
                crate::apple_hme::submit_two_factor_service(
                    state,
                    owner_id,
                    "mcp".into(),
                    account_id,
                    pending_id,
                    code,
                    phone,
                )
                .await
                .and_then(|value| {
                    serde_json::to_value(value)
                        .map_err(|_| crate::apple_hme::IntegrationError::Internal)
                })
            }
            "apple_hme_list" => crate::apple_hme::list_service(state, owner_id, account_id).await,
            "apple_hme_sync" => crate::apple_hme::sync_service(state, owner_id, account_id).await,
            "apple_hme_create" => {
                let label = optional_string(&arguments, "label")
                    .unwrap_or_default()
                    .to_string();
                let note = optional_string(&arguments, "note")
                    .unwrap_or_default()
                    .to_string();
                let channel = arguments
                    .get("channel")
                    .cloned()
                    .map(parse)
                    .transpose()?
                    .unwrap_or_default();
                crate::apple_hme::create_service(state, owner_id, account_id, label, note, channel)
                    .await
            }
            "apple_hme_deactivate" => {
                let anonymous_id = required_string(&arguments, "anonymousId")?.to_string();
                crate::apple_hme::deactivate_service(state, owner_id, account_id, anonymous_id)
                    .await
            }
            "apple_hme_delete" => {
                let anonymous_id = required_string(&arguments, "anonymousId")?.to_string();
                crate::apple_hme::delete_service(state, owner_id, account_id, anonymous_id).await
            }
            "apple_hme_disconnect" => {
                crate::apple_hme::disconnect_service(state, owner_id, account_id).await
            }
            _ => return Err(McpError::MethodNotFound),
        }
        .map_err(|error| McpError::Application(error.public_message()))?;
        return Ok(tool_output(result));
    }
    if name == "account_add_with_code" {
        let account = crate::accounts::mcp_add_with_code(state, owner_id, arguments).await?;
        return Ok(tool_output(json!({"account":safe_account(account)})));
    }
    if name == "account_start_oauth" {
        let result = crate::oauth::mcp_start(state, owner_id, arguments).await?;
        return Ok(tool_output(result));
    }
    if name == "account_reconnect_oauth" {
        let email = required_string(&arguments, "email")?.to_string();
        let lookup_owner = owner_id.clone();
        let account_id = run(database, move |store| {
            Ok(account_by_email(store, &lookup_owner, &email)?.id)
        })
        .await?;
        let result = crate::oauth::mcp_reconnect(state, owner_id, account_id).await?;
        return Ok(tool_output(result));
    }
    if name == "account_update_authorization_code" {
        let email = required_string(&arguments, "email")?.to_string();
        let password = required_string(&arguments, "authorizationCode")?.to_string();
        let lookup_owner = owner_id.clone();
        let account_id = run(database.clone(), move |store| {
            Ok(account_by_email(store, &lookup_owner, &email)?.id)
        })
        .await?;
        let account =
            crate::accounts::mcp_replace_password(state, owner_id, account_id, password).await?;
        return Ok(tool_output(json!({"account":safe_account(account)})));
    }
    if name == "account_proxy_update" {
        let input: McpProxyInput = parse(arguments)?;
        let lookup_owner = owner_id.clone();
        let email = input.email.clone();
        let source_email = input.source_email.clone();
        let (account_id, source_id) = run(database.clone(), move |store| {
            let account_id = account_by_email(store, &lookup_owner, &email)?.id;
            let source_id = source_email
                .as_deref()
                .map(|value| account_by_email(store, &lookup_owner, value).map(|a| a.id))
                .transpose()?;
            Ok((account_id, source_id))
        })
        .await?;
        let update = if !input.enabled {
            AccountProxyUpdate::Disabled
        } else if let Some(source_account_id) = source_id {
            AccountProxyUpdate::CopyFrom { source_account_id }
        } else {
            AccountProxyUpdate::Explicit {
                protocol: input.protocol.ok_or(McpError::Invalid("代理参数无效"))?,
                host: input.host.ok_or(McpError::Invalid("代理参数无效"))?,
                port: input.port.ok_or(McpError::Invalid("代理参数无效"))?,
                username: input.username,
                password: input.password,
            }
        };
        let account =
            crate::accounts::mcp_update_proxy(state, owner_id, account_id, update).await?;
        return Ok(tool_output(json!({"account":safe_account(account)})));
    }
    if name == "account_test_connection" {
        let email = required_string(&arguments, "email")?.to_string();
        let lookup_owner = owner_id.clone();
        let account_id = run(database.clone(), move |store| {
            Ok(account_by_email(store, &lookup_owner, &email)?.id)
        })
        .await?;
        let account = crate::accounts::mcp_test_connection(state, owner_id, account_id).await?;
        return Ok(tool_output(json!({"account":safe_account(account)})));
    }
    if name == "message_update" {
        let input: McpMessageUpdate = parse(arguments)?;
        if input.unread.is_none()
            && input.flagged.is_none()
            && input.labels.is_none()
            && input.snoozed_until.is_none()
        {
            return Err(McpError::Invalid("至少提供一个要更新的字段"));
        }
        let labels = input.labels.map(validate_labels).transpose()?;
        let snoozed = input.snoozed_until.map(validate_snoozed).transpose()?;
        let message = crate::messages::update_for(
            state,
            owner_id.clone(),
            input.message_id,
            input.unread,
            input.flagged,
            labels,
            snoozed,
        )
        .await?;
        let account_id = message.account_id.clone();
        let email = run(database.clone(), move |store| {
            account_email(store, &owner_id, &account_id)
        })
        .await?;
        return Ok(tool_output(
            json!({"message":message_detail(message,&email)}),
        ));
    }
    if name == "message_move" {
        let message_id = required_string(&arguments, "messageId")?.to_string();
        let destination: MessageMoveDestination = parse(
            arguments
                .get("destination")
                .cloned()
                .ok_or(McpError::Invalid("缺少必填参数"))?,
        )?;
        let (_, moved) =
            crate::messages::move_for(state, owner_id, message_id.clone(), destination).await?;
        return Ok(tool_output(
            json!({"moved":true,"messageId":message_id,"destination":destination,"mailbox":moved.mailbox}),
        ));
    }
    if name == "attachment_download" {
        let message_id = required_string(&arguments, "messageId")?.to_string();
        let index = arguments
            .get("index")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or(McpError::Invalid("附件索引无效"))?;
        let attachment =
            crate::messages::download_attachment_for(state, owner_id, message_id, index).await?;
        return Ok(tool_output(
            json!({"filename":attachment.filename,"contentType":attachment.content_type,"size":attachment.content.len(),"data":STANDARD.encode(attachment.content)}),
        ));
    }
    if name == "message_send" {
        let input: McpSendInput = parse(arguments)?;
        let lookup_owner = owner_id.clone();
        let email = input.account_email.clone();
        let account_id = run(database, move |store| {
            Ok(account_by_email(store, &lookup_owner, &email)?.id)
        })
        .await?;
        let result = crate::messages::send_for(
            state,
            owner_id,
            SendMessageInput {
                envelope: input.envelope(),
                account_id,
                to: input.to,
                cc: input.cc,
                subject: input.subject,
                text: input.text,
                html: input.html,
                attachments: input.attachments,
            },
            None,
        )
        .await?;
        return Ok(tool_output(json!({"delivery":result})));
    }
    let name = name.to_string();
    let sync_database = database.clone();
    run(database, move |store| {
        let value = match name.as_str() {
            "imail_status" => {
                let accounts = store.list_accounts(&owner_id)?;
                let messages = store.list_messages(&owner_id)?;
                let drafts = store.list_drafts(&owner_id)?;
                let unread = messages.iter().filter(|message| message.unread).count();
                let last_sync_at = accounts.iter().filter_map(|account| account.last_sync_at.clone()).max();
                json!({"accounts":accounts.len(),"messages":messages.len(),"unread":unread,"drafts":drafts.len(),"lastSyncAt":last_sync_at})
            }
            "settings_get" => json!({"preferences": PreferencesService::new(store).read(&owner_id)?}),
            "settings_update" => {
                let patch: AppPreferencesPatch = parse(arguments)?;
                json!({"preferences": PreferencesService::new(store).update(&owner_id, patch)?})
            }
            "theme_custom_get" => json!({"theme": CustomThemeService::new(store).read(&owner_id)?}),
            "theme_custom_update" => {
                let theme: CustomTheme = parse(arguments)?;
                json!({"theme": CustomThemeService::new(store).update(&owner_id, theme)?})
            }
            "accounts_list" => {
                let accounts = store.list_accounts(&owner_id)?.into_iter().map(|account| json!({
                    "id":account.id,"provider":account.provider,"email":account.email,"displayName":account.display_name,
                    "group":account.group,"groupIcon":account.group_icon,"color":account.color,"settings":public_settings(account.settings),
                    "proxy":public_proxy(account.proxy),"authMethod":account.auth_method,"createdAt":account.created_at,
                    "lastSyncAt":account.last_sync_at,"status":account.status,"lastError":account.last_error,"mailboxes":account.mailboxes
                })).collect::<Vec<_>>();
                json!({"accounts":accounts})
            }
            "account_update" => {
                let email = required_string(&arguments, "email")?.to_string();
                let account = account_by_email(store, &owner_id, &email)?;
                let mut patch = arguments;
                patch.as_object_mut().ok_or(McpError::Invalid("工具参数无效"))?.remove("email");
                let patch: AccountMetadataPatch = parse(patch)?;
                let account = AccountService::new(store).update_metadata(&owner_id, &account.id, patch)?;
                json!({"account": safe_account(account)})
            }
            "account_remove" => {
                let email = required_string(&arguments, "email")?.to_string();
                let account = account_by_email(store, &owner_id, &email)?;
                AccountService::new(store).remove(&owner_id, &account.id)?;
                json!({"removed":true,"email":account.email})
            }
            "mailbox_sync" => mailbox_sync(store, &sync_database, &owner_id, arguments)?,
            "sync_policy_get" => sync_policy_get(store, &sync_database, &owner_id, arguments)?,
            "sync_policy_update" => sync_policy_update(store, &sync_database, &owner_id, arguments)?,
            "smart_folders_list" => crate::search::execute(store, &owner_id, "GET", None, None)?,
            "smart_folder_save" => {
                let id = optional_string(&arguments, "folderId");
                crate::search::execute(store, &owner_id, if id.is_some() { "PUT" } else { "POST" }, id, Some(json!({"name":arguments.get("name"),"filters":arguments.get("filters")})))?
            }
            "smart_folder_delete" => crate::search::execute(store, &owner_id, "DELETE", Some(required_string(&arguments, "folderId")?), None)?,
            "messages_list" => list_messages(store, &owner_id, arguments)?,
            "message_get" => {
                let message_id = required_string(&arguments, "messageId")?;
                let message = MessageQueryService::new(store).get(&owner_id, message_id)?;
                let email = account_email(store, &owner_id, &message.account_id)?;
                json!({"message": message_detail(message, &email)})
            }
            "conversation_get" => {
                let message_id = required_string(&arguments, "messageId")?;
                let messages = MessageQueryService::new(store).conversation(&owner_id, message_id)?;
                let mut summaries = Vec::with_capacity(messages.len());
                for message in messages {
                    let email = account_email(store, &owner_id, &message.account_id)?;
                    summaries.push(message_summary(message, &email));
                }
                json!({"messages":summaries})
            }
            "drafts_list" => list_drafts(store, &owner_id, arguments)?,
            "draft_get" => {
                let id = required_string(&arguments, "draftId")?;
                let draft = DraftService::new(store).get(&owner_id, id)?;
                let email = account_email(store, &owner_id, &draft.account_id)?;
                let mut value = serde_json::to_value(draft).map_err(|_| McpError::Storage)?;
                value.as_object_mut().expect("draft is object").remove("accountId");
                value.as_object_mut().expect("draft is object").insert("accountEmail".into(), Value::String(email));
                json!({"draft":value})
            }
            "draft_save" => save_draft(store, &owner_id, arguments)?,
            "draft_delete" => {
                let id = required_string(&arguments, "draftId")?.to_string();
                DraftService::new(store).get(&owner_id, &id)?;
                DraftService::new(store).delete(&owner_id, &id)?;
                json!({"deleted":true,"draftId":id})
            }
            "labels_list" => {
                json!({"labels":MailOverviewService::new(store).labels(&owner_id)?})
            }
            "notifications_list" => {
                let limit = arguments.get("limit").and_then(Value::as_u64).unwrap_or(30) as usize;
                if !(1..=100).contains(&limit) { return Err(McpError::Invalid("limit 必须在 1..100 之间")); }
                let accounts = store.list_accounts(&owner_id)?.into_iter().map(|account|(account.id,account.email)).collect::<HashMap<_,_>>();
                let notifications = MailOverviewService::new(store).notifications(&owner_id,limit,&now())?.into_iter().map(|notification| json!({
                    "id":notification.id,"kind":notification.kind,"title":notification.title,"detail":notification.detail,"date":notification.date,
                    "messageId":notification.message_id,"accountEmail":accounts.get(&notification.account_id).cloned().unwrap_or_default()
                })).collect::<Vec<_>>();
                json!({"notifications":notifications})
            }
            _ => return Err(McpError::MethodNotFound),
        };
        Ok(tool_output(value))
    }).await
}

fn mailbox_sync(
    store: &mut SqliteAuthStore,
    database: &PathBuf,
    owner: &str,
    input: Value,
) -> Result<Value, McpError> {
    let role = optional_string(&input, "mailboxRole")
        .unwrap_or("inbox")
        .to_string();
    let mailbox = optional_string(&input, "mailboxPath")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if !crate::accounts::valid_mailbox_role(&role)
        || mailbox
            .as_ref()
            .is_some_and(|value| value.encode_utf16().count() > 500)
    {
        return Err(McpError::Invalid("邮箱同步参数无效"));
    }
    let accounts = match optional_string(&input, "email") {
        Some(email) => vec![account_by_email(store, owner, email)?],
        None => store.list_accounts(owner)?,
    };
    let mut sync = SyncRuntimeStore::open_database(database)?;
    let mut results = Vec::with_capacity(accounts.len());
    for account in accounts {
        let public = imail_core::accounts::public_account(&account);
        let (mailbox_role, mailbox) =
            crate::accounts::canonical_target(&public, role.clone(), mailbox.clone());
        if mailbox_role == "custom" && mailbox.is_none() {
            return Err(McpError::Invalid("自定义文件夹必须提供 mailboxPath"));
        }
        let job = sync.enqueue(
            &SyncEnqueue {
                account_id: account.id,
                mailbox,
                mailbox_role,
                reason: "manual".into(),
                priority: 100,
                not_before: None,
            },
            Utc::now(),
        )?;
        results.push(
            json!({"accountEmail":account.email,"status":"queued","synced":0,"jobId":job.id}),
        );
    }
    Ok(json!({"results":results}))
}

fn sync_policy_get(
    store: &mut SqliteAuthStore,
    database: &PathBuf,
    owner: &str,
    input: Value,
) -> Result<Value, McpError> {
    let accounts = match optional_string(&input, "email") {
        Some(email) => vec![account_by_email(store, owner, email)?],
        None => store.list_accounts(owner)?,
    };
    let sync = SyncRuntimeStore::open_database(database)?;
    let defaults = sync.default_policy(owner)?;
    let mut values = Vec::with_capacity(accounts.len());
    for account in accounts {
        let policy = sync.ensure_policy(&account.id, owner, Utc::now())?;
        values.push(json!({"accountEmail":account.email,"policy":policy,"states":sync.account_mailbox_states(&account.id)?,"jobs":sync.account_jobs(&account.id,10)?}));
    }
    Ok(json!({"defaultPolicy":policy_value(&defaults),"accounts":values}))
}

fn sync_policy_update(
    store: &mut SqliteAuthStore,
    database: &PathBuf,
    owner: &str,
    mut input: Value,
) -> Result<Value, McpError> {
    let email = optional_string(&input, "email").map(str::to_string);
    input
        .as_object_mut()
        .ok_or(McpError::Invalid("工具参数无效"))?
        .remove("email");
    let patch: SyncPolicyPatch = parse(input)?;
    validate_policy_patch(&patch)?;
    let mut sync = SyncRuntimeStore::open_database(database)?;
    if let Some(email) = email {
        let account = account_by_email(store, owner, &email)?;
        if let Some(selected) = patch.selected_mailboxes.as_ref() {
            validate_selected_mailboxes(&account.mailboxes, selected)?;
        }
        let current = sync.ensure_policy(&account.id, owner, Utc::now())?;
        let settings = apply_policy_patch(
            SyncPolicySettings {
                enabled: current.enabled,
                folder_mode: current.folder_mode,
                selected_mailboxes: current
                    .selected_mailboxes
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect(),
                notify_on_error: current.notify_on_error,
            },
            patch,
        );
        let policy = sync.update_policy(&account.id, &settings, Utc::now())?;
        Ok(json!({"accountEmail":account.email,"policy":policy}))
    } else {
        let settings = apply_policy_patch(sync.default_policy(owner)?, patch);
        sync.update_default_policy(owner, &settings)?;
        Ok(json!({"policy":policy_value(&settings)}))
    }
}

fn validate_policy_patch(patch: &SyncPolicyPatch) -> Result<(), McpError> {
    if patch.enabled.is_none()
        && patch.folder_mode.is_none()
        && patch.selected_mailboxes.is_none()
        && patch.notify_on_error.is_none()
    {
        return Err(McpError::Invalid("至少提供一个同步设置"));
    }
    if patch
        .folder_mode
        .as_deref()
        .is_some_and(|value| !matches!(value, "inbox" | "standard" | "selected"))
    {
        return Err(McpError::Invalid("同步文件夹范围无效"));
    }
    if patch.selected_mailboxes.as_ref().is_some_and(|values| {
        values.len() > 100
            || values
                .iter()
                .any(|value| value.trim().is_empty() || value.encode_utf16().count() > 500)
    }) {
        return Err(McpError::Invalid("同步文件夹列表无效"));
    }
    Ok(())
}

fn validate_selected_mailboxes(mailboxes: &Value, selected: &[String]) -> Result<(), McpError> {
    let available = mailboxes
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| value.get("path").and_then(Value::as_str))
        .collect::<Vec<_>>();
    if selected
        .iter()
        .any(|candidate| !available.iter().any(|path| path == candidate))
    {
        return Err(McpError::Invalid("包含邮箱中不存在的文件夹"));
    }
    Ok(())
}

fn apply_policy_patch(
    mut current: SyncPolicySettings,
    patch: SyncPolicyPatch,
) -> SyncPolicySettings {
    if let Some(value) = patch.enabled {
        current.enabled = value;
    }
    if let Some(value) = patch.folder_mode {
        current.folder_mode = value;
    }
    if let Some(value) = patch.selected_mailboxes {
        current.selected_mailboxes = value
            .into_iter()
            .map(|value| value.trim().to_string())
            .collect();
    }
    if let Some(value) = patch.notify_on_error {
        current.notify_on_error = value;
    }
    current
}

fn policy_value(value: &SyncPolicySettings) -> Value {
    json!({"enabled":value.enabled,"folderMode":value.folder_mode,"selectedMailboxes":value.selected_mailboxes,"notifyOnError":value.notify_on_error})
}

fn validate_labels(labels: Vec<String>) -> Result<Vec<String>, McpError> {
    if labels.len() > 12
        || labels
            .iter()
            .any(|label| label.trim().is_empty() || label.trim().encode_utf16().count() > 40)
    {
        return Err(McpError::Invalid("邮件标签无效"));
    }
    Ok(labels
        .into_iter()
        .map(|label| label.trim().to_string())
        .collect())
}

fn validate_snoozed(value: Value) -> Result<Option<String>, McpError> {
    match value {
        Value::Null => Ok(None),
        Value::String(value)
            if value.ends_with('Z') && chrono::DateTime::parse_from_rfc3339(&value).is_ok() =>
        {
            Ok(Some(value))
        }
        _ => Err(McpError::Invalid("稍后处理时间无效")),
    }
}

fn list_messages(store: &SqliteAuthStore, owner: &str, input: Value) -> Result<Value, McpError> {
    let email = optional_string(&input, "email");
    let account_id = email
        .map(|email| account_by_email(store, owner, email).map(|account| account.id))
        .transpose()?;
    let limit = input.get("limit").and_then(Value::as_u64).unwrap_or(25) as usize;
    let offset = input.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
    if !(1..=100).contains(&limit) {
        return Err(McpError::Invalid("limit 必须在 1..100 之间"));
    }
    let query = MessageQuery {
        filters: input.get("filters").cloned().map(parse).transpose()?,
        account_id,
        group: optional_string(&input, "group").map(str::to_string),
        text: optional_string(&input, "query").map(str::to_string),
        unread: input
            .get("unread")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        flagged: input
            .get("flagged")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        has_attachments: input
            .get("hasAttachments")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        mailbox_role: optional_string(&input, "mailboxRole").map(str::to_string),
        mailbox: optional_string(&input, "mailboxPath").map(str::to_string),
        snoozed: input
            .get("snoozed")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        label: optional_string(&input, "label").map(str::to_string),
        limit,
        offset,
        ..Default::default()
    };
    let page = MessageQueryService::new(store).query(owner, &query, &now())?;
    let accounts = store
        .list_accounts(owner)?
        .into_iter()
        .map(|account| (account.id, account.email))
        .collect::<HashMap<_, _>>();
    let count = page.messages.len();
    let messages = page
        .messages
        .into_iter()
        .map(|message| {
            let account_id = message.account_id.clone();
            message_summary(
                message,
                accounts.get(&account_id).map(String::as_str).unwrap_or(""),
            )
        })
        .collect::<Vec<_>>();
    Ok(
        json!({"messages":messages,"total":page.total,"offset":offset,"nextOffset":offset+count,"hasMore":offset+count<page.total}),
    )
}

fn list_drafts(store: &mut SqliteAuthStore, owner: &str, input: Value) -> Result<Value, McpError> {
    let account = optional_string(&input, "accountEmail")
        .map(|email| account_by_email(store, owner, email))
        .transpose()?;
    let accounts = store
        .list_accounts(owner)?
        .into_iter()
        .map(|account| (account.id, account.email))
        .collect::<HashMap<_, _>>();
    let mut drafts = DraftService::new(store).list(owner)?;
    drafts.retain(|draft| {
        account
            .as_ref()
            .map_or(true, |account| draft.account_id == account.id)
    });
    drafts.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(json!({"drafts":drafts.into_iter().map(|draft| json!({
        "id":draft.id,"accountEmail":accounts.get(&draft.account_id).cloned().unwrap_or_default(),"to":draft.to,"cc":draft.cc,"bcc":draft.envelope.bcc,"inReplyTo":draft.envelope.reply.in_reply_to,"references":draft.envelope.reply.references,
        "subject":draft.subject,"textPreview":draft.text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(180).collect::<String>(),
        "attachments":without_attachment_data(draft.attachments),"createdAt":draft.created_at,"updatedAt":draft.updated_at
    })).collect::<Vec<_>>() }))
}

fn save_draft(store: &mut SqliteAuthStore, owner: &str, input: Value) -> Result<Value, McpError> {
    let account = account_by_email(store, owner, required_string(&input, "accountEmail")?)?;
    if let Some(id) = optional_string(&input, "draftId") {
        if Uuid::parse_str(id).is_err() {
            return Err(McpError::Invalid("草稿 ID 无效"));
        }
    }
    let id = optional_string(&input, "draftId")
        .map(str::to_string)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let to = validate_address_value(input.get("to").cloned().unwrap_or_else(|| json!([])))?;
    let cc = validate_address_value(input.get("cc").cloned().unwrap_or_else(|| json!([])))?;
    let subject = optional_string(&input, "subject").unwrap_or("");
    let text = optional_string(&input, "text").unwrap_or("");
    let html = optional_string(&input, "html").unwrap_or("");
    if subject.encode_utf16().count() > 500
        || text.encode_utf16().count() > 2_000_000
        || html.encode_utf16().count() > 8_000_000
    {
        return Err(McpError::Invalid("草稿正文或主题超出限制"));
    }
    let attachments = normalize_draft_attachments(
        input
            .get("attachments")
            .cloned()
            .unwrap_or_else(|| json!([])),
    )?;
    let envelope: imail_protocol::ComposeEnvelope = parse(input.clone())?;
    if !envelope.is_valid() {
        return Err(McpError::Invalid("密送或回复关联无效"));
    }
    let draft_input = DraftInput {
        envelope,
        account_id: account.id.clone(),
        to,
        cc,
        subject: subject.into(),
        text: text.into(),
        html: html.into(),
        attachments,
    };
    let service = &mut DraftService::new(store);
    let draft = if optional_string(&input, "draftId").is_some() {
        service.save_existing(owner, &id, &now(), draft_input)?
    } else {
        service.create(owner, &id, &now(), draft_input)?
    };
    Ok(
        json!({"draft":{"id":draft.id,"accountEmail":account.email,"to":draft.to,"cc":draft.cc,"bcc":draft.envelope.bcc,"inReplyTo":draft.envelope.reply.in_reply_to,"references":draft.envelope.reply.references,"subject":draft.subject,"textPreview":draft.text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(180).collect::<String>(),"attachments":without_attachment_data(draft.attachments),"createdAt":draft.created_at,"updatedAt":draft.updated_at}}),
    )
}

fn validate_address_value(value: Value) -> Result<Value, McpError> {
    let values = value
        .as_array()
        .ok_or(McpError::Invalid("收件人列表无效"))?;
    if values.len() > 100
        || values
            .iter()
            .any(|value| value.as_str().map_or(true, |value| !valid_email(value)))
    {
        return Err(McpError::Invalid("收件人列表无效"));
    }
    Ok(value)
}

fn normalize_draft_attachments(value: Value) -> Result<Value, McpError> {
    let values = value.as_array().ok_or(McpError::Invalid("草稿附件无效"))?;
    if values.len() > 10 {
        return Err(McpError::Invalid("草稿附件无效"));
    }
    let mut total = 0usize;
    let mut normalized = Vec::with_capacity(values.len());
    for value in values {
        let object = value.as_object().ok_or(McpError::Invalid("草稿附件无效"))?;
        let filename = object
            .get("filename")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty() && value.encode_utf16().count() <= 255)
            .ok_or(McpError::Invalid("草稿附件无效"))?;
        let content_type = object
            .get("contentType")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty() && value.encode_utf16().count() <= 150)
            .ok_or(McpError::Invalid("草稿附件无效"))?;
        let data = object
            .get("data")
            .and_then(Value::as_str)
            .filter(|value| value.len() <= 21_000_000)
            .ok_or(McpError::Invalid("草稿附件无效"))?;
        let size = STANDARD
            .decode(data)
            .map_err(|_| McpError::Invalid("草稿附件 Base64 无效"))?
            .len();
        total = total
            .checked_add(size)
            .ok_or(McpError::Invalid("草稿附件过大"))?;
        if total > 15 * 1024 * 1024 {
            return Err(McpError::Invalid("草稿附件总大小不能超过 15 MB"));
        }
        let id = object
            .get("id")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty() && value.encode_utf16().count() <= 100)
            .map(str::to_string)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        normalized.push(
            json!({"id":id,"filename":filename,"contentType":content_type,"size":size,"data":data}),
        );
    }
    Ok(Value::Array(normalized))
}

fn tools() -> Vec<Value> {
    static CONTRACT: OnceLock<Vec<Value>> = OnceLock::new();
    CONTRACT
        .get_or_init(|| {
            let contract: Value =
                serde_json::from_str(include_str!("../../../contracts/mcp-tools.json"))
                    .expect("checked-in MCP tools contract must be valid JSON");
            let tools = contract
                .get("tools")
                .and_then(Value::as_array)
                .expect("checked-in MCP tools contract must contain a tools array")
                .clone();
            debug_assert!(tools.iter().all(|tool| {
                tool.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| tool_schema(name).is_object())
            }));
            tools
        })
        .clone()
}

fn search_filters_schema() -> Value {
    serde_json::from_str(include_str!("../../../contracts/search-filters.json"))
        .expect("valid search schema")
}

fn tool_schema(name: &str) -> Value {
    let empty = || json!({"type":"object","properties":{},"additionalProperties":false});
    match name {
        "smart_folders_list" => empty(),
        "smart_folder_save" => {
            json!({"type":"object","properties":{"folderId":{"type":"string","format":"uuid"},"name":{"type":"string","minLength":1,"maxLength":80},"filters":search_filters_schema()},"required":["name","filters"],"additionalProperties":false})
        }
        "smart_folder_delete" => {
            json!({"type":"object","properties":{"folderId":{"type":"string","format":"uuid"}},"required":["folderId"],"additionalProperties":false})
        }
        "imail_status" | "settings_get" | "theme_custom_get" | "accounts_list" | "labels_list" => {
            empty()
        }
        "settings_update" => json!({"type":"object","properties":{
            "theme":{"type":"string","enum":["mint-fresh","tech","business-blue","soft-neubrutalism","constructivist-red","custom"]},
            "customTheme":custom_theme_schema(),
            "composition":composition_schema(),
            "language":{"type":"string","enum":["zh-CN","en-US"]},
            "startupView":{"type":"string","enum":["inbox","starred"]},"markReadOnOpen":{"type":"boolean"},
            "defaultMessageView":{"type":"string","enum":["source","rendered"]},
            "notificationKinds":{"type":"object","properties":{"unread":{"type":"boolean"},"snooze":{"type":"boolean"},"error":{"type":"boolean"}},"additionalProperties":false},
            "shortcutBindings":{"type":"object","properties":shortcut_properties(),"additionalProperties":false}
        },"additionalProperties":false,"minProperties":1}),
        "theme_custom_update" => custom_theme_schema(),
        "account_add_with_code" => json!({"type":"object","properties":{
            "provider":{"type":"string","enum":["outlook","gmail","qq","yahoo","hotmail","icloud","custom"]},"email":email_schema(),
            "displayName":{"type":"string","minLength":1,"maxLength":80},"authorizationCode":{"type":"string","minLength":1,"maxLength":512},
            "group":{"type":"string","minLength":1,"maxLength":40,"default":"个人"},"groupIcon":workspace_icon_schema(),"color":color_schema(),
            "settings":mail_settings_schema(),"proxy":proxy_schema(false)
        },"required":["provider","email","displayName","authorizationCode"],"additionalProperties":false}),
        "account_start_oauth" => json!({"type":"object","properties":{
            "provider":{"type":"string","enum":["gmail","outlook","hotmail","yahoo"]},"displayName":{"type":"string","maxLength":80},
            "group":{"type":"string","minLength":1,"maxLength":40,"default":"个人"},"color":color_schema(),"proxy":proxy_schema(false)
        },"required":["provider"],"additionalProperties":false}),
        "account_reconnect_oauth" | "account_test_connection" | "account_remove" => {
            email_only_schema()
        }
        "account_update" => {
            json!({"type":"object","properties":{"email":email_schema(),"displayName":{"type":"string","minLength":1,"maxLength":80},"group":{"type":"string","minLength":1,"maxLength":40},"groupIcon":workspace_icon_schema(),"color":color_schema()},"required":["email"],"additionalProperties":false,"minProperties":2})
        }
        "account_update_authorization_code" => {
            json!({"type":"object","properties":{"email":email_schema(),"authorizationCode":{"type":"string","minLength":1,"maxLength":512}},"required":["email","authorizationCode"],"additionalProperties":false})
        }
        "account_proxy_update" => {
            json!({"type":"object","properties":{"email":email_schema(),"enabled":{"type":"boolean"},"sourceEmail":email_schema(),"protocol":{"type":"string","enum":["http","https","socks5"]},"host":{"type":"string","minLength":1,"maxLength":253},"port":{"type":"integer","minimum":1,"maximum":65535},"username":{"type":"string","maxLength":256},"password":{"type":"string","maxLength":512}},"required":["email","enabled"],"additionalProperties":false})
        }
        "apple_hme_status" | "apple_hme_list" | "apple_hme_sync" | "apple_hme_disconnect" => {
            email_only_schema()
        }
        "apple_hme_start_login" => json!({"type":"object","properties":{
            "email":email_schema(),"kind":{"type":"string","enum":["icloudWeb","appleAccount"]},
            "password":{"type":"string","minLength":1,"maxLength":512},"appleId":email_schema(),
            "twoFactorMethod":{"type":"string","enum":["trustedDevice","phone"]},
            "phoneNumber":{"type":"object"},"icloudHost":{"type":"string","enum":["https://www.icloud.com","https://www.icloud.com.cn"]}
        },"required":["email","kind","password"],"additionalProperties":false}),
        "apple_hme_submit_two_factor" => json!({"type":"object","properties":{
            "email":email_schema(),"pendingId":{"type":"string","minLength":1,"maxLength":200},
            "code":{"type":"string","pattern":"^[0-9]{6}$"},"phoneNumber":{"type":"object"}
        },"required":["email","pendingId","code"],"additionalProperties":false}),
        "apple_hme_create" => json!({"type":"object","properties":{
            "email":email_schema(),"label":{"type":"string","maxLength":200},"note":{"type":"string","maxLength":500},
            "channel":{"type":"string","enum":["auto","appleAccount","icloudWeb"],"default":"auto"}
        },"required":["email"],"additionalProperties":false}),
        "apple_hme_deactivate" | "apple_hme_delete" => json!({"type":"object","properties":{
            "email":email_schema(),"anonymousId":{"type":"string","minLength":1,"maxLength":200}
        },"required":["email","anonymousId"],"additionalProperties":false}),
        "mailbox_sync" => {
            json!({"type":"object","properties":{"email":email_schema(),"mailboxRole":mailbox_role_schema(),"mailboxPath":{"type":"string","minLength":1,"maxLength":500}},"additionalProperties":false})
        }
        "sync_policy_get" => {
            json!({"type":"object","properties":{"email":email_schema()},"additionalProperties":false})
        }
        "sync_policy_update" => {
            json!({"type":"object","properties":{"email":email_schema(),"enabled":{"type":"boolean"},"folderMode":{"type":"string","enum":["inbox","standard","selected"]},"selectedMailboxes":{"type":"array","items":{"type":"string","minLength":1,"maxLength":500},"maxItems":100},"notifyOnError":{"type":"boolean"}},"additionalProperties":false,"minProperties":1})
        }
        "messages_list" => {
            json!({"type":"object","properties":{"filters":search_filters_schema(),"email":email_schema(),"group":{"type":"string","maxLength":40},"query":{"type":"string","maxLength":200},"mailboxRole":mailbox_role_schema(),"mailboxPath":{"type":"string","maxLength":500},"unread":{"type":"boolean"},"flagged":{"type":"boolean"},"hasAttachments":{"type":"boolean"},"snoozed":{"type":"boolean"},"label":{"type":"string","maxLength":80},"limit":{"type":"integer","minimum":1,"maximum":100,"default":25},"offset":{"type":"integer","minimum":0,"default":0}},"additionalProperties":false})
        }
        "message_get" | "conversation_get" => message_id_schema(),
        "translation_profiles_list" => empty(),
        "message_translate" => {
            json!({"type":"object","properties":{
                "messageId":{"type":"string","minLength":1},
                "profileId":{"type":"string","minLength":1,"maxLength":80},
                "sourceLanguage":{"type":"string","minLength":2,"maxLength":35,"pattern":"^[A-Za-z0-9-]+$"},
                "targetLanguage":{"type":"string","minLength":2,"maxLength":35,"pattern":"^[A-Za-z0-9-]+$"}
            },"required":["messageId","profileId","targetLanguage"],"additionalProperties":false})
        }
        "message_update" => {
            json!({"type":"object","properties":{"messageId":{"type":"string","minLength":1},"unread":{"type":"boolean"},"flagged":{"type":"boolean"},"labels":{"type":"array","items":{"type":"string","minLength":1,"maxLength":40},"maxItems":12},"snoozedUntil":{"type":["string","null"],"format":"date-time"}},"required":["messageId"],"additionalProperties":false,"minProperties":2})
        }
        "message_move" => {
            json!({"type":"object","properties":{"messageId":{"type":"string","minLength":1},"destination":{"type":"string","enum":["archive","trash"]}},"required":["messageId","destination"],"additionalProperties":false})
        }
        "message_send" => {
            json!({"type":"object","properties":{"accountEmail":email_schema(),"to":email_array(0),"cc":email_array(0),"bcc":email_array(0),"inReplyTo":reply_ids_schema(),"references":reply_ids_schema(),"subject":{"type":"string","minLength":1,"maxLength":500},"text":{"type":"string","minLength":1,"maxLength":2000000},"html":{"type":"string","maxLength":8000000},"attachments":attachments_schema()},"required":["accountEmail","to","subject","text"],"additionalProperties":false})
        }
        "attachment_download" => {
            json!({"type":"object","properties":{"messageId":{"type":"string","minLength":1},"index":{"type":"integer","minimum":0}},"required":["messageId","index"],"additionalProperties":false})
        }
        "drafts_list" => {
            json!({"type":"object","properties":{"accountEmail":email_schema()},"additionalProperties":false})
        }
        "draft_get" | "draft_delete" => {
            json!({"type":"object","properties":{"draftId":{"type":"string","format":"uuid"}},"required":["draftId"],"additionalProperties":false})
        }
        "draft_save" => {
            json!({"type":"object","properties":{"draftId":{"type":"string","format":"uuid"},"accountEmail":email_schema(),"to":email_array(0),"cc":email_array(0),"bcc":email_array(0),"inReplyTo":reply_ids_schema(),"references":reply_ids_schema(),"subject":{"type":"string","maxLength":500},"text":{"type":"string","maxLength":2000000},"html":{"type":"string","maxLength":8000000},"attachments":attachments_schema()},"required":["accountEmail"],"additionalProperties":false})
        }
        "notifications_list" => {
            json!({"type":"object","properties":{"limit":{"type":"integer","minimum":1,"maximum":100,"default":30}},"additionalProperties":false})
        }
        _ => empty(),
    }
}

fn email_schema() -> Value {
    json!({"type":"string","format":"email"})
}
fn reply_ids_schema() -> Value {
    json!({"type":"array","items":{"type":"string","minLength":3,"maxLength":998},"maxItems":100})
}
fn composition_schema() -> Value {
    json!({"type":"object","additionalProperties":false,"properties":{
        "signatures":{"type":"array","maxItems":100,"items":{
            "type":"object","additionalProperties":false,
            "properties":{"accountId":{"type":"string","minLength":1,"maxLength":200},"text":{"type":"string","maxLength":16000},"newMessages":{"type":"boolean"},"replies":{"type":"boolean"}},
            "required":["accountId","text","newMessages","replies"]
        }},
        "templates":{"type":"array","maxItems":100,"items":{
            "type":"object","additionalProperties":false,
            "properties":{"id":{"type":"string","minLength":1,"maxLength":200},"name":{"type":"string","minLength":1,"maxLength":200},"subject":{"type":"string","maxLength":1000},"text":{"type":"string","maxLength":64000}},
            "required":["id","name","subject","text"]
        }}
    }})
}
fn email_array(minimum: usize) -> Value {
    json!({"type":"array","items":email_schema(),"minItems":minimum,"maxItems":100})
}
fn color_schema() -> Value {
    json!({"type":"string","pattern":"^#[0-9A-Fa-f]{6}$"})
}
fn custom_theme_schema() -> Value {
    json!({"type":"object","properties":{
        "name":{"type":"string","minLength":1,"maxLength":40},
        "canvas":color_schema(),"surface":color_schema(),"surfaceSubtle":color_schema(),"rail":color_schema(),"text":color_schema(),
        "textSecondary":color_schema(),"border":color_schema(),"accent":color_schema(),"accentSubtle":color_schema(),
        "radius":{"type":"string","enum":["compact","balanced","rounded"]},"shadow":{"type":"string","enum":["none","soft","offset"]},
        "typography":{"type":"string","enum":["system","technical","rounded"]}
    },"required":["name","canvas","surface","surfaceSubtle","rail","text","textSecondary","border","accent","accentSubtle","radius","shadow","typography"],"additionalProperties":false})
}
fn mailbox_role_schema() -> Value {
    json!({"type":"string","enum":["inbox","sent","archive","drafts","trash","junk","custom"]})
}
fn workspace_icon_schema() -> Value {
    json!({"type":"string","enum":["folder","briefcase","building","home","users","code","heart","star"]})
}
fn email_only_schema() -> Value {
    json!({"type":"object","properties":{"email":email_schema()},"required":["email"],"additionalProperties":false})
}
fn message_id_schema() -> Value {
    json!({"type":"object","properties":{"messageId":{"type":"string","minLength":1}},"required":["messageId"],"additionalProperties":false})
}
fn shortcut_properties() -> Value {
    let keys = [
        "focusSearch",
        "compose",
        "sync",
        "nextMessage",
        "previousMessage",
        "reply",
        "forward",
        "toggleStar",
        "markUnread",
        "archive",
        "delete",
        "openShortcutSettings",
    ];
    Value::Object(
        keys.into_iter()
            .map(|key| (key.into(), json!({"type":"string","maxLength":60})))
            .collect(),
    )
}
fn mail_settings_schema() -> Value {
    json!({"type":"object","properties":{"imapHost":{"type":"string","minLength":1},"imapPort":{"type":"integer","minimum":1,"maximum":65535},"imapSecure":{"type":"boolean"},"smtpHost":{"type":"string","minLength":1},"smtpPort":{"type":"integer","minimum":1,"maximum":65535},"smtpSecure":{"type":"boolean"}},"required":["imapHost","imapPort","imapSecure","smtpHost","smtpPort","smtpSecure"],"additionalProperties":false})
}
fn proxy_schema(require_enabled: bool) -> Value {
    let mut schema = json!({"type":"object","properties":{"protocol":{"type":"string","enum":["http","https","socks5"]},"host":{"type":"string","minLength":1,"maxLength":253},"port":{"type":"integer","minimum":1,"maximum":65535},"username":{"type":"string","maxLength":256},"password":{"type":"string","maxLength":512}},"required":["protocol","host","port"],"additionalProperties":false});
    if require_enabled {
        schema["properties"]["enabled"] = json!({"type":"boolean"});
    }
    schema
}
fn attachments_schema() -> Value {
    json!({"type":"array","maxItems":10,"items":{"type":"object","properties":{"id":{"type":"string","minLength":1,"maxLength":100},"filename":{"type":"string","minLength":1,"maxLength":255},"contentType":{"type":"string","minLength":1,"maxLength":150},"data":{"type":"string","maxLength":21000000}},"required":["filename","contentType","data"],"additionalProperties":false}})
}

fn tool_output(value: Value) -> Value {
    json!({"content":[{"type":"text","text":serde_json::to_string_pretty(&value).unwrap_or_else(|_|"{}".into())}],"structuredContent":value})
}

fn safe_application_error(body: &Value) -> String {
    body.get("error")
        .and_then(Value::as_str)
        .filter(|message| !message.trim().is_empty() && message.chars().count() <= 200)
        .unwrap_or("翻译服务暂时无法完成请求")
        .to_string()
}
fn message_summary(message: imail_protocol::MessageReadModel, email: &str) -> Value {
    json!({"id":message.id,"accountEmail":email,"folder":message.mailbox,"mailboxRole":message.mailbox_role,"messageId":message.message_id,"cc":message.headers.cc,"replyTo":message.headers.reply_to,"inReplyTo":message.headers.reply.in_reply_to,"references":message.headers.reply.references,"from":message.from,"to":message.to,"subject":message.subject,"preview":message.preview,"date":message.date,"unread":message.unread,"flagged":message.flagged,"hasAttachments":message.has_attachments,"attachments":message.attachments,"labels":message.labels,"snoozedUntil":message.snoozed_until})
}
fn message_detail(message: imail_protocol::MessageReadModel, email: &str) -> Value {
    let mut value = message_summary(message.clone(), email);
    let object = value.as_object_mut().unwrap();
    object.insert("text".into(), Value::String(message.text));
    object.insert(
        "html".into(),
        message.html.map(Value::String).unwrap_or(Value::Null),
    );
    value
}
fn public_proxy(proxy: Option<Value>) -> Option<Value> {
    proxy.map(|value| public_object(value, &["protocol", "host", "port", "username"]))
}
fn public_settings(value: Value) -> Value {
    public_object(
        value,
        &[
            "imapHost",
            "imapPort",
            "imapSecure",
            "smtpHost",
            "smtpPort",
            "smtpSecure",
        ],
    )
}
fn public_object(value: Value, allowed: &[&str]) -> Value {
    let Some(source) = value.as_object() else {
        return json!({});
    };
    Value::Object(
        allowed
            .iter()
            .filter_map(|key| {
                source
                    .get(*key)
                    .cloned()
                    .map(|value| ((*key).to_string(), value))
            })
            .collect(),
    )
}
fn safe_account(account: imail_protocol::AccountReadModel) -> Value {
    json!({"id":account.id,"provider":account.provider,"email":account.email,"displayName":account.display_name,"group":account.group,
        "groupIcon":account.group_icon,"color":account.color,"settings":public_settings(account.settings),"proxy":public_proxy(account.proxy),"authMethod":account.auth_method,
        "createdAt":account.created_at,"lastSyncAt":account.last_sync_at,"status":account.status,"lastError":account.last_error,"mailboxes":account.mailboxes})
}
fn without_attachment_data(value: Value) -> Value {
    Value::Array(
        value
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|mut item| {
                if let Some(object) = item.as_object_mut() {
                    object.remove("data");
                }
                item
            })
            .collect(),
    )
}
fn account_by_email(
    store: &SqliteAuthStore,
    owner: &str,
    email: &str,
) -> Result<imail_core::AccountRecord, McpError> {
    store
        .list_accounts(owner)?
        .into_iter()
        .find(|a| a.email.eq_ignore_ascii_case(email))
        .ok_or(McpError::Invalid("邮箱账户不存在"))
}
fn account_email(store: &SqliteAuthStore, owner: &str, id: &str) -> Result<String, McpError> {
    store
        .account(owner, id)?
        .map(|a| a.email)
        .ok_or(McpError::Invalid("邮箱账户不存在"))
}
fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, McpError> {
    serde_json::from_value(value).map_err(|_| McpError::Invalid("工具参数无效"))
}
fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, McpError> {
    optional_string(value, key)
        .filter(|v| !v.is_empty())
        .ok_or(McpError::Invalid("缺少必填参数"))
}
fn optional_string<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}
fn valid_email(value: &str) -> bool {
    let value = value.trim();
    let Some((local, domain)) = value.rsplit_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && !value.chars().any(char::is_whitespace)
        && value.encode_utf16().count() <= 320
}
fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}
fn modern_request(
    rpc: &RpcRequest,
    protocol_header: Option<&str>,
    method_header: Option<&str>,
    name_header: Option<&str>,
) -> Result<bool, (i64, String)> {
    let meta = rpc.params.get("_meta").and_then(Value::as_object);
    let claim = meta.and_then(|meta| meta.get(PROTOCOL_META));
    let Some(claim) = claim else {
        if protocol_header == Some(MODERN_PROTOCOL) {
            return Err((
                -32602,
                format!("Invalid params: the MCP-Protocol-Version header names protocol revision {MODERN_PROTOCOL}, but the request is missing the required per-request envelope key(s)"),
            ));
        }
        return Ok(false);
    };
    let claimed = claim.as_str().ok_or_else(|| {
        (
            -32602,
            format!("Invalid _meta envelope for protocol revision {MODERN_PROTOCOL}: {PROTOCOL_META}: expected a protocol version string"),
        )
    })?;
    if protocol_header.is_some_and(|value| value != claimed) {
        return Err((
            -32020,
            "Bad Request: the request headers and body disagree: protocol version mismatch".into(),
        ));
    }
    if claimed != MODERN_PROTOCOL {
        return Ok(false);
    }
    if !meta
        .and_then(|meta| meta.get(CLIENT_CAPABILITIES_META))
        .is_some_and(Value::is_object)
    {
        return Err((
            -32602,
            format!("Invalid _meta envelope for protocol revision {MODERN_PROTOCOL}: {CLIENT_CAPABILITIES_META}: missing or invalid"),
        ));
    }
    if method_header != Some(rpc.method.as_str()) {
        return Err((
            -32020,
            "Bad Request: the request headers and body disagree: Mcp-Method is missing or mismatched".into(),
        ));
    }
    if rpc.method == "tools/call" {
        let body_name = rpc.params.get("name").and_then(Value::as_str);
        if body_name.is_none() || name_header != body_name {
            return Err((
                -32020,
                "Bad Request: the request headers and body disagree: Mcp-Name is missing or mismatched".into(),
            ));
        }
    }
    Ok(true)
}

fn modern_result(method: &str, mut result: Value) -> Value {
    let Some(object) = result.as_object_mut() else {
        return result;
    };
    object
        .entry("resultType")
        .or_insert_with(|| Value::String("complete".into()));
    let metadata = object
        .entry("_meta")
        .or_insert_with(|| json!({}))
        .as_object_mut();
    if let Some(metadata) = metadata {
        metadata
            .entry(SERVER_INFO_META)
            .or_insert_with(|| json!({"name":"imail","version":"1.0.0"}));
    }
    if matches!(method, "tools/list" | "server/discover") {
        object.entry("ttlMs").or_insert_with(|| json!(0));
        object
            .entry("cacheScope")
            .or_insert_with(|| Value::String("private".into()));
    }
    result
}
fn negotiated_protocol(params: &Value) -> &str {
    match params.get("protocolVersion").and_then(Value::as_str) {
        Some(version) if LEGACY_PROTOCOLS.contains(&version) => version,
        _ => "2025-11-25",
    }
}
fn bearer(value: Option<&HeaderValue>) -> Option<&str> {
    let value = value?.to_str().ok()?.trim();
    value
        .get(..7)
        .filter(|p| p.eq_ignore_ascii_case("bearer "))
        .and_then(|_| value.get(7..))
        .map(str::trim)
        .filter(|v| !v.is_empty())
}
fn unauthorized() -> Response {
    let mut response = (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error":"MCP 授权码无效、已过期或已撤销"})),
    )
        .into_response();
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static("Bearer realm=\"iMail MCP\", scope=\"mcp:full\""),
    );
    response
}
fn rpc_http_error(status: StatusCode, id: Value, code: i64, message: &str) -> Response {
    (
        status,
        Json(json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})),
    )
        .into_response()
}
fn tool_error_value(id: Value, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":{"content":[{"type":"text","text":message}],"isError":true}})
}

enum McpError {
    Storage,
    Join,
    Unauthorized,
    Disabled,
    MethodNotFound,
    Invalid(&'static str),
    Application(String),
}
impl From<imail_storage_sqlite::AuthStoreError> for McpError {
    fn from(_: imail_storage_sqlite::AuthStoreError) -> Self {
        Self::Storage
    }
}
impl From<imail_storage_sqlite::SyncRuntimeError> for McpError {
    fn from(_: imail_storage_sqlite::SyncRuntimeError) -> Self {
        Self::Storage
    }
}
impl From<imail_core::ApplicationError<imail_storage_sqlite::AuthStoreError>> for McpError {
    fn from(error: imail_core::ApplicationError<imail_storage_sqlite::AuthStoreError>) -> Self {
        Self::Application(error.to_string())
    }
}
impl From<imail_core::mail_operations::MailApplicationError<imail_storage_sqlite::AuthStoreError>>
    for McpError
{
    fn from(
        error: imail_core::mail_operations::MailApplicationError<
            imail_storage_sqlite::AuthStoreError,
        >,
    ) -> Self {
        Self::Application(error.to_string())
    }
}
async fn run<T: Send + 'static>(
    database: PathBuf,
    operation: impl FnOnce(&mut SqliteAuthStore) -> Result<T, McpError> + Send + 'static,
) -> Result<T, McpError> {
    tokio::task::spawn_blocking(move || {
        let mut store = SqliteAuthStore::open_database(database)?;
        operation(&mut store)
    })
    .await
    .map_err(|_| McpError::Join)?
}

#[cfg(test)]
mod composition_tests {
    use super::*;
    #[test]
    fn flattened_envelope_accepts_known_fields_and_rejects_unknown_fields() {
        let input = json!({"accountEmail":"owner@example.com","to":[],"bcc":["hidden@example.com"],"subject":"Reply","text":"Body","inReplyTo":["<parent@example.com>"]});
        let parsed: McpSendInput = serde_json::from_value(input.clone()).unwrap();
        assert_eq!(parsed.envelope().bcc, ["hidden@example.com"]);
        assert_eq!(
            parsed.envelope().reply.in_reply_to,
            ["<parent@example.com>"]
        );
        let mut invalid = input;
        invalid["unexpected"] = json!(true);
        assert!(serde_json::from_value::<McpSendInput>(invalid).is_err());
    }
}
