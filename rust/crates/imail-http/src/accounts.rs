use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use axum::{
    extract::{rejection::JsonRejection, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use chrono::Utc;
use imail_core::{
    accounts::{public_account, AccountService, CredentialValidationError},
    mail_operations::connection_config,
    oauth_refresh::RefreshingConnectionService,
    AccountRepository, ApplicationError, AuthRepository,
};
use imail_mail::{mailbox_role_for, redact_protocol_detail};
use imail_protocol::{
    AccountMetadataPatch, AccountProxyUpdate, AccountReadModel, ProxyProtocol, SyncJobReadModel,
    WorkspaceIconId,
};
use imail_security::MasterKey;
use imail_storage_sqlite::{
    AuthStoreError, MasterKeyCredentialCodec, SqliteAuthStore, SyncEnqueue, SyncRuntimeError,
    SyncRuntimeStore,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{auth::AuthenticatedUser, AppState};

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/accounts", get(list).post(create_account))
        .route(
            "/api/accounts/:id",
            axum::routing::patch(update_metadata).delete(remove),
        )
        .route("/api/accounts/:id/sync", post(sync_account))
        .route("/api/accounts/:id/connection-test", post(test_connection))
        .route(
            "/api/accounts/:id/credential",
            axum::routing::put(update_credential),
        )
        .route("/api/accounts/:id/proxy", axum::routing::put(update_proxy))
        .route(
            "/api/accounts/:id/mailboxes/:role/sync",
            post(sync_account_role),
        )
        .route(
            "/api/accounts/:id/mailboxes/sync",
            post(sync_account_mailbox),
        )
        .route("/api/mailboxes/:role/sync", post(sync_all_role))
        .route("/api/sync", post(sync_all))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HttpAccount {
    id: String,
    provider: String,
    email: String,
    display_name: String,
    group: String,
    group_icon: String,
    color: String,
    settings: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    proxy: Option<Value>,
    auth_method: String,
    created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_sync_at: Option<String>,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_error: Option<String>,
    mailboxes: Value,
}

impl From<AccountReadModel> for HttpAccount {
    fn from(account: AccountReadModel) -> Self {
        Self {
            id: account.id,
            provider: account.provider,
            email: account.email,
            display_name: account.display_name,
            group: account.group,
            group_icon: if account.group_icon.is_empty() {
                "folder".into()
            } else {
                account.group_icon
            },
            color: account.color,
            settings: safe_object(
                account.settings,
                &[
                    "imapHost",
                    "imapPort",
                    "imapSecure",
                    "smtpHost",
                    "smtpPort",
                    "smtpSecure",
                ],
            ),
            proxy: account
                .proxy
                .map(|proxy| safe_object(proxy, &["protocol", "host", "port", "username"])),
            auth_method: account.auth_method.unwrap_or_else(|| "app-password".into()),
            created_at: account.created_at,
            last_sync_at: account.last_sync_at,
            status: account.status,
            last_error: account.last_error,
            mailboxes: if account.mailboxes.is_array() {
                account.mailboxes
            } else {
                Value::Array(Vec::new())
            },
        }
    }
}

pub fn embedded_accounts(accounts: Vec<AccountReadModel>) -> Value {
    serde_json::json!({
        "accounts": accounts.into_iter().map(HttpAccount::from).collect::<Vec<_>>()
    })
}

pub fn embedded_account(account: AccountReadModel) -> Value {
    serde_json::json!({"account": HttpAccount::from(account)})
}

fn safe_object(value: Value, allowed: &[&str]) -> Value {
    let Some(source) = value.as_object() else {
        return Value::Object(Map::new());
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

#[derive(Serialize)]
struct AccountsBody {
    accounts: Vec<HttpAccount>,
}

#[derive(Serialize)]
pub(crate) struct AccountBody {
    pub(crate) account: HttpAccount,
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct QueuedBody {
    synced: i64,
    queued: bool,
    job_id: String,
}

impl From<SyncJobReadModel> for QueuedBody {
    fn from(job: SyncJobReadModel) -> Self {
        Self {
            synced: 0,
            queued: true,
            job_id: job.id,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct QueuedAccountResult {
    account_id: String,
    status: &'static str,
    #[serde(flatten)]
    queued: QueuedBody,
}

#[derive(Serialize)]
struct QueuedResultsBody {
    results: Vec<QueuedAccountResult>,
}

#[derive(Deserialize)]
struct MailboxInput {
    mailbox: String,
}

async fn create_account(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    input: Result<Json<CreateAccountInput>, JsonRejection>,
) -> Response {
    let Json(input) = match input {
        Ok(input) => input,
        Err(_) => return error(StatusCode::BAD_REQUEST, "请求参数无效"),
    };
    match create_account_application(state, user.user_id, user.actor, input).await {
        Ok(account) => (
            StatusCode::CREATED,
            Json(AccountBody {
                account: account.into(),
            }),
        )
            .into_response(),
        Err(cause) => application_error(cause),
    }
}

#[derive(Deserialize)]
struct CredentialInput {
    password: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateAccountInput {
    provider: String,
    email: String,
    display_name: String,
    #[serde(default = "default_group")]
    group: String,
    #[serde(default = "default_group_icon")]
    group_icon: WorkspaceIconId,
    #[serde(default = "default_color")]
    color: String,
    password: Option<String>,
    access_token: Option<String>,
    settings: Option<MailSettingsInput>,
    proxy: Option<CreateProxyInput>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MailSettingsInput {
    imap_host: String,
    imap_port: u16,
    imap_secure: bool,
    smtp_host: String,
    smtp_port: u16,
    smtp_secure: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateProxyInput {
    protocol: ProxyProtocol,
    host: String,
    port: u16,
    username: Option<String>,
    password: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProxyInput {
    enabled: bool,
    source_account_id: Option<String>,
    protocol: Option<ProxyProtocol>,
    host: Option<String>,
    port: Option<u16>,
    username: Option<String>,
    password: Option<String>,
}

impl ProxyInput {
    fn into_update(self) -> Option<AccountProxyUpdate> {
        if !self.enabled {
            return Some(AccountProxyUpdate::Disabled);
        }
        if let Some(source_account_id) = self.source_account_id {
            if uuid::Uuid::parse_str(&source_account_id).is_err() {
                return None;
            }
            return Some(AccountProxyUpdate::CopyFrom { source_account_id });
        }
        Some(AccountProxyUpdate::Explicit {
            protocol: self.protocol?,
            host: self.host?,
            port: self.port?,
            username: self.username,
            password: self.password,
        })
    }
}

pub(crate) async fn create_account_value_application(
    state: Arc<AppState>,
    user_id: String,
    actor: String,
    input: Value,
) -> Result<AccountReadModel, ApplicationError<AuthStoreError>> {
    let input = serde_json::from_value::<CreateAccountInput>(input)
        .map_err(|_| invalid_create_request())?;
    create_account_application(state, user_id, actor, input).await
}

pub(crate) async fn create_account_application(
    state: Arc<AppState>,
    user_id: String,
    actor: String,
    input: CreateAccountInput,
) -> Result<AccountReadModel, ApplicationError<AuthStoreError>> {
    let data_dir = state.config.data_dir.clone();
    let database = data_dir.join("imail.sqlite");
    let probe = Arc::clone(&state.config.connection_probe);
    run_sensitive(data_dir, move |store, master_key| {
        let codec = MasterKeyCredentialCodec::new(master_key);
        let account = build_account(&user_id, input, master_key)?;
        let validator = |candidate: &imail_core::AccountRecord| {
            let config = connection_config::<AuthStoreError, _>(candidate, &codec)
                .map_err(|_| CredentialValidationError)?;
            probe.verify(&config).map_err(|_| CredentialValidationError)
        };
        let account = AccountService::new(store).create(account, &validator)?;
        SyncRuntimeStore::open_database(&database)
            .and_then(|sync| sync.ensure_policy(&account.id, &user_id, Utc::now()))
            .map_err(|_| ApplicationError::Domain {
                code: "SYNC_POLICY_UNAVAILABLE",
                status: 500,
                message: "服务暂时无法完成请求",
            })?;
        store
            .record_security_event(
                "account.created",
                &actor,
                Some(&user_id),
                &BTreeMap::from([
                    ("accountId".into(), account.id.clone()),
                    ("provider".into(), account.provider.clone()),
                ]),
            )
            .map_err(ApplicationError::Repository)?;
        Ok(account)
    })
    .await
}

async fn list(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    let database = state.config.data_dir.join("imail.sqlite");
    match run(database, move |store| {
        AccountService::new(store).list(&user.user_id)
    })
    .await
    {
        Ok(accounts) => Json(AccountsBody {
            accounts: accounts.into_iter().map(HttpAccount::from).collect(),
        })
        .into_response(),
        Err(cause) => application_error(cause),
    }
}

async fn update_metadata(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(account_id): Path<String>,
    patch: Result<Json<AccountMetadataPatch>, JsonRejection>,
) -> Response {
    let Json(patch) = match patch {
        Ok(patch) => patch,
        Err(_) => return error(StatusCode::BAD_REQUEST, "请求参数无效"),
    };
    let database = state.config.data_dir.join("imail.sqlite");
    match run(database, move |store| {
        AccountService::new(store).update_metadata(&user.user_id, &account_id, patch)
    })
    .await
    {
        Ok(account) => Json(AccountBody {
            account: account.into(),
        })
        .into_response(),
        Err(cause) => application_error(cause),
    }
}

async fn update_credential(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(account_id): Path<String>,
    input: Result<Json<CredentialInput>, JsonRejection>,
) -> Response {
    let Json(input) = match input {
        Ok(input) => input,
        Err(_) => return error(StatusCode::BAD_REQUEST, "请求参数无效"),
    };
    match update_credential_application(state, user.user_id, user.actor, account_id, input.password)
        .await
    {
        Ok(account) => Json(AccountBody {
            account: account.into(),
        })
        .into_response(),
        Err(cause) => application_error(cause),
    }
}

async fn update_proxy(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(account_id): Path<String>,
    input: Result<Json<ProxyInput>, JsonRejection>,
) -> Response {
    let input = match input.ok().and_then(|Json(input)| input.into_update()) {
        Some(input) => input,
        None => return error(StatusCode::BAD_REQUEST, "请求参数无效"),
    };
    match update_proxy_application(state, user.user_id, user.actor, account_id, input).await {
        Ok(account) => Json(AccountBody {
            account: account.into(),
        })
        .into_response(),
        Err(cause) => application_error(cause),
    }
}

async fn test_connection(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(account_id): Path<String>,
) -> Response {
    match test_connection_application(state, user.user_id, account_id).await {
        Ok(account) => Json(AccountBody {
            account: account.into(),
        })
        .into_response(),
        Err(cause) => application_error(cause),
    }
}

pub fn embedded_credential_password(input: Value) -> Option<String> {
    serde_json::from_value::<CredentialInput>(input)
        .ok()
        .map(|input| input.password)
}

pub fn embedded_proxy_update(input: Value) -> Option<AccountProxyUpdate> {
    serde_json::from_value::<ProxyInput>(input)
        .ok()?
        .into_update()
}

pub(crate) async fn update_credential_application(
    state: Arc<AppState>,
    user_id: String,
    actor: String,
    account_id: String,
    password: String,
) -> Result<AccountReadModel, ApplicationError<AuthStoreError>> {
    let data_dir = state.config.data_dir.clone();
    let probe = Arc::clone(&state.config.connection_probe);
    run_sensitive(data_dir, move |store, master_key| {
        let codec = MasterKeyCredentialCodec::new(master_key);
        let validator = |candidate: &imail_core::AccountRecord| {
            let config = connection_config::<AuthStoreError, _>(candidate, &codec)
                .map_err(|_| CredentialValidationError)?;
            probe.verify(&config).map_err(|_| CredentialValidationError)
        };
        let account = AccountService::new(store).replace_password(
            &user_id,
            &account_id,
            &password,
            &codec,
            &validator,
        )?;
        store
            .record_security_event(
                "account.credential-updated",
                &actor,
                Some(&user_id),
                &BTreeMap::from([("accountId".into(), account_id)]),
            )
            .map_err(ApplicationError::Repository)?;
        Ok(account)
    })
    .await
}

pub(crate) async fn update_proxy_application(
    state: Arc<AppState>,
    user_id: String,
    actor: String,
    account_id: String,
    input: AccountProxyUpdate,
) -> Result<AccountReadModel, ApplicationError<AuthStoreError>> {
    let enabled = !matches!(input, AccountProxyUpdate::Disabled);
    let data_dir = state.config.data_dir.clone();
    let probe = Arc::clone(&state.config.connection_probe);
    let environment = state.config.oauth_environment.clone();
    let coordinator = Arc::clone(&state.refresh_coordinator);
    let oauth_factory = Arc::clone(&state.config.oauth_provider_factory);
    let oauth_resolver = Arc::clone(&state.config.oauth_config_resolver);
    run_sensitive(data_dir, move |store, master_key| {
        let codec = MasterKeyCredentialCodec::new(master_key);
        let current = store
            .account(&user_id, &account_id)
            .map_err(ApplicationError::Repository)?
            .ok_or(ApplicationError::Domain {
                code: "ACCOUNT_NOT_FOUND",
                status: 404,
                message: "邮箱账户不存在",
            })?;
        if current.auth_method.as_deref() == Some("oauth2") {
            let mut oauth = oauth_factory.create();
            RefreshingConnectionService::new_with_config_resolver(
                store,
                &codec,
                oauth.as_mut(),
                coordinator.as_ref(),
                &environment,
                oauth_resolver.as_ref(),
            )
            .resolve(&user_id, &account_id, Utc::now().timestamp_millis())
            .map_err(|_| ApplicationError::Domain {
                code: "ACCOUNT_CONNECTION_FAILED",
                status: 422,
                message: "邮箱连接验证失败，未保存更改",
            })?;
        }
        let validator = |candidate: &imail_core::AccountRecord| {
            let config = connection_config::<AuthStoreError, _>(candidate, &codec)
                .map_err(|_| CredentialValidationError)?;
            probe.verify(&config).map_err(|_| CredentialValidationError)
        };
        let account = AccountService::new(store).update_proxy(
            &user_id,
            &account_id,
            input,
            &codec,
            &validator,
        )?;
        store
            .record_security_event(
                "account.proxy-updated",
                &actor,
                Some(&user_id),
                &BTreeMap::from([
                    ("accountId".into(), account_id),
                    ("enabled".into(), enabled.to_string()),
                ]),
            )
            .map_err(ApplicationError::Repository)?;
        Ok(account)
    })
    .await
}

pub(crate) async fn test_connection_application(
    state: Arc<AppState>,
    user_id: String,
    account_id: String,
) -> Result<AccountReadModel, ApplicationError<AuthStoreError>> {
    let data_dir = state.config.data_dir.clone();
    let probe = Arc::clone(&state.config.connection_probe);
    let environment = state.config.oauth_environment.clone();
    let coordinator = Arc::clone(&state.refresh_coordinator);
    let oauth_factory = Arc::clone(&state.config.oauth_provider_factory);
    let oauth_resolver = Arc::clone(&state.config.oauth_config_resolver);
    run_sensitive(data_dir, move |store, master_key| {
        if store
            .account(&user_id, &account_id)
            .map_err(ApplicationError::Repository)?
            .is_none()
        {
            return Err(ApplicationError::Domain {
                code: "ACCOUNT_NOT_FOUND",
                status: 404,
                message: "邮箱账户不存在",
            });
        }
        let codec = MasterKeyCredentialCodec::new(master_key);
        let validation = {
            let mut oauth = oauth_factory.create();
            RefreshingConnectionService::new_with_config_resolver(
                store,
                &codec,
                oauth.as_mut(),
                coordinator.as_ref(),
                &environment,
                oauth_resolver.as_ref(),
            )
            .resolve(&user_id, &account_id, Utc::now().timestamp_millis())
            .map_err(|cause| cause.to_string())
            .and_then(|config| probe.verify(&config))
        };
        let mut account = store
            .account(&user_id, &account_id)
            .map_err(ApplicationError::Repository)?
            .ok_or(ApplicationError::Domain {
                code: "ACCOUNT_NOT_FOUND",
                status: 404,
                message: "邮箱账户已被移除",
            })?;
        match validation {
            Ok(()) => {
                account.status = "connected".into();
                account.last_error = None;
            }
            Err(cause) => {
                account.status = "error".into();
                account.last_error = Some(redact_protocol_detail(&cause));
            }
        }
        store
            .upsert_account(&account)
            .map_err(ApplicationError::Repository)?;
        Ok(public_account(&account))
    })
    .await
}

async fn remove(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(account_id): Path<String>,
) -> Response {
    let database = state.config.data_dir.join("imail.sqlite");
    match run(database, move |store| {
        AccountService::new(store)
            .remove_with_audit(&user.user_id, &user.actor, &account_id)
            .map(|_| ())
    })
    .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(cause) => application_error(cause),
    }
}

async fn sync_account(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(account_id): Path<String>,
) -> Response {
    queue_one(
        state.config.data_dir.join("imail.sqlite"),
        user.user_id,
        account_id,
        "inbox".into(),
        None,
    )
    .await
}

async fn sync_account_role(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((account_id, role)): Path<(String, String)>,
) -> Response {
    if !valid_mailbox_role(&role) {
        return error(StatusCode::BAD_REQUEST, "请求参数无效");
    }
    queue_one(
        state.config.data_dir.join("imail.sqlite"),
        user.user_id,
        account_id,
        role,
        None,
    )
    .await
}

async fn sync_account_mailbox(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(account_id): Path<String>,
    input: Result<Json<MailboxInput>, JsonRejection>,
) -> Response {
    let Json(input) = match input {
        Ok(input) => input,
        Err(_) => return error(StatusCode::BAD_REQUEST, "请求参数无效"),
    };
    let mailbox = input.mailbox.trim();
    if mailbox.is_empty() || mailbox.encode_utf16().count() > 500 {
        return error(StatusCode::BAD_REQUEST, "请求参数无效");
    }
    queue_one(
        state.config.data_dir.join("imail.sqlite"),
        user.user_id,
        account_id,
        "custom".into(),
        Some(mailbox.into()),
    )
    .await
}

async fn sync_all_role(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(role): Path<String>,
) -> Response {
    if !valid_mailbox_role(&role) {
        return error(StatusCode::BAD_REQUEST, "请求参数无效");
    }
    queue_all(
        state.config.data_dir.join("imail.sqlite"),
        user.user_id,
        role,
    )
    .await
}

async fn sync_all(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    queue_all(
        state.config.data_dir.join("imail.sqlite"),
        user.user_id,
        "inbox".into(),
    )
    .await
}

async fn queue_one(
    database: PathBuf,
    user_id: String,
    account_id: String,
    role: String,
    mailbox: Option<String>,
) -> Response {
    match run_sync(database, move |auth, sync| {
        let account = AccountService::new(auth).get(&user_id, &account_id)?;
        let (mailbox_role, mailbox) = canonical_target(&account, role, mailbox);
        sync.enqueue(
            &SyncEnqueue {
                account_id,
                mailbox,
                mailbox_role,
                reason: "manual".into(),
                priority: 100,
                not_before: None,
            },
            Utc::now(),
        )
        .map_err(SyncHttpError::Runtime)
    })
    .await
    {
        Ok(job) => Json(QueuedBody::from(job)).into_response(),
        Err(cause) => sync_error(cause),
    }
}

async fn queue_all(database: PathBuf, user_id: String, role: String) -> Response {
    match run_sync(database, move |auth, sync| {
        let accounts = AccountService::new(auth).list(&user_id)?;
        accounts
            .into_iter()
            .map(|account| {
                let account_id = account.id;
                sync.enqueue(
                    &SyncEnqueue {
                        account_id: account_id.clone(),
                        mailbox: None,
                        mailbox_role: role.clone(),
                        reason: "manual".into(),
                        priority: 100,
                        not_before: None,
                    },
                    Utc::now(),
                )
                .map(|job| QueuedAccountResult {
                    account_id,
                    status: "fulfilled",
                    queued: job.into(),
                })
                .map_err(SyncHttpError::Runtime)
            })
            .collect()
    })
    .await
    {
        Ok(results) => Json(QueuedResultsBody { results }).into_response(),
        Err(cause) => sync_error(cause),
    }
}

pub(crate) async fn embedded_queue_one(
    state: Arc<AppState>,
    user_id: String,
    account_id: String,
    role: String,
    mailbox: Option<String>,
) -> Result<Value, crate::EmbeddedOperationError> {
    if !valid_mailbox_role(&role)
        || mailbox
            .as_deref()
            .is_some_and(|value| value.trim().is_empty() || value.encode_utf16().count() > 500)
    {
        return Err(embedded_operation_error(400, "请求参数无效"));
    }
    let mailbox = mailbox.map(|value| value.trim().to_string());
    let job = run_sync(
        state.config.data_dir.join("imail.sqlite"),
        move |auth, sync| {
            let account = AccountService::new(auth).get(&user_id, &account_id)?;
            let (mailbox_role, mailbox) = canonical_target(&account, role, mailbox);
            sync.enqueue(
                &SyncEnqueue {
                    account_id,
                    mailbox,
                    mailbox_role,
                    reason: "manual".into(),
                    priority: 100,
                    not_before: None,
                },
                Utc::now(),
            )
            .map_err(SyncHttpError::Runtime)
        },
    )
    .await
    .map_err(embedded_sync_error)?;
    serde_json::to_value(QueuedBody::from(job))
        .map_err(|_| embedded_operation_error(500, "服务暂时无法完成请求"))
}

pub(crate) async fn embedded_queue_all(
    state: Arc<AppState>,
    user_id: String,
    role: String,
) -> Result<Value, crate::EmbeddedOperationError> {
    if !valid_mailbox_role(&role) {
        return Err(embedded_operation_error(400, "请求参数无效"));
    }
    let results = run_sync(
        state.config.data_dir.join("imail.sqlite"),
        move |auth, sync| {
            AccountService::new(auth)
                .list(&user_id)?
                .into_iter()
                .map(|account| {
                    let account_id = account.id;
                    sync.enqueue(
                        &SyncEnqueue {
                            account_id: account_id.clone(),
                            mailbox: None,
                            mailbox_role: role.clone(),
                            reason: "manual".into(),
                            priority: 100,
                            not_before: None,
                        },
                        Utc::now(),
                    )
                    .map(|job| QueuedAccountResult {
                        account_id,
                        status: "fulfilled",
                        queued: job.into(),
                    })
                    .map_err(SyncHttpError::Runtime)
                })
                .collect::<Result<Vec<_>, _>>()
        },
    )
    .await
    .map_err(embedded_sync_error)?;
    serde_json::to_value(QueuedResultsBody { results })
        .map_err(|_| embedded_operation_error(500, "服务暂时无法完成请求"))
}

fn embedded_sync_error(cause: SyncHttpError) -> crate::EmbeddedOperationError {
    match cause {
        SyncHttpError::Application(ApplicationError::Domain {
            status, message, ..
        }) if status < 500 => embedded_operation_error(status, message),
        _ => embedded_operation_error(500, "服务暂时无法完成请求"),
    }
}

fn embedded_operation_error(
    status: u16,
    message: impl Into<String>,
) -> crate::EmbeddedOperationError {
    crate::EmbeddedOperationError {
        status,
        message: message.into(),
    }
}

pub(crate) fn canonical_target(
    account: &AccountReadModel,
    role: String,
    mailbox: Option<String>,
) -> (String, Option<String>) {
    let Some(mailbox) = mailbox else {
        return (role, None);
    };
    let special_use = account.mailboxes.as_array().and_then(|folders| {
        folders.iter().find_map(|folder| {
            let path = folder.get("path")?.as_str()?;
            if path == mailbox
                || (mailbox.eq_ignore_ascii_case("INBOX") && path.eq_ignore_ascii_case("INBOX"))
            {
                folder.get("specialUse").and_then(Value::as_str)
            } else {
                None
            }
        })
    });
    let resolved = mailbox_role_for(&mailbox, special_use);
    if resolved == "custom" {
        (resolved, Some(mailbox))
    } else {
        (resolved, None)
    }
}

pub(crate) fn valid_mailbox_role(role: &str) -> bool {
    matches!(
        role,
        "inbox" | "sent" | "archive" | "drafts" | "trash" | "junk" | "custom"
    )
}

fn build_account(
    owner_id: &str,
    input: CreateAccountInput,
    master_key: &MasterKey,
) -> Result<imail_core::AccountRecord, ApplicationError<AuthStoreError>> {
    if !matches!(
        input.provider.as_str(),
        "outlook" | "gmail" | "qq" | "yahoo" | "hotmail" | "icloud" | "custom"
    ) || !valid_email(&input.email)
        || invalid_create_text(&input.display_name, 80)
        || invalid_create_text(&input.group, 40)
        || !valid_account_color(&input.color)
        || input
            .password
            .as_ref()
            .is_some_and(|value| value.is_empty() || value.encode_utf16().count() > 512)
        || input
            .access_token
            .as_ref()
            .is_some_and(|value| value.is_empty() || value.encode_utf16().count() > 8192)
        || (input.password.is_none() && input.access_token.is_none())
    {
        return Err(invalid_create_request());
    }
    let settings = provider_settings(&input.provider, input.settings)?;
    let (proxy, proxy_password) = match input.proxy {
        Some(proxy) => {
            let (proxy, password) = create_proxy(proxy).ok_or_else(invalid_create_request)?;
            (Some(proxy), password)
        }
        None => (None, None),
    };
    let (auth_method, mut secret) = if let Some(password) = input.password {
        (
            "app-password",
            Map::from_iter([
                ("authType".into(), Value::String("app-password".into())),
                ("password".into(), Value::String(password)),
            ]),
        )
    } else {
        (
            "oauth2",
            Map::from_iter([
                ("authType".into(), Value::String("oauth2".into())),
                (
                    "accessToken".into(),
                    Value::String(input.access_token.unwrap_or_default()),
                ),
            ]),
        )
    };
    if let Some(password) = proxy_password {
        secret.insert("proxyPassword".into(), Value::String(password));
    }
    let encrypted_secret = master_key
        .encrypt_json(&Value::Object(secret))
        .map_err(|_| ApplicationError::Domain {
            code: "ACCOUNT_SECRET_UNAVAILABLE",
            status: 500,
            message: "邮箱账户凭据不可用",
        })?;
    Ok(imail_core::AccountRecord {
        id: uuid::Uuid::new_v4().to_string(),
        owner_id: owner_id.to_string(),
        provider: input.provider,
        email: input.email.to_lowercase(),
        display_name: input.display_name.trim().to_string(),
        group: input.group.trim().to_string(),
        group_icon: input.group_icon.as_str().into(),
        color: input.color,
        settings,
        proxy,
        encrypted_secret,
        auth_method: Some(auth_method.into()),
        created_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        last_sync_at: None,
        status: "syncing".into(),
        last_error: None,
        mailboxes: Value::Array(Vec::new()),
    })
}

pub(crate) async fn mcp_add_with_code(
    state: Arc<AppState>,
    owner_id: String,
    mut input: Value,
) -> Result<AccountReadModel, ApplicationError<AuthStoreError>> {
    let object = input.as_object_mut().ok_or_else(invalid_create_request)?;
    if let Some(code) = object.remove("authorizationCode") {
        object.insert("password".into(), code);
    }
    let input: CreateAccountInput =
        serde_json::from_value(input).map_err(|_| invalid_create_request())?;
    let data_dir = state.config.data_dir.clone();
    let database = data_dir.join("imail.sqlite");
    let probe = Arc::clone(&state.config.connection_probe);
    run_sensitive(data_dir, move |store, master_key| {
        let codec = MasterKeyCredentialCodec::new(master_key);
        let account = build_account(&owner_id, input, master_key)?;
        let validator = |candidate: &imail_core::AccountRecord| {
            let config = connection_config::<AuthStoreError, _>(candidate, &codec)
                .map_err(|_| CredentialValidationError)?;
            probe.verify(&config).map_err(|_| CredentialValidationError)
        };
        let account = AccountService::new(store).create(account, &validator)?;
        SyncRuntimeStore::open_database(&database)
            .and_then(|sync| sync.ensure_policy(&account.id, &owner_id, Utc::now()))
            .map_err(|_| ApplicationError::Domain {
                code: "SYNC_POLICY_UNAVAILABLE",
                status: 500,
                message: "服务暂时无法完成请求",
            })?;
        Ok(account)
    })
    .await
}

pub(crate) async fn mcp_replace_password(
    state: Arc<AppState>,
    owner_id: String,
    account_id: String,
    password: String,
) -> Result<AccountReadModel, ApplicationError<AuthStoreError>> {
    let data_dir = state.config.data_dir.clone();
    let probe = Arc::clone(&state.config.connection_probe);
    run_sensitive(data_dir, move |store, master_key| {
        let codec = MasterKeyCredentialCodec::new(master_key);
        let validator = |candidate: &imail_core::AccountRecord| {
            let config = connection_config::<AuthStoreError, _>(candidate, &codec)
                .map_err(|_| CredentialValidationError)?;
            probe.verify(&config).map_err(|_| CredentialValidationError)
        };
        AccountService::new(store).replace_password(
            &owner_id,
            &account_id,
            &password,
            &codec,
            &validator,
        )
    })
    .await
}

pub(crate) async fn mcp_update_proxy(
    state: Arc<AppState>,
    owner_id: String,
    account_id: String,
    input: AccountProxyUpdate,
) -> Result<AccountReadModel, ApplicationError<AuthStoreError>> {
    let data_dir = state.config.data_dir.clone();
    let probe = Arc::clone(&state.config.connection_probe);
    run_sensitive(data_dir, move |store, master_key| {
        let codec = MasterKeyCredentialCodec::new(master_key);
        let validator = |candidate: &imail_core::AccountRecord| {
            let config = connection_config::<AuthStoreError, _>(candidate, &codec)
                .map_err(|_| CredentialValidationError)?;
            probe.verify(&config).map_err(|_| CredentialValidationError)
        };
        AccountService::new(store).update_proxy(&owner_id, &account_id, input, &codec, &validator)
    })
    .await
}

pub(crate) async fn mcp_test_connection(
    state: Arc<AppState>,
    owner_id: String,
    account_id: String,
) -> Result<AccountReadModel, ApplicationError<AuthStoreError>> {
    let data_dir = state.config.data_dir.clone();
    let probe = Arc::clone(&state.config.connection_probe);
    let environment = state.config.oauth_environment.clone();
    let coordinator = Arc::clone(&state.refresh_coordinator);
    let oauth_factory = Arc::clone(&state.config.oauth_provider_factory);
    let oauth_resolver = Arc::clone(&state.config.oauth_config_resolver);
    run_sensitive(data_dir, move |store, master_key| {
        if store
            .account(&owner_id, &account_id)
            .map_err(ApplicationError::Repository)?
            .is_none()
        {
            return Err(ApplicationError::Domain {
                code: "ACCOUNT_NOT_FOUND",
                status: 404,
                message: "邮箱账户不存在",
            });
        }
        let codec = MasterKeyCredentialCodec::new(master_key);
        let validation = {
            let mut oauth = oauth_factory.create();
            RefreshingConnectionService::new_with_config_resolver(
                store,
                &codec,
                oauth.as_mut(),
                coordinator.as_ref(),
                &environment,
                oauth_resolver.as_ref(),
            )
            .resolve(&owner_id, &account_id, Utc::now().timestamp_millis())
            .map_err(|cause| cause.to_string())
            .and_then(|config| probe.verify(&config))
        };
        let mut account = store
            .account(&owner_id, &account_id)
            .map_err(ApplicationError::Repository)?
            .ok_or(ApplicationError::Domain {
                code: "ACCOUNT_NOT_FOUND",
                status: 404,
                message: "邮箱账户已被移除",
            })?;
        match validation {
            Ok(()) => {
                account.status = "connected".into();
                account.last_error = None;
            }
            Err(cause) => {
                account.status = "error".into();
                account.last_error = Some(redact_protocol_detail(&cause));
            }
        }
        store
            .upsert_account(&account)
            .map_err(ApplicationError::Repository)?;
        Ok(public_account(&account))
    })
    .await
}

fn provider_settings(
    provider: &str,
    custom: Option<MailSettingsInput>,
) -> Result<Value, ApplicationError<AuthStoreError>> {
    let value = match provider {
        "outlook" | "hotmail" => json_settings(
            "outlook.office365.com",
            993,
            true,
            "smtp.office365.com",
            587,
            false,
        ),
        "gmail" => json_settings("imap.gmail.com", 993, true, "smtp.gmail.com", 465, true),
        "qq" => json_settings("imap.qq.com", 993, true, "smtp.qq.com", 465, true),
        "yahoo" => json_settings(
            "imap.mail.yahoo.com",
            993,
            true,
            "smtp.mail.yahoo.com",
            465,
            true,
        ),
        "icloud" => json_settings(
            "imap.mail.me.com",
            993,
            true,
            "smtp.mail.me.com",
            587,
            false,
        ),
        "custom" => {
            let custom = custom.ok_or_else(invalid_create_request)?;
            if custom.imap_host.is_empty()
                || custom.imap_port == 0
                || custom.smtp_host.is_empty()
                || custom.smtp_port == 0
            {
                return Err(invalid_create_request());
            }
            json_settings(
                &custom.imap_host,
                custom.imap_port,
                custom.imap_secure,
                &custom.smtp_host,
                custom.smtp_port,
                custom.smtp_secure,
            )
        }
        _ => return Err(invalid_create_request()),
    };
    Ok(value)
}

fn json_settings(
    imap_host: &str,
    imap_port: u16,
    imap_secure: bool,
    smtp_host: &str,
    smtp_port: u16,
    smtp_secure: bool,
) -> Value {
    serde_json::json!({
        "imapHost": imap_host,
        "imapPort": imap_port,
        "imapSecure": imap_secure,
        "smtpHost": smtp_host,
        "smtpPort": smtp_port,
        "smtpSecure": smtp_secure,
    })
}

fn create_proxy(input: CreateProxyInput) -> Option<(Value, Option<String>)> {
    let host = input.host.trim();
    let username = input
        .username
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if !valid_create_proxy_host(host)
        || input.port == 0
        || username
            .as_ref()
            .is_some_and(|value| value.encode_utf16().count() > 256)
        || input
            .password
            .as_ref()
            .is_some_and(|value| value.encode_utf16().count() > 512)
    {
        return None;
    }
    let mut proxy = Map::from_iter([
        (
            "protocol".into(),
            Value::String(input.protocol.as_str().into()),
        ),
        ("host".into(), Value::String(host.into())),
        ("port".into(), serde_json::json!(input.port)),
    ]);
    if let Some(username) = username {
        proxy.insert("username".into(), Value::String(username));
    }
    Some((Value::Object(proxy), input.password))
}

fn valid_email(value: &str) -> bool {
    if value.is_empty()
        || value.encode_utf16().count() > 320
        || value.chars().any(char::is_whitespace)
    {
        return false;
    }
    let mut parts = value.split('@');
    let local = parts.next().unwrap_or_default();
    let domain = parts.next().unwrap_or_default();
    !local.is_empty()
        && !domain.is_empty()
        && parts.next().is_none()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
}

fn invalid_create_text(value: &str, maximum: usize) -> bool {
    value.trim().is_empty() || value.encode_utf16().count() > maximum
}

fn valid_account_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..].chars().all(|value| value.is_ascii_hexdigit())
}

fn valid_create_proxy_host(value: &str) -> bool {
    !value.is_empty()
        && value.encode_utf16().count() <= 253
        && !value.contains("://")
        && !value
            .chars()
            .any(|character| character.is_whitespace() || matches!(character, '/' | '?' | '#'))
}

fn invalid_create_request() -> ApplicationError<AuthStoreError> {
    ApplicationError::Domain {
        code: "ACCOUNT_CREATE_INVALID",
        status: 400,
        message: "请求参数无效",
    }
}

fn default_group() -> String {
    "个人".into()
}

fn default_group_icon() -> WorkspaceIconId {
    WorkspaceIconId::Folder
}

fn default_color() -> String {
    "#168f78".into()
}

async fn run<T>(
    database: PathBuf,
    operation: impl FnOnce(&mut SqliteAuthStore) -> Result<T, ApplicationError<AuthStoreError>>
        + Send
        + 'static,
) -> Result<T, ApplicationError<AuthStoreError>>
where
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(move || {
        let mut store =
            SqliteAuthStore::open_database(database).map_err(ApplicationError::Repository)?;
        operation(&mut store)
    })
    .await
    {
        Ok(result) => result,
        Err(_) => Err(ApplicationError::Domain {
            code: "AUTH_WORKER_STOPPED",
            status: 500,
            message: "服务暂时无法完成请求",
        }),
    }
}

async fn run_sensitive<T>(
    data_dir: PathBuf,
    operation: impl FnOnce(&mut SqliteAuthStore, &MasterKey) -> Result<T, ApplicationError<AuthStoreError>>
        + Send
        + 'static,
) -> Result<T, ApplicationError<AuthStoreError>>
where
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(move || {
        let master_key = MasterKey::from_file(data_dir.join("master.key")).map_err(|_| {
            ApplicationError::Domain {
                code: "MASTER_KEY_UNAVAILABLE",
                status: 500,
                message: "服务暂时无法完成请求",
            }
        })?;
        let mut store = SqliteAuthStore::open_database(data_dir.join("imail.sqlite"))
            .map_err(ApplicationError::Repository)?;
        operation(&mut store, &master_key)
    })
    .await
    {
        Ok(result) => result,
        Err(_) => Err(ApplicationError::Domain {
            code: "AUTH_WORKER_STOPPED",
            status: 500,
            message: "服务暂时无法完成请求",
        }),
    }
}

enum SyncHttpError {
    Application(ApplicationError<AuthStoreError>),
    Runtime(SyncRuntimeError),
}

impl From<ApplicationError<AuthStoreError>> for SyncHttpError {
    fn from(error: ApplicationError<AuthStoreError>) -> Self {
        Self::Application(error)
    }
}

async fn run_sync<T>(
    database: PathBuf,
    operation: impl FnOnce(&mut SqliteAuthStore, &mut SyncRuntimeStore) -> Result<T, SyncHttpError>
        + Send
        + 'static,
) -> Result<T, SyncHttpError>
where
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(move || {
        let mut auth = SqliteAuthStore::open_database(&database)
            .map_err(ApplicationError::Repository)
            .map_err(SyncHttpError::Application)?;
        let mut sync =
            SyncRuntimeStore::open_database(&database).map_err(SyncHttpError::Runtime)?;
        operation(&mut auth, &mut sync)
    })
    .await
    {
        Ok(result) => result,
        Err(_) => Err(SyncHttpError::Application(ApplicationError::Domain {
            code: "AUTH_WORKER_STOPPED",
            status: 500,
            message: "服务暂时无法完成请求",
        })),
    }
}

fn sync_error(cause: SyncHttpError) -> Response {
    match cause {
        SyncHttpError::Application(cause) => application_error(cause),
        SyncHttpError::Runtime(cause) => {
            eprintln!("[imail-http] sync runtime error: {cause}");
            error(StatusCode::INTERNAL_SERVER_ERROR, "服务暂时无法完成请求")
        }
    }
}

pub(crate) fn application_error(cause: ApplicationError<AuthStoreError>) -> Response {
    match cause {
        ApplicationError::Domain {
            status, message, ..
        } if status < 500 => error(
            StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_REQUEST),
            message,
        ),
        ApplicationError::Domain { .. } => {
            error(StatusCode::INTERNAL_SERVER_ERROR, "服务暂时无法完成请求")
        }
        ApplicationError::Repository(storage_error) => {
            eprintln!("[imail-http] accounts storage error: {storage_error}");
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
