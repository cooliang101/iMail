use std::{
    collections::{BTreeMap, HashMap},
    path::{Path as FsPath, PathBuf},
    sync::{mpsc, Arc, Mutex as StdMutex, OnceLock},
    thread,
    time::{Duration as StdDuration, SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::{rejection::JsonRejection, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Extension, Json, Router,
};
use chrono::{DateTime, Utc};
use imail_apple_hme::{
    AppleAuthClient, AppleHmeClient, AppleHmeError, AppleHmeErrorCode, AppleSession, CreateChannel,
    CreateHmeRequest, HmeAddress, LoginRequest, LoginStateKind, TwoFactorMethod, UreqTransport,
};
use imail_core::{AccountRepository, AuthRepository};
use imail_security::MasterKey;
use imail_storage_sqlite::{
    AppleHmeAddressRecord, AuthStoreError, SqliteAuthStore, SqliteReadOnlyStore, StorageError,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{auth::AuthenticatedUser, AppState, AppleHmePendingOwner};

const KEEPALIVE_MIN_SECONDS: u64 = 3 * 60;
const KEEPALIVE_MAX_SECONDS: u64 = 5 * 60;

type SessionLockMap = HashMap<String, Arc<StdMutex<()>>>;

fn session_locks() -> &'static StdMutex<SessionLockMap> {
    static LOCKS: OnceLock<StdMutex<SessionLockMap>> = OnceLock::new();
    LOCKS.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn session_lock(owner_id: &str, account_id: &str) -> Arc<StdMutex<()>> {
    let key = format!("{owner_id}\0{account_id}");
    let mut locks = session_locks()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    locks
        .entry(key)
        .or_insert_with(|| Arc::new(StdMutex::new(())))
        .clone()
}

pub(crate) struct AppleHmeKeepaliveRuntime {
    shutdown: mpsc::Sender<()>,
    stopped: mpsc::Receiver<()>,
    worker: Option<thread::JoinHandle<()>>,
}

impl AppleHmeKeepaliveRuntime {
    pub(crate) fn start(data_dir: PathBuf) -> std::io::Result<Self> {
        let (shutdown, shutdown_rx) = mpsc::channel();
        let (stopped_tx, stopped) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("imail-apple-hme-keepalive".into())
            .spawn(move || {
                if keep_alive_persisted_sessions(&data_dir).is_err() {
                    eprintln!("[imail-http] Apple HME 会话启动检查访问本地存储失败");
                }
                loop {
                    match shutdown_rx.recv_timeout(random_keepalive_interval()) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            if keep_alive_persisted_sessions(&data_dir).is_err() {
                                eprintln!("[imail-http] Apple HME 会话保活任务访问本地存储失败");
                            }
                        }
                    }
                }
                let _ = stopped_tx.send(());
            })?;
        Ok(Self {
            shutdown,
            stopped,
            worker: Some(worker),
        })
    }

    pub(crate) fn shutdown(&mut self, maximum_wait: StdDuration) -> bool {
        let _ = self.shutdown.send(());
        if self.stopped.recv_timeout(maximum_wait).is_err() {
            return false;
        }
        match self.worker.take() {
            Some(worker) => worker.join().is_ok(),
            None => true,
        }
    }
}

impl Drop for AppleHmeKeepaliveRuntime {
    fn drop(&mut self) {
        let _ = self.shutdown(StdDuration::from_secs(2));
    }
}

fn random_keepalive_interval() -> StdDuration {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let mixed = nanos ^ nanos.rotate_left(17) ^ ((std::process::id() as u64) << 32);
    let seconds =
        KEEPALIVE_MIN_SECONDS + mixed % (KEEPALIVE_MAX_SECONDS - KEEPALIVE_MIN_SECONDS + 1);
    StdDuration::from_secs(seconds)
}

fn keep_alive_persisted_sessions(data_dir: &FsPath) -> Result<(), IntegrationError> {
    let records = SqliteReadOnlyStore::open_data_dir(data_dir)?.apple_hme_sessions()?;
    for record in records {
        if keep_alive_persisted_session(data_dir, &record.user_id, &record.account_id).is_err() {
            eprintln!(
                "[imail-http] Apple HME 会话保活无法读取或保存 account={}",
                record.account_id
            );
        }
    }
    Ok(())
}

fn keep_alive_persisted_session(
    data_dir: &FsPath,
    owner_id: &str,
    account_id: &str,
) -> Result<(), IntegrationError> {
    let lock = session_lock(owner_id, account_id);
    let _guard = lock.lock().unwrap_or_else(|error| error.into_inner());
    let mut store = open_store(data_dir)?;
    let Some(mut session) = load_session(&store, data_dir, owner_id, account_id)? else {
        return Ok(());
    };
    let client = AppleHmeClient::default();
    for (kind, result) in keep_alive_session_channels(&client, &mut session) {
        if let Err(error) = result {
            eprintln!(
                "[imail-http] {} 会话保活失败 account={} code={:?}: {}",
                login_state_kind_name(kind),
                account_id,
                error.code,
                error.message
            );
        }
    }
    persist_session(&mut store, data_dir, owner_id, account_id, &session)?;
    Ok(())
}

/// Runs every available channel independently. A rejected Apple Account request
/// must not prevent iCloud Web from being checked in the same or a later cycle.
fn keep_alive_session_channels<T: imail_apple_hme::HttpTransport>(
    client: &AppleHmeClient<T>,
    session: &mut AppleSession,
) -> Vec<(LoginStateKind, Result<(), AppleHmeError>)> {
    let mut attempts = Vec::with_capacity(2);
    for kind in [LoginStateKind::AppleAccount, LoginStateKind::ICloudWeb] {
        if session.state(kind).is_none() {
            continue;
        }
        let result = match kind {
            LoginStateKind::AppleAccount => client.keep_alive_apple_account(session),
            LoginStateKind::ICloudWeb => client.keep_alive_icloud_web(session),
        };
        attempts.push((kind, result));
    }
    attempts
}

fn login_state_kind_name(kind: LoginStateKind) -> &'static str {
    match kind {
        LoginStateKind::AppleAccount => "Apple Account",
        LoginStateKind::ICloudWeb => "iCloud Web",
    }
}

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/api/accounts/:id/apple-hme",
            get(status).delete(disconnect),
        )
        .route("/api/accounts/:id/apple-hme/login", post(start_login))
        .route(
            "/api/accounts/:id/apple-hme/two-factor",
            post(submit_two_factor),
        )
        .route(
            "/api/accounts/:id/apple-hme/addresses",
            get(list_addresses).post(create_address),
        )
        .route(
            "/api/accounts/:id/apple-hme/addresses/sync",
            post(sync_addresses),
        )
        .route(
            "/api/accounts/:id/apple-hme/addresses/:anonymous_id/deactivate",
            post(deactivate_address),
        )
        .route(
            "/api/accounts/:id/apple-hme/addresses/:anonymous_id",
            delete(delete_address),
        )
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum LoginKindInput {
    IcloudWeb,
    AppleAccount,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum TwoFactorMethodInput {
    TrustedDevice,
    Phone,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StartLoginInput {
    kind: LoginKindInput,
    password: String,
    apple_id: Option<String>,
    two_factor_method: Option<TwoFactorMethodInput>,
    phone_number: Option<Value>,
    icloud_host: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TwoFactorInput {
    pending_id: String,
    code: String,
    phone_number: Option<Value>,
}

#[derive(Default, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CreateChannelInput {
    #[default]
    Auto,
    AppleAccount,
    IcloudWeb,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateAddressInput {
    #[serde(default)]
    label: String,
    #[serde(default)]
    note: String,
    #[serde(default)]
    channel: CreateChannelInput,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionStatus {
    account_id: String,
    authorized: bool,
    connected: bool,
    icloud_web_authorized: bool,
    icloud_web_connected: bool,
    icloud_web_status_message: Option<String>,
    icloud_web_last_successful_keepalive_at: Option<DateTime<Utc>>,
    apple_account_authorized: bool,
    apple_account_connected: bool,
    apple_account_status_message: Option<String>,
    apple_account_last_successful_keepalive_at: Option<DateTime<Utc>>,
    is_icloud_plus: bool,
    can_create_hme: bool,
    updated_at: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoginView {
    status: SessionStatus,
    needs_two_factor: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pending_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_at: Option<DateTime<Utc>>,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorBody {
    error: String,
    code: &'static str,
    retryable: bool,
}

async fn status(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(account_id): Path<String>,
) -> Response {
    response(status_service(state, user.user_id, account_id).await)
}

async fn start_login(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(account_id): Path<String>,
    input: Result<Json<StartLoginInput>, JsonRejection>,
) -> Response {
    let Json(input) = match input {
        Ok(input) => input,
        Err(_) => return bad_request("请求参数无效"),
    };
    response(
        start_login_service(
            state,
            user.user_id,
            user.actor,
            account_id,
            input.kind,
            input.password,
            input.apple_id,
            input.two_factor_method,
            input.phone_number,
            input.icloud_host,
        )
        .await,
    )
}

async fn submit_two_factor(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(account_id): Path<String>,
    input: Result<Json<TwoFactorInput>, JsonRejection>,
) -> Response {
    let Json(input) = match input {
        Ok(input) => input,
        Err(_) => return bad_request("请求参数无效"),
    };
    response(
        submit_two_factor_service(
            state,
            user.user_id,
            user.actor,
            account_id,
            input.pending_id,
            input.code,
            input.phone_number,
        )
        .await,
    )
}

async fn list_addresses(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(account_id): Path<String>,
) -> Response {
    response(list_service(state, user.user_id, account_id).await)
}

async fn sync_addresses(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(account_id): Path<String>,
) -> Response {
    response(sync_service(state, user.user_id, account_id).await)
}

async fn create_address(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(account_id): Path<String>,
    input: Result<Json<CreateAddressInput>, JsonRejection>,
) -> Response {
    let Json(input) = match input {
        Ok(input) => input,
        Err(_) => return bad_request("请求参数无效"),
    };
    if input.label.encode_utf16().count() > 200 || input.note.encode_utf16().count() > 500 {
        return bad_request("HME 标签或备注过长");
    }
    match create_service(
        state,
        user.user_id,
        account_id,
        input.label,
        input.note,
        input.channel,
    )
    .await
    {
        Ok(value) => (StatusCode::CREATED, Json(value)).into_response(),
        Err(error) => error_response(error),
    }
}

async fn deactivate_address(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((account_id, anonymous_id)): Path<(String, String)>,
) -> Response {
    response(deactivate_service(state, user.user_id, account_id, anonymous_id).await)
}

async fn delete_address(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((account_id, anonymous_id)): Path<(String, String)>,
) -> Response {
    response(delete_service(state, user.user_id, account_id, anonymous_id).await)
}

async fn disconnect(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(account_id): Path<String>,
) -> Response {
    response(disconnect_service(state, user.user_id, account_id).await)
}

pub(crate) async fn status_service(
    state: Arc<AppState>,
    owner_id: String,
    account_id: String,
) -> Result<SessionStatus, IntegrationError> {
    let data_dir = state.config.data_dir.clone();
    tokio::task::spawn_blocking(move || {
        let store = open_store(&data_dir)?;
        require_icloud_account(&store, &owner_id, &account_id)?;
        session_status(&store, &data_dir, &owner_id, &account_id)
    })
    .await
    .map_err(|_| IntegrationError::Internal)?
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn start_login_service(
    state: Arc<AppState>,
    owner_id: String,
    actor: String,
    account_id: String,
    kind: LoginKindInput,
    password: String,
    apple_id: Option<String>,
    two_factor_method: Option<TwoFactorMethodInput>,
    phone_number: Option<Value>,
    icloud_host: Option<String>,
) -> Result<LoginView, IntegrationError> {
    if password.is_empty() || password.encode_utf16().count() > 512 {
        return Err(IntegrationError::invalid("Apple 密码格式无效"));
    }
    let pending = Arc::clone(&state.apple_hme_pending);
    let data_dir = state.config.data_dir.clone();
    let pending_owner = owner_id.clone();
    let pending_account = account_id.clone();
    let result = tokio::task::spawn_blocking(move || {
        let lock = session_lock(&owner_id, &account_id);
        let _guard = lock.lock().unwrap_or_else(|error| error.into_inner());
        let mut store = open_store(&data_dir)?;
        let account = require_icloud_account(&store, &owner_id, &account_id)?;
        let apple_id = apple_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(&account.email)
            .to_string();
        let mut request = match kind {
            LoginKindInput::IcloudWeb => LoginRequest::icloud_web(apple_id, password),
            LoginKindInput::AppleAccount => LoginRequest::apple_account(apple_id, password),
        };
        request.two_factor_method = match two_factor_method {
            Some(TwoFactorMethodInput::Phone) => TwoFactorMethod::Phone,
            _ => TwoFactorMethod::TrustedDevice,
        };
        request.phone_number = phone_number;
        request.icloud_host = icloud_host;
        let result =
            AppleAuthClient::new(UreqTransport::default(), pending).start_login(request)?;
        let status = match result.session {
            Some(session) => {
                persist_merged_session(&mut store, &data_dir, &owner_id, &account_id, session)?
            }
            None => session_status(&store, &data_dir, &owner_id, &account_id)?,
        };
        store.record_security_event(
            "apple-hme.login-started",
            &actor,
            Some(&owner_id),
            &BTreeMap::from([
                ("accountId".into(), account_id),
                ("kind".into(), login_kind_name(kind).into()),
                (
                    "twoFactorRequired".into(),
                    result.needs_two_factor.to_string(),
                ),
            ]),
        )?;
        Ok::<_, IntegrationError>(LoginView {
            status,
            needs_two_factor: result.needs_two_factor,
            pending_id: result.pending_id,
            expires_at: result.expires_at,
            message: result.message,
        })
    })
    .await
    .map_err(|_| IntegrationError::Internal)??;
    if let (Some(pending_id), Some(expires_at)) = (&result.pending_id, result.expires_at) {
        let mut owners = state
            .apple_hme_pending_owners
            .lock()
            .map_err(|_| IntegrationError::Internal)?;
        let now = Utc::now().timestamp_millis();
        owners.retain(|_, value| value.expires_at_ms > now);
        owners.insert(
            pending_id.clone(),
            AppleHmePendingOwner {
                owner_id: pending_owner,
                account_id: pending_account,
                expires_at_ms: expires_at.timestamp_millis(),
            },
        );
    }
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn submit_two_factor_service(
    state: Arc<AppState>,
    owner_id: String,
    actor: String,
    account_id: String,
    pending_id: String,
    code: String,
    phone_number: Option<Value>,
) -> Result<LoginView, IntegrationError> {
    {
        let mut owners = state
            .apple_hme_pending_owners
            .lock()
            .map_err(|_| IntegrationError::Internal)?;
        let now = Utc::now().timestamp_millis();
        owners.retain(|_, value| value.expires_at_ms > now);
        if !owners
            .get(&pending_id)
            .is_some_and(|pending| pending.owner_id == owner_id && pending.account_id == account_id)
        {
            return Err(IntegrationError::apple(
                AppleHmeErrorCode::PendingLoginExpired,
                "Apple 登录验证已过期",
                true,
            ));
        }
    }
    let pending = Arc::clone(&state.apple_hme_pending);
    let data_dir = state.config.data_dir.clone();
    let pending_for_request = pending_id.clone();
    let result =
        tokio::task::spawn_blocking(move || {
            let lock = session_lock(&owner_id, &account_id);
            let _guard = lock.lock().unwrap_or_else(|error| error.into_inner());
            let mut store = open_store(&data_dir)?;
            require_icloud_account(&store, &owner_id, &account_id)?;
            let session = AppleAuthClient::new(UreqTransport::default(), pending)
                .submit_two_factor(&pending_for_request, &code, phone_number)?;
            let status =
                persist_merged_session(&mut store, &data_dir, &owner_id, &account_id, session)?;
            store.record_security_event(
                "apple-hme.login-completed",
                &actor,
                Some(&owner_id),
                &BTreeMap::from([("accountId".into(), account_id)]),
            )?;
            Ok::<_, IntegrationError>(LoginView {
                status,
                needs_two_factor: false,
                pending_id: None,
                expires_at: None,
                message: "Apple 登录成功".into(),
            })
        })
        .await
        .map_err(|_| IntegrationError::Internal)??;
    state
        .apple_hme_pending_owners
        .lock()
        .map_err(|_| IntegrationError::Internal)?
        .remove(&pending_id);
    Ok(result)
}

pub(crate) async fn list_service(
    state: Arc<AppState>,
    owner_id: String,
    account_id: String,
) -> Result<Value, IntegrationError> {
    let data_dir = state.config.data_dir.clone();
    tokio::task::spawn_blocking(move || {
        let store = open_store(&data_dir)?;
        require_icloud_account(&store, &owner_id, &account_id)?;
        let snapshot = store.apple_hme_addresses(&owner_id, &account_id)?;
        Ok(snapshot_value(snapshot.addresses, snapshot.last_synced_at))
    })
    .await
    .map_err(|_| IntegrationError::Internal)?
}

pub(crate) async fn sync_service(
    state: Arc<AppState>,
    owner_id: String,
    account_id: String,
) -> Result<Value, IntegrationError> {
    let data_dir = state.config.data_dir.clone();
    let addresses = mutate_session(
        data_dir.clone(),
        owner_id.clone(),
        account_id.clone(),
        |session| AppleHmeClient::default().list(session),
    )
    .await?;
    tokio::task::spawn_blocking(move || {
        let mut store = open_store(&data_dir)?;
        let records = addresses
            .iter()
            .map(|address| address_record(&owner_id, &account_id, address))
            .collect::<Vec<_>>();
        let snapshot = store.replace_apple_hme_addresses(&owner_id, &account_id, &records)?;
        Ok(snapshot_value(snapshot.addresses, snapshot.last_synced_at))
    })
    .await
    .map_err(|_| IntegrationError::Internal)?
}

pub(crate) async fn create_service(
    state: Arc<AppState>,
    owner_id: String,
    account_id: String,
    label: String,
    note: String,
    channel: CreateChannelInput,
) -> Result<Value, IntegrationError> {
    if label.encode_utf16().count() > 200 || note.encode_utf16().count() > 500 {
        return Err(IntegrationError::invalid("HME 标签或备注过长"));
    }
    let channel = match channel {
        CreateChannelInput::Auto => CreateChannel::Auto,
        CreateChannelInput::AppleAccount => CreateChannel::AppleAccount,
        CreateChannelInput::IcloudWeb => CreateChannel::ICloudWeb,
    };
    let data_dir = state.config.data_dir.clone();
    let persisted_owner = owner_id.clone();
    let persisted_account = account_id.clone();
    let created = mutate_session(data_dir.clone(), owner_id, account_id, move |session| {
        AppleHmeClient::default().create(
            session,
            CreateHmeRequest {
                label,
                note,
                channel,
            },
        )
    })
    .await?;
    let record = address_record(&persisted_owner, &persisted_account, &created.address);
    tokio::task::spawn_blocking(move || {
        open_store(&data_dir)?.upsert_apple_hme_address(&record)?;
        Ok::<_, IntegrationError>(())
    })
    .await
    .map_err(|_| IntegrationError::Internal)??;
    Ok(json!({"address":created.address,"channel":channel_name(created.channel)}))
}

pub(crate) async fn deactivate_service(
    state: Arc<AppState>,
    owner_id: String,
    account_id: String,
    anonymous_id: String,
) -> Result<Value, IntegrationError> {
    let data_dir = state.config.data_dir.clone();
    let persisted_owner = owner_id.clone();
    let persisted_account = account_id.clone();
    let persisted_id = anonymous_id.clone();
    let id = anonymous_id.clone();
    mutate_session(
        state.config.data_dir.clone(),
        owner_id,
        account_id,
        move |session| AppleHmeClient::default().deactivate(session, &id),
    )
    .await?;
    tokio::task::spawn_blocking(move || {
        open_store(&data_dir)?.set_apple_hme_address_active(
            &persisted_owner,
            &persisted_account,
            &persisted_id,
            false,
        )?;
        Ok::<_, IntegrationError>(())
    })
    .await
    .map_err(|_| IntegrationError::Internal)??;
    Ok(json!({"success":true,"anonymousId":anonymous_id}))
}

pub(crate) async fn delete_service(
    state: Arc<AppState>,
    owner_id: String,
    account_id: String,
    anonymous_id: String,
) -> Result<Value, IntegrationError> {
    let data_dir = state.config.data_dir.clone();
    let persisted_owner = owner_id.clone();
    let persisted_account = account_id.clone();
    let persisted_id = anonymous_id.clone();
    let id = anonymous_id.clone();
    mutate_session(
        state.config.data_dir.clone(),
        owner_id,
        account_id,
        move |session| AppleHmeClient::default().delete_inactive(session, &id),
    )
    .await?;
    tokio::task::spawn_blocking(move || {
        open_store(&data_dir)?.delete_apple_hme_address(
            &persisted_owner,
            &persisted_account,
            &persisted_id,
        )?;
        Ok::<_, IntegrationError>(())
    })
    .await
    .map_err(|_| IntegrationError::Internal)??;
    Ok(json!({"success":true,"anonymousId":anonymous_id}))
}

pub(crate) async fn disconnect_service(
    state: Arc<AppState>,
    owner_id: String,
    account_id: String,
) -> Result<Value, IntegrationError> {
    let data_dir = state.config.data_dir.clone();
    tokio::task::spawn_blocking(move || {
        let lock = session_lock(&owner_id, &account_id);
        let _guard = lock.lock().unwrap_or_else(|error| error.into_inner());
        let mut store = open_store(&data_dir)?;
        require_icloud_account(&store, &owner_id, &account_id)?;
        let removed = store.delete_apple_hme_session(&owner_id, &account_id)?;
        Ok(json!({"disconnected":removed}))
    })
    .await
    .map_err(|_| IntegrationError::Internal)?
}

fn address_record(owner_id: &str, account_id: &str, address: &HmeAddress) -> AppleHmeAddressRecord {
    AppleHmeAddressRecord {
        account_id: account_id.to_string(),
        user_id: owner_id.to_string(),
        anonymous_id: address.anonymous_id.clone(),
        email: address.email.clone(),
        label: address.label.clone(),
        note: address.note.clone(),
        forward_to_email: address.forward_to_email.clone(),
        active: address.active,
        origin: address.origin.clone(),
        created_at: address.created_at.map(|value| value.to_rfc3339()),
        updated_at: String::new(),
    }
}

fn snapshot_value(addresses: Vec<AppleHmeAddressRecord>, last_synced_at: Option<String>) -> Value {
    let addresses = addresses
        .into_iter()
        .map(|address| {
            json!({
                "anonymousId": address.anonymous_id,
                "email": address.email,
                "label": address.label,
                "note": address.note,
                "forwardToEmail": address.forward_to_email,
                "active": address.active,
                "origin": address.origin,
                "createdAt": address.created_at,
                "cachedAt": address.updated_at,
            })
        })
        .collect::<Vec<_>>();
    let address_count = addresses.len();
    json!({
        "addresses": addresses,
        "addressCount": address_count,
        "lastSyncedAt": last_synced_at,
    })
}

pub(crate) async fn embedded_status(
    state: Arc<AppState>,
    owner_id: String,
    account_id: String,
) -> Result<Value, crate::EmbeddedOperationError> {
    status_service(state, owner_id, account_id)
        .await
        .and_then(|status| serde_json::to_value(status).map_err(|_| IntegrationError::Internal))
        .map_err(embedded_error)
}

pub(crate) async fn embedded_start_login(
    state: Arc<AppState>,
    owner_id: String,
    actor: String,
    account_id: String,
    input: Value,
) -> Result<Value, crate::EmbeddedOperationError> {
    let input: StartLoginInput = serde_json::from_value(input)
        .map_err(|_| embedded_error(IntegrationError::invalid("请求参数无效")))?;
    start_login_service(
        state,
        owner_id,
        actor,
        account_id,
        input.kind,
        input.password,
        input.apple_id,
        input.two_factor_method,
        input.phone_number,
        input.icloud_host,
    )
    .await
    .and_then(|result| serde_json::to_value(result).map_err(|_| IntegrationError::Internal))
    .map_err(embedded_error)
}

pub(crate) async fn embedded_submit_two_factor(
    state: Arc<AppState>,
    owner_id: String,
    actor: String,
    account_id: String,
    input: Value,
) -> Result<Value, crate::EmbeddedOperationError> {
    let input: TwoFactorInput = serde_json::from_value(input)
        .map_err(|_| embedded_error(IntegrationError::invalid("请求参数无效")))?;
    submit_two_factor_service(
        state,
        owner_id,
        actor,
        account_id,
        input.pending_id,
        input.code,
        input.phone_number,
    )
    .await
    .and_then(|result| serde_json::to_value(result).map_err(|_| IntegrationError::Internal))
    .map_err(embedded_error)
}

pub(crate) async fn embedded_list(
    state: Arc<AppState>,
    owner_id: String,
    account_id: String,
) -> Result<Value, crate::EmbeddedOperationError> {
    list_service(state, owner_id, account_id)
        .await
        .map_err(embedded_error)
}

pub(crate) async fn embedded_sync(
    state: Arc<AppState>,
    owner_id: String,
    account_id: String,
) -> Result<Value, crate::EmbeddedOperationError> {
    sync_service(state, owner_id, account_id)
        .await
        .map_err(embedded_error)
}

pub(crate) async fn embedded_create(
    state: Arc<AppState>,
    owner_id: String,
    account_id: String,
    input: Value,
) -> Result<Value, crate::EmbeddedOperationError> {
    let input: CreateAddressInput = serde_json::from_value(input)
        .map_err(|_| embedded_error(IntegrationError::invalid("请求参数无效")))?;
    create_service(
        state,
        owner_id,
        account_id,
        input.label,
        input.note,
        input.channel,
    )
    .await
    .map_err(embedded_error)
}

pub(crate) async fn embedded_deactivate(
    state: Arc<AppState>,
    owner_id: String,
    account_id: String,
    anonymous_id: String,
) -> Result<Value, crate::EmbeddedOperationError> {
    deactivate_service(state, owner_id, account_id, anonymous_id)
        .await
        .map_err(embedded_error)
}

pub(crate) async fn embedded_delete(
    state: Arc<AppState>,
    owner_id: String,
    account_id: String,
    anonymous_id: String,
) -> Result<Value, crate::EmbeddedOperationError> {
    delete_service(state, owner_id, account_id, anonymous_id)
        .await
        .map_err(embedded_error)
}

pub(crate) async fn embedded_disconnect(
    state: Arc<AppState>,
    owner_id: String,
    account_id: String,
) -> Result<Value, crate::EmbeddedOperationError> {
    disconnect_service(state, owner_id, account_id)
        .await
        .map_err(embedded_error)
}

async fn mutate_session<T, F>(
    data_dir: PathBuf,
    owner_id: String,
    account_id: String,
    operation: F,
) -> Result<T, IntegrationError>
where
    T: Send + 'static,
    F: FnOnce(&mut AppleSession) -> Result<T, AppleHmeError> + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let lock = session_lock(&owner_id, &account_id);
        let _guard = lock.lock().unwrap_or_else(|error| error.into_inner());
        let mut store = open_store(&data_dir)?;
        require_icloud_account(&store, &owner_id, &account_id)?;
        let mut session =
            load_session(&store, &data_dir, &owner_id, &account_id)?.ok_or_else(|| {
                IntegrationError::apple(
                    AppleHmeErrorCode::SessionMissing,
                    "尚未授权 Apple Hide My Email",
                    false,
                )
            })?;
        let result = operation(&mut session).map_err(IntegrationError::Apple);
        persist_session(&mut store, &data_dir, &owner_id, &account_id, &session)?;
        result
    })
    .await
    .map_err(|_| IntegrationError::Internal)?
}

fn open_store(data_dir: &FsPath) -> Result<SqliteAuthStore, IntegrationError> {
    Ok(SqliteAuthStore::open_database(
        data_dir.join("imail.sqlite"),
    )?)
}

fn require_icloud_account(
    store: &SqliteAuthStore,
    owner_id: &str,
    account_id: &str,
) -> Result<imail_core::AccountRecord, IntegrationError> {
    let account = store
        .account(owner_id, account_id)?
        .ok_or(IntegrationError::AccountNotFound)?;
    if !account.provider.eq_ignore_ascii_case("icloud") {
        return Err(IntegrationError::NotIcloudAccount);
    }
    Ok(account)
}

fn load_session(
    store: &SqliteAuthStore,
    data_dir: &FsPath,
    owner_id: &str,
    account_id: &str,
) -> Result<Option<AppleSession>, IntegrationError> {
    let Some(record) = store.apple_hme_session(owner_id, account_id)? else {
        return Ok(None);
    };
    let key = MasterKey::from_file(data_dir.join("master.key"))
        .map_err(|_| IntegrationError::Internal)?;
    key.decrypt_json(&record.encrypted_session)
        .map(Some)
        .map_err(|_| IntegrationError::CorruptSession)
}

fn persist_merged_session(
    store: &mut SqliteAuthStore,
    data_dir: &FsPath,
    owner_id: &str,
    account_id: &str,
    incoming: AppleSession,
) -> Result<SessionStatus, IntegrationError> {
    let merged = match load_session(store, data_dir, owner_id, account_id)? {
        Some(existing) if !existing.apple_id.eq_ignore_ascii_case(&incoming.apple_id) => {
            return Err(IntegrationError::AppleAccountMismatch)
        }
        Some(existing) => existing.merge(incoming),
        None => incoming,
    };
    persist_session(store, data_dir, owner_id, account_id, &merged)?;
    session_status(store, data_dir, owner_id, account_id)
}

fn persist_session(
    store: &mut SqliteAuthStore,
    data_dir: &FsPath,
    owner_id: &str,
    account_id: &str,
    session: &AppleSession,
) -> Result<(), IntegrationError> {
    let key = MasterKey::from_file(data_dir.join("master.key"))
        .map_err(|_| IntegrationError::Internal)?;
    let encrypted = key
        .encrypt_json(session)
        .map_err(|_| IntegrationError::Internal)?;
    store.upsert_apple_hme_session(owner_id, account_id, &encrypted)?;
    Ok(())
}

fn session_status(
    store: &SqliteAuthStore,
    data_dir: &FsPath,
    owner_id: &str,
    account_id: &str,
) -> Result<SessionStatus, IntegrationError> {
    let record = store.apple_hme_session(owner_id, account_id)?;
    let (session, updated_at) = match record {
        Some(record) => {
            let key = MasterKey::from_file(data_dir.join("master.key"))
                .map_err(|_| IntegrationError::Internal)?;
            let session = key
                .decrypt_json::<AppleSession>(&record.encrypted_session)
                .map_err(|_| IntegrationError::CorruptSession)?;
            (Some(session), Some(record.updated_at))
        }
        None => (None, None),
    };
    let icloud_web_state = session
        .as_ref()
        .and_then(|value| value.state(LoginStateKind::ICloudWeb));
    let apple_account_state = session
        .as_ref()
        .and_then(|value| value.state(LoginStateKind::AppleAccount));
    let freshness_cutoff = Utc::now() - chrono::Duration::minutes(6);
    let state_is_connected = |state: &imail_apple_hme::LoginState| {
        state.last_check_ok
            && state
                .last_checked_at
                .is_some_and(|checked_at| checked_at >= freshness_cutoff)
    };
    let icloud_web_connected = icloud_web_state.is_some_and(state_is_connected);
    let apple_account_connected = apple_account_state.is_some_and(state_is_connected);
    Ok(SessionStatus {
        account_id: account_id.to_string(),
        authorized: session.is_some(),
        connected: icloud_web_connected || apple_account_connected,
        icloud_web_authorized: icloud_web_state.is_some(),
        icloud_web_connected,
        icloud_web_status_message: icloud_web_state
            .and_then(|state| state.last_status_message.clone()),
        icloud_web_last_successful_keepalive_at: icloud_web_state
            .and_then(|state| state.last_successful_keepalive_at),
        apple_account_authorized: apple_account_state.is_some(),
        apple_account_connected,
        apple_account_status_message: apple_account_state
            .and_then(|state| state.last_status_message.clone()),
        apple_account_last_successful_keepalive_at: apple_account_state
            .and_then(|state| state.last_successful_keepalive_at),
        is_icloud_plus: session.as_ref().is_some_and(|value| value.is_icloud_plus),
        can_create_hme: session.as_ref().is_some_and(|value| value.can_create_hme),
        updated_at,
    })
}

fn login_kind_name(value: LoginKindInput) -> &'static str {
    match value {
        LoginKindInput::IcloudWeb => "icloud-web",
        LoginKindInput::AppleAccount => "apple-account",
    }
}

fn channel_name(value: CreateChannel) -> &'static str {
    match value {
        CreateChannel::Auto => "auto",
        CreateChannel::AppleAccount => "appleAccount",
        CreateChannel::ICloudWeb => "icloudWeb",
    }
}

pub(crate) enum IntegrationError {
    Storage,
    Apple(AppleHmeError),
    AccountNotFound,
    NotIcloudAccount,
    AppleAccountMismatch,
    CorruptSession,
    Internal,
}

impl IntegrationError {
    fn apple(code: AppleHmeErrorCode, message: &str, retryable: bool) -> Self {
        Self::Apple(AppleHmeError {
            code,
            message: message.into(),
            retryable,
        })
    }

    fn invalid(message: &str) -> Self {
        Self::apple(AppleHmeErrorCode::InvalidInput, message, false)
    }

    pub(crate) fn public_message(&self) -> String {
        match self {
            Self::Apple(error) => error.message.clone(),
            Self::AccountNotFound => "邮箱账户不存在".into(),
            Self::NotIcloudAccount => "Hide My Email 只支持 iCloud 邮箱账户".into(),
            Self::AppleAccountMismatch => "已有 HME 会话属于另一个 Apple 账户，请先断开授权".into(),
            Self::CorruptSession => "Apple HME 会话无法解密，请重新授权".into(),
            Self::Storage | Self::Internal => "服务暂时无法完成请求".into(),
        }
    }
}

fn embedded_error(error: IntegrationError) -> crate::EmbeddedOperationError {
    let status = match &error {
        IntegrationError::Apple(error) => match error.code {
            AppleHmeErrorCode::InvalidInput => 400,
            AppleHmeErrorCode::CredentialsInvalid | AppleHmeErrorCode::TwoFactorInvalid => 401,
            AppleHmeErrorCode::AddressNotFound => 404,
            AppleHmeErrorCode::AddressLimitReached => 429,
            AppleHmeErrorCode::SubscriptionRequired | AppleHmeErrorCode::Unsupported => 422,
            AppleHmeErrorCode::SessionMissing
            | AppleHmeErrorCode::SessionExpired
            | AppleHmeErrorCode::PendingLoginExpired
            | AppleHmeErrorCode::TwoFactorRequired => 409,
            AppleHmeErrorCode::Network
            | AppleHmeErrorCode::BadResponse
            | AppleHmeErrorCode::Protocol => 502,
        },
        IntegrationError::AccountNotFound => 404,
        IntegrationError::NotIcloudAccount => 400,
        IntegrationError::AppleAccountMismatch | IntegrationError::CorruptSession => 409,
        IntegrationError::Storage | IntegrationError::Internal => 500,
    };
    crate::EmbeddedOperationError {
        status,
        message: error.public_message(),
    }
}

impl From<AuthStoreError> for IntegrationError {
    fn from(_: AuthStoreError) -> Self {
        Self::Storage
    }
}

impl From<StorageError> for IntegrationError {
    fn from(_: StorageError) -> Self {
        Self::Storage
    }
}

impl From<AppleHmeError> for IntegrationError {
    fn from(value: AppleHmeError) -> Self {
        Self::Apple(value)
    }
}

fn response<T: Serialize>(result: Result<T, IntegrationError>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => error_response(error),
    }
}

fn error_response(error: IntegrationError) -> Response {
    match error {
        IntegrationError::Apple(error) => apple_error(error),
        IntegrationError::AccountNotFound => error_body(
            StatusCode::NOT_FOUND,
            "ACCOUNT_NOT_FOUND",
            "邮箱账户不存在",
            false,
        ),
        IntegrationError::NotIcloudAccount => error_body(
            StatusCode::BAD_REQUEST,
            "NOT_ICLOUD_ACCOUNT",
            "Hide My Email 只支持 iCloud 邮箱账户",
            false,
        ),
        IntegrationError::AppleAccountMismatch => error_body(
            StatusCode::CONFLICT,
            "APPLE_ACCOUNT_MISMATCH",
            "已有 HME 会话属于另一个 Apple 账户，请先断开授权",
            false,
        ),
        IntegrationError::CorruptSession => error_body(
            StatusCode::CONFLICT,
            "APPLE_HME_SESSION_INVALID",
            "Apple HME 会话无法解密，请重新授权",
            false,
        ),
        IntegrationError::Storage | IntegrationError::Internal => internal_error(),
    }
}

fn apple_error(error: AppleHmeError) -> Response {
    let status = match error.code {
        AppleHmeErrorCode::InvalidInput => StatusCode::BAD_REQUEST,
        AppleHmeErrorCode::CredentialsInvalid | AppleHmeErrorCode::TwoFactorInvalid => {
            StatusCode::UNAUTHORIZED
        }
        AppleHmeErrorCode::AddressNotFound => StatusCode::NOT_FOUND,
        AppleHmeErrorCode::AddressLimitReached => StatusCode::TOO_MANY_REQUESTS,
        AppleHmeErrorCode::SubscriptionRequired | AppleHmeErrorCode::Unsupported => {
            StatusCode::UNPROCESSABLE_ENTITY
        }
        AppleHmeErrorCode::SessionMissing
        | AppleHmeErrorCode::SessionExpired
        | AppleHmeErrorCode::PendingLoginExpired
        | AppleHmeErrorCode::TwoFactorRequired => StatusCode::CONFLICT,
        AppleHmeErrorCode::Network
        | AppleHmeErrorCode::BadResponse
        | AppleHmeErrorCode::Protocol => StatusCode::BAD_GATEWAY,
    };
    error_body(
        status,
        apple_code(error.code),
        &error.message,
        error.retryable,
    )
}

fn apple_code(code: AppleHmeErrorCode) -> &'static str {
    match code {
        AppleHmeErrorCode::InvalidInput => "APPLE_INVALID_INPUT",
        AppleHmeErrorCode::CredentialsInvalid => "APPLE_CREDENTIALS_INVALID",
        AppleHmeErrorCode::TwoFactorRequired => "APPLE_TWO_FACTOR_REQUIRED",
        AppleHmeErrorCode::TwoFactorInvalid => "APPLE_TWO_FACTOR_INVALID",
        AppleHmeErrorCode::PendingLoginExpired => "APPLE_PENDING_LOGIN_EXPIRED",
        AppleHmeErrorCode::SessionMissing => "APPLE_SESSION_MISSING",
        AppleHmeErrorCode::SessionExpired => "APPLE_SESSION_EXPIRED",
        AppleHmeErrorCode::SubscriptionRequired => "APPLE_ICLOUD_PLUS_REQUIRED",
        AppleHmeErrorCode::AddressLimitReached => "APPLE_HME_LIMIT_REACHED",
        AppleHmeErrorCode::AddressNotFound => "APPLE_HME_ADDRESS_NOT_FOUND",
        AppleHmeErrorCode::Unsupported => "APPLE_HME_UNSUPPORTED",
        AppleHmeErrorCode::Network => "APPLE_NETWORK_ERROR",
        AppleHmeErrorCode::BadResponse => "APPLE_BAD_RESPONSE",
        AppleHmeErrorCode::Protocol => "APPLE_PROTOCOL_ERROR",
    }
}

fn error_body(status: StatusCode, code: &'static str, message: &str, retryable: bool) -> Response {
    (
        status,
        Json(ErrorBody {
            error: message.into(),
            code,
            retryable,
        }),
    )
        .into_response()
}

fn bad_request(message: &str) -> Response {
    error_body(StatusCode::BAD_REQUEST, "INVALID_INPUT", message, false)
}

fn internal_error() -> Response {
    error_body(
        StatusCode::INTERNAL_SERVER_ERROR,
        "INTERNAL_ERROR",
        "服务暂时无法完成请求",
        true,
    )
}

#[cfg(test)]
mod keepalive_tests {
    use super::*;

    #[derive(Clone)]
    struct ScriptedTransport {
        responses: Arc<StdMutex<Vec<imail_apple_hme::HttpResponse>>>,
        urls: Arc<StdMutex<Vec<String>>>,
    }

    impl imail_apple_hme::HttpTransport for ScriptedTransport {
        fn execute(
            &self,
            request: imail_apple_hme::HttpRequest,
        ) -> Result<imail_apple_hme::HttpResponse, AppleHmeError> {
            self.urls
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(request.url);
            Ok(self
                .responses
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(0))
        }
    }

    fn login_state(kind: LoginStateKind) -> imail_apple_hme::LoginState {
        let now = Utc::now();
        imail_apple_hme::LoginState {
            kind,
            host: match kind {
                LoginStateKind::AppleAccount => "appleid.apple.com",
                LoginStateKind::ICloudWeb => "www.icloud.com",
            }
            .into(),
            origin: match kind {
                LoginStateKind::AppleAccount => "https://account.apple.com",
                LoginStateKind::ICloudWeb => "https://www.icloud.com",
            }
            .into(),
            cookies: vec![imail_apple_hme::AppleCookie {
                name: "session".into(),
                value: "secret".into(),
                domain: ".apple.com".into(),
                path: "/".into(),
                expires_at: None,
                secure: true,
                http_only: true,
            }],
            scnt: (kind == LoginStateKind::AppleAccount).then(|| "scnt".into()),
            session_id: (kind == LoginStateKind::AppleAccount).then(|| "session-id".into()),
            api_key: (kind == LoginStateKind::AppleAccount).then(|| "api-key".into()),
            data_access_token: None,
            user_agent: "test".into(),
            saved_at: now,
            manage_expires_at: None,
            last_checked_at: Some(now),
            last_check_ok: true,
            last_status_message: None,
            last_successful_keepalive_at: None,
        }
    }

    #[test]
    fn keepalive_interval_stays_within_three_to_five_minutes() {
        for _ in 0..256 {
            let interval = random_keepalive_interval();
            assert!(interval >= StdDuration::from_secs(KEEPALIVE_MIN_SECONDS));
            assert!(interval <= StdDuration::from_secs(KEEPALIVE_MAX_SECONDS));
        }
    }

    #[test]
    fn icloud_web_keepalive_runs_when_apple_account_keepalive_fails() {
        let urls = Arc::new(StdMutex::new(Vec::new()));
        let client = AppleHmeClient::new(ScriptedTransport {
            responses: Arc::new(StdMutex::new(vec![
                imail_apple_hme::HttpResponse {
                    status: 401,
                    headers: BTreeMap::new(),
                    body: serde_json::to_vec(&json!({"error": "expired"})).unwrap(),
                },
                imail_apple_hme::HttpResponse {
                    status: 200,
                    headers: BTreeMap::new(),
                    body: serde_json::to_vec(&json!({
                        "success": true,
                        "result": {"hmeEmails": []}
                    }))
                    .unwrap(),
                },
            ])),
            urls: Arc::clone(&urls),
        });
        let mut session = AppleSession::empty("owner@icloud.com");
        session.dsid = Some("123".into());
        session.client_id = Some("client".into());
        session.premium_mail_base_url = Some("https://premium.example".into());
        session.host = "www.icloud.com".into();
        session.put_state(login_state(LoginStateKind::AppleAccount));
        session.put_state(login_state(LoginStateKind::ICloudWeb));

        let attempts = keep_alive_session_channels(&client, &mut session);

        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].0, LoginStateKind::AppleAccount);
        assert!(attempts[0].1.is_err());
        assert_eq!(attempts[1].0, LoginStateKind::ICloudWeb);
        assert!(attempts[1].1.is_ok());
        assert!(
            !session
                .state(LoginStateKind::AppleAccount)
                .unwrap()
                .last_check_ok
        );
        let icloud_state = session.state(LoginStateKind::ICloudWeb).unwrap();
        assert!(icloud_state.last_check_ok);
        assert!(icloud_state.last_successful_keepalive_at.is_some());
        let urls = urls.lock().unwrap_or_else(|error| error.into_inner());
        assert_eq!(urls.len(), 2);
        assert!(urls[0].contains("/account/manage/gs/ws/token"));
        assert!(urls[1].contains("/v2/hme/list"));
    }

    #[test]
    fn keepalive_runtime_stops_without_waiting_for_the_next_interval() {
        let mut runtime = AppleHmeKeepaliveRuntime::start(PathBuf::from("unused-test-data-dir"))
            .expect("keepalive worker starts");
        assert!(runtime.shutdown(StdDuration::from_secs(1)));
    }
}
