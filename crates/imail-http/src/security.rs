use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{
        header::{
            CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE, EXPIRES, PRAGMA,
            RETRY_AFTER,
        },
        HeaderValue, StatusCode,
    },
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use chrono::{SecondsFormat, Utc};
use imail_core::{
    authorization_export::AuthorizationExportService, privacy::PrivacyService, AuthRepository,
};
use imail_security::MasterKey;
use imail_storage_sqlite::{
    MasterKeyCredentialCodec, PortableAuthorizationExportEncryptor, SqliteAuthStore,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{auth::AuthenticatedUser, AppState};

const EXPORT_TTL_MS: i64 = 2 * 60_000;
const MAX_PENDING_EXPORTS: usize = 100;
const SENSITIVE_SOURCE_MAXIMUM: u32 = 20;
const SENSITIVE_USER_MAXIMUM: u32 = 5;
const SENSITIVE_WINDOW_MS: u64 = 15 * 60_000;
const CLEAR_CONFIRMATION: &str = "清除我的邮箱数据";

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/security/audit-events", get(audit_events))
        .route(
            "/api/security/mail-authorization-exports",
            post(prepare_export),
        )
        .route(
            "/api/security/mail-authorization-exports/:id",
            get(download_export),
        )
        .route("/api/security/clear-user-data", post(clear_user_data))
}

#[derive(Default)]
pub(crate) struct SecurityState {
    exports: Mutex<HashMap<String, PendingExport>>,
    user_locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl SecurityState {
    fn user_lock(&self, user_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        Arc::clone(
            self.user_locks
                .lock()
                .expect("security user lock poisoned")
                .entry(user_id.to_string())
                .or_default(),
        )
    }

    fn insert_export(&self, pending: PendingExport) {
        let now = now_ms();
        let mut exports = self.exports.lock().expect("pending export lock poisoned");
        exports.retain(|_, item| item.expires_at_ms > now && item.user_id != pending.user_id);
        if exports.len() >= MAX_PENDING_EXPORTS {
            if let Some(oldest) = exports
                .values()
                .min_by_key(|item| item.prepared_at_ms)
                .map(|item| item.id.clone())
            {
                exports.remove(&oldest);
            }
        }
        exports.insert(pending.id.clone(), pending);
    }

    fn consume_export(&self, id: &str, user_id: &str) -> Option<PendingExport> {
        let now = now_ms();
        let mut exports = self.exports.lock().expect("pending export lock poisoned");
        exports.retain(|_, item| item.expires_at_ms > now);
        if exports.get(id).is_some_and(|item| item.user_id == user_id) {
            exports.remove(id)
        } else {
            None
        }
    }

    fn invalidate_user(&self, user_id: &str) {
        self.exports
            .lock()
            .expect("pending export lock poisoned")
            .retain(|_, item| item.user_id != user_id);
    }
}

struct PendingExport {
    id: String,
    user_id: String,
    body: Vec<u8>,
    filename: String,
    account_count: usize,
    prepared_at_ms: i64,
    expires_at_ms: i64,
}

impl Drop for PendingExport {
    fn drop(&mut self) {
        self.body.fill(0);
    }
}

#[derive(Deserialize)]
struct AuditQuery {
    limit: Option<usize>,
}

#[derive(Serialize)]
struct AuditBody {
    events: Vec<imail_protocol::SecurityAuditEventReadModel>,
}

async fn audit_events(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Query(query): Query<AuditQuery>,
) -> Response {
    let limit = query.limit.unwrap_or(100);
    if !(1..=500).contains(&limit) {
        return error(StatusCode::BAD_REQUEST, "请求参数无效");
    }
    let database = database_path(&state);
    let user_id = user.user_id;
    match run_store(database, move |store| {
        store
            .security_audit_events(&user_id, limit)
            .map_err(|error| error.to_string())
    })
    .await
    {
        Ok(events) => Json(AuditBody { events }).into_response(),
        Err(_) => internal_error(),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrepareExportInput {
    current_password: String,
    export_password: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PreparedExportBody {
    download_path: String,
    filename: String,
    account_count: usize,
    expires_at: String,
}

pub(crate) async fn embedded_prepare_export(
    state: Arc<AppState>,
    user_id: String,
    actor: String,
    input: serde_json::Value,
) -> Result<serde_json::Value, crate::EmbeddedOperationError> {
    let input = serde_json::from_value::<PrepareExportInput>(input)
        .map_err(|_| embedded_error(400, "请求参数无效"))?;
    if !(1..=256).contains(&input.current_password.encode_utf16().count())
        || !(12..=256).contains(&input.export_password.encode_utf16().count())
    {
        return Err(embedded_error(400, "请求参数无效"));
    }
    let lock = state.security.user_lock(&user_id);
    let _guard = lock.lock().await;
    embedded_reauthenticate(
        &state,
        &user_id,
        &actor,
        input.current_password,
        "mail-authorization-export",
    )
    .await?;
    let database = database_path(&state);
    let key_path = state.config.data_dir.join("master.key");
    let export_owner = user_id.clone();
    let export_password = input.export_password;
    let artifact = run_store(database, move |store| {
        let key = MasterKey::from_file(key_path).map_err(|error| error.to_string())?;
        let codec = MasterKeyCredentialCodec::new(&key);
        AuthorizationExportService::new(store)
            .prepare(
                &export_owner,
                &Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
                &export_password,
                &codec,
                &PortableAuthorizationExportEncryptor,
            )
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|_| embedded_error(500, "服务暂时不可用"))?;
    let body = serde_json::to_vec(&artifact.envelope)
        .map_err(|_| embedded_error(500, "服务暂时不可用"))?;
    let prepared_at_ms = now_ms();
    let expires_at_ms = prepared_at_ms + EXPORT_TTL_MS;
    let id = Uuid::new_v4().simple().to_string();
    let account_count = artifact.account_count;
    let filename = artifact.filename;
    state.security.insert_export(PendingExport {
        id: id.clone(),
        user_id: user_id.clone(),
        body,
        filename: filename.clone(),
        account_count,
        prepared_at_ms,
        expires_at_ms,
    });
    record_event_for(
        &state,
        &user_id,
        &actor,
        "privacy.mail-authorization-export.prepared",
        account_count,
    )
    .await
    .map_err(|_| {
        state.security.invalidate_user(&user_id);
        embedded_error(500, "服务暂时不可用")
    })?;
    let expires_at = chrono::DateTime::from_timestamp_millis(expires_at_ms)
        .expect("export expiry timestamp must be valid")
        .to_rfc3339_opts(SecondsFormat::Millis, true);
    serde_json::to_value(PreparedExportBody {
        download_path: format!("/api/security/mail-authorization-exports/{id}"),
        filename,
        account_count,
        expires_at,
    })
    .map_err(|_| embedded_error(500, "服务暂时不可用"))
}

pub(crate) async fn embedded_download_export(
    state: Arc<AppState>,
    user_id: String,
    actor: String,
    id: String,
) -> Result<Vec<u8>, crate::EmbeddedOperationError> {
    let Some(mut pending) = state.security.consume_export(&id, &user_id) else {
        return Err(embedded_error(404, "导出文件不存在、已过期或已经下载"));
    };
    record_event_for(
        &state,
        &user_id,
        &actor,
        "privacy.mail-authorization-export.downloaded",
        pending.account_count,
    )
    .await
    .map_err(|_| embedded_error(500, "服务暂时不可用"))?;
    Ok(std::mem::take(&mut pending.body))
}

pub(crate) async fn embedded_clear_user_data(
    state: Arc<AppState>,
    user_id: String,
    actor: String,
    input: serde_json::Value,
) -> Result<(), crate::EmbeddedOperationError> {
    let input = serde_json::from_value::<ClearInput>(input)
        .map_err(|_| embedded_error(400, "请求参数无效"))?;
    if !(1..=256).contains(&input.current_password.encode_utf16().count())
        || input.confirmation != CLEAR_CONFIRMATION
    {
        return Err(embedded_error(400, "请求参数无效"));
    }
    let lock = state.security.user_lock(&user_id);
    let _guard = lock.lock().await;
    embedded_reauthenticate(
        &state,
        &user_id,
        &actor,
        input.current_password,
        "clear-user-data",
    )
    .await?;
    let clear_owner = user_id.clone();
    let cleared = run_store(database_path(&state), move |store| {
        PrivacyService::new(store)
            .clear_mail_data(&clear_owner)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|_| embedded_error(500, "服务暂时不可用"))?;
    let account_count = cleared.account_count as usize;
    let cache_dir = state.config.data_dir.clone();
    let cache_owner = user_id.clone();
    tokio::task::spawn_blocking(move || {
        crate::attachment_cache::clear_user(&cache_dir, &cache_owner)
    })
    .await
    .map_err(|_| embedded_error(500, "服务暂时不可用"))?
    .map_err(|_| embedded_error(500, "服务暂时不可用"))?;
    state.security.invalidate_user(&user_id);
    record_event_for(
        &state,
        &user_id,
        &actor,
        "privacy.user-data-cleared",
        account_count,
    )
    .await
    .map_err(|_| embedded_error(500, "服务暂时不可用"))
}

async fn prepare_export(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    input: Result<Json<PrepareExportInput>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(input) = match input {
        Ok(value) => value,
        Err(_) => return error(StatusCode::BAD_REQUEST, "请求参数无效"),
    };
    if !(1..=256).contains(&input.current_password.encode_utf16().count())
        || !(12..=256).contains(&input.export_password.encode_utf16().count())
    {
        return error(StatusCode::BAD_REQUEST, "请求参数无效");
    }
    let lock = state.security.user_lock(&user.user_id);
    let _guard = lock.lock().await;
    if let Err(response) = reauthenticate(
        &state,
        &user,
        input.current_password,
        "mail-authorization-export",
    )
    .await
    {
        return response;
    }
    let database = database_path(&state);
    let key_path = state.config.data_dir.join("master.key");
    let user_id = user.user_id.clone();
    let export_password = input.export_password;
    let prepared = run_store(database, move |store| {
        let key = MasterKey::from_file(key_path).map_err(|error| error.to_string())?;
        let codec = MasterKeyCredentialCodec::new(&key);
        AuthorizationExportService::new(store)
            .prepare(
                &user_id,
                &Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
                &export_password,
                &codec,
                &PortableAuthorizationExportEncryptor,
            )
            .map_err(|error| error.to_string())
    })
    .await;
    let artifact = match prepared {
        Ok(value) => value,
        Err(_) => return internal_error(),
    };
    let body = match serde_json::to_vec(&artifact.envelope) {
        Ok(value) => value,
        Err(_) => return internal_error(),
    };
    let prepared_at_ms = now_ms();
    let expires_at_ms = prepared_at_ms + EXPORT_TTL_MS;
    let id = Uuid::new_v4().simple().to_string();
    let account_count = artifact.account_count;
    let filename = artifact.filename;
    state.security.insert_export(PendingExport {
        id: id.clone(),
        user_id: user.user_id.clone(),
        body,
        filename: filename.clone(),
        account_count,
        prepared_at_ms,
        expires_at_ms,
    });
    if record_event(
        &state,
        &user,
        "privacy.mail-authorization-export.prepared",
        account_count,
    )
    .await
    .is_err()
    {
        state.security.invalidate_user(&user.user_id);
        return internal_error();
    }
    let expires_at = chrono::DateTime::from_timestamp_millis(expires_at_ms)
        .expect("export expiry timestamp must be valid")
        .to_rfc3339_opts(SecondsFormat::Millis, true);
    let mut response = Json(PreparedExportBody {
        download_path: format!("/api/security/mail-authorization-exports/{id}"),
        filename,
        account_count,
        expires_at,
    })
    .into_response();
    response.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static("private, no-store, max-age=0"),
    );
    response
}

async fn download_export(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    let Some(mut pending) = state.security.consume_export(&id, &user.user_id) else {
        return error(StatusCode::NOT_FOUND, "导出文件不存在、已过期或已经下载");
    };
    if record_event(
        &state,
        &user,
        "privacy.mail-authorization-export.downloaded",
        pending.account_count,
    )
    .await
    .is_err()
    {
        return internal_error();
    }
    let body = std::mem::take(&mut pending.body);
    let body_length = body.len();
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static(
            "application/vnd.imail.mail-authorization-export+json; charset=utf-8",
        ),
    );
    if let Ok(value) =
        HeaderValue::from_str(&format!("attachment; filename=\"{}\"", pending.filename))
    {
        headers.insert(CONTENT_DISPOSITION, value);
    }
    headers.insert(
        CACHE_CONTROL,
        HeaderValue::from_static("private, no-store, max-age=0"),
    );
    headers.insert(PRAGMA, HeaderValue::from_static("no-cache"));
    headers.insert(EXPIRES, HeaderValue::from_static("0"));
    response.headers_mut().insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&body_length.to_string()).expect("content length is valid"),
    );
    response
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClearInput {
    current_password: String,
    confirmation: String,
}

async fn clear_user_data(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    input: Result<Json<ClearInput>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(input) = match input {
        Ok(value) => value,
        Err(_) => return error(StatusCode::BAD_REQUEST, "请求参数无效"),
    };
    if !(1..=256).contains(&input.current_password.encode_utf16().count())
        || input.confirmation != CLEAR_CONFIRMATION
    {
        return error(StatusCode::BAD_REQUEST, "请求参数无效");
    }
    let lock = state.security.user_lock(&user.user_id);
    let _guard = lock.lock().await;
    if let Err(response) =
        reauthenticate(&state, &user, input.current_password, "clear-user-data").await
    {
        return response;
    }
    let database = database_path(&state);
    let user_id = user.user_id.clone();
    let cleared = run_store(database, move |store| {
        PrivacyService::new(store)
            .clear_mail_data(&user_id)
            .map_err(|error| error.to_string())
    })
    .await;
    let account_count = match cleared {
        Ok(value) => value.account_count as usize,
        Err(_) => return internal_error(),
    };
    let cache_dir = state.config.data_dir.clone();
    let cache_owner = user.user_id.clone();
    let cache_cleared = tokio::task::spawn_blocking(move || {
        crate::attachment_cache::clear_user(&cache_dir, &cache_owner)
    })
    .await;
    if !matches!(cache_cleared, Ok(Ok(()))) {
        return internal_error();
    }
    state.security.invalidate_user(&user.user_id);
    if record_event(&state, &user, "privacy.user-data-cleared", account_count)
        .await
        .is_err()
    {
        return internal_error();
    }
    StatusCode::NO_CONTENT.into_response()
}

enum Reauthentication {
    Allowed,
    Limited(u64),
    WrongPassword,
}

async fn reauthenticate(
    state: &AppState,
    user: &AuthenticatedUser,
    password: String,
    action: &'static str,
) -> Result<(), Response> {
    match reauthenticate_result(state, &user.user_id, &user.actor, password, action).await {
        Ok(Reauthentication::Allowed) => Ok(()),
        Ok(Reauthentication::WrongPassword) => {
            Err(error(StatusCode::FORBIDDEN, "当前 iMail 密码不正确"))
        }
        Ok(Reauthentication::Limited(seconds)) => {
            let mut response = error(StatusCode::TOO_MANY_REQUESTS, "尝试过多，请稍后再试");
            response.headers_mut().insert(
                RETRY_AFTER,
                HeaderValue::from_str(&seconds.max(1).to_string()).expect("retry-after is valid"),
            );
            Err(response)
        }
        Err(_) => Err(internal_error()),
    }
}

async fn reauthenticate_result(
    state: &AppState,
    user_id: &str,
    actor: &str,
    password: String,
    action: &'static str,
) -> Result<Reauthentication, String> {
    let database = database_path(state);
    let actor = actor.to_string();
    let user_id = user_id.to_string();
    run_store(database, move |store| {
        let source_key = format!("sensitive-action:{action}:source:{actor}");
        let user_key = format!("sensitive-action:{action}:user:{user_id}");
        let source = store
            .consume_attempt(&source_key, SENSITIVE_SOURCE_MAXIMUM, SENSITIVE_WINDOW_MS)
            .map_err(|error| error.to_string())?;
        let owner = store
            .consume_attempt(&user_key, SENSITIVE_USER_MAXIMUM, SENSITIVE_WINDOW_MS)
            .map_err(|error| error.to_string())?;
        let detail = BTreeMap::from([("action".into(), action.into())]);
        if !source.allowed || !owner.allowed {
            store
                .record_security_event(
                    "sensitive-action.reauthentication-rate-limited",
                    &actor,
                    Some(&user_id),
                    &detail,
                )
                .map_err(|error| error.to_string())?;
            return Ok(Reauthentication::Limited(
                source.retry_after.max(owner.retry_after),
            ));
        }
        if !store
            .verify_user_password(&user_id, &password)
            .map_err(|error| error.to_string())?
        {
            store
                .record_security_event(
                    "sensitive-action.reauthentication-failed",
                    &actor,
                    Some(&user_id),
                    &detail,
                )
                .map_err(|error| error.to_string())?;
            return Ok(Reauthentication::WrongPassword);
        }
        store
            .clear_attempt(&source_key)
            .map_err(|error| error.to_string())?;
        store
            .clear_attempt(&user_key)
            .map_err(|error| error.to_string())?;
        Ok(Reauthentication::Allowed)
    })
    .await
}

async fn embedded_reauthenticate(
    state: &AppState,
    user_id: &str,
    actor: &str,
    password: String,
    action: &'static str,
) -> Result<(), crate::EmbeddedOperationError> {
    match reauthenticate_result(state, user_id, actor, password, action).await {
        Ok(Reauthentication::Allowed) => Ok(()),
        Ok(Reauthentication::WrongPassword) => Err(embedded_error(403, "当前 iMail 密码不正确")),
        Ok(Reauthentication::Limited(_)) => Err(embedded_error(429, "尝试过多，请稍后再试")),
        Err(_) => Err(embedded_error(500, "服务暂时不可用")),
    }
}

async fn record_event(
    state: &AppState,
    user: &AuthenticatedUser,
    event_type: &'static str,
    account_count: usize,
) -> Result<(), String> {
    record_event_for(state, &user.user_id, &user.actor, event_type, account_count).await
}

async fn record_event_for(
    state: &AppState,
    user_id: &str,
    actor: &str,
    event_type: &'static str,
    account_count: usize,
) -> Result<(), String> {
    let database = database_path(state);
    let actor = actor.to_string();
    let user_id = user_id.to_string();
    run_store(database, move |store| {
        store
            .record_security_event(
                event_type,
                &actor,
                Some(&user_id),
                &BTreeMap::from([("accountCount".into(), account_count.to_string())]),
            )
            .map_err(|error| error.to_string())
    })
    .await
}

fn database_path(state: &AppState) -> PathBuf {
    state.config.data_dir.join("imail.sqlite")
}

async fn run_store<T: Send + 'static>(
    database: PathBuf,
    operation: impl FnOnce(&mut SqliteAuthStore) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(move || {
        let mut store =
            SqliteAuthStore::open_database(database).map_err(|error| error.to_string())?;
        operation(&mut store)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn embedded_error(status: u16, message: impl Into<String>) -> crate::EmbeddedOperationError {
    crate::EmbeddedOperationError {
        status,
        message: message.into(),
    }
}

#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
}
fn error(status: StatusCode, message: &'static str) -> Response {
    (status, Json(ErrorBody { error: message })).into_response()
}
fn internal_error() -> Response {
    error(StatusCode::INTERNAL_SERVER_ERROR, "服务暂时不可用")
}
