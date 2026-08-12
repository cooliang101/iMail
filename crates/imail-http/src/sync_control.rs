use std::{collections::BTreeSet, convert::Infallible, path::PathBuf, sync::Arc, time::Duration};

use axum::{
    body::{Body, Bytes},
    extract::{rejection::JsonRejection, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Extension, Json, Router,
};
use chrono::{SecondsFormat, Utc};
use futures_util::stream;
use imail_core::{accounts::AccountService, ApplicationError};
use imail_protocol::{
    AccountReadModel, MailboxSyncStateReadModel, SyncJobReadModel, SyncPolicyReadModel,
    SyncWorkerHealthReadModel,
};
use imail_storage_sqlite::{
    AuthStoreError, SqliteAuthStore, SyncPolicySettings, SyncRuntimeError, SyncRuntimeStore,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{auth::AuthenticatedUser, AppState};

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/api/sync-policy",
            get(default_policy).patch(update_default_policy),
        )
        .route(
            "/api/accounts/:id/sync-policy",
            get(account_policy).patch(update_account_policy),
        )
        .route("/api/sync-status", get(sync_status))
        .route("/api/sync-jobs/:id", get(sync_job))
        .route("/api/events", get(events))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PolicySettingsView {
    enabled: bool,
    folder_mode: String,
    selected_mailboxes: Vec<String>,
    notify_on_error: bool,
}

impl From<SyncPolicySettings> for PolicySettingsView {
    fn from(settings: SyncPolicySettings) -> Self {
        Self {
            enabled: settings.enabled,
            folder_mode: settings.folder_mode,
            selected_mailboxes: settings.selected_mailboxes,
            notify_on_error: settings.notify_on_error,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PolicyPatch {
    enabled: Option<bool>,
    folder_mode: Option<String>,
    selected_mailboxes: Option<Vec<String>>,
    notify_on_error: Option<bool>,
}

impl PolicyPatch {
    fn validate(mut self) -> Option<Self> {
        if self.enabled.is_none()
            && self.folder_mode.is_none()
            && self.selected_mailboxes.is_none()
            && self.notify_on_error.is_none()
        {
            return None;
        }
        if self
            .folder_mode
            .as_deref()
            .is_some_and(|mode| !matches!(mode, "inbox" | "standard" | "selected"))
        {
            return None;
        }
        if let Some(mailboxes) = &mut self.selected_mailboxes {
            if mailboxes.len() > 100 {
                return None;
            }
            for mailbox in mailboxes {
                *mailbox = mailbox.trim().to_string();
                if mailbox.is_empty() || mailbox.encode_utf16().count() > 500 {
                    return None;
                }
            }
        }
        Some(self)
    }

    fn apply(self, mut current: SyncPolicySettings) -> SyncPolicySettings {
        if let Some(value) = self.enabled {
            current.enabled = value;
        }
        if let Some(value) = self.folder_mode {
            current.folder_mode = value;
        }
        if let Some(value) = self.selected_mailboxes {
            current.selected_mailboxes = value;
        }
        if let Some(value) = self.notify_on_error {
            current.notify_on_error = value;
        }
        current
    }
}

#[derive(Serialize)]
struct SettingsBody {
    policy: PolicySettingsView,
}

#[derive(Serialize)]
struct AccountPolicyBody {
    policy: SyncPolicyReadModel,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountSyncStatus {
    account_id: String,
    policy: SyncPolicyReadModel,
    states: Vec<MailboxSyncStateReadModel>,
    jobs: Vec<SyncJobReadModel>,
}

#[derive(Serialize)]
struct SyncStatusBody {
    accounts: Vec<AccountSyncStatus>,
    worker: SyncWorkerHealthReadModel,
}

#[derive(Serialize)]
struct JobBody {
    job: SyncJobReadModel,
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

#[derive(Default, Deserialize)]
struct EventQuery {
    after: Option<String>,
}

struct SseState {
    database: PathBuf,
    user_id: String,
    account_ids: Vec<String>,
    shutdown: tokio::sync::broadcast::Receiver<()>,
    cursor: i64,
    initial: Option<String>,
    seconds_since_status: u64,
}

async fn default_policy(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    let database = state.config.data_dir.join("imail.sqlite");
    match run(database, move |_, sync| {
        sync.default_policy(&user.user_id)
            .map_err(SyncControlError::Runtime)
    })
    .await
    {
        Ok(policy) => Json(SettingsBody {
            policy: policy.into(),
        })
        .into_response(),
        Err(cause) => control_error(cause),
    }
}

async fn update_default_policy(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    patch: Result<Json<PolicyPatch>, JsonRejection>,
) -> Response {
    let Some(patch) = parse_patch(patch) else {
        return error(StatusCode::BAD_REQUEST, "至少提供一个同步设置");
    };
    let database = state.config.data_dir.join("imail.sqlite");
    match run(database, move |_, sync| {
        let current = sync
            .default_policy(&user.user_id)
            .map_err(SyncControlError::Runtime)?;
        let updated = patch.apply(current);
        sync.update_default_policy(&user.user_id, &updated)
            .map_err(SyncControlError::Runtime)?;
        Ok(updated)
    })
    .await
    {
        Ok(policy) => Json(SettingsBody {
            policy: policy.into(),
        })
        .into_response(),
        Err(cause) => control_error(cause),
    }
}

async fn account_policy(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(account_id): Path<String>,
) -> Response {
    let database = state.config.data_dir.join("imail.sqlite");
    match run(database, move |auth, sync| {
        AccountService::new(auth).get(&user.user_id, &account_id)?;
        sync.ensure_policy(&account_id, &user.user_id, Utc::now())
            .map_err(SyncControlError::Runtime)
    })
    .await
    {
        Ok(policy) => Json(AccountPolicyBody { policy }).into_response(),
        Err(cause) => control_error(cause),
    }
}

async fn update_account_policy(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(account_id): Path<String>,
    patch: Result<Json<PolicyPatch>, JsonRejection>,
) -> Response {
    let Some(patch) = parse_patch(patch) else {
        return error(StatusCode::BAD_REQUEST, "至少提供一个同步设置");
    };
    let database = state.config.data_dir.join("imail.sqlite");
    match run(database, move |auth, sync| {
        let account = AccountService::new(auth).get(&user.user_id, &account_id)?;
        validate_selected_mailboxes(&account, patch.selected_mailboxes.as_deref())?;
        let current = sync
            .ensure_policy(&account_id, &user.user_id, Utc::now())
            .map_err(SyncControlError::Runtime)?;
        let updated = patch.apply(settings_from_policy(&current));
        sync.update_policy(&account_id, &updated, Utc::now())
            .map_err(SyncControlError::Runtime)
    })
    .await
    {
        Ok(policy) => Json(AccountPolicyBody { policy }).into_response(),
        Err(cause) => control_error(cause),
    }
}

async fn sync_status(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    let database = state.config.data_dir.join("imail.sqlite");
    match run(database, move |auth, sync| {
        let accounts = AccountService::new(auth).list(&user.user_id)?;
        let account_ids: Vec<String> = accounts.into_iter().map(|account| account.id).collect();
        status_for_accounts(sync, &user.user_id, &account_ids).map_err(SyncControlError::Runtime)
    })
    .await
    {
        Ok(status) => Json(status).into_response(),
        Err(cause) => control_error(cause),
    }
}

pub(crate) async fn embedded_sync_status(
    state: Arc<AppState>,
    user_id: String,
) -> Result<Value, crate::EmbeddedOperationError> {
    let status = run(
        state.config.data_dir.join("imail.sqlite"),
        move |auth, sync| {
            let account_ids = AccountService::new(auth)
                .list(&user_id)?
                .into_iter()
                .map(|account| account.id)
                .collect::<Vec<_>>();
            status_for_accounts(sync, &user_id, &account_ids).map_err(SyncControlError::Runtime)
        },
    )
    .await
    .map_err(|cause| match cause {
        SyncControlError::Application(ApplicationError::Domain {
            status, message, ..
        }) if status < 500 => crate::EmbeddedOperationError {
            status,
            message: message.into(),
        },
        _ => crate::EmbeddedOperationError {
            status: 500,
            message: "服务暂时无法完成请求".into(),
        },
    })?;
    serde_json::to_value(status).map_err(|_| crate::EmbeddedOperationError {
        status: 500,
        message: "服务暂时无法完成请求".into(),
    })
}

async fn sync_job(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(job_id): Path<String>,
) -> Response {
    let database = state.config.data_dir.join("imail.sqlite");
    match run(database, move |auth, sync| {
        let Some(job) = sync.job(&job_id).map_err(SyncControlError::Runtime)? else {
            return Ok(None);
        };
        match AccountService::new(auth).get(&user.user_id, &job.account_id) {
            Ok(_) => Ok(Some(job)),
            Err(ApplicationError::Domain { status: 404, .. }) => Ok(None),
            Err(cause) => Err(SyncControlError::Application(cause)),
        }
    })
    .await
    {
        Ok(Some(job)) => Json(JobBody { job }).into_response(),
        Ok(None) => error(StatusCode::NOT_FOUND, "同步任务不存在"),
        Err(cause) => control_error(cause),
    }
}

async fn events(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Query(query): Query<EventQuery>,
    headers: HeaderMap,
) -> Response {
    let requested_cursor = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .or(query.after);
    let requested_cursor = requested_cursor.map(|value| value.parse::<i64>().unwrap_or(0).max(0));
    let database = state.config.data_dir.join("imail.sqlite");
    let shutdown = state.shutdown.subscribe();
    let user_id = user.user_id;
    let bootstrap = run(database.clone(), move |auth, sync| {
        let account_ids: Vec<String> = AccountService::new(auth)
            .list(&user_id)?
            .into_iter()
            .map(|account| account.id)
            .collect();
        let cursor = match requested_cursor {
            Some(cursor) => cursor,
            None => sync.latest_event_id().map_err(SyncControlError::Runtime)?,
        };
        let (cursor, event_frames, changed) =
            event_frames(sync, cursor, &account_ids).map_err(SyncControlError::Runtime)?;
        let status =
            status_for_accounts(sync, &user_id, &account_ids).map_err(SyncControlError::Runtime)?;
        let mut initial = format!(
            "event: connected\ndata: {}\n\n",
            serde_json::json!({"connectedAt": Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)})
        );
        initial.push_str(&event_frames);
        if changed {
            initial.push_str(&status_frame(&status));
        }
        initial.push_str(&status_frame(&status));
        Ok(SseState {
            database,
            user_id,
            account_ids,
            shutdown,
            cursor,
            initial: Some(initial),
            seconds_since_status: 0,
        })
    })
    .await;
    let state = match bootstrap {
        Ok(state) => state,
        Err(cause) => return control_error(cause),
    };
    let stream = stream::unfold(state, |mut state| async move {
        if let Some(initial) = state.initial.take() {
            return Some((Ok::<Bytes, Infallible>(Bytes::from(initial)), state));
        }
        loop {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                _ = state.shutdown.recv() => return None,
            }
            state.seconds_since_status += 1;
            let heartbeat = state.seconds_since_status >= 15;
            match sse_tick(
                state.database.clone(),
                state.user_id.clone(),
                state.account_ids.clone(),
                state.cursor,
                heartbeat,
            )
            .await
            {
                Ok((cursor, chunk, sent_status)) => {
                    state.cursor = cursor;
                    if sent_status {
                        state.seconds_since_status = 0;
                    }
                    if !chunk.is_empty() {
                        return Some((Ok(Bytes::from(chunk)), state));
                    }
                }
                Err(cause) => {
                    eprintln!("[imail-http] sync event stream error: {cause}");
                    return Some((Ok(Bytes::from_static(b": unavailable\n\n")), state));
                }
            }
        }
    });
    let mut response = Body::from_stream(stream).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream; charset=utf-8"),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, no-transform"),
    );
    response
        .headers_mut()
        .insert(header::CONNECTION, HeaderValue::from_static("keep-alive"));
    response
}

async fn sse_tick(
    database: PathBuf,
    user_id: String,
    account_ids: Vec<String>,
    cursor: i64,
    heartbeat: bool,
) -> Result<(i64, String, bool), SyncRuntimeError> {
    tokio::task::spawn_blocking(move || {
        let sync = SyncRuntimeStore::open_database(database)?;
        let (cursor, mut chunk, changed) = event_frames(&sync, cursor, &account_ids)?;
        if changed || heartbeat {
            let status = status_for_accounts(&sync, &user_id, &account_ids)?;
            chunk.push_str(&status_frame(&status));
        }
        Ok((cursor, chunk, changed || heartbeat))
    })
    .await
    .map_err(|_| SyncRuntimeError::InvalidInput("event stream worker"))?
}

fn event_frames(
    sync: &SyncRuntimeStore,
    cursor: i64,
    account_ids: &[String],
) -> Result<(i64, String, bool), SyncRuntimeError> {
    let allowed: BTreeSet<&str> = account_ids.iter().map(String::as_str).collect();
    let mut cursor = cursor;
    let mut frames = String::new();
    let mut changed = false;
    for event in sync.events(cursor, 100)? {
        cursor = event.id;
        if !allowed.contains(event.account_id.as_str()) {
            continue;
        }
        changed = true;
        frames.push_str(&format!(
            "id: {}\nevent: {}\ndata: {}\n\n",
            event.id,
            event.event_type,
            serde_json::to_string(&event).expect("sync event serialization must succeed")
        ));
    }
    Ok((cursor, frames, changed))
}

fn status_for_accounts(
    sync: &SyncRuntimeStore,
    user_id: &str,
    account_ids: &[String],
) -> Result<SyncStatusBody, SyncRuntimeError> {
    let mut output = Vec::with_capacity(account_ids.len());
    for account_id in account_ids {
        if sync.account_owner_id(account_id)?.as_deref() != Some(user_id) {
            continue;
        }
        output.push(AccountSyncStatus {
            policy: sync.ensure_policy(account_id, user_id, Utc::now())?,
            states: sync.account_mailbox_states(account_id)?,
            jobs: sync.account_jobs(account_id, 10)?,
            account_id: account_id.clone(),
        });
    }
    Ok(SyncStatusBody {
        accounts: output,
        worker: sync.worker_health(Utc::now())?,
    })
}

fn status_frame(status: &SyncStatusBody) -> String {
    format!(
        "event: sync.status\ndata: {}\n\n",
        serde_json::to_string(status).expect("sync status serialization must succeed")
    )
}

fn parse_patch(patch: Result<Json<PolicyPatch>, JsonRejection>) -> Option<PolicyPatch> {
    patch.ok()?.0.validate()
}

fn settings_from_policy(policy: &SyncPolicyReadModel) -> SyncPolicySettings {
    SyncPolicySettings {
        enabled: policy.enabled,
        folder_mode: policy.folder_mode.clone(),
        selected_mailboxes: policy
            .selected_mailboxes
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        notify_on_error: policy.notify_on_error,
    }
}

fn validate_selected_mailboxes(
    account: &AccountReadModel,
    selected: Option<&[String]>,
) -> Result<(), SyncControlError> {
    let Some(selected) = selected else {
        return Ok(());
    };
    let selectable: BTreeSet<&str> = account
        .mailboxes
        .as_array()
        .into_iter()
        .flatten()
        .filter(|mailbox| {
            mailbox
                .get("selectable")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .filter_map(|mailbox| mailbox.get("path").and_then(Value::as_str))
        .collect();
    if selected
        .iter()
        .any(|mailbox| !selectable.contains(mailbox.as_str()))
    {
        return Err(SyncControlError::Domain(
            StatusCode::BAD_REQUEST,
            "同步文件夹必须来自该账户已发现的可选择文件夹",
        ));
    }
    Ok(())
}

enum SyncControlError {
    Application(ApplicationError<AuthStoreError>),
    Runtime(SyncRuntimeError),
    Domain(StatusCode, &'static str),
}

impl From<ApplicationError<AuthStoreError>> for SyncControlError {
    fn from(error: ApplicationError<AuthStoreError>) -> Self {
        Self::Application(error)
    }
}

async fn run<T>(
    database: PathBuf,
    operation: impl FnOnce(&mut SqliteAuthStore, &mut SyncRuntimeStore) -> Result<T, SyncControlError>
        + Send
        + 'static,
) -> Result<T, SyncControlError>
where
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(move || {
        let mut auth = SqliteAuthStore::open_database(&database)
            .map_err(ApplicationError::Repository)
            .map_err(SyncControlError::Application)?;
        let mut sync =
            SyncRuntimeStore::open_database(&database).map_err(SyncControlError::Runtime)?;
        operation(&mut auth, &mut sync)
    })
    .await
    {
        Ok(result) => result,
        Err(_) => Err(SyncControlError::Domain(
            StatusCode::INTERNAL_SERVER_ERROR,
            "服务暂时无法完成请求",
        )),
    }
}

fn control_error(cause: SyncControlError) -> Response {
    match cause {
        SyncControlError::Domain(status, message) => error(status, message),
        SyncControlError::Application(ApplicationError::Domain {
            status, message, ..
        }) if status < 500 => error(
            StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_REQUEST),
            message,
        ),
        SyncControlError::Application(ApplicationError::Domain { .. }) => {
            error(StatusCode::INTERNAL_SERVER_ERROR, "服务暂时无法完成请求")
        }
        SyncControlError::Application(ApplicationError::Repository(cause)) => {
            eprintln!("[imail-http] sync control storage error: {cause}");
            error(StatusCode::INTERNAL_SERVER_ERROR, "服务暂时无法完成请求")
        }
        SyncControlError::Runtime(cause) => {
            eprintln!("[imail-http] sync control runtime error: {cause}");
            error(StatusCode::INTERNAL_SERVER_ERROR, "服务暂时无法完成请求")
        }
    }
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
