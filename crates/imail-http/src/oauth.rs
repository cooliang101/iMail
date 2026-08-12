use std::{collections::BTreeMap, net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    extract::{ConnectInfo, Path, Query, State},
    http::{header::CONTENT_TYPE, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use chrono::Utc;
use imail_core::{
    accounts::AccountService, mail_operations::connection_config,
    oauth_accounts::OAuthAccountService, ApplicationError, AuthRepository,
};
use imail_mail::{redact_protocol_detail, ProtocolFailure, ProtocolStage};
use imail_oauth::{
    callback_error, provider_for_account, BeginOAuthInput, OAuthCallback, OAuthConfig, OAuthError,
    OAuthProviderKey, OAuthProxyInput, OAuthService, OAUTH_STATE_TTL_MS,
};
use imail_oauth_http::loopback::LoopbackCallbackServer;
use imail_security::MasterKey;
use imail_storage_sqlite::{
    AuthStoreError, MasterKeyCredentialCodec, SqliteAuthStore, SyncRuntimeStore,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::Value;
use uuid::Uuid;

use crate::{
    accounts::{application_error, HttpAccount},
    auth::AuthenticatedUser,
    AppState, EmbeddedOperationError,
};

pub(crate) fn protected_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/oauth/start", post(start))
        .route("/api/oauth/status", post(status))
        .route("/api/accounts/:id/oauth/reconnect", post(reconnect))
}

pub(crate) fn public_routes() -> Router<Arc<AppState>> {
    Router::new().route("/api/oauth/:provider/callback", get(callback))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartInput {
    provider: String,
    display_name: Option<String>,
    #[serde(default = "default_group")]
    group: String,
    #[serde(default = "default_color")]
    color: String,
    proxy: Option<ProxyInput>,
}

#[derive(Deserialize)]
struct ProxyInput {
    protocol: String,
    host: String,
    port: u16,
    username: Option<String>,
    password: Option<String>,
}

#[derive(Deserialize)]
struct StateInput {
    state: String,
}

#[derive(Deserialize)]
struct CallbackQuery {
    state: Option<String>,
    code: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusBody {
    completed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    account: Option<HttpAccount>,
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

async fn start(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(input): Json<StartInput>,
) -> Response {
    let (key, input) = match validated_start(input) {
        Ok(input) => input,
        Err(()) => return error(StatusCode::BAD_REQUEST, "请求参数无效"),
    };
    begin(state, user, key, input, "account.oauth-started").await
}

async fn reconnect(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(account_id): Path<String>,
) -> Response {
    let data_dir = state.config.data_dir.clone();
    let owner = user.user_id.clone();
    let account_id_for_load = account_id.clone();
    let loaded = tokio::task::spawn_blocking(move || {
        let mut store = SqliteAuthStore::open_database(data_dir.join("imail.sqlite"))
            .map_err(ApplicationError::Repository)?;
        AccountService::new(&mut store).get(&owner, &account_id_for_load)
    })
    .await;
    let account = match loaded {
        Ok(Ok(account)) => account,
        Ok(Err(cause)) => return application_error(cause),
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "服务暂时无法完成请求"),
    };
    if account.auth_method.as_deref() != Some("oauth2") {
        return error(StatusCode::CONFLICT, "这个邮箱不使用 OAuth 授权");
    }
    let Some(key) = provider_for_account(&account.provider) else {
        return error(StatusCode::BAD_REQUEST, "这个邮箱服务商不支持 OAuth");
    };
    let proxy = account.proxy.and_then(public_proxy_to_oauth);
    begin(
        state,
        user,
        key,
        BeginOAuthInput {
            owner_id: String::new(),
            account_provider: account.provider,
            display_name: Some(account.display_name),
            group: Some(account.group),
            color: Some(account.color),
            account_id: Some(account_id),
            expected_email: Some(account.email),
            proxy,
        },
        "account.oauth-started",
    )
    .await
}

async fn begin(
    state: Arc<AppState>,
    user: AuthenticatedUser,
    key: OAuthProviderKey,
    mut input: BeginOAuthInput,
    event: &'static str,
) -> Response {
    input.owner_id = user.user_id.clone();
    let data_dir = state.config.data_dir.clone();
    let environment = state.config.oauth_environment.clone();
    let factory = Arc::clone(&state.config.oauth_provider_factory);
    let resolver = Arc::clone(&state.config.oauth_config_resolver);
    let owner = user.user_id;
    let actor = user.actor;
    let result = tokio::task::spawn_blocking(move || {
        let master_key = MasterKey::from_file(data_dir.join("master.key"))
            .map_err(|_| OAuthHttpError::Internal)?;
        let config = resolver.resolve(&environment, key, Some(&input.account_provider));
        let mut provider = factory.create();
        let result = OAuthService::new(&master_key, provider.as_mut())
            .begin(&config, input, Utc::now().timestamp_millis())
            .map_err(OAuthHttpError::OAuth)?;
        let mut store = SqliteAuthStore::open_database(data_dir.join("imail.sqlite"))
            .map_err(OAuthHttpError::Storage)?;
        let mut detail = BTreeMap::new();
        detail.insert("provider".into(), result.provider.as_str().into());
        store
            .record_security_event(event, &actor, Some(&owner), &detail)
            .map_err(OAuthHttpError::Storage)?;
        Ok::<_, OAuthHttpError>(result)
    })
    .await;
    match result {
        Ok(Ok(result)) => Json(result).into_response(),
        Ok(Err(cause)) => oauth_error(cause),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "服务暂时无法完成请求"),
    }
}

pub(crate) async fn mcp_start(
    state: Arc<AppState>,
    owner_id: String,
    input: Value,
) -> Result<Value, ApplicationError<AuthStoreError>> {
    let input: StartInput = serde_json::from_value(input).map_err(|_| oauth_mcp_invalid())?;
    let (key, mut input) = validated_start(input).map_err(|_| oauth_mcp_invalid())?;
    input.owner_id = owner_id;
    mcp_begin(state, key, input).await
}

pub(crate) async fn mcp_reconnect(
    state: Arc<AppState>,
    owner_id: String,
    account_id: String,
) -> Result<Value, ApplicationError<AuthStoreError>> {
    let database = state.config.data_dir.join("imail.sqlite");
    let lookup_owner = owner_id.clone();
    let lookup_id = account_id.clone();
    let account = tokio::task::spawn_blocking(move || {
        let mut store =
            SqliteAuthStore::open_database(database).map_err(ApplicationError::Repository)?;
        AccountService::new(&mut store).get(&lookup_owner, &lookup_id)
    })
    .await
    .map_err(|_| oauth_mcp_internal())??;
    if account.auth_method.as_deref() != Some("oauth2") {
        return Err(ApplicationError::Domain {
            code: "OAUTH_RECONNECT_REQUIRED",
            status: 409,
            message: "这个邮箱不使用 OAuth 授权",
        });
    }
    let key = provider_for_account(&account.provider).ok_or_else(oauth_mcp_invalid)?;
    let proxy = account.proxy.and_then(public_proxy_to_oauth);
    mcp_begin(
        state,
        key,
        BeginOAuthInput {
            owner_id,
            account_provider: account.provider,
            display_name: Some(account.display_name),
            group: Some(account.group),
            color: Some(account.color),
            account_id: Some(account_id),
            expected_email: Some(account.email),
            proxy,
        },
    )
    .await
}

async fn mcp_begin(
    state: Arc<AppState>,
    key: OAuthProviderKey,
    input: BeginOAuthInput,
) -> Result<Value, ApplicationError<AuthStoreError>> {
    let data_dir = state.config.data_dir.clone();
    let environment = state.config.oauth_environment.clone();
    let factory = Arc::clone(&state.config.oauth_provider_factory);
    let resolver = Arc::clone(&state.config.oauth_config_resolver);
    tokio::task::spawn_blocking(move || {
        let master_key =
            MasterKey::from_file(data_dir.join("master.key")).map_err(|_| oauth_mcp_internal())?;
        let config = resolver.resolve(&environment, key, Some(&input.account_provider));
        let mut provider = factory.create();
        let result = OAuthService::new(&master_key, provider.as_mut())
            .begin(&config, input, Utc::now().timestamp_millis())
            .map_err(|_| ApplicationError::Domain {
                code: "OAUTH_START_FAILED",
                status: 422,
                message: "无法开始 OAuth 授权",
            })?;
        serde_json::to_value(result).map_err(|_| oauth_mcp_internal())
    })
    .await
    .map_err(|_| oauth_mcp_internal())?
}

fn oauth_mcp_invalid() -> ApplicationError<AuthStoreError> {
    ApplicationError::Domain {
        code: "OAUTH_REQUEST_INVALID",
        status: 400,
        message: "OAuth 请求参数无效",
    }
}
fn oauth_mcp_internal() -> ApplicationError<AuthStoreError> {
    ApplicationError::Domain {
        code: "OAUTH_SERVICE_UNAVAILABLE",
        status: 500,
        message: "OAuth 服务暂时不可用",
    }
}

pub(crate) async fn embedded_start(
    state: Arc<AppState>,
    owner_id: String,
    actor: String,
    input: Value,
) -> Result<Value, EmbeddedOperationError> {
    let input = serde_json::from_value::<StartInput>(input)
        .map_err(|_| embedded_error(422, "请求参数无效"))?;
    let (key, mut input) =
        validated_start(input).map_err(|_| embedded_error(400, "请求参数无效"))?;
    input.owner_id = owner_id.clone();
    embedded_begin(state, owner_id, actor, key, input).await
}

pub(crate) async fn embedded_reconnect(
    state: Arc<AppState>,
    owner_id: String,
    actor: String,
    account_id: String,
) -> Result<Value, EmbeddedOperationError> {
    let database = state.config.data_dir.join("imail.sqlite");
    let lookup_owner = owner_id.clone();
    let lookup_id = account_id.clone();
    let account = tokio::task::spawn_blocking(move || {
        let mut store = SqliteAuthStore::open_database(database)
            .map_err(|_| embedded_error(500, "服务暂时无法完成请求"))?;
        AccountService::new(&mut store)
            .get(&lookup_owner, &lookup_id)
            .map_err(embedded_application_error)
    })
    .await
    .map_err(|_| embedded_error(500, "服务暂时无法完成请求"))??;
    if account.auth_method.as_deref() != Some("oauth2") {
        return Err(embedded_error(409, "这个邮箱不使用 OAuth 授权"));
    }
    let key = provider_for_account(&account.provider)
        .ok_or_else(|| embedded_error(400, "这个邮箱服务商不支持 OAuth"))?;
    let proxy = account.proxy.and_then(public_proxy_to_oauth);
    embedded_begin(
        state,
        owner_id.clone(),
        actor,
        key,
        BeginOAuthInput {
            owner_id,
            account_provider: account.provider,
            display_name: Some(account.display_name),
            group: Some(account.group),
            color: Some(account.color),
            account_id: Some(account_id),
            expected_email: Some(account.email),
            proxy,
        },
    )
    .await
}

async fn embedded_begin(
    state: Arc<AppState>,
    owner_id: String,
    actor: String,
    key: OAuthProviderKey,
    input: BeginOAuthInput,
) -> Result<Value, EmbeddedOperationError> {
    let begin_state = Arc::clone(&state);
    let audit_owner_id = owner_id.clone();
    let (result, server, config) = tokio::task::spawn_blocking(move || {
        let data_dir = begin_state.config.data_dir.clone();
        let master_key = MasterKey::from_file(data_dir.join("master.key"))
            .map_err(|_| embedded_error(500, "服务暂时无法完成请求"))?;
        let mut config = begin_state.config.oauth_config_resolver.resolve(
            &begin_state.config.oauth_environment,
            key,
            Some(&input.account_provider),
        );
        let server = LoopbackCallbackServer::bind(&config.redirect_uri)
            .map_err(|error| embedded_oauth_error(OAuthHttpError::OAuth(error)))?;
        config.redirect_uri = server.redirect_uri().to_string();
        let mut provider = begin_state.config.oauth_provider_factory.create();
        let result = OAuthService::new(&master_key, provider.as_mut())
            .begin(&config, input, Utc::now().timestamp_millis())
            .map_err(|error| embedded_oauth_error(OAuthHttpError::OAuth(error)))?;
        let mut store = SqliteAuthStore::open_database(data_dir.join("imail.sqlite"))
            .map_err(|_| embedded_error(500, "服务暂时无法完成请求"))?;
        store
            .record_security_event(
                "account.oauth-started",
                &actor,
                Some(&audit_owner_id),
                &BTreeMap::from([("provider".into(), result.provider.as_str().into())]),
            )
            .map_err(|_| embedded_error(500, "服务暂时无法完成请求"))?;
        Ok::<_, EmbeddedOperationError>((result, server, config))
    })
    .await
    .map_err(|_| embedded_error(500, "服务暂时无法完成请求"))??;

    let raw_state = result.state.clone();
    let callback_state = raw_state.clone();
    let callback_owner = owner_id;
    let callback_app_state = Arc::clone(&state);
    let mut callback_shutdown = state.shutdown.subscribe();
    tokio::task::spawn_blocking(move || {
        let callback = server.wait_until(
            &callback_state,
            Duration::from_millis(OAUTH_STATE_TTL_MS as u64),
            || {
                !matches!(
                    callback_shutdown.try_recv(),
                    Err(tokio::sync::broadcast::error::TryRecvError::Empty)
                )
            },
        );
        let outcome = match callback {
            Ok(OAuthCallback {
                error: Some(provider_error),
                error_description,
                ..
            }) => crate::OAuthCompletionOutcome::Failed {
                message: redact_protocol_detail(&callback_error(
                    &provider_error,
                    error_description.as_deref(),
                )),
            },
            Ok(callback) => match complete_callback_application(
                &callback_app_state,
                key,
                &callback_state,
                callback.code.as_deref(),
                "tauri-loopback",
                Some(config),
            ) {
                Ok((_, account_id)) => crate::OAuthCompletionOutcome::Success { account_id },
                Err(error) => crate::OAuthCompletionOutcome::Failed {
                    message: error.safe_message(),
                },
            },
            Err(error) => crate::OAuthCompletionOutcome::Failed {
                message: OAuthHttpError::OAuth(error).safe_message(),
            },
        };
        callback_app_state
            .completed_oauth
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(
                raw_state,
                crate::CompletedOAuthRecord {
                    owner_id: callback_owner,
                    completed_at_ms: Utc::now().timestamp_millis(),
                    outcome,
                },
            );
    });
    serde_json::to_value(result).map_err(|_| embedded_error(500, "服务暂时无法完成请求"))
}

pub(crate) async fn embedded_status(
    state: Arc<AppState>,
    owner_id: String,
    input: Value,
) -> Result<Value, EmbeddedOperationError> {
    let input = serde_json::from_value::<StateInput>(input)
        .map_err(|_| embedded_error(422, "请求参数无效"))?;
    if input.state.is_empty() || input.state.len() > 16_384 {
        return Err(embedded_error(400, "请求参数无效"));
    }
    let now = Utc::now().timestamp_millis();
    let record = {
        let mut completed = state
            .completed_oauth
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        completed.retain(|_, item| now.saturating_sub(item.completed_at_ms) <= OAUTH_STATE_TTL_MS);
        completed
            .get(&input.state)
            .filter(|item| item.owner_id == owner_id)
            .cloned()
    };
    let Some(record) = record else {
        return Ok(json!({"completed":false}));
    };
    let account_id = match record.outcome {
        crate::OAuthCompletionOutcome::Success { account_id } => account_id,
        crate::OAuthCompletionOutcome::Failed { message } => {
            return Err(embedded_error(400, &message))
        }
    };
    let database = state.config.data_dir.join("imail.sqlite");
    let account = tokio::task::spawn_blocking(move || {
        let mut store = SqliteAuthStore::open_database(database)
            .map_err(|_| embedded_error(500, "服务暂时无法完成请求"))?;
        AccountService::new(&mut store)
            .get(&owner_id, &account_id)
            .map_err(embedded_application_error)
    })
    .await
    .map_err(|_| embedded_error(500, "服务暂时无法完成请求"))??;
    Ok(json!({"completed":true,"account":HttpAccount::from(account)}))
}

fn embedded_application_error(error: ApplicationError<AuthStoreError>) -> EmbeddedOperationError {
    match error {
        ApplicationError::Domain {
            status, message, ..
        } if status < 500 => embedded_error(status, message),
        _ => embedded_error(500, "服务暂时无法完成请求"),
    }
}

fn embedded_oauth_error(error: OAuthHttpError) -> EmbeddedOperationError {
    EmbeddedOperationError {
        status: error.status().as_u16(),
        message: error.safe_message(),
    }
}

fn embedded_error(status: u16, message: &str) -> EmbeddedOperationError {
    EmbeddedOperationError {
        status,
        message: message.to_string(),
    }
}

async fn status(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(input): Json<StateInput>,
) -> Response {
    if input.state.is_empty() || input.state.len() > 16_384 {
        return error(StatusCode::BAD_REQUEST, "请求参数无效");
    }
    let now = Utc::now().timestamp_millis();
    let record = {
        let mut completed = state
            .completed_oauth
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        completed.retain(|_, item| now.saturating_sub(item.completed_at_ms) <= OAUTH_STATE_TTL_MS);
        completed
            .get(&input.state)
            .filter(|item| item.owner_id == user.user_id)
            .cloned()
    };
    let Some(record) = record else {
        return Json(StatusBody {
            completed: false,
            account: None,
        })
        .into_response();
    };
    let account_id = match record.outcome {
        crate::OAuthCompletionOutcome::Success { account_id } => account_id,
        crate::OAuthCompletionOutcome::Failed { message } => {
            return error(StatusCode::BAD_REQUEST, message)
        }
    };
    let database = state.config.data_dir.join("imail.sqlite");
    let owner = user.user_id;
    match tokio::task::spawn_blocking(move || {
        let mut store =
            SqliteAuthStore::open_database(database).map_err(ApplicationError::Repository)?;
        AccountService::new(&mut store).get(&owner, &account_id)
    })
    .await
    {
        Ok(Ok(account)) => Json(StatusBody {
            completed: true,
            account: Some(account.into()),
        })
        .into_response(),
        Ok(Err(cause)) => application_error(cause),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "服务暂时无法完成请求"),
    }
}

async fn callback(
    State(state): State<Arc<AppState>>,
    Path(provider): Path<String>,
    Query(query): Query<CallbackQuery>,
    connect: Option<ConnectInfo<SocketAddr>>,
) -> Response {
    let Some(key) = callback_provider(&provider) else {
        return callback_page(
            StatusCode::NOT_FOUND,
            false,
            "不支持的 OAuth 服务商",
            &state.config.oauth_frontend_origin,
        );
    };
    let Some(raw_state) = query
        .state
        .filter(|value| !value.is_empty() && value.len() <= 16_384)
    else {
        return callback_page(
            StatusCode::BAD_REQUEST,
            false,
            "OAuth 回调缺少 state",
            &state.config.oauth_frontend_origin,
        );
    };
    let already_completed = {
        let now = Utc::now().timestamp_millis();
        let mut completed = state
            .completed_oauth
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        completed.retain(|_, item| now.saturating_sub(item.completed_at_ms) <= OAUTH_STATE_TTL_MS);
        completed.contains_key(&raw_state)
    };
    if already_completed {
        return callback_page(
            StatusCode::OK,
            true,
            "邮箱授权已完成",
            &state.config.oauth_frontend_origin,
        );
    }
    if let Some(provider_error) = query.error {
        let detail = callback_error(&provider_error, query.error_description.as_deref());
        return callback_page(
            StatusCode::BAD_REQUEST,
            false,
            &redact_protocol_detail(&detail),
            &state.config.oauth_frontend_origin,
        );
    }
    if !state
        .oauth_in_flight
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(raw_state.clone())
    {
        return callback_page(
            StatusCode::ACCEPTED,
            false,
            "邮箱授权正在处理，请稍候",
            &state.config.oauth_frontend_origin,
        );
    }
    let callback_state = raw_state.clone();
    let callback_app_state = Arc::clone(&state);
    let callback_code = query.code;
    let actor = connect
        .map(|value| value.0.ip().to_string())
        .unwrap_or_else(|| "http:unknown".into());
    let completed = tokio::task::spawn_blocking(move || {
        complete_callback_application(
            &callback_app_state,
            key,
            &callback_state,
            callback_code.as_deref(),
            &actor,
            None,
        )
    })
    .await;
    match completed {
        Ok(Ok((owner_id, account_id))) => {
            state
                .completed_oauth
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(
                    raw_state.clone(),
                    crate::CompletedOAuthRecord {
                        owner_id,
                        completed_at_ms: Utc::now().timestamp_millis(),
                        outcome: crate::OAuthCompletionOutcome::Success { account_id },
                    },
                );
            state
                .oauth_in_flight
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&raw_state);
            callback_page(
                StatusCode::OK,
                true,
                "邮箱授权成功，可以关闭此窗口",
                &state.config.oauth_frontend_origin,
            )
        }
        Ok(Err(cause)) => {
            state
                .oauth_in_flight
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&raw_state);
            callback_page(
                cause.status(),
                false,
                &cause.safe_message(),
                &state.config.oauth_frontend_origin,
            )
        }
        Err(_) => {
            state
                .oauth_in_flight
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&raw_state);
            callback_page(
                StatusCode::INTERNAL_SERVER_ERROR,
                false,
                "服务暂时无法完成请求",
                &state.config.oauth_frontend_origin,
            )
        }
    }
}

fn complete_callback_application(
    state: &Arc<AppState>,
    key: OAuthProviderKey,
    callback_state: &str,
    code: Option<&str>,
    actor: &str,
    config_override: Option<OAuthConfig>,
) -> Result<(String, String), OAuthHttpError> {
    let data_dir = state.config.data_dir.clone();
    let master_key =
        MasterKey::from_file(data_dir.join("master.key")).map_err(|_| OAuthHttpError::Internal)?;
    let mut provider = state.config.oauth_provider_factory.create();
    let mut service = OAuthService::new(&master_key, provider.as_mut());
    let pending = service
        .validate_state(key, callback_state, Utc::now().timestamp_millis())
        .map_err(OAuthHttpError::OAuth)?;
    let config = config_override.unwrap_or_else(|| {
        state.config.oauth_config_resolver.resolve(
            &state.config.oauth_environment,
            key,
            Some(&pending.account_provider),
        )
    });
    let completed = service
        .complete(
            &config,
            key,
            Some(callback_state),
            code,
            Utc::now().timestamp_millis(),
        )
        .map_err(OAuthHttpError::OAuth)?;
    let owner = completed.pending.owner_id.clone();
    let database = data_dir.join("imail.sqlite");
    let mut store = SqliteAuthStore::open_database(&database).map_err(OAuthHttpError::Storage)?;
    let codec = MasterKeyCredentialCodec::new(&master_key);
    let validator = |candidate: &imail_core::AccountRecord| {
        let config = connection_config::<AuthStoreError, _>(candidate, &codec).map_err(|_| {
            ProtocolFailure::from_provider(ProtocolStage::Imap, None, "邮箱凭据不可用")
        })?;
        state
            .config
            .connection_probe
            .verify(&config)
            .map_err(|detail| {
                ProtocolFailure::from_provider(
                    ProtocolStage::Imap,
                    None,
                    &redact_protocol_detail(&detail),
                )
            })
    };
    let account = OAuthAccountService::new(&mut store)
        .persist_completion(
            completed,
            &Uuid::new_v4().to_string(),
            &Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            &codec,
            &validator,
        )
        .map_err(OAuthHttpError::Application)?;
    SyncRuntimeStore::open_database(&database)
        .and_then(|sync| sync.ensure_policy(&account.id, &owner, Utc::now()))
        .map_err(|_| OAuthHttpError::Internal)?;
    store
        .record_security_event(
            "account.oauth-completed",
            actor,
            Some(&owner),
            &BTreeMap::from([
                ("accountId".into(), account.id.clone()),
                ("provider".into(), key.as_str().into()),
            ]),
        )
        .map_err(OAuthHttpError::Storage)?;
    Ok((owner, account.id))
}

enum OAuthHttpError {
    OAuth(OAuthError),
    Storage(AuthStoreError),
    Application(ApplicationError<AuthStoreError>),
    Internal,
}

impl OAuthHttpError {
    fn status(&self) -> StatusCode {
        match self {
            Self::OAuth(OAuthError::Configuration(_)) => StatusCode::SERVICE_UNAVAILABLE,
            Self::OAuth(
                OAuthError::InvalidState
                | OAuthError::MissingOwner
                | OAuthError::MissingCallbackParameters,
            ) => StatusCode::BAD_REQUEST,
            Self::Application(ApplicationError::Domain { status, .. }) => {
                StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_REQUEST)
            }
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
    fn safe_message(&self) -> String {
        match self {
            Self::OAuth(OAuthError::Configuration(message)) => message.clone(),
            Self::OAuth(
                OAuthError::InvalidState
                | OAuthError::MissingOwner
                | OAuthError::MissingCallbackParameters,
            ) => self.to_string(),
            Self::Application(ApplicationError::Domain {
                message, status, ..
            }) if *status < 500 => (*message).into(),
            _ => "服务暂时无法完成请求".into(),
        }
    }
}

impl std::fmt::Display for OAuthHttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OAuth(error) => write!(f, "{error}"),
            Self::Storage(error) => write!(f, "{error}"),
            Self::Application(error) => write!(f, "{error}"),
            Self::Internal => f.write_str("服务暂时无法完成请求"),
        }
    }
}

fn oauth_error(error_value: OAuthHttpError) -> Response {
    error(error_value.status(), error_value.safe_message())
}
fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        Json(ErrorBody {
            error: message.into(),
        }),
    )
        .into_response()
}

fn callback_page(status: StatusCode, success: bool, message: &str, origin: &str) -> Response {
    let payload = serde_json::to_string(
        &json!({"type":"imail-oauth-callback","success":success,"message":message}),
    )
    .unwrap_or_else(|_| "{}".into())
    .replace('<', "\\u003c");
    let target =
        serde_json::to_string(origin).unwrap_or_else(|_| "\"http://localhost:5173\"".into());
    let title = if success {
        "授权完成"
    } else {
        "授权失败"
    };
    let html = format!("<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\"><title>{}</title></head><body><main><h1>{}</h1><p>{}</p></main><script>if(window.opener)window.opener.postMessage({},{});</script></body></html>", escape_html(title), escape_html(title), escape_html(message), payload, target);
    (
        status,
        [(CONTENT_TYPE, "text/html; charset=utf-8")],
        Html(html),
    )
        .into_response()
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn default_group() -> String {
    "个人".into()
}
fn default_color() -> String {
    "#168f78".into()
}
fn invalid_text(value: Option<&str>, max: usize) -> bool {
    value.is_some_and(|v| v.trim().is_empty() || v.encode_utf16().count() > max)
}
fn valid_color(value: &str) -> bool {
    value.len() == 7 && value.starts_with('#') && value[1..].bytes().all(|b| b.is_ascii_hexdigit())
}
fn start_provider(value: &str) -> Option<(OAuthProviderKey, &'static str)> {
    match value {
        "gmail" | "google" => Some((OAuthProviderKey::Google, "gmail")),
        "outlook" | "hotmail" | "microsoft" => Some((
            OAuthProviderKey::Microsoft,
            if value == "hotmail" {
                "hotmail"
            } else {
                "outlook"
            },
        )),
        "yahoo" => Some((OAuthProviderKey::Yahoo, "yahoo")),
        _ => None,
    }
}

fn validated_start(input: StartInput) -> Result<(OAuthProviderKey, BeginOAuthInput), ()> {
    let (key, account_provider) = start_provider(&input.provider).ok_or(())?;
    if invalid_text(input.display_name.as_deref(), 80)
        || input.group.trim().is_empty()
        || input.group.encode_utf16().count() > 40
        || !valid_color(&input.color)
    {
        return Err(());
    }
    let proxy = input.proxy.map(valid_proxy).transpose()?;
    Ok((
        key,
        BeginOAuthInput {
            owner_id: String::new(),
            account_provider: account_provider.into(),
            display_name: input.display_name,
            group: Some(input.group),
            color: Some(input.color),
            account_id: None,
            expected_email: None,
            proxy,
        },
    ))
}
fn callback_provider(value: &str) -> Option<OAuthProviderKey> {
    match value {
        "google" => Some(OAuthProviderKey::Google),
        "microsoft" => Some(OAuthProviderKey::Microsoft),
        "yahoo" => Some(OAuthProviderKey::Yahoo),
        _ => None,
    }
}
fn valid_proxy(value: ProxyInput) -> Result<OAuthProxyInput, ()> {
    if !matches!(value.protocol.as_str(), "socks5" | "http")
        || value.host.trim().is_empty()
        || value.port == 0
        || value.username.as_ref().is_some_and(|v| v.is_empty())
        || value.password.as_ref().is_some_and(|v| v.is_empty())
    {
        return Err(());
    }
    Ok(OAuthProxyInput {
        protocol: value.protocol,
        host: value.host.trim().into(),
        port: value.port,
        username: value.username,
        password: value.password,
    })
}
fn public_proxy_to_oauth(value: serde_json::Value) -> Option<OAuthProxyInput> {
    Some(OAuthProxyInput {
        protocol: value.get("protocol")?.as_str()?.into(),
        host: value.get("host")?.as_str()?.into(),
        port: u16::try_from(value.get("port")?.as_u64()?).ok()?,
        username: value
            .get("username")
            .and_then(|v| v.as_str())
            .map(str::to_owned),
        password: None,
    })
}
