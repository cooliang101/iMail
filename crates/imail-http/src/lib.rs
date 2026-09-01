use std::future::Future;
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fs::{self, OpenOptions},
    io::{self, Write},
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Request, State},
    http::{
        header::{
            ACCESS_CONTROL_ALLOW_CREDENTIALS, ACCESS_CONTROL_ALLOW_HEADERS,
            ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN, CACHE_CONTROL, HOST, ORIGIN,
            VARY,
        },
        HeaderName, HeaderValue, Method, StatusCode,
    },
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{any, get},
    Json, Router,
};
use imail_mail::{ImapPort, MailConnectionConfig, SmtpPort};
use imail_mail_network::NetworkMailAdapter;
use imail_oauth::{
    OAuthConfigResolver, OAuthEnvironment, OAuthProviderPort, OAuthProviderPortFactory,
    RefreshCoordinator, StandardOAuthConfigResolver,
};
use imail_oauth_http::OAuthHttpAdapter;
use imail_runtime::{
    EmbeddedSyncExecutor, NetworkSyncMailTransportFactory, PersistentSyncRuntime, SyncEventSignal,
    SyncMailTransportFactory, SyncWorkerConfig,
};
use serde::Serialize;
use url::Url;
use uuid::{Uuid, Version};

pub mod accounts;
mod apple_hme;
mod attachment_cache;
mod attachment_previews;
mod auth;
mod developer_tokens;
pub mod drafts;
mod external_access;
mod gateway;
pub mod logo;
mod mcp;
pub mod messages;
mod oauth;
pub mod outbox;
mod preferences;
pub mod rules;
pub mod search;
mod security;
mod sync_control;
mod system;
mod translation_providers;
pub mod translation_settings;
pub mod translations;
mod web_client;

const SERVICE_VERSION: &str = env!("CARGO_PKG_VERSION");
const PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeMode {
    Embedded,
    Http { host: IpAddr, port: u16 },
}

impl BridgeMode {
    pub fn parse<'a>(arguments: impl IntoIterator<Item = &'a str>) -> Result<Self, ConfigError> {
        let mut http = false;
        let mut host = None;
        let mut port = None;
        let mut values = arguments.into_iter();
        while let Some(argument) = values.next() {
            match argument {
                "--http" => http = true,
                "--host" => {
                    host = Some(
                        values
                            .next()
                            .ok_or(ConfigError::MissingValue("--host"))?
                            .parse()
                            .map_err(|_| ConfigError::InvalidHost)?,
                    );
                }
                "--port" => {
                    let parsed = values
                        .next()
                        .ok_or(ConfigError::MissingValue("--port"))?
                        .parse()
                        .map_err(|_| ConfigError::InvalidPort)?;
                    if parsed == 0 {
                        return Err(ConfigError::InvalidPort);
                    }
                    port = Some(parsed);
                }
                value => return Err(ConfigError::UnknownArgument(value.into())),
            }
        }
        if !http {
            if host.is_some() || port.is_some() {
                return Err(ConfigError::HttpOptionWithoutBridge);
            }
            return Ok(Self::Embedded);
        }
        Ok(Self::Http {
            host: host.unwrap_or(IpAddr::from([127, 0, 0, 1])),
            port: port.unwrap_or(8787),
        })
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("未知启动参数：{0}")]
    UnknownArgument(String),
    #[error("启动参数缺少值：{0}")]
    MissingValue(&'static str),
    #[error("HTTP host 必须是明确的 IP 地址")]
    InvalidHost,
    #[error("HTTP port 必须是 1..65535")]
    InvalidPort,
    #[error("--host/--port 只能与 --http 一起使用")]
    HttpOptionWithoutBridge,
    #[error("至少配置一个允许的 Host")]
    MissingAllowedHost,
    #[error("允许的 Host 无效：{0}")]
    InvalidAllowedHost(String),
    #[error("CORS Origin 无效：{0}")]
    InvalidCorsOrigin(String),
    #[error("Web 静态目录必须存在且包含 index.html")]
    InvalidWebRoot,
}

#[derive(Debug, thiserror::Error)]
pub enum HttpAdapterError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("iMail 服务实例身份文件无效")]
    InvalidInstanceId,
    #[error("无法读取或创建 iMail 服务实例身份：{0}")]
    InstanceIo(#[from] io::Error),
    #[error("HTTP listener 失败：{0}")]
    Listener(io::Error),
    #[error("HTTP 服务失败：{0}")]
    Serve(io::Error),
    #[error("同步运行时启动失败：{0}")]
    SyncRuntimeStart(String),
    #[error("同步运行时未能优雅关闭：{0:?}")]
    SyncRuntimeShutdown(Vec<String>),
    #[error("Apple HME 会话保活任务启动失败：{0}")]
    AppleHmeKeepaliveStart(String),
    #[error("Apple HME 会话保活任务未能优雅关闭")]
    AppleHmeKeepaliveShutdown,
    #[error("发件箱运行时启动失败：{0}")]
    OutboxRuntimeStart(String),
    #[error("发件箱运行时未能优雅关闭")]
    OutboxRuntimeShutdown,
}

#[derive(Clone)]
pub struct HttpAdapterConfig {
    pub data_dir: PathBuf,
    production: bool,
    pub allowed_hosts: BTreeSet<String>,
    pub cors_origins: BTreeSet<String>,
    web_client_root: Option<PathBuf>,
    pub gateway: bool,
    pub mcp: bool,
    sync_worker: bool,
    apple_hme_keepalive: bool,
    sync_runtime: SyncRuntimeOptions,
    registration_open: bool,
    secure_cookies: bool,
    trust_proxy_one_hop: bool,
    oauth_environment: OAuthEnvironment,
    oauth_config_resolver: Arc<dyn OAuthConfigResolver>,
    refresh_coordinator: Arc<RefreshCoordinator>,
    connection_probe: Arc<dyn MailConnectionProbe>,
    oauth_provider_factory: Arc<dyn OAuthProviderPortFactory>,
    mail_transport_factory: Arc<dyn MailTransportFactory>,
    sync_mail_transport_factory: Arc<dyn SyncMailTransportFactory>,
    logo_discovery: Arc<dyn logo::LogoDiscoveryPort>,
    oauth_frontend_origin: String,
    daemon_control_file: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct SyncRuntimeOptions {
    pub worker_count: usize,
    pub poll_interval: Duration,
    pub lease_duration: Duration,
    pub reconciliation_minutes: i64,
    pub host_name: String,
    pub idle_enabled: bool,
    pub watcher_reconcile_interval: Duration,
    pub watcher_maximum_wait: Duration,
    pub scheduler_interval: Duration,
    pub scheduler_startup_delay: Duration,
}

impl Default for SyncRuntimeOptions {
    fn default() -> Self {
        Self {
            worker_count: 3,
            poll_interval: Duration::from_secs(1),
            lease_duration: Duration::from_secs(120),
            reconciliation_minutes: 30,
            host_name: "local".into(),
            idle_enabled: true,
            watcher_reconcile_interval: Duration::from_secs(5),
            watcher_maximum_wait: Duration::from_secs(60),
            scheduler_interval: Duration::from_secs(5),
            scheduler_startup_delay: Duration::from_secs(1),
        }
    }
}

pub trait MailConnectionProbe: Send + Sync {
    fn verify(&self, config: &MailConnectionConfig) -> Result<(), String>;
}

impl<F> MailConnectionProbe for F
where
    F: Fn(&MailConnectionConfig) -> Result<(), String> + Send + Sync,
{
    fn verify(&self, config: &MailConnectionConfig) -> Result<(), String> {
        self(config)
    }
}

struct NetworkConnectionProbe;

impl MailConnectionProbe for NetworkConnectionProbe {
    fn verify(&self, config: &MailConnectionConfig) -> Result<(), String> {
        let mut adapter =
            NetworkMailAdapter::new().map_err(|_| "邮箱网络运行时不可用".to_string())?;
        ImapPort::verify(&mut adapter, config).map_err(|error| error.to_string())?;
        SmtpPort::verify(&mut adapter, config).map_err(|error| error.to_string())
    }
}

pub trait MailTransportFactory: Send + Sync {
    fn create_imap(&self) -> Result<Box<dyn ImapPort>, String>;
    fn create_smtp(&self) -> Result<Box<dyn SmtpPort>, String>;
}

struct NetworkMailTransportFactory;

impl MailTransportFactory for NetworkMailTransportFactory {
    fn create_imap(&self) -> Result<Box<dyn ImapPort>, String> {
        NetworkMailAdapter::new()
            .map(|adapter| Box::new(adapter) as Box<dyn ImapPort>)
            .map_err(|_| "邮箱网络运行时不可用".into())
    }

    fn create_smtp(&self) -> Result<Box<dyn SmtpPort>, String> {
        NetworkMailAdapter::new()
            .map(|adapter| Box::new(adapter) as Box<dyn SmtpPort>)
            .map_err(|_| "邮箱网络运行时不可用".into())
    }
}

struct HttpOAuthProviderFactory;

impl OAuthProviderPortFactory for HttpOAuthProviderFactory {
    fn create(&self) -> Box<dyn OAuthProviderPort> {
        Box::new(OAuthHttpAdapter::new())
    }
}

impl HttpAdapterConfig {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            production: false,
            allowed_hosts: ["localhost", "127.0.0.1", "::1"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            cors_origins: ["http://localhost:5173", "http://127.0.0.1:5173"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            web_client_root: None,
            gateway: false,
            mcp: false,
            sync_worker: false,
            apple_hme_keepalive: true,
            sync_runtime: SyncRuntimeOptions::default(),
            registration_open: true,
            secure_cookies: false,
            trust_proxy_one_hop: false,
            oauth_environment: OAuthEnvironment::default(),
            oauth_config_resolver: Arc::new(StandardOAuthConfigResolver),
            refresh_coordinator: Arc::new(RefreshCoordinator::default()),
            connection_probe: Arc::new(NetworkConnectionProbe),
            oauth_provider_factory: Arc::new(HttpOAuthProviderFactory),
            mail_transport_factory: Arc::new(NetworkMailTransportFactory),
            sync_mail_transport_factory: Arc::new(NetworkSyncMailTransportFactory),
            logo_discovery: Arc::new(logo::NetworkLogoDiscovery),
            oauth_frontend_origin: "http://localhost:5173".into(),
            daemon_control_file: None,
        }
    }

    pub fn production(data_dir: impl Into<PathBuf>) -> Self {
        let mut config = Self::new(data_dir);
        config.production = true;
        config.cors_origins.clear();
        config.registration_open = false;
        config.secure_cookies = true;
        config
    }

    pub fn with_registration_open(mut self, open: bool) -> Self {
        self.registration_open = open;
        self
    }

    pub fn with_sync_worker(mut self, enabled: bool) -> Self {
        self.sync_worker = enabled;
        self
    }

    pub fn with_apple_hme_keepalive(mut self, enabled: bool) -> Self {
        self.apple_hme_keepalive = enabled;
        self
    }

    pub fn with_sync_worker_options(mut self, options: SyncRuntimeOptions) -> Self {
        self.sync_runtime = options;
        self
    }

    pub fn with_trusted_proxy_one_hop(mut self, enabled: bool) -> Self {
        self.trust_proxy_one_hop = enabled;
        self
    }

    pub fn with_oauth_environment(mut self, environment: OAuthEnvironment) -> Self {
        self.oauth_environment = environment;
        self
    }

    pub fn with_connection_probe(mut self, probe: Arc<dyn MailConnectionProbe>) -> Self {
        self.connection_probe = probe;
        self
    }

    pub fn with_oauth_provider_factory(
        mut self,
        factory: Arc<dyn OAuthProviderPortFactory>,
    ) -> Self {
        self.oauth_provider_factory = factory;
        self
    }

    pub fn with_oauth_config_resolver(mut self, resolver: Arc<dyn OAuthConfigResolver>) -> Self {
        self.oauth_config_resolver = resolver;
        self
    }

    pub fn with_oauth_frontend_origin(mut self, origin: impl Into<String>) -> Self {
        self.oauth_frontend_origin = origin.into();
        self
    }

    pub fn with_mail_transport_factory(mut self, factory: Arc<dyn MailTransportFactory>) -> Self {
        self.mail_transport_factory = factory;
        self
    }

    pub fn with_sync_mail_transport_factory(
        mut self,
        factory: Arc<dyn SyncMailTransportFactory>,
    ) -> Self {
        self.sync_mail_transport_factory = factory;
        self
    }

    pub fn with_logo_discovery(mut self, discovery: Arc<dyn logo::LogoDiscoveryPort>) -> Self {
        self.logo_discovery = discovery;
        self
    }

    pub fn with_web_client_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.web_client_root = Some(root.into());
        self
    }

    pub fn with_daemon_control_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.daemon_control_file = Some(path.into());
        self
    }

    fn validate(mut self) -> Result<Self, ConfigError> {
        self.allowed_hosts = self
            .allowed_hosts
            .into_iter()
            .map(|host| normalize_allowed_host(&host))
            .collect::<Result<_, _>>()?;
        if self.allowed_hosts.is_empty() {
            return Err(ConfigError::MissingAllowedHost);
        }
        self.cors_origins = self
            .cors_origins
            .into_iter()
            .map(|origin| normalize_origin(&origin))
            .collect::<Result<_, _>>()?;
        self.oauth_frontend_origin = normalize_origin(&self.oauth_frontend_origin)?;
        if let Some(root) = self.web_client_root.take() {
            let root = root
                .canonicalize()
                .map_err(|_| ConfigError::InvalidWebRoot)?;
            if !root.is_dir() || !root.join("index.html").is_file() {
                return Err(ConfigError::InvalidWebRoot);
            }
            self.web_client_root = Some(root);
        }
        Ok(self)
    }
}

struct AppState {
    config: HttpAdapterConfig,
    info: ServiceInfo,
    shutdown: tokio::sync::broadcast::Sender<()>,
    refresh_coordinator: Arc<RefreshCoordinator>,
    completed_oauth: Mutex<HashMap<String, CompletedOAuthRecord>>,
    oauth_in_flight: Mutex<HashSet<String>>,
    logo_in_flight: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    attachment_previews: attachment_previews::PreviewState,
    security: security::SecurityState,
    apple_hme_pending: Arc<imail_apple_hme::MemoryPendingLoginStore>,
    apple_hme_pending_owners: Mutex<HashMap<String, AppleHmePendingOwner>>,
    daemon_shutdown: Option<tokio::sync::mpsc::Sender<()>>,
}

struct AppleHmePendingOwner {
    owner_id: String,
    account_id: String,
    expires_at_ms: i64,
}

#[derive(Clone)]
struct CompletedOAuthRecord {
    owner_id: String,
    completed_at_ms: i64,
    outcome: OAuthCompletionOutcome,
}

#[derive(Clone)]
enum OAuthCompletionOutcome {
    Success { account_id: String },
    Failed { message: String },
}

#[derive(Debug, Clone)]
pub struct EmbeddedOperationError {
    pub status: u16,
    pub message: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceInfo {
    service: &'static str,
    instance_id: String,
    version: &'static str,
    protocol_version: u16,
    capabilities: ServiceCapabilities,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceCapabilities {
    gateway: bool,
    mcp: bool,
    sync_worker: bool,
    web_client: bool,
}

#[derive(Serialize)]
struct Health {
    ok: bool,
    service: &'static str,
}

#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
}

pub fn build_router(config: HttpAdapterConfig) -> Result<Router, HttpAdapterError> {
    build_router_state(config, None).map(|(router, _, _)| router)
}

fn build_router_with_shutdown(config: HttpAdapterConfig) -> Result<HostedRouter, HttpAdapterError> {
    let (daemon_sender, daemon_receiver) = if config.daemon_control_file.is_some() {
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        (Some(sender), Some(receiver))
    } else {
        (None, None)
    };
    let data_dir = config.data_dir.clone();
    let apple_hme_keepalive_enabled = config.apple_hme_keepalive;
    let (router, connections, state) = build_router_state(config, daemon_sender)?;
    let apple_hme_keepalive = apple_hme_keepalive_enabled
        .then(|| apple_hme::AppleHmeKeepaliveRuntime::start(data_dir))
        .transpose()
        .map_err(|error| HttpAdapterError::AppleHmeKeepaliveStart(error.to_string()))?;
    let outbox_runtime = outbox::OutboxRuntime::start(state)
        .map_err(|error| HttpAdapterError::OutboxRuntimeStart(error.to_string()))?;
    Ok(HostedRouter {
        router,
        connection_shutdown: connections,
        daemon_shutdown: daemon_receiver,
        apple_hme_keepalive,
        outbox_runtime: Some(outbox_runtime),
    })
}

struct HostedRouter {
    router: Router,
    connection_shutdown: tokio::sync::broadcast::Sender<()>,
    daemon_shutdown: Option<tokio::sync::mpsc::Receiver<()>>,
    apple_hme_keepalive: Option<apple_hme::AppleHmeKeepaliveRuntime>,
    outbox_runtime: Option<outbox::OutboxRuntime>,
}

/// In-process service host for desktop and other native callers.
///
/// This owns the same router and persistent sync runtime as the HTTP server but
/// never creates or binds a TCP listener.
pub struct EmbeddedServiceHost {
    router: Router,
    state: Arc<AppState>,
    connection_shutdown: tokio::sync::broadcast::Sender<()>,
    sync_runtime: std::sync::Mutex<Option<PersistentSyncRuntime>>,
    apple_hme_keepalive: std::sync::Mutex<Option<apple_hme::AppleHmeKeepaliveRuntime>>,
    outbox_runtime: std::sync::Mutex<Option<outbox::OutboxRuntime>>,
    sync_event_signal: SyncEventSignal,
}

impl EmbeddedServiceHost {
    pub fn start(config: HttpAdapterConfig) -> Result<Self, HttpAdapterError> {
        let (router, connection_shutdown, state) = build_router_state(config.clone(), None)?;
        let sync_event_signal = SyncEventSignal::default();
        let sync_runtime = start_sync_runtime(&config, Some(sync_event_signal.clone()))?;
        let apple_hme_keepalive = config
            .apple_hme_keepalive
            .then(|| apple_hme::AppleHmeKeepaliveRuntime::start(config.data_dir.clone()))
            .transpose()
            .map_err(|error| HttpAdapterError::AppleHmeKeepaliveStart(error.to_string()))?;
        let outbox_runtime = outbox::OutboxRuntime::start(Arc::clone(&state))
            .map_err(|error| HttpAdapterError::OutboxRuntimeStart(error.to_string()))?;
        Ok(Self {
            router,
            state,
            connection_shutdown,
            sync_runtime: std::sync::Mutex::new(sync_runtime),
            apple_hme_keepalive: std::sync::Mutex::new(apple_hme_keepalive),
            outbox_runtime: std::sync::Mutex::new(Some(outbox_runtime)),
            sync_event_signal,
        })
    }

    pub fn router(&self) -> Router {
        self.router.clone()
    }

    pub fn service_info(&self) -> serde_json::Value {
        serde_json::to_value(&self.state.info).expect("service info serializes")
    }

    pub fn providers(&self) -> serde_json::Value {
        system::embedded_providers(&self.state)
    }

    pub async fn apple_hme_status(
        &self,
        owner_id: String,
        account_id: String,
    ) -> Result<serde_json::Value, EmbeddedOperationError> {
        apple_hme::embedded_status(Arc::clone(&self.state), owner_id, account_id).await
    }

    pub async fn apple_hme_start_login(
        &self,
        owner_id: String,
        actor: String,
        account_id: String,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, EmbeddedOperationError> {
        apple_hme::embedded_start_login(Arc::clone(&self.state), owner_id, actor, account_id, input)
            .await
    }

    pub async fn apple_hme_submit_two_factor(
        &self,
        owner_id: String,
        actor: String,
        account_id: String,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, EmbeddedOperationError> {
        apple_hme::embedded_submit_two_factor(
            Arc::clone(&self.state),
            owner_id,
            actor,
            account_id,
            input,
        )
        .await
    }

    pub async fn apple_hme_list(
        &self,
        owner_id: String,
        account_id: String,
    ) -> Result<serde_json::Value, EmbeddedOperationError> {
        apple_hme::embedded_list(Arc::clone(&self.state), owner_id, account_id).await
    }

    pub async fn apple_hme_sync(
        &self,
        owner_id: String,
        account_id: String,
    ) -> Result<serde_json::Value, EmbeddedOperationError> {
        apple_hme::embedded_sync(Arc::clone(&self.state), owner_id, account_id).await
    }

    pub async fn apple_hme_create(
        &self,
        owner_id: String,
        account_id: String,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, EmbeddedOperationError> {
        apple_hme::embedded_create(Arc::clone(&self.state), owner_id, account_id, input).await
    }

    pub async fn apple_hme_deactivate(
        &self,
        owner_id: String,
        account_id: String,
        anonymous_id: String,
    ) -> Result<serde_json::Value, EmbeddedOperationError> {
        apple_hme::embedded_deactivate(Arc::clone(&self.state), owner_id, account_id, anonymous_id)
            .await
    }

    pub async fn apple_hme_delete(
        &self,
        owner_id: String,
        account_id: String,
        anonymous_id: String,
    ) -> Result<serde_json::Value, EmbeddedOperationError> {
        apple_hme::embedded_delete(Arc::clone(&self.state), owner_id, account_id, anonymous_id)
            .await
    }

    pub async fn apple_hme_disconnect(
        &self,
        owner_id: String,
        account_id: String,
    ) -> Result<serde_json::Value, EmbeddedOperationError> {
        apple_hme::embedded_disconnect(Arc::clone(&self.state), owner_id, account_id).await
    }

    pub fn sync_event_signal(&self) -> SyncEventSignal {
        self.sync_event_signal.clone()
    }

    pub async fn update_account_credential(
        &self,
        user_id: String,
        actor: String,
        account_id: String,
        password: String,
    ) -> Result<
        imail_protocol::AccountReadModel,
        imail_core::ApplicationError<imail_storage_sqlite::AuthStoreError>,
    > {
        accounts::update_credential_application(
            Arc::clone(&self.state),
            user_id,
            actor,
            account_id,
            password,
        )
        .await
    }

    pub async fn create_account(
        &self,
        user_id: String,
        actor: String,
        input: serde_json::Value,
    ) -> Result<
        imail_protocol::AccountReadModel,
        imail_core::ApplicationError<imail_storage_sqlite::AuthStoreError>,
    > {
        accounts::create_account_value_application(Arc::clone(&self.state), user_id, actor, input)
            .await
    }

    pub async fn update_account_proxy(
        &self,
        user_id: String,
        actor: String,
        account_id: String,
        input: imail_protocol::AccountProxyUpdate,
    ) -> Result<
        imail_protocol::AccountReadModel,
        imail_core::ApplicationError<imail_storage_sqlite::AuthStoreError>,
    > {
        accounts::update_proxy_application(
            Arc::clone(&self.state),
            user_id,
            actor,
            account_id,
            input,
        )
        .await
    }

    pub async fn test_account_connection(
        &self,
        user_id: String,
        account_id: String,
    ) -> Result<
        imail_protocol::AccountReadModel,
        imail_core::ApplicationError<imail_storage_sqlite::AuthStoreError>,
    > {
        accounts::test_connection_application(Arc::clone(&self.state), user_id, account_id).await
    }

    pub async fn start_oauth(
        &self,
        owner_id: String,
        actor: String,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, EmbeddedOperationError> {
        oauth::embedded_start(Arc::clone(&self.state), owner_id, actor, input).await
    }

    pub async fn reconnect_oauth(
        &self,
        owner_id: String,
        actor: String,
        account_id: String,
    ) -> Result<serde_json::Value, EmbeddedOperationError> {
        oauth::embedded_reconnect(Arc::clone(&self.state), owner_id, actor, account_id).await
    }

    pub async fn oauth_status(
        &self,
        owner_id: String,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, EmbeddedOperationError> {
        oauth::embedded_status(Arc::clone(&self.state), owner_id, input).await
    }

    pub async fn update_message(
        &self,
        owner_id: String,
        message_id: String,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, EmbeddedOperationError> {
        messages::embedded_update(Arc::clone(&self.state), owner_id, message_id, input).await
    }

    pub async fn move_message(
        &self,
        owner_id: String,
        message_id: String,
        destination: String,
    ) -> Result<serde_json::Value, EmbeddedOperationError> {
        messages::embedded_move(
            Arc::clone(&self.state),
            owner_id,
            message_id,
            serde_json::json!({"destination":destination}),
        )
        .await
    }

    pub async fn send_message(
        &self,
        owner_id: String,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, EmbeddedOperationError> {
        messages::embedded_send(Arc::clone(&self.state), owner_id, input).await
    }

    pub async fn outbox_operation(
        &self,
        owner_id: String,
        operation: &'static str,
        id: Option<String>,
        input: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, EmbeddedOperationError> {
        let database = self.state.config.data_dir.join("imail.sqlite");
        tokio::task::spawn_blocking(move || {
            let mut store = imail_storage_sqlite::SqliteAuthStore::open_database(database)
                .map_err(|_| EmbeddedOperationError {
                    status: 500,
                    message: "发件箱暂时不可用".into(),
                })?;
            outbox::execute(&mut store, &owner_id, operation, id.as_deref(), input)
        })
        .await
        .map_err(|_| EmbeddedOperationError {
            status: 500,
            message: "发件箱任务中断".into(),
        })?
    }

    pub async fn sync_account(
        &self,
        owner_id: String,
        account_id: String,
        role: String,
        mailbox: Option<String>,
    ) -> Result<serde_json::Value, EmbeddedOperationError> {
        accounts::embedded_queue_one(Arc::clone(&self.state), owner_id, account_id, role, mailbox)
            .await
    }

    pub async fn sync_all(
        &self,
        owner_id: String,
        role: String,
    ) -> Result<serde_json::Value, EmbeddedOperationError> {
        accounts::embedded_queue_all(Arc::clone(&self.state), owner_id, role).await
    }

    pub async fn prepare_authorization_export(
        &self,
        owner_id: String,
        actor: String,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, EmbeddedOperationError> {
        security::embedded_prepare_export(Arc::clone(&self.state), owner_id, actor, input).await
    }

    pub async fn download_authorization_export(
        &self,
        owner_id: String,
        actor: String,
        id: String,
    ) -> Result<Vec<u8>, EmbeddedOperationError> {
        security::embedded_download_export(Arc::clone(&self.state), owner_id, actor, id).await
    }

    pub async fn clear_user_data(
        &self,
        owner_id: String,
        actor: String,
        input: serde_json::Value,
    ) -> Result<(), EmbeddedOperationError> {
        security::embedded_clear_user_data(Arc::clone(&self.state), owner_id, actor, input).await
    }

    pub async fn sync_status(
        &self,
        owner_id: String,
    ) -> Result<serde_json::Value, EmbeddedOperationError> {
        sync_control::embedded_sync_status(Arc::clone(&self.state), owner_id).await
    }

    pub async fn download_attachment(
        &self,
        owner_id: String,
        message_id: String,
        index: usize,
    ) -> Result<Vec<u8>, EmbeddedOperationError> {
        messages::embedded_download_attachment(Arc::clone(&self.state), owner_id, message_id, index)
            .await
    }

    pub async fn create_attachment_preview(
        &self,
        owner_id: String,
        message_id: String,
        index: usize,
    ) -> Result<serde_json::Value, EmbeddedOperationError> {
        attachment_previews::embedded_create(Arc::clone(&self.state), owner_id, message_id, index)
            .await
    }

    pub async fn read_attachment_preview(
        &self,
        owner_id: String,
        preview_id: String,
        entry_id: Option<String>,
    ) -> Result<Vec<u8>, EmbeddedOperationError> {
        let state = Arc::clone(&self.state);
        tokio::task::spawn_blocking(move || {
            attachment_previews::embedded_content(
                &state,
                &owner_id,
                &preview_id,
                entry_id.as_deref(),
            )
        })
        .await
        .map_err(|_| EmbeddedOperationError {
            status: 500,
            message: "附件预览处理失败".into(),
        })?
    }

    pub fn delete_attachment_preview(
        &self,
        owner_id: String,
        preview_id: String,
    ) -> Result<(), EmbeddedOperationError> {
        attachment_previews::embedded_delete(&self.state, &owner_id, &preview_id)
    }

    pub async fn contact_logo(
        &self,
        owner_id: String,
        address: String,
    ) -> Result<Option<Vec<u8>>, EmbeddedOperationError> {
        messages::embedded_contact_logo(Arc::clone(&self.state), owner_id, address)
            .await
            .map(|logo| logo.map(|(_, content)| content))
    }

    pub fn shutdown(&self, maximum_wait: Duration) -> Result<(), HttpAdapterError> {
        let _ = self.connection_shutdown.send(());
        let mut outbox = self
            .outbox_runtime
            .lock()
            .map_err(|_| HttpAdapterError::OutboxRuntimeShutdown)?;
        let outbox_graceful = !outbox
            .as_mut()
            .is_some_and(|runtime| !runtime.shutdown(maximum_wait));
        if outbox_graceful {
            *outbox = None;
        }
        drop(outbox);
        let mut keepalive = self
            .apple_hme_keepalive
            .lock()
            .map_err(|_| HttpAdapterError::AppleHmeKeepaliveShutdown)?;
        let keepalive_graceful = !keepalive
            .as_mut()
            .is_some_and(|runtime| !runtime.shutdown(maximum_wait));
        if keepalive_graceful {
            *keepalive = None;
        }
        drop(keepalive);
        let mut runtime = self.sync_runtime.lock().map_err(|_| {
            HttpAdapterError::SyncRuntimeShutdown(vec!["runtime lock poisoned".into()])
        })?;
        if let Some(runtime) = runtime.as_mut() {
            let report = runtime.shutdown(maximum_wait);
            if !report.graceful() {
                return Err(HttpAdapterError::SyncRuntimeShutdown(report.timed_out));
            }
        }
        *runtime = None;
        if !keepalive_graceful {
            return Err(HttpAdapterError::AppleHmeKeepaliveShutdown);
        }
        if !outbox_graceful {
            return Err(HttpAdapterError::OutboxRuntimeShutdown);
        }
        Ok(())
    }
}

impl Drop for EmbeddedServiceHost {
    fn drop(&mut self) {
        let _ = self.shutdown(Duration::from_secs(10));
    }
}

fn build_router_state(
    config: HttpAdapterConfig,
    daemon_shutdown: Option<tokio::sync::mpsc::Sender<()>>,
) -> Result<(Router, tokio::sync::broadcast::Sender<()>, Arc<AppState>), HttpAdapterError> {
    let config = config.validate()?;
    let (shutdown, _) = tokio::sync::broadcast::channel(1);
    let state = Arc::new(AppState {
        info: ServiceInfo {
            service: "imail",
            instance_id: load_or_create_instance_id(&config.data_dir)?,
            version: SERVICE_VERSION,
            protocol_version: PROTOCOL_VERSION,
            capabilities: ServiceCapabilities {
                gateway: config.gateway,
                mcp: config.mcp,
                sync_worker: config.sync_worker,
                web_client: config.web_client_root.is_some(),
            },
        },
        shutdown: shutdown.clone(),
        refresh_coordinator: Arc::clone(&config.refresh_coordinator),
        config,
        completed_oauth: Mutex::new(HashMap::new()),
        oauth_in_flight: Mutex::new(HashSet::new()),
        logo_in_flight: Mutex::new(HashMap::new()),
        attachment_previews: attachment_previews::PreviewState::default(),
        security: security::SecurityState::default(),
        apple_hme_pending: Arc::new(imail_apple_hme::MemoryPendingLoginStore::default()),
        apple_hme_pending_owners: Mutex::new(HashMap::new()),
        daemon_shutdown,
    });
    let protected = Router::new()
        .merge(accounts::routes())
        .merge(apple_hme::routes())
        .merge(attachment_previews::routes())
        .merge(drafts::routes())
        .merge(developer_tokens::routes())
        .merge(external_access::routes())
        .merge(messages::routes())
        .merge(oauth::protected_routes())
        .merge(outbox::routes())
        .merge(preferences::routes())
        .merge(search::routes())
        .merge(rules::routes())
        .merge(security::routes())
        .merge(sync_control::routes())
        .merge(system::protected_routes())
        .merge(translation_settings::routes())
        .merge(translations::routes())
        .route("/api/*path", any(api_not_found))
        .route_layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            auth::require_session,
        ));
    let mut router = Router::new()
        .route("/api/system/info", get(system_info))
        .route("/api/health", get(health))
        .merge(system::public_routes())
        .merge(auth::routes())
        .merge(oauth::public_routes())
        .merge(protected);
    if state.config.gateway {
        router = router
            .nest("/gateway/v1", gateway::routes())
            .nest("/gateway", gateway::docs_routes());
    }
    if state.config.mcp {
        router = router.merge(mcp::routes());
    }
    if state.config.web_client_root.is_some() {
        router = router.fallback(web_client::serve);
    }
    let embedded_state = Arc::clone(&state);
    Ok((
        router
            .with_state(Arc::clone(&state))
            .layer(DefaultBodyLimit::max(25 * 1024 * 1024))
            .layer(middleware::from_fn_with_state(state, security_boundary)),
        shutdown,
        embedded_state,
    ))
}

pub async fn serve(
    config: HttpAdapterConfig,
    host: IpAddr,
    port: u16,
) -> Result<(), HttpAdapterError> {
    serve_with_shutdown(config, host, port, std::future::pending()).await
}

pub async fn serve_with_shutdown(
    config: HttpAdapterConfig,
    host: IpAddr,
    port: u16,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), HttpAdapterError> {
    let listener = tokio::net::TcpListener::bind((host, port))
        .await
        .map_err(HttpAdapterError::Listener)?;
    let HostedRouter {
        router,
        connection_shutdown: shutdown_connections,
        mut daemon_shutdown,
        mut apple_hme_keepalive,
        mut outbox_runtime,
    } = build_router_with_shutdown(config.clone())?;
    let mut sync_runtime = start_sync_runtime(&config, None)?;
    let notify_connections = shutdown_connections.clone();
    let result = axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        tokio::select! {
            _ = shutdown => {},
            _ = async {
                match daemon_shutdown.as_mut() {
                    Some(receiver) => { receiver.recv().await; }
                    None => std::future::pending::<()>().await,
                }
            } => {},
        }
        let _ = notify_connections.send(());
    })
    .await
    .map_err(HttpAdapterError::Serve);
    let keepalive_graceful = apple_hme_keepalive
        .as_mut()
        .map_or(true, |runtime| runtime.shutdown(Duration::from_secs(10)));
    let outbox_graceful = outbox_runtime
        .as_mut()
        .map_or(true, |runtime| runtime.shutdown(Duration::from_secs(10)));
    if let Some(runtime) = &mut sync_runtime {
        let report = runtime.shutdown(Duration::from_secs(10));
        if !report.graceful() {
            return Err(HttpAdapterError::SyncRuntimeShutdown(report.timed_out));
        }
    }
    if !keepalive_graceful {
        return Err(HttpAdapterError::AppleHmeKeepaliveShutdown);
    }
    if !outbox_graceful {
        return Err(HttpAdapterError::OutboxRuntimeShutdown);
    }
    result
}

fn start_sync_runtime(
    config: &HttpAdapterConfig,
    event_signal: Option<SyncEventSignal>,
) -> Result<Option<PersistentSyncRuntime>, HttpAdapterError> {
    if !config.sync_worker {
        return Ok(None);
    }
    let executor = Arc::new(
        EmbeddedSyncExecutor::open_data_dir_with_dependencies(
            &config.data_dir,
            config.oauth_environment.clone(),
            Arc::clone(&config.refresh_coordinator),
            Arc::clone(&config.sync_mail_transport_factory),
            Arc::clone(&config.oauth_provider_factory),
            Arc::clone(&config.oauth_config_resolver),
        )
        .map_err(|error| HttpAdapterError::SyncRuntimeStart(error.to_string()))?,
    );
    let mut runtime_config = SyncWorkerConfig::new(config.data_dir.join("imail.sqlite"));
    runtime_config.worker_count = config.sync_runtime.worker_count;
    runtime_config.poll_interval = config.sync_runtime.poll_interval;
    runtime_config.lease_duration = config.sync_runtime.lease_duration;
    runtime_config.reconciliation_minutes = config.sync_runtime.reconciliation_minutes;
    runtime_config.host_name = config.sync_runtime.host_name.clone();
    runtime_config.watcher_reconcile_interval = config.sync_runtime.watcher_reconcile_interval;
    runtime_config.watcher_maximum_wait = config.sync_runtime.watcher_maximum_wait;
    runtime_config.scheduler_interval = config.sync_runtime.scheduler_interval;
    runtime_config.scheduler_startup_delay = config.sync_runtime.scheduler_startup_delay;
    runtime_config.event_signal = event_signal;
    let runtime = if config.sync_runtime.idle_enabled {
        PersistentSyncRuntime::start_with_watchers(runtime_config, executor.clone(), executor)
    } else {
        PersistentSyncRuntime::start(runtime_config, executor)
    };
    runtime
        .map(Some)
        .map_err(|error| HttpAdapterError::SyncRuntimeStart(error.to_string()))
}

async fn system_info(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    ([(CACHE_CONTROL, "no-store")], Json(state.info.clone()))
}

async fn health() -> Json<Health> {
    Json(Health {
        ok: true,
        service: "imail",
    })
}

async fn api_not_found() -> StatusCode {
    StatusCode::NOT_FOUND
}

async fn security_boundary(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    if state.config.trust_proxy_one_hop && !valid_forwarded_headers(request.headers()) {
        return secured_response(
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorBody {
                    error: "代理转发头无效",
                }),
            )
                .into_response(),
            false,
        );
    }
    let forwarded_https = state.config.trust_proxy_one_hop
        && request
            .headers()
            .get("x-forwarded-proto")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .is_some_and(|value| value.trim() == "https");
    let host = request
        .headers()
        .get(HOST)
        .and_then(|value| value.to_str().ok());
    if state.config.production && !host_allowed(host, &state.config.allowed_hosts) {
        return secured_response(
            (
                StatusCode::MISDIRECTED_REQUEST,
                Json(ErrorBody {
                    error: "请求主机名不在服务允许列表中",
                }),
            )
                .into_response(),
            forwarded_https,
        );
    }

    let origin = request
        .headers()
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok());
    let allowed_origin = origin.and_then(|value| {
        normalize_origin(value)
            .ok()
            .filter(|normalized| state.config.cors_origins.contains(normalized))
    });
    if request.method() == Method::OPTIONS && allowed_origin.is_some() {
        return with_cors(
            secured_response(StatusCode::NO_CONTENT.into_response(), forwarded_https),
            allowed_origin,
        );
    }
    with_cors(
        secured_response(next.run(request).await, forwarded_https),
        allowed_origin,
    )
}

fn valid_forwarded_headers(headers: &axum::http::HeaderMap) -> bool {
    let forwarded_for = headers
        .get("x-forwarded-for")
        .map(|value| {
            value
                .to_str()
                .ok()
                .and_then(|value| value.rsplit(',').next())
                .map(str::trim)
                .is_some_and(|value| !value.is_empty() && value.parse::<IpAddr>().is_ok())
        })
        .unwrap_or(true);
    let forwarded_proto = headers
        .get("x-forwarded-proto")
        .map(|value| {
            value
                .to_str()
                .ok()
                .and_then(|value| value.split(',').next())
                .map(str::trim)
                .is_some_and(|value| matches!(value, "http" | "https"))
        })
        .unwrap_or(true);
    forwarded_for && forwarded_proto
}

fn secured_response(mut response: Response<Body>, secure: bool) -> Response<Body> {
    let headers = response.headers_mut();
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    if secure {
        headers.insert(
            HeaderName::from_static("strict-transport-security"),
            HeaderValue::from_static("max-age=31536000"),
        );
    }
    headers.insert(
        HeaderName::from_static("x-frame-options"),
        HeaderValue::from_static("DENY"),
    );
    headers.insert(
        HeaderName::from_static("referrer-policy"),
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    response
}

fn with_cors(mut response: Response<Body>, origin: Option<String>) -> Response<Body> {
    if let Some(origin) = origin.and_then(|value| HeaderValue::from_str(&value).ok()) {
        let headers = response.headers_mut();
        headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin);
        headers.insert(
            ACCESS_CONTROL_ALLOW_CREDENTIALS,
            HeaderValue::from_static("true"),
        );
        headers.insert(VARY, HeaderValue::from_static("Origin"));
        headers.insert(
            ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET,POST,PUT,PATCH,DELETE,OPTIONS"),
        );
        headers.insert(
            ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("Authorization,Content-Type"),
        );
    }
    response
}

fn normalize_allowed_host(value: &str) -> Result<String, ConfigError> {
    let value = value.trim().trim_start_matches('[').trim_end_matches(']');
    if value.is_empty()
        || value.contains('/')
        || value.contains('@')
        || value.contains(char::is_whitespace)
    {
        return Err(ConfigError::InvalidAllowedHost(value.into()));
    }
    if value.contains(':') && IpAddr::from_str(value).is_err() {
        return Err(ConfigError::InvalidAllowedHost(value.into()));
    }
    Ok(value.to_ascii_lowercase())
}

fn normalize_origin(value: &str) -> Result<String, ConfigError> {
    let parsed = Url::parse(value).map_err(|_| ConfigError::InvalidCorsOrigin(value.into()))?;
    let loopback = parsed.host_str().map(is_loopback_host).unwrap_or_default();
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.username() != ""
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.path() != "/"
        || parsed.host_str().is_none()
        || (parsed.scheme() == "http" && !loopback)
    {
        return Err(ConfigError::InvalidCorsOrigin(value.into()));
    }
    Ok(parsed.origin().ascii_serialization())
}

fn host_allowed(value: Option<&str>, allowed: &BTreeSet<String>) -> bool {
    value
        .and_then(|value| value.parse::<axum::http::uri::Authority>().ok())
        .map(|authority| {
            authority
                .host()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_ascii_lowercase()
        })
        .is_some_and(|host| allowed.contains(&host))
}

fn is_loopback_host(value: &str) -> bool {
    value.eq_ignore_ascii_case("localhost")
        || value
            .parse::<IpAddr>()
            .map(|address| address.is_loopback())
            .unwrap_or(false)
}

fn load_or_create_instance_id(data_dir: &Path) -> Result<String, HttpAdapterError> {
    let file = data_dir.join("instance-id");
    match fs::read_to_string(&file) {
        Ok(value) => validate_instance_id(value.trim()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(data_dir)?;
            let created = Uuid::new_v4().to_string();
            match OpenOptions::new().write(true).create_new(true).open(&file) {
                Ok(mut output) => {
                    output.write_all(created.as_bytes())?;
                    output.write_all(b"\n")?;
                    output.sync_all()?;
                    Ok(created)
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    validate_instance_id(fs::read_to_string(file)?.trim())
                }
                Err(error) => Err(error.into()),
            }
        }
        Err(error) => Err(error.into()),
    }
}

fn validate_instance_id(value: &str) -> Result<String, HttpAdapterError> {
    let parsed = Uuid::parse_str(value).map_err(|_| HttpAdapterError::InvalidInstanceId)?;
    if parsed.get_version() != Some(Version::Random) {
        return Err(HttpAdapterError::InvalidInstanceId);
    }
    Ok(parsed.to_string())
}

#[cfg(test)]
mod tests {
    mod real_mail_fixture;
    #[path = "../rules_http_tests.rs"]
    mod rules_http_tests;
    #[path = "../search_http_tests.rs"]
    mod search_http_tests;

    use super::*;
    use axum::{
        body::to_bytes,
        http::{
            header::{CONTENT_DISPOSITION, CONTENT_TYPE},
            Request,
        },
    };
    use futures_util::StreamExt;
    use imail_core::{
        AccountRecord, AccountRepository, AuthRepository, ContentRepository,
        DeveloperTokenRepository,
    };
    use imail_mail::{
        MailAuthentication, MailConnectionConfig, OutgoingMessage, ProtocolFailure, ProtocolStage,
        RemoteMailbox, RemoteMessageLocator, RemoteMoveConfirmation,
    };
    use imail_oauth::{OAuthConfig, OAuthError, OAuthGrant, OAuthIdentity, OAuthTokenResponse};
    use imail_security::{decrypt_portable_export, MasterKey, PortableEncryptedPayload};
    use imail_storage_sqlite::{
        migrate_database, AppleHmeAddressRecord, SqliteAuthStore, SyncEnqueue, SyncRuntimeStore,
    };
    use serde_json::{json, Value};
    use tower::ServiceExt;

    #[derive(Default)]
    struct MailTestState {
        flag_updates: usize,
        moves: usize,
        fetches: usize,
        sends: usize,
        last_sent: Option<OutgoingMessage>,
        reject_flags: bool,
    }

    struct TestMailTransportFactory {
        state: Arc<Mutex<MailTestState>>,
        source: Vec<u8>,
    }

    struct TestLogoDiscovery {
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl logo::LogoDiscoveryPort for TestLogoDiscovery {
        fn discover(
            &self,
            source: &logo::LogoSource,
            _: &[imail_core::LogoFetchAttemptRecord],
        ) -> logo::LogoDiscoveryReport {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            assert_eq!(source.address, "sender@example.org");
            logo::LogoDiscoveryReport {
                result: Some(logo::DiscoveredLogo {
                    content: vec![137, 80, 78, 71, 13, 10, 26, 10, 1],
                    content_type: "image/png".into(),
                    source_url: "https://example.org/favicon.png".into(),
                    fetched_at: "2026-08-10T09:00:00.000Z".into(),
                    key: "domain:example.org".into(),
                }),
                permanent_failure: false,
                attempts: vec![logo::LogoAttempt {
                    target: "https://example.org".into(),
                    domain_key: "domain:example.org".into(),
                    status: "success".into(),
                    detail: "https://example.org/favicon.png".into(),
                    attempted_at: "2026-08-10T09:00:00.000Z".into(),
                }],
            }
        }
    }

    impl MailTransportFactory for TestMailTransportFactory {
        fn create_imap(&self) -> Result<Box<dyn ImapPort>, String> {
            Ok(Box::new(TestImap {
                state: Arc::clone(&self.state),
                source: self.source.clone(),
            }))
        }

        fn create_smtp(&self) -> Result<Box<dyn SmtpPort>, String> {
            Ok(Box::new(TestSmtp {
                state: Arc::clone(&self.state),
            }))
        }
    }

    struct TestImap {
        state: Arc<Mutex<MailTestState>>,
        source: Vec<u8>,
    }

    impl ImapPort for TestImap {
        fn verify(&mut self, _: &MailConnectionConfig) -> Result<(), ProtocolFailure> {
            Ok(())
        }

        fn fetch_source(
            &mut self,
            _: &MailConnectionConfig,
            _: &RemoteMessageLocator,
        ) -> Result<Vec<u8>, ProtocolFailure> {
            self.state.lock().unwrap().fetches += 1;
            Ok(self.source.clone())
        }

        fn update_flags(
            &mut self,
            _: &MailConnectionConfig,
            _: &RemoteMessageLocator,
            _: &imail_protocol::RemoteMessageFlagPatch,
        ) -> Result<(), ProtocolFailure> {
            let mut state = self.state.lock().unwrap();
            state.flag_updates += 1;
            if state.reject_flags {
                return Err(ProtocolFailure::from_provider(
                    ProtocolStage::Imap,
                    None,
                    "authorization=remote-secret",
                ));
            }
            Ok(())
        }

        fn list_mailboxes(
            &mut self,
            _: &MailConnectionConfig,
        ) -> Result<Vec<RemoteMailbox>, ProtocolFailure> {
            Ok(vec![RemoteMailbox {
                path: "Archive".into(),
                special_use: Some("\\Archive".into()),
            }])
        }

        fn move_message(
            &mut self,
            _: &MailConnectionConfig,
            _: &RemoteMessageLocator,
            target: &str,
        ) -> Result<RemoteMoveConfirmation, ProtocolFailure> {
            assert_eq!(target, "Archive");
            self.state.lock().unwrap().moves += 1;
            Ok(RemoteMoveConfirmation {
                confirmed: true,
                uid: Some(44),
            })
        }
    }

    struct TestSmtp {
        state: Arc<Mutex<MailTestState>>,
    }

    impl SmtpPort for TestSmtp {
        fn verify(&mut self, _: &MailConnectionConfig) -> Result<(), ProtocolFailure> {
            Ok(())
        }

        fn send(
            &mut self,
            _: &MailConnectionConfig,
            message: &OutgoingMessage,
        ) -> Result<imail_protocol::SendMessageResult, ProtocolFailure> {
            let mut state = self.state.lock().unwrap();
            state.sends += 1;
            state.last_sent = Some(message.clone());
            Ok(imail_protocol::SendMessageResult {
                message_id: "<sent@example.com>".into(),
                accepted: message.to.clone(),
            })
        }
    }

    fn temporary_directory(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("imail-http-{label}-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn authentication_directory(label: &str) -> PathBuf {
        let directory = temporary_directory(label);
        fs::File::create(directory.join("imail.sqlite")).unwrap();
        migrate_database(directory.join("imail.sqlite")).unwrap();
        directory
    }

    async fn json(response: Response) -> Value {
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn text_body(response: Response) -> String {
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[test]
    fn defaults_to_embedded_and_requires_explicit_http_bridge() {
        assert_eq!(BridgeMode::parse([]).unwrap(), BridgeMode::Embedded);
        assert_eq!(
            BridgeMode::parse(["--http"]).unwrap(),
            BridgeMode::Http {
                host: IpAddr::from([127, 0, 0, 1]),
                port: 8787
            }
        );
        assert_eq!(
            BridgeMode::parse(["--http", "--host", "0.0.0.0", "--port", "8080"]).unwrap(),
            BridgeMode::Http {
                host: IpAddr::from([0, 0, 0, 0]),
                port: 8080
            }
        );
        assert_eq!(
            BridgeMode::parse(["--port", "8080"]),
            Err(ConfigError::HttpOptionWithoutBridge)
        );
        assert_eq!(
            BridgeMode::parse(["--http", "--port", "0"]),
            Err(ConfigError::InvalidPort)
        );
    }

    #[tokio::test]
    async fn embedded_host_runs_account_applications_without_the_router() {
        let directory = authentication_directory("embedded-account-applications");
        fs::write(directory.join("master.key"), "52".repeat(32)).unwrap();
        let user = SqliteAuthStore::open_database(directory.join("imail.sqlite"))
            .unwrap()
            .create_user("owner", "Owner", "correct horse battery staple")
            .unwrap();
        let config = HttpAdapterConfig::new(&directory)
            .with_connection_probe(Arc::new(|_: &MailConnectionConfig| Ok(())));
        let host = EmbeddedServiceHost::start(config).unwrap();
        let created = host
            .create_account(
                user.id.clone(),
                "embedded-test".into(),
                json!({
                    "provider":"gmail",
                    "email":"owner@gmail.example",
                    "displayName":"Mailbox",
                    "password":"initial-secret"
                }),
            )
            .await
            .unwrap();
        assert_eq!(created.status, "connected");
        let updated = host
            .update_account_credential(
                user.id.clone(),
                "embedded-test".into(),
                created.id.clone(),
                "replacement-secret".into(),
            )
            .await
            .unwrap();
        assert_eq!(updated.status, "connected");
        let proxied = host
            .update_account_proxy(
                user.id.clone(),
                "embedded-test".into(),
                created.id.clone(),
                imail_protocol::AccountProxyUpdate::Explicit {
                    protocol: imail_protocol::ProxyProtocol::Socks5,
                    host: "proxy.example.test".into(),
                    port: 1080,
                    username: Some("owner".into()),
                    password: Some("proxy-secret".into()),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            proxied.proxy.as_ref().unwrap()["host"],
            "proxy.example.test"
        );
        let tested = host
            .test_account_connection(user.id.clone(), created.id.clone())
            .await
            .unwrap();
        assert_eq!(tested.status, "connected");
        assert!(tested.last_error.is_none());

        let store = SqliteAuthStore::open_database(directory.join("imail.sqlite")).unwrap();
        let stored = store.account(&user.id, &created.id).unwrap().unwrap();
        assert!(!stored.encrypted_secret.contains("replacement-secret"));
        assert!(!stored.encrypted_secret.contains("proxy-secret"));
        for event_type in [
            "account.created",
            "account.credential-updated",
            "account.proxy-updated",
        ] {
            assert_eq!(
                store
                    .security_audit_details(&user.id, event_type, 10)
                    .unwrap()
                    .len(),
                1,
                "{event_type}"
            );
        }
        drop(store);
        host.shutdown(Duration::from_secs(2)).unwrap();
        drop(host);
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn matches_public_health_and_service_info_contract_without_exposing_data() {
        let directory = temporary_directory("identity");
        let router = build_router(HttpAdapterConfig::new(&directory)).unwrap();
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/api/system/info")
                    .header(HOST, "127.0.0.1:8787")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
        assert_eq!(response.headers()["x-frame-options"], "DENY");
        assert_eq!(response.headers()[CONTENT_TYPE], "application/json");
        let body = json(response).await;
        assert_eq!(body["service"], "imail");
        assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(body["protocolVersion"], 1);
        assert_eq!(body["capabilities"]["gateway"], false);
        assert_eq!(body["capabilities"]["mcp"], false);
        assert_eq!(body["capabilities"]["syncWorker"], false);
        assert!(Uuid::parse_str(body["instanceId"].as_str().unwrap()).is_ok());
        assert!(!body.to_string().contains("encrypted"));

        let persisted = fs::read_to_string(directory.join("instance-id")).unwrap();
        assert_eq!(persisted.trim(), body["instanceId"]);

        let restarted = build_router(HttpAdapterConfig::new(&directory))
            .unwrap()
            .oneshot(
                Request::builder()
                    .uri("/api/system/info")
                    .header(HOST, "127.0.0.1:8787")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(json(restarted).await["instanceId"], body["instanceId"]);
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn serves_static_web_assets_and_spa_without_swallowing_reserved_routes() {
        let directory = authentication_directory("web-client");
        let web = directory.join("web");
        fs::create_dir_all(web.join("assets")).unwrap();
        fs::write(
            web.join("index.html"),
            "<!doctype html><title>Rust hosted iMail</title>",
        )
        .unwrap();
        fs::write(web.join("asset.txt"), "asset").unwrap();
        fs::write(web.join("assets").join("app.js"), "export {};").unwrap();
        let router =
            build_router(HttpAdapterConfig::new(&directory).with_web_client_root(&web)).unwrap();

        let info = json(
            router
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/api/system/info")
                        .header(HOST, "127.0.0.1:8787")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(info["capabilities"]["webClient"], true);

        let asset = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/asset.txt")
                    .header(HOST, "127.0.0.1:8787")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(asset.status(), StatusCode::OK);
        assert_eq!(asset.headers()[CONTENT_TYPE], "text/plain; charset=utf-8");
        assert_eq!(text_body(asset).await, "asset");

        let immutable = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/assets/app.js")
                    .header(HOST, "127.0.0.1:8787")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            immutable.headers()[CACHE_CONTROL],
            "public, max-age=31536000, immutable"
        );

        let route = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/settings")
                    .header(HOST, "127.0.0.1:8787")
                    .header("accept", "text/html")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(route.status(), StatusCode::OK);
        assert_eq!(route.headers()[CACHE_CONTROL], "no-cache");
        assert!(route
            .headers()
            .get("content-security-policy")
            .is_some_and(|value| value.to_str().unwrap().contains("frame-ancestors 'none'")));
        assert!(text_body(route).await.contains("Rust hosted iMail"));

        let missing_api = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/not-a-route")
                    .header(HOST, "127.0.0.1:8787")
                    .header("accept", "text/html")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_api.status(), StatusCode::UNAUTHORIZED);
        assert!(!text_body(missing_api).await.contains("Rust hosted iMail"));

        let traversal = router
            .oneshot(
                Request::builder()
                    .uri("/%2e%2e/master.key")
                    .header(HOST, "127.0.0.1:8787")
                    .header("accept", "application/octet-stream")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(traversal.status(), StatusCode::NOT_FOUND);
        assert!(!text_body(traversal).await.contains(&"52".repeat(32)));
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn standalone_http_host_stops_after_the_shutdown_signal() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let directory = authentication_directory("graceful-shutdown");
        let database = directory.join("imail.sqlite");
        let mut store = SqliteAuthStore::open_database(&database).unwrap();
        let owner = store
            .create_user("shutdown-owner", "Shutdown Owner", "shutdown-password-123")
            .unwrap();
        let session = store.create_session(&owner.id).unwrap();
        drop(store);
        let reserved = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = reserved.local_addr().unwrap().port();
        drop(reserved);
        let (shutdown, wait_for_shutdown) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(serve_with_shutdown(
            HttpAdapterConfig::new(&directory),
            std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            port,
            async move {
                let _ = wait_for_shutdown.await;
            },
        ));
        let connected = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
                    .await
                    .is_ok()
                {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await;
        assert!(connected.is_ok(), "HTTP listener did not start");
        let mut event_stream =
            tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
                .await
                .unwrap();
        event_stream
            .write_all(
                format!(
                    "GET /api/events HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nCookie: imail_session={}\r\nConnection: keep-alive\r\n\r\n",
                    session
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let mut received = Vec::new();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let mut chunk = [0_u8; 2048];
                let count = event_stream.read(&mut chunk).await.unwrap();
                assert!(count > 0, "SSE connection closed before its initial event");
                received.extend_from_slice(&chunk[..count]);
                if String::from_utf8_lossy(&received).contains("event: connected") {
                    break;
                }
            }
        })
        .await
        .expect("SSE stream did not send its initial event");
        shutdown.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), server)
            .await
            .expect("HTTP host ignored graceful shutdown")
            .unwrap()
            .unwrap();
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn daemon_shutdown_requires_loopback_control_token_and_stops_the_host() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let directory = authentication_directory("daemon-shutdown");
        fs::write(directory.join("master.key"), "73".repeat(32)).unwrap();
        let control_file = directory.join("daemon-control-token");
        fs::write(&control_file, "local-control-secret\n").unwrap();
        let unmanaged = build_router(HttpAdapterConfig::new(&directory)).unwrap();
        let unmanaged = unmanaged
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/system/shutdown")
                    .header(HOST, "127.0.0.1:8787")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unmanaged.status(), StatusCode::NOT_FOUND);

        let reserved = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = reserved.local_addr().unwrap().port();
        drop(reserved);
        let server = tokio::spawn(serve_with_shutdown(
            HttpAdapterConfig::new(&directory).with_daemon_control_file(&control_file),
            std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            port,
            std::future::pending(),
        ));
        let request = |token: &'static str| async move {
            let mut stream = loop {
                match tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).await {
                    Ok(stream) => break stream,
                    Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
                }
            };
            stream
                .write_all(
                    format!("POST /api/system/shutdown HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-iMail-Daemon-Token: {token}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes(),
                )
                .await
                .unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).await.unwrap();
            response
        };
        let rejected = request("wrong-control-secret").await;
        assert!(rejected.starts_with("HTTP/1.1 403"));
        assert!(!server.is_finished());
        let accepted = request("local-control-secret").await;
        assert!(accepted.starts_with("HTTP/1.1 202"));
        assert!(accepted.contains("\"stopping\":true"));
        tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .expect("daemon shutdown must stop the host")
            .unwrap()
            .unwrap();
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn standalone_host_reports_only_a_started_sync_runtime_and_removes_heartbeats() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let directory = authentication_directory("host-sync-runtime");
        fs::write(directory.join("master.key"), "53".repeat(32)).unwrap();
        let database = directory.join("imail.sqlite");
        let mut auth = SqliteAuthStore::open_database(&database).unwrap();
        let owner = auth
            .create_user("runtime-owner", "Runtime Owner", "runtime-password-123")
            .unwrap();
        let account_id = Uuid::new_v4().to_string();
        auth.upsert_account(&AccountRecord {
            id: account_id.clone(),
            owner_id: owner.id,
            provider: "custom".into(),
            email: "runtime@example.com".into(),
            display_name: "Runtime".into(),
            group: "Contract".into(),
            group_icon: "folder".into(),
            color: "#168f78".into(),
            settings: json!({
                "imapHost": "imap.example.com", "imapPort": 993, "imapSecure": true,
                "smtpHost": "smtp.example.com", "smtpPort": 465, "smtpSecure": true
            }),
            proxy: None,
            encrypted_secret: "invalid-contract-ciphertext".into(),
            auth_method: Some("app-password".into()),
            created_at: "2026-08-10T00:00:00.000Z".into(),
            last_sync_at: None,
            status: "connected".into(),
            last_error: None,
            mailboxes: json!([]),
        })
        .unwrap();
        drop(auth);
        let mut sync = SyncRuntimeStore::open_database(&database).unwrap();
        let job = sync
            .enqueue(
                &SyncEnqueue {
                    account_id,
                    mailbox: None,
                    mailbox_role: "inbox".into(),
                    reason: "manual".into(),
                    priority: 100,
                    not_before: None,
                },
                chrono::Utc::now(),
            )
            .unwrap();
        drop(sync);
        let reserved = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = reserved.local_addr().unwrap().port();
        drop(reserved);
        let (shutdown, wait_for_shutdown) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(serve_with_shutdown(
            HttpAdapterConfig::new(&directory)
                .with_sync_worker(true)
                .with_sync_worker_options(SyncRuntimeOptions {
                    worker_count: 2,
                    poll_interval: Duration::from_millis(25),
                    host_name: "contract-host".into(),
                    idle_enabled: false,
                    ..SyncRuntimeOptions::default()
                }),
            std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            port,
            async move {
                let _ = wait_for_shutdown.await;
            },
        ));
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let workers = SyncRuntimeStore::open_database(&database)
                    .and_then(|store| store.worker_health(chrono::Utc::now()))
                    .map(|health| health.workers)
                    .unwrap_or_default();
                let job_failed = SyncRuntimeStore::open_database(&database)
                    .and_then(|store| store.job(&job.id))
                    .ok()
                    .flatten()
                    .is_some_and(|job| job.status == "failed");
                if job_failed
                    && workers.len() == 2
                    && workers
                        .iter()
                        .all(|worker| worker.host_name == "contract-host")
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("sync runtime did not publish heartbeats and execute the queued job");

        let mut connection = tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap();
        connection
            .write_all(
                b"GET /api/system/info HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        let mut response = String::new();
        connection.read_to_string(&mut response).await.unwrap();
        assert!(response.contains("\"syncWorker\":true"));

        shutdown.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .expect("HTTP host or sync runtime ignored graceful shutdown")
            .unwrap()
            .unwrap();
        let health = SyncRuntimeStore::open_database(&database)
            .unwrap()
            .worker_health(chrono::Utc::now())
            .unwrap();
        assert!(health.workers.is_empty());
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn mcp_requires_full_scope_and_user_switch_then_serves_protocol_and_tools() {
        use imail_core::external_access::{ExternalAccessChanges, ExternalAccessService};

        struct McpOAuthProvider;
        impl OAuthProviderPort for McpOAuthProvider {
            fn token_request(
                &mut self,
                _: &OAuthConfig,
                _: OAuthGrant<'_>,
            ) -> Result<OAuthTokenResponse, OAuthError> {
                unreachable!("MCP OAuth begin must not exchange a token")
            }
            fn fetch_identity(
                &mut self,
                _: &OAuthConfig,
                _: &OAuthTokenResponse,
                _: &str,
            ) -> Result<OAuthIdentity, OAuthError> {
                unreachable!("MCP OAuth begin must not fetch identity")
            }
        }

        let directory = authentication_directory("mcp-transport");
        let database = directory.join("imail.sqlite");
        fs::write(directory.join("master.key"), "52".repeat(32)).unwrap();
        let (token, owner_id) = {
            let mut store = SqliteAuthStore::open_database(&database).unwrap();
            let user = store
                .create_user("mcp@example.com", "MCP", "Owner password 123!")
                .unwrap();
            store
                .insert_account_if_email_available(&AccountRecord {
                    id: "mcp-account".into(), owner_id: user.id.clone(), provider: "custom".into(), email: "mailbox@example.com".into(),
                    display_name: "Mailbox".into(), group: "个人".into(), group_icon: "folder".into(), color: "#168aad".into(),
                    settings: json!({"imapHost":"imap.example.com","imapPort":993,"imapSecure":true,"smtpHost":"smtp.example.com","smtpPort":465,"smtpSecure":true,"password":"settings-must-not-leak"}),
                    proxy: Some(json!({"protocol":"http","host":"proxy.example.com","port":8080,"password":"proxy-must-not-leak"})),
                    encrypted_secret: "mail-secret-must-not-leak".into(), auth_method: Some("app-password".into()),
                    created_at: "2026-08-10T00:00:00.000Z".into(), last_sync_at: None, status: "connected".into(), last_error: None,
                    mailboxes: json!([{"path":"INBOX","specialUse":"\\Inbox"},{"path":"Projects","specialUse":null}]),
                })
                .unwrap();
            let issued = store
                .issue_developer_token(&user.id, "MCP", &["mcp:full".into()], &[], 3_600)
                .unwrap();
            (issued.raw, user.id)
        };
        let mut config = HttpAdapterConfig::new(&directory);
        config.mcp = true;
        config.connection_probe = Arc::new(|_: &MailConnectionConfig| Ok(()));
        config.oauth_environment = OAuthEnvironment {
            callback_base_url: "http://127.0.0.1:8787/api/oauth".into(),
            google_client_id: Some("mcp-client-id".into()),
            google_client_secret: Some("mcp-client-secret".into()),
            ..OAuthEnvironment::default()
        };
        config.oauth_provider_factory =
            Arc::new(|| Box::new(McpOAuthProvider) as Box<dyn OAuthProviderPort>);
        let mail_state = Arc::new(Mutex::new(MailTestState::default()));
        config.mail_transport_factory = Arc::new(TestMailTransportFactory {
            state: Arc::clone(&mail_state),
            source: b"From: sender@example.org\r\nTo: future@example.net\r\nSubject: MCP attachment\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=part\r\n\r\n--part\r\nContent-Type: text/plain\r\n\r\nBody\r\n--part\r\nContent-Type: text/plain; name=report.txt\r\nContent-Disposition: attachment; filename=report.txt\r\nContent-Transfer-Encoding: base64\r\n\r\naGVsbG8=\r\n--part--\r\n".to_vec(),
        });
        let router = build_router(config).unwrap();
        let rpc = |body: &str, raw: &str| {
            Request::builder()
                .method(Method::POST)
                .uri("/mcp")
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("accept", "application/json, text/event-stream")
                .header("authorization", format!("Bearer {raw}"))
                .body(Body::from(body.to_owned()))
                .unwrap()
        };
        let hostile = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/mcp")
                    .header(HOST, "attacker.example")
                    .header(CONTENT_TYPE, "application/json")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(hostile.status(), StatusCode::FORBIDDEN);
        let bad_accept = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/mcp")
                    .header(HOST, "127.0.0.1:8787")
                    .header(CONTENT_TYPE, "application/json")
                    .header("accept", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(bad_accept.status(), StatusCode::NOT_ACCEPTABLE);
        let bad_content = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/mcp")
                    .header(HOST, "127.0.0.1:8787")
                    .header(CONTENT_TYPE, "text/plain")
                    .header("accept", "application/json, text/event-stream")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(bad_content.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
        let disabled = router
            .clone()
            .oneshot(rpc(
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}"#,
                &token,
            ))
            .await
            .unwrap();
        assert_eq!(disabled.status(), StatusCode::FORBIDDEN);
        {
            let mut store = SqliteAuthStore::open_database(&database).unwrap();
            ExternalAccessService::new(&mut store)
                .update(
                    &owner_id,
                    ExternalAccessChanges {
                        mcp_enabled: Some(true),
                        gateway_enabled: None,
                    },
                )
                .unwrap();
        }
        let initialized = router
            .clone()
            .oneshot(rpc(
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}"#,
                &token,
            ))
            .await
            .unwrap();
        assert_eq!(initialized.status(), StatusCode::OK);
        assert_eq!(
            json(initialized).await["result"]["serverInfo"]["name"],
            "imail"
        );
        let legacy_header = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/mcp")
                    .header(HOST, "127.0.0.1:8787")
                    .header(CONTENT_TYPE, "application/json")
                    .header("accept", "application/json, text/event-stream")
                    .header("authorization", format!("Bearer {token}"))
                    .header("mcp-protocol-version", "2025-06-18")
                    .body(Body::from(r#"{"jsonrpc":"2.0","id":100,"method":"ping"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(legacy_header.status(), StatusCode::OK);
        assert_eq!(json(legacy_header).await["id"], 100);
        let modern_rpc = |body: &'static str, method: &'static str, name: Option<&'static str>| {
            let mut request = Request::builder()
                .method(Method::POST)
                .uri("/mcp")
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("accept", "application/json, text/event-stream")
                .header("authorization", format!("Bearer {token}"))
                .header("mcp-protocol-version", "2026-07-28")
                .header("mcp-method", method);
            if let Some(name) = name {
                request = request.header("mcp-name", name);
            }
            request.body(Body::from(body)).unwrap()
        };
        let discovered = json(
            router
                .clone()
                .oneshot(modern_rpc(
                    r#"{"jsonrpc":"2.0","id":1001,"method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{},"io.modelcontextprotocol/clientInfo":{"name":"rust-test","version":"1.0.0"}}}}"#,
                    "server/discover",
                    None,
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(
            discovered["result"]["supportedVersions"],
            json!(["2026-07-28"])
        );
        assert_eq!(
            discovered["result"]["capabilities"]["tools"]["listChanged"],
            true
        );
        assert_eq!(discovered["result"]["resultType"], "complete");
        assert_eq!(discovered["result"]["ttlMs"], 0);
        assert_eq!(discovered["result"]["cacheScope"], "private");
        let modern_list = router
            .clone()
            .oneshot(modern_rpc(
                r#"{"jsonrpc":"2.0","id":101,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#,
                "tools/list",
                None,
            ))
            .await
            .unwrap();
        assert_eq!(modern_list.status(), StatusCode::OK);
        let modern_list = json(modern_list).await;
        assert_eq!(modern_list["result"]["resultType"], "complete");
        assert_eq!(modern_list["result"]["ttlMs"], 0);
        assert_eq!(modern_list["result"]["cacheScope"], "private");
        assert_eq!(
            modern_list["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
            "imail"
        );
        let tool_names = modern_list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect::<Vec<_>>();
        assert!(tool_names.contains(&"translation_profiles_list"));
        assert!(tool_names.contains(&"message_translate"));
        let translation_profiles = json(
            router
                .clone()
                .oneshot(modern_rpc(
                    r#"{"jsonrpc":"2.0","id":108,"method":"tools/call","params":{"name":"translation_profiles_list","arguments":{},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#,
                    "tools/call",
                    Some("translation_profiles_list"),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(
            translation_profiles["result"]["structuredContent"]["profiles"],
            json!([])
        );
        let modern_call = json(
            router
                .clone()
                .oneshot(modern_rpc(
                    r#"{"jsonrpc":"2.0","id":102,"method":"tools/call","params":{"name":"accounts_list","arguments":{},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#,
                    "tools/call",
                    Some("accounts_list"),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(modern_call["result"]["resultType"], "complete");
        assert_eq!(
            modern_call["result"]["structuredContent"]["accounts"][0]["email"],
            "mailbox@example.com"
        );
        let missing_envelope = router
            .clone()
            .oneshot(modern_rpc(
                r#"{"jsonrpc":"2.0","id":103,"method":"tools/list","params":{}}"#,
                "tools/list",
                None,
            ))
            .await
            .unwrap();
        assert_eq!(missing_envelope.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json(missing_envelope).await["error"]["code"], -32602);
        let mismatched_method = router
            .clone()
            .oneshot(modern_rpc(
                r#"{"jsonrpc":"2.0","id":104,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#,
                "ping",
                None,
            ))
            .await
            .unwrap();
        assert_eq!(mismatched_method.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json(mismatched_method).await["error"]["code"], -32020);
        let modern_batch = router
            .clone()
            .oneshot(modern_rpc(
                r#"[{"jsonrpc":"2.0","id":105,"method":"ping","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}]"#,
                "ping",
                None,
            ))
            .await
            .unwrap();
        assert_eq!(modern_batch.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json(modern_batch).await["error"]["code"], -32600);
        let legacy_batch = json(
            router
                .clone()
                .oneshot(rpc(
                    r#"[{"jsonrpc":"2.0","id":106,"method":"ping"},{"jsonrpc":"2.0","method":"notifications/initialized"},{"jsonrpc":"2.0","id":107,"method":"tools/list","params":{}}]"#,
                    &token,
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(legacy_batch.as_array().unwrap().len(), 2);
        assert_eq!(legacy_batch[0]["id"], 106);
        assert_eq!(legacy_batch[1]["id"], 107);
        let listed = json(
            router
                .clone()
                .oneshot(rpc(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
                    &token,
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 58);
        assert!(listed.to_string().contains("accounts_list"));
        let tools = listed["result"]["tools"].as_array().unwrap();
        let shared_contract: Value =
            serde_json::from_str(include_str!("../../../contracts/mcp-tools.json")).unwrap();
        assert_eq!(listed["result"]["tools"], shared_contract["tools"]);
        let move_tool = tools
            .iter()
            .find(|tool| tool["name"] == "message_move")
            .unwrap();
        assert_eq!(move_tool["annotations"]["destructiveHint"], true);
        assert_eq!(move_tool["annotations"]["idempotentHint"], false);
        let send_tool = tools
            .iter()
            .find(|tool| tool["name"] == "message_send")
            .unwrap();
        assert_eq!(
            send_tool["inputSchema"]["required"],
            json!(["accountEmail", "to", "subject", "text"])
        );
        let add_tool = tools
            .iter()
            .find(|tool| tool["name"] == "account_add_with_code")
            .unwrap();
        assert!(add_tool["inputSchema"]["properties"]["groupIcon"]["enum"]
            .as_array()
            .unwrap()
            .contains(&json!("star")));
        let accounts = json(
            router
                .clone()
                .oneshot(rpc(
                    r#"{"jsonrpc":"2.0","id":21,"method":"tools/call","params":{"name":"accounts_list","arguments":{}}}"#,
                    &token,
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(
            accounts["result"]["structuredContent"]["accounts"][0]["email"],
            "mailbox@example.com"
        );
        assert!(!accounts.to_string().contains("mail-secret-must-not-leak"));
        assert!(!accounts.to_string().contains("proxy-must-not-leak"));
        assert!(!accounts.to_string().contains("settings-must-not-leak"));
        let added = json(
            router
                .clone()
                .oneshot(rpc(
                    r##"{"jsonrpc":"2.0","id":22,"method":"tools/call","params":{"name":"account_add_with_code","arguments":{"provider":"custom","email":"future@example.net","displayName":"Future","authorizationCode":"future-secret-must-not-leak","group":"个人","groupIcon":"folder","color":"#168aad","settings":{"imapHost":"imap.example.net","imapPort":993,"imapSecure":true,"smtpHost":"smtp.example.net","smtpPort":465,"smtpSecure":true}}}}"##,
                    &token,
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(
            added["result"]["structuredContent"]["account"]["email"],
            "future@example.net"
        );
        assert!(!added.to_string().contains("future-secret-must-not-leak"));
        let proxied = json(
            router
                .clone()
                .oneshot(rpc(
                    r#"{"jsonrpc":"2.0","id":23,"method":"tools/call","params":{"name":"account_proxy_update","arguments":{"email":"future@example.net","enabled":true,"protocol":"http","host":"127.0.0.1","port":8080,"username":"agent","password":"proxy-new-must-not-leak"}}}"#,
                    &token,
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(
            proxied["result"]["structuredContent"]["account"]["proxy"]["host"],
            "127.0.0.1"
        );
        assert!(!proxied.to_string().contains("proxy-new-must-not-leak"));
        let bad_draft = json(router.clone().oneshot(rpc(
            r#"{"jsonrpc":"2.0","id":230,"method":"tools/call","params":{"name":"draft_save","arguments":{"accountEmail":"future@example.net","to":["friend@example.com"],"attachments":[{"filename":"bad.bin","contentType":"application/octet-stream","data":"not-base64"}]}}}"#,&token)).await.unwrap()).await;
        assert_eq!(bad_draft["result"]["isError"], true);
        assert!(SqliteAuthStore::open_database(&database)
            .unwrap()
            .list_drafts(&owner_id)
            .unwrap()
            .is_empty());
        let good_draft = json(router.clone().oneshot(rpc(
            r#"{"jsonrpc":"2.0","id":2301,"method":"tools/call","params":{"name":"draft_save","arguments":{"accountEmail":"future@example.net","to":["friend@example.com"],"attachments":[{"filename":"hello.txt","contentType":"text/plain","data":"aGVsbG8="}]}}}"#,&token)).await.unwrap()).await;
        assert_eq!(
            good_draft["result"]["structuredContent"]["draft"]["attachments"][0]["size"],
            5
        );
        assert!(
            good_draft["result"]["structuredContent"]["draft"]["attachments"][0]["id"].is_string()
        );
        assert!(!good_draft.to_string().contains("aGVsbG8="));
        let oauth = json(router.clone().oneshot(rpc(
            r##"{"jsonrpc":"2.0","id":231,"method":"tools/call","params":{"name":"account_start_oauth","arguments":{"provider":"gmail","displayName":"OAuth via MCP","group":"个人","color":"#168aad"}}}"##,&token)).await.unwrap()).await;
        assert!(oauth["result"]["structuredContent"]["authorizationUrl"]
            .as_str()
            .is_some_and(|value| value.starts_with("https://")));
        assert!(oauth["result"]["structuredContent"]["state"].is_string());
        assert!(!oauth.to_string().contains("mcp-client-secret"));
        {
            let mut store = SqliteAuthStore::open_database(&database).unwrap();
            let future = store
                .list_accounts(&owner_id)
                .unwrap()
                .into_iter()
                .find(|account| account.email == "future@example.net")
                .unwrap();
            store
                .upsert_message(
                    &owner_id,
                    &imail_protocol::MessageReadModel {
                        headers: Default::default(),
                        id: "mcp-side-effect".into(), account_id: future.id, mailbox: "INBOX".into(), mailbox_role: "inbox".into(), uid: 7,
                        message_id: Some("<mcp@example.net>".into()), from: json!({"name":"Sender","address":"sender@example.org"}), to: json!([{"address":"future@example.net"}]),
                        subject: "MCP side effects".into(), preview: "Body".into(), text: "Body".into(), html: None, date: "2026-08-10T08:00:00.000Z".into(),
                        unread: true, flagged: false, has_attachments: true, attachments: json!([{"filename":"report.txt","contentType":"text/plain","size":5,"index":0}]), labels: json!([]), snoozed_until: None,
                    },
                )
                .unwrap();
        }
        let updated = json(router.clone().oneshot(rpc(
            r#"{"jsonrpc":"2.0","id":24,"method":"tools/call","params":{"name":"message_update","arguments":{"messageId":"mcp-side-effect","unread":false,"labels":["MCP"]}}}"#,&token)).await.unwrap()).await;
        assert_eq!(
            updated["result"]["structuredContent"]["message"]["unread"],
            false
        );
        mail_state.lock().unwrap().reject_flags = true;
        let rejected = json(router.clone().oneshot(rpc(
            r#"{"jsonrpc":"2.0","id":241,"method":"tools/call","params":{"name":"message_update","arguments":{"messageId":"mcp-side-effect","unread":true}}}"#,&token)).await.unwrap()).await;
        assert_eq!(rejected["result"]["isError"], true);
        assert!(
            !SqliteAuthStore::open_database(&database)
                .unwrap()
                .list_messages(&owner_id)
                .unwrap()
                .into_iter()
                .find(|message| message.id == "mcp-side-effect")
                .unwrap()
                .unread
        );
        mail_state.lock().unwrap().reject_flags = false;
        let attachment = json(router.clone().oneshot(rpc(
            r#"{"jsonrpc":"2.0","id":25,"method":"tools/call","params":{"name":"attachment_download","arguments":{"messageId":"mcp-side-effect","index":0}}}"#,&token)).await.unwrap()).await;
        assert_eq!(
            attachment["result"]["structuredContent"]["data"],
            "aGVsbG8="
        );
        let moved = json(router.clone().oneshot(rpc(
            r#"{"jsonrpc":"2.0","id":26,"method":"tools/call","params":{"name":"message_move","arguments":{"messageId":"mcp-side-effect","destination":"archive"}}}"#,&token)).await.unwrap()).await;
        assert_eq!(moved["result"]["structuredContent"]["mailbox"], "Archive");
        let sent = json(router.clone().oneshot(rpc(
            r#"{"jsonrpc":"2.0","id":27,"method":"tools/call","params":{"name":"message_send","arguments":{"accountEmail":"future@example.net","to":["friend@example.com"],"subject":"MCP send","text":"Hello"}}}"#,&token)).await.unwrap()).await;
        assert_eq!(
            sent["result"]["structuredContent"]["delivery"]["messageId"],
            "<sent@example.com>"
        );
        let send_at = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
        let schedule_request = format!(
            r#"{{"jsonrpc":"2.0","id":271,"method":"tools/call","params":{{"name":"outbox_schedule","arguments":{{"accountEmail":"future@example.net","to":["later@example.com"],"subject":"MCP scheduled","text":"Later","attachments":[{{"filename":"note.txt","contentType":"text/plain","data":"aGVsbG8="}}],"requestId":"00000000-0000-4000-8000-000000000271","sendAt":"{send_at}"}}}}}}"#
        );
        let scheduled = json(
            router
                .clone()
                .oneshot(rpc(&schedule_request, &token))
                .await
                .unwrap(),
        )
        .await;
        let outbox_id = scheduled["result"]["structuredContent"]["item"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        let repeated = json(
            router
                .clone()
                .oneshot(rpc(&schedule_request, &token))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(
            repeated["result"]["structuredContent"]["item"]["id"],
            outbox_id
        );
        let outbox = json(router.clone().oneshot(rpc(
            r#"{"jsonrpc":"2.0","id":272,"method":"tools/call","params":{"name":"outbox_list","arguments":{}}}"#,
            &token,
        )).await.unwrap()).await;
        assert_eq!(
            outbox["result"]["structuredContent"]["items"][0]["subject"],
            "MCP scheduled"
        );
        let cancel_request = format!(
            r#"{{"jsonrpc":"2.0","id":273,"method":"tools/call","params":{{"name":"outbox_cancel","arguments":{{"outboxId":"{outbox_id}"}}}}}}"#
        );
        let cancelled = json(
            router
                .clone()
                .oneshot(rpc(&cancel_request, &token))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(cancelled["result"]["structuredContent"]["cancelled"], true);
        {
            let mail = mail_state.lock().unwrap();
            assert_eq!(mail.flag_updates, 2);
            assert_eq!(mail.fetches, 1);
            assert_eq!(mail.moves, 1);
            assert_eq!(mail.sends, 1);
        }
        let settings = json(
            router
                .clone()
                .oneshot(rpc(
                    r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"settings_update","arguments":{"startupView":"inbox"}}}"#,
                    &token,
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_ne!(settings["result"]["isError"], true);
        let policy = json(
            router
                .clone()
                .oneshot(rpc(
                    r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"sync_policy_update","arguments":{"email":"mailbox@example.com","folderMode":"selected","selectedMailboxes":["Projects"],"notifyOnError":false}}}"#,
                    &token,
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(
            policy["result"]["structuredContent"]["policy"]["folderMode"],
            "selected"
        );
        assert_eq!(
            policy["result"]["structuredContent"]["policy"]["selectedMailboxes"],
            json!(["Projects"])
        );
        let queued = json(
            router
                .clone()
                .oneshot(rpc(
                    r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"mailbox_sync","arguments":{"email":"mailbox@example.com","mailboxRole":"custom","mailboxPath":"Projects"}}}"#,
                    &token,
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(
            queued["result"]["structuredContent"]["results"][0]["status"],
            "queued"
        );
        let sync = SyncRuntimeStore::open_database(&database).unwrap();
        let jobs = sync.account_jobs("mcp-account", 10).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].mailbox.as_deref(), Some("Projects"));
        drop(sync);
        let audits = SqliteAuthStore::open_database(&database)
            .unwrap()
            .security_audit_details(&owner_id, "mcp.management-tool-called", 50)
            .unwrap();
        assert!(audits.iter().any(|detail| {
            detail.get("tool").map(String::as_str) == Some("settings_update")
                && detail
                    .get("authorizationCodeId")
                    .is_some_and(|value| !value.is_empty())
        }));
        assert!(!serde_json::to_string(&audits).unwrap().contains(&token));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_an_invalid_existing_instance_identity_without_replacing_it() {
        let directory = temporary_directory("invalid-identity");
        let identity = directory.join("instance-id");
        fs::write(&identity, "not-a-valid-instance\n").unwrap();
        let error = build_router(HttpAdapterConfig::new(&directory));
        assert!(matches!(error, Err(HttpAdapterError::InvalidInstanceId)));
        assert_eq!(
            fs::read_to_string(identity).unwrap(),
            "not-a-valid-instance\n"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn rejects_untrusted_production_hosts_and_applies_security_headers() {
        let directory = temporary_directory("host");
        let mut config = HttpAdapterConfig::production(&directory);
        config.allowed_hosts = ["mail.example.com".into()].into_iter().collect();
        let response = build_router(config)
            .unwrap()
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .header(HOST, "attacker.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::MISDIRECTED_REQUEST);
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert_eq!(
            json(response).await["error"],
            "请求主机名不在服务允许列表中"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn trusts_exactly_one_forwarding_hop_only_when_explicitly_enabled() {
        let ignored_directory = temporary_directory("proxy-ignored");
        let ignored = build_router(HttpAdapterConfig::new(&ignored_directory))
            .unwrap()
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .header("x-forwarded-for", "not-an-ip")
                    .header("x-forwarded-proto", "gopher")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ignored.status(), StatusCode::OK);

        let trusted_directory = authentication_directory("proxy-trusted");
        let config = HttpAdapterConfig::new(&trusted_directory).with_trusted_proxy_one_hop(true);
        let router = build_router(config).unwrap();
        let malformed = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .header("x-forwarded-for", "203.0.113.4, not-an-ip")
                    .header("x-forwarded-proto", "https")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);

        let registration = router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/auth/register")
                    .header(HOST, "mail.example.test")
                    .header(CONTENT_TYPE, "application/json")
                    .header("x-forwarded-for", "198.51.100.10, 203.0.113.4")
                    .header("x-forwarded-proto", "https, http")
                    .body(Body::from(
                        r#"{"login":"proxy.owner","displayName":"Proxy Owner","password":"test-password-123"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(registration.status(), StatusCode::CREATED);
        assert!(registration.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .contains("Secure"));
        assert_eq!(
            registration.headers()["strict-transport-security"],
            "max-age=31536000"
        );
        fs::remove_dir_all(ignored_directory).unwrap();
        fs::remove_dir_all(trusted_directory).unwrap();
    }

    #[tokio::test]
    async fn discovers_persists_and_reuses_a_contact_logo_without_network_in_tests() {
        let directory = authentication_directory("logo-discovery");
        let database = directory.join("imail.sqlite");
        let mut store = SqliteAuthStore::open_database(&database).unwrap();
        let owner = store
            .create_user("logo-owner", "Logo Owner", "owner-password-123")
            .unwrap();
        let session = store.create_session(&owner.id).unwrap();
        store
            .upsert_contact(&imail_protocol::ContactReadModel {
                owner_id: owner.id.clone(),
                address: "sender@example.org".into(),
                name: "Sender".into(),
                message_count: 1,
                last_contact_at: "2026-08-10T08:00:00.000Z".into(),
                logo_key: None,
                logo_content_type: None,
                logo_source_url: None,
                logo_fetched_at: None,
            })
            .unwrap();
        drop(store);
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let router = build_router(HttpAdapterConfig::new(&directory).with_logo_discovery(
            Arc::new(TestLogoDiscovery {
                calls: Arc::clone(&calls),
            }),
        ))
        .unwrap();
        let request = || {
            Request::builder()
                .uri("/api/contacts/logo?address=sender%40example.org")
                .header(HOST, "127.0.0.1:8787")
                .header("cookie", format!("imail_session={session}"))
                .body(Body::empty())
                .unwrap()
        };
        for _ in 0..2 {
            let response = router.clone().oneshot(request()).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[CONTENT_TYPE], "image/png");
        }
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        let store = SqliteAuthStore::open_database(&database).unwrap();
        let contact = store.list_contacts(&owner.id).unwrap().remove(0);
        assert_eq!(contact.logo_key.as_deref(), Some("domain:example.org"));
        assert_eq!(store.list_logo_fetch_attempts(&owner.id).unwrap().len(), 1);
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn manages_user_scoped_external_access_and_developer_tokens() {
        let directory = authentication_directory("developer-controls");
        let database = directory.join("imail.sqlite");
        let mut store = SqliteAuthStore::open_database(&database).unwrap();
        let owner = store
            .create_user("developer-owner", "Developer Owner", "owner-password-123")
            .unwrap();
        let other = store
            .create_user("developer-other", "Developer Other", "other-password-123")
            .unwrap();
        let owner_session = store.create_session(&owner.id).unwrap();
        let other_session = store.create_session(&other.id).unwrap();
        let account_id = Uuid::new_v4().to_string();
        store
            .upsert_account(&AccountRecord {
                id: account_id.clone(),
                owner_id: owner.id.clone(),
                provider: "icloud".into(),
                email: "owner@example.com".into(),
                display_name: "Owner Mail".into(),
                group: "工作".into(),
                group_icon: "folder".into(),
                color: "#168f78".into(),
                settings: json!({}),
                proxy: None,
                encrypted_secret: "encrypted-placeholder".into(),
                auth_method: Some("app-password".into()),
                created_at: "2026-08-10T00:00:00.000Z".into(),
                last_sync_at: None,
                status: "connected".into(),
                last_error: None,
                mailboxes: json!([]),
            })
            .unwrap();
        for (id, uid, date, subject) in [
            ("gateway-message-2", 2, "2026-08-10T09:00:00.000Z", "Newest"),
            ("gateway-message-1", 1, "2026-08-10T08:00:00.000Z", "Older"),
        ] {
            store
                .upsert_message(
                    &owner.id,
                    &imail_protocol::MessageReadModel {
                        headers: Default::default(),
                        id: id.into(),
                        account_id: account_id.clone(),
                        mailbox: "INBOX".into(),
                        mailbox_role: "inbox".into(),
                        uid,
                        message_id: None,
                        from: json!({ "name": "Sender", "address": "sender@example.net" }),
                        to: if id == "gateway-message-2" {
                            json!([{ "name": "Private", "address": "private-alias@icloud.com" }])
                        } else {
                            json!([{ "name": "Owner", "address": "owner@example.com" }])
                        },
                        subject: subject.into(),
                        preview: "Gateway preview".into(),
                        text: "Gateway body".into(),
                        html: Some("<p>Gateway body</p>".into()),
                        date: date.into(),
                        unread: true,
                        flagged: false,
                        has_attachments: false,
                        attachments: json!([]),
                        labels: json!(["gateway"]),
                        snoozed_until: None,
                    },
                )
                .unwrap();
        }
        store
            .upsert_apple_hme_address(&AppleHmeAddressRecord {
                account_id: account_id.clone(),
                user_id: owner.id.clone(),
                anonymous_id: "gateway-hme-1".into(),
                email: "private-alias@icloud.com".into(),
                label: "Gateway private mailbox".into(),
                note: String::new(),
                forward_to_email: "owner@example.com".into(),
                active: true,
                origin: "WEB".into(),
                created_at: Some("2026-08-10T07:00:00.000Z".into()),
                updated_at: "2026-08-10T07:00:00.000Z".into(),
            })
            .unwrap();
        drop(store);
        let mut config = HttpAdapterConfig::new(&directory);
        config.gateway = true;
        let router = build_router(config).unwrap();
        let request = |method: Method, uri: &str, session: &str, body: Body| {
            Request::builder()
                .method(method)
                .uri(uri)
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={session}"))
                .body(body)
                .unwrap()
        };

        let defaults = json(
            router
                .clone()
                .oneshot(request(
                    Method::GET,
                    "/api/external-access",
                    &owner_session,
                    Body::empty(),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(
            defaults["settings"],
            json!({ "gatewayEnabled": false, "mcpEnabled": false })
        );
        let updated = json(
            router
                .clone()
                .oneshot(request(
                    Method::PATCH,
                    "/api/external-access",
                    &owner_session,
                    Body::from(r#"{"gatewayEnabled":true}"#),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(
            updated["settings"],
            json!({ "gatewayEnabled": true, "mcpEnabled": false })
        );
        let other_settings = json(
            router
                .clone()
                .oneshot(request(
                    Method::GET,
                    "/api/external-access",
                    &other_session,
                    Body::empty(),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(other_settings["settings"]["gatewayEnabled"], false);

        let missing_mailbox = router
            .clone()
            .oneshot(request(
                Method::POST,
                "/api/developer-tokens",
                &owner_session,
                Body::from(r#"{"name":"Bad","scopes":["messages:read"],"mailboxes":["missing@example.com"],"ttlSeconds":3600}"#),
            ))
            .await
            .unwrap();
        assert_eq!(missing_mailbox.status(), StatusCode::BAD_REQUEST);
        let created = router
            .clone()
            .oneshot(request(
                Method::POST,
                "/api/developer-tokens",
                &owner_session,
                Body::from(r#"{"name":"Gateway","scopes":["messages:read","accounts:read"],"mailboxes":["OWNER@example.com"],"ttlSeconds":3600}"#),
            ))
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::CREATED);
        let created = json(created).await;
        let raw = created["token"].as_str().unwrap().to_string();
        let token_id = created["detail"]["id"].as_str().unwrap().to_string();
        assert!(raw.starts_with("imail_"));
        assert_eq!(created["detail"]["mailboxes"], json!(["owner@example.com"]));
        assert!(created["detail"].get("ownerId").is_none());
        assert!(created["detail"].get("accountIds").is_none());
        assert!(created["detail"].get("tokenHash").is_none());

        let disable = router
            .clone()
            .oneshot(request(
                Method::PATCH,
                "/api/external-access",
                &owner_session,
                Body::from(r#"{"gatewayEnabled":false}"#),
            ))
            .await
            .unwrap();
        assert_eq!(disable.status(), StatusCode::OK);
        let gateway_request = |uri: String, token: &str| {
            Request::builder()
                .uri(uri)
                .header(HOST, "127.0.0.1:8787")
                .header("authorization", format!("Bearer {token}"))
                .header("x-request-id", "gateway.test:1")
                .body(Body::empty())
                .unwrap()
        };
        let disabled_gateway = router
            .clone()
            .oneshot(gateway_request("/gateway/v1/messages".into(), &raw))
            .await
            .unwrap();
        assert_eq!(disabled_gateway.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            json(disabled_gateway).await["error"]["code"],
            "GATEWAY_DISABLED"
        );
        assert_eq!(
            router
                .clone()
                .oneshot(request(
                    Method::PATCH,
                    "/api/external-access",
                    &owner_session,
                    Body::from(r#"{"gatewayEnabled":true}"#),
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        let mailboxes = router
            .clone()
            .oneshot(gateway_request("/gateway/v1/mailboxes".into(), &raw))
            .await
            .unwrap();
        assert_eq!(mailboxes.status(), StatusCode::OK);
        assert_eq!(mailboxes.headers()["x-request-id"], "gateway.test:1");
        let mailboxes = json(mailboxes).await;
        assert_eq!(mailboxes["mailboxes"][0]["email"], "owner@example.com");
        assert!(mailboxes["mailboxes"][0].get("id").is_none());
        assert!(mailboxes["mailboxes"][0].get("settings").is_none());
        let first_page = json(
            router
                .clone()
                .oneshot(gateway_request(
                    "/gateway/v1/messages?limit=1&unread=true".into(),
                    &raw,
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(first_page["messages"][0]["id"], "gateway-message-2");
        assert!(first_page["messages"][0].get("text").is_none());
        assert_eq!(first_page["page"]["hasMore"], true);
        let cursor = first_page["page"]["nextCursor"].as_str().unwrap();
        let second_page = json(
            router
                .clone()
                .oneshot(gateway_request(
                    format!("/gateway/v1/messages?limit=1&cursor={cursor}"),
                    &raw,
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(second_page["messages"][0]["id"], "gateway-message-1");
        let hme_page = json(
            router
                .clone()
                .oneshot(gateway_request(
                    "/gateway/v1/mailboxes/private-alias@icloud.com/messages".into(),
                    &raw,
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(hme_page["messages"].as_array().unwrap().len(), 1);
        assert_eq!(hme_page["messages"][0]["id"], "gateway-message-2");
        assert_eq!(
            hme_page["messages"][0]["accountEmail"],
            "private-alias@icloud.com"
        );
        assert!(!hme_page.to_string().contains("owner@example.com"));
        let detail = json(
            router
                .clone()
                .oneshot(gateway_request(
                    "/gateway/v1/messages/gateway-message-2".into(),
                    &raw,
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(detail["message"]["text"], "Gateway body");
        assert_eq!(
            detail["message"]["accountEmail"],
            "private-alias@icloud.com"
        );
        assert!(!detail.to_string().contains("owner@example.com"));
        let invalid_cursor = router
            .clone()
            .oneshot(gateway_request(
                "/gateway/v1/messages?cursor=broken".into(),
                &raw,
            ))
            .await
            .unwrap();
        assert_eq!(invalid_cursor.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            json(invalid_cursor).await["error"]["code"],
            "INVALID_CURSOR"
        );
        let denied_send = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/gateway/v1/send")
                    .header(HOST, "127.0.0.1:8787")
                    .header("authorization", format!("Bearer {raw}"))
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"mailbox":"owner@example.com","to":["to@example.com"],"subject":"Hello","text":"Body"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(denied_send.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(json(denied_send).await["error"]["code"], "UNAUTHORIZED");
        let openapi = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/gateway/openapi.json")
                    .header(HOST, "127.0.0.1:8787")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(openapi.status(), StatusCode::OK);
        assert_eq!(openapi.headers()[CACHE_CONTROL], "no-store");
        let openapi = json(openapi).await;
        assert_eq!(openapi["openapi"], "3.1.0");
        assert_eq!(openapi["servers"][0]["url"], "/gateway/v1");
        assert_eq!(
            openapi["components"]["securitySchemes"]["bearerAuth"]["scheme"],
            "bearer"
        );
        for path in [
            "/health",
            "/mailboxes",
            "/messages",
            "/mailboxes/{mailbox}/messages",
            "/messages/{messageId}",
            "/messages/{messageId}/attachments/{index}",
            "/send",
        ] {
            assert!(openapi["paths"].get(path).is_some(), "missing {path}");
        }
        assert_eq!(openapi["x-websocket"]["url"], "/gateway/v1/events");
        assert_eq!(
            openapi["x-websocket"]["authentication"]["requiredScope"],
            "messages:read"
        );
        let docs = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/gateway/docs")
                    .header(HOST, "127.0.0.1:8787")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(docs.status(), StatusCode::OK);
        assert_eq!(docs.headers()[CACHE_CONTROL], "no-store");
        assert!(text_body(docs).await.contains("/gateway/openapi.json"));

        let mcp = json(
            router
                .clone()
                .oneshot(request(
                    Method::POST,
                    "/api/developer-tokens",
                    &owner_session,
                    Body::from(r#"{"name":"MCP","scopes":["messages:read","mcp:full"],"mailboxes":[],"ttlSeconds":3600}"#),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert!(mcp["token"].as_str().unwrap().starts_with("imail_mcp_"));
        assert_eq!(mcp["detail"]["scopes"], json!(["mcp:full"]));
        assert_eq!(mcp["detail"]["mailboxes"], json!(["owner@example.com"]));

        let listing = json(
            router
                .clone()
                .oneshot(request(
                    Method::GET,
                    "/api/developer-tokens",
                    &owner_session,
                    Body::empty(),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(listing["tokens"].as_array().unwrap().len(), 2);
        assert!(!listing.to_string().contains(&raw));
        assert_eq!(
            SqliteAuthStore::open_database(&database)
                .unwrap()
                .authenticate_developer_token(&raw, "messages:read")
                .unwrap()
                .unwrap()
                .account_ids,
            vec![account_id]
        );
        assert_eq!(
            router
                .clone()
                .oneshot(request(
                    Method::DELETE,
                    &format!("/api/developer-tokens/{token_id}"),
                    &owner_session,
                    Body::empty(),
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::NO_CONTENT
        );
        assert!(SqliteAuthStore::open_database(&database)
            .unwrap()
            .authenticate_developer_token(&raw, "messages:read")
            .unwrap()
            .is_none());
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn gateway_websocket_enforces_origin_scope_account_and_live_revocation() {
        use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

        let directory = authentication_directory("gateway-websocket");
        let database = directory.join("imail.sqlite");
        let mut store = SqliteAuthStore::open_database(&database).unwrap();
        let owner = store
            .create_user("gateway-ws-owner", "Gateway WS", "owner-password-123")
            .unwrap();
        let account_id = Uuid::new_v4().to_string();
        store
            .upsert_account(&AccountRecord {
                id: account_id.clone(),
                owner_id: owner.id.clone(),
                provider: "custom".into(),
                email: "events@example.com".into(),
                display_name: "Events".into(),
                group: "工作".into(),
                group_icon: "folder".into(),
                color: "#168f78".into(),
                settings: json!({}),
                proxy: None,
                encrypted_secret: "encrypted-placeholder".into(),
                auth_method: Some("app-password".into()),
                created_at: "2026-08-10T00:00:00.000Z".into(),
                last_sync_at: None,
                status: "connected".into(),
                last_error: None,
                mailboxes: json!([]),
            })
            .unwrap();
        store
            .set_user_metadata(
                &owner.id,
                "external_access_v1",
                r#"{"gatewayEnabled":true,"mcpEnabled":false}"#,
            )
            .unwrap();
        let issued = store
            .issue_developer_token(
                &owner.id,
                "WebSocket",
                &["messages:read".into()],
                std::slice::from_ref(&account_id),
                3600,
            )
            .unwrap();
        drop(store);
        let mut config = HttpAdapterConfig::new(&directory);
        config.gateway = true;
        let router = build_router(config).unwrap();
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
        });

        let url = format!("ws://{address}/gateway/v1/events");
        let mut rejected = url.clone().into_client_request().unwrap();
        rejected
            .headers_mut()
            .insert("origin", "https://attacker.example".parse().unwrap());
        assert!(matches!(
            tokio_tungstenite::connect_async(rejected).await,
            Err(tokio_tungstenite::tungstenite::Error::Http(response))
                if response.status() == StatusCode::FORBIDDEN
        ));

        let mut request = url.into_client_request().unwrap();
        request.headers_mut().insert(
            "authorization",
            format!("Bearer {}", issued.raw).parse().unwrap(),
        );
        request
            .headers_mut()
            .insert("origin", format!("http://{address}").parse().unwrap());
        let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        let connected = tokio::time::timeout(std::time::Duration::from_secs(2), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let Message::Text(connected) = connected else {
            panic!("expected connected text frame");
        };
        assert_eq!(
            serde_json::from_str::<Value>(&connected).unwrap()["type"],
            "connected"
        );

        let event_message = imail_protocol::MessageReadModel {
            headers: Default::default(),
            id: "ws-message".into(),
            account_id: account_id.clone(),
            mailbox: "INBOX".into(),
            mailbox_role: "inbox".into(),
            uid: 1,
            message_id: None,
            from: json!({ "name": "Sender", "address": "sender@example.net" }),
            to: json!([{ "name": "Events", "address": "events@example.com" }]),
            subject: "WebSocket event".into(),
            preview: "Event preview".into(),
            text: "must not be emitted".into(),
            html: Some("<p>must not be emitted</p>".into()),
            date: "2026-08-10T10:00:00.000Z".into(),
            unread: true,
            flagged: false,
            has_attachments: false,
            attachments: json!([]),
            labels: json!([]),
            snoozed_until: None,
        };
        SyncRuntimeStore::open_database(&database)
            .unwrap()
            .record_message_created(
                &account_id,
                "events@example.com",
                &[event_message],
                chrono::Utc::now(),
            )
            .unwrap();
        let event = tokio::time::timeout(std::time::Duration::from_secs(3), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let Message::Text(event) = event else {
            panic!("expected message.created text frame");
        };
        let event = serde_json::from_str::<Value>(&event).unwrap();
        assert_eq!(event["type"], "message.created");
        assert_eq!(event["data"]["message"]["id"], "ws-message");
        assert!(event["data"]["message"].get("text").is_none());
        assert!(event.to_string().find(&account_id).is_none());

        SqliteAuthStore::open_database(&database)
            .unwrap()
            .revoke_developer_token(&owner.id, &issued.token.id)
            .unwrap();
        let closed = tokio::time::timeout(std::time::Duration::from_secs(3), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(
            matches!(closed, Message::Close(Some(frame)) if frame.code == tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Policy)
        );
        drop(socket);
        server.abort();
        let _ = server.await;
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn emits_credentials_cors_only_for_an_exact_valid_origin() {
        let directory = temporary_directory("cors");
        let mut config = HttpAdapterConfig::new(&directory);
        config.cors_origins = ["https://desktop.example.com".into()].into_iter().collect();
        let router = build_router(config).unwrap();
        let allowed = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .header(ORIGIN, "https://desktop.example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            allowed.headers()[ACCESS_CONTROL_ALLOW_ORIGIN],
            "https://desktop.example.com"
        );
        assert_eq!(allowed.headers()[ACCESS_CONTROL_ALLOW_CREDENTIALS], "true");

        let denied = router
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .header(ORIGIN, "https://attacker.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(denied.headers().get(ACCESS_CONTROL_ALLOW_ORIGIN).is_none());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_unsafe_origin_and_host_configuration() {
        let directory = temporary_directory("config");
        let mut config = HttpAdapterConfig::new(&directory);
        config.cors_origins = ["http://mail.example.com".into()].into_iter().collect();
        assert!(matches!(
            build_router(config),
            Err(HttpAdapterError::Config(ConfigError::InvalidCorsOrigin(_)))
        ));
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn registers_resolves_and_revokes_a_persistent_application_session() {
        let directory = authentication_directory("auth-session");
        let router = build_router(HttpAdapterConfig::new(&directory)).unwrap();
        let registration = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/auth/register")
                    .header(HOST, "127.0.0.1:8787")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"login":" Test.Owner ","displayName":" Test Owner ","password":"test-password-123"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(registration.status(), StatusCode::CREATED);
        let set_cookie = registration.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .to_string();
        assert!(set_cookie.starts_with("imail_session="));
        assert!(set_cookie.contains("HttpOnly"));
        assert!(set_cookie.contains("SameSite=Lax"));
        assert!(!set_cookie.contains("Secure"));
        let cookie = set_cookie.split(';').next().unwrap().to_string();
        let registered = json(registration).await;
        assert_eq!(registered["user"]["login"], "test.owner");
        assert_eq!(registered["user"]["displayName"], "Test Owner");
        assert!(registered["user"].get("createdAt").is_none());

        let status = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/auth/status")
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = json(status).await;
        assert_eq!(status["setupRequired"], false);
        assert_eq!(status["registrationOpen"], true);
        assert_eq!(status["user"]["id"], registered["user"]["id"]);

        let session = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/auth/session")
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(session.status(), StatusCode::OK);

        let logout = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/auth/logout")
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(logout.status(), StatusCode::NO_CONTENT);
        assert!(logout.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .contains("Max-Age=0"));
        let expired = router
            .oneshot(
                Request::builder()
                    .uri("/api/auth/session")
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(expired.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(json(expired).await["error"], "请先登录");
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn allows_only_one_concurrent_production_setup_and_uses_secure_cookies() {
        let directory = authentication_directory("auth-production");
        let router = build_router(HttpAdapterConfig::production(&directory)).unwrap();
        let register = |login: &'static str| {
            router.clone().oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/auth/register")
                    .header(HOST, "localhost:443")
                    .header(ORIGIN, "https://localhost")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(
                        r#"{{"login":"{login}","displayName":"Owner","password":"test-password-123"}}"#
                    )))
                    .unwrap(),
            )
        };
        let (first, second) =
            tokio::join!(register("production-owner"), register("production-other"));
        let responses = [first.unwrap(), second.unwrap()];
        assert_eq!(
            responses
                .iter()
                .filter(|response| response.status() == StatusCode::CREATED)
                .count(),
            1
        );
        assert_eq!(
            responses
                .iter()
                .filter(|response| response.status() == StatusCode::FORBIDDEN)
                .count(),
            1
        );
        let created = responses
            .iter()
            .find(|response| response.status() == StatusCode::CREATED)
            .unwrap();
        assert!(created.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .contains("Secure"));
        assert!(created.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .contains("SameSite=Lax"));
        let denied = responses
            .into_iter()
            .find(|response| response.status() == StatusCode::FORBIDDEN)
            .unwrap();
        assert_eq!(json(denied).await["error"], "此服务已关闭新用户注册");
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn enforces_persistent_login_limits_before_password_verification() {
        let directory = authentication_directory("auth-limit");
        let mut store = SqliteAuthStore::open_database(directory.join("imail.sqlite")).unwrap();
        store
            .create_user("limited-owner", "Limited", "test-password-123")
            .unwrap();
        for _ in 0..10 {
            assert!(
                store
                    .consume_attempt("login-account:limited-owner", 10, 15 * 60_000)
                    .unwrap()
                    .allowed
            );
        }
        drop(store);
        let response = build_router(HttpAdapterConfig::new(&directory))
            .unwrap()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/auth/login")
                    .header(HOST, "127.0.0.1:8787")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"login":"limited-owner","password":"wrong-password"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(response.headers().get("retry-after").is_some());
        assert_eq!(json(response).await["error"], "尝试过多，请稍后再试");
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn protects_and_isolates_preferences_with_the_session_user_context() {
        let directory = authentication_directory("preferences");
        let mut store = SqliteAuthStore::open_database(directory.join("imail.sqlite")).unwrap();
        let owner = store
            .create_user("preferences-owner", "Owner", "owner-password-123")
            .unwrap();
        let other = store
            .create_user("preferences-other", "Other", "other-password-123")
            .unwrap();
        let owner_session = store.create_session(&owner.id).unwrap();
        let other_session = store.create_session(&other.id).unwrap();
        drop(store);
        let router = build_router(HttpAdapterConfig::new(&directory)).unwrap();

        let unauthorized = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/preferences")
                    .header(HOST, "127.0.0.1:8787")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(json(unauthorized).await["error"], "登录已过期，请重新登录");

        let updated = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri("/api/preferences")
                    .header(HOST, "127.0.0.1:8787")
                    .header(CONTENT_TYPE, "application/json")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::from(format!(
                        r##"{{"userId":"{}","theme":"constructivist-red","customTheme":{{"name":"服务端构成","canvas":"#d8d0be","surface":"#f5eedb","surfaceSubtle":"#eee5d1","rail":"#24201e","text":"#24201e","textSecondary":"#5e5751","border":"#b9ae9d","accent":"#c42a22","accentSubtle":"#e9c9bf","radius":"compact","shadow":"offset","typography":"technical"}},"startupView":"starred","defaultMessageView":"rendered","notificationKinds":{{"snooze":false}}}}"##,
                        other.id
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(updated.status(), StatusCode::OK);
        let updated = json(updated).await;
        assert_eq!(updated["preferences"]["theme"], "constructivist-red");
        assert_eq!(updated["preferences"]["customTheme"]["name"], "服务端构成");
        assert_eq!(updated["preferences"]["customTheme"]["accent"], "#c42a22");
        assert_eq!(updated["preferences"]["startupView"], "starred");
        assert_eq!(updated["preferences"]["defaultMessageView"], "rendered");
        assert_eq!(updated["preferences"]["notificationKinds"]["unread"], true);
        assert_eq!(updated["preferences"]["notificationKinds"]["snooze"], false);
        assert_eq!(
            updated["preferences"]["shortcutBindings"]["focusSearch"],
            "Mod+K"
        );

        let isolated = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/preferences")
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={other_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let isolated = json(isolated).await;
        assert_eq!(isolated["preferences"]["theme"], "mint-fresh");
        assert_eq!(isolated["preferences"]["startupView"], "inbox");

        let invalid = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri("/api/preferences")
                    .header(HOST, "127.0.0.1:8787")
                    .header(CONTENT_TYPE, "application/json")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::from(r#"{"startupView":"invalid"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

        let invalid_custom_theme = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri("/api/preferences")
                    .header(HOST, "127.0.0.1:8787")
                    .header(CONTENT_TYPE, "application/json")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::from(
                        r#"{"customTheme":{"name":"unsafe","canvas":"red"}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invalid_custom_theme.status(), StatusCode::BAD_REQUEST);

        let persisted = router
            .oneshot(
                Request::builder()
                    .uri("/api/preferences")
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(json(persisted).await, updated);
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn exposes_provider_catalog_and_enforces_private_export_and_clear_contracts() {
        let directory = authentication_directory("security-http");
        let key_hex = "67".repeat(32);
        fs::write(directory.join("master.key"), &key_hex).unwrap();
        let key = MasterKey::from_hex(&key_hex).unwrap();
        let mut store = SqliteAuthStore::open_database(directory.join("imail.sqlite")).unwrap();
        let owner = store
            .create_user("privacy-owner", "Owner", "owner-password-123")
            .unwrap();
        let other = store
            .create_user("privacy-other", "Other", "other-password-123")
            .unwrap();
        let owner_session = store.create_session(&owner.id).unwrap();
        let other_session = store.create_session(&other.id).unwrap();
        for (id, user_id, email, password) in [
            (
                "owner-account",
                &owner.id,
                "owner@example.com",
                "owner-mail-secret",
            ),
            (
                "other-account",
                &other.id,
                "other@example.com",
                "other-mail-secret",
            ),
        ] {
            store
                .upsert_account(&AccountRecord {
                    id: id.into(),
                    owner_id: user_id.clone(),
                    provider: "gmail".into(),
                    email: email.into(),
                    display_name: email.into(),
                    group: "个人".into(),
                    group_icon: "folder".into(),
                    color: "#168f78".into(),
                    settings: json!({
                        "imapHost": "imap.gmail.com", "imapPort": 993, "imapSecure": true,
                        "smtpHost": "smtp.gmail.com", "smtpPort": 465, "smtpSecure": true
                    }),
                    proxy: None,
                    encrypted_secret: key.encrypt_json(&json!({ "password": password })).unwrap(),
                    auth_method: Some("app-password".into()),
                    created_at: "2026-08-10T00:00:00.000Z".into(),
                    last_sync_at: None,
                    status: "connected".into(),
                    last_error: None,
                    mailboxes: json!([]),
                })
                .unwrap();
        }
        drop(store);
        let router = build_router(HttpAdapterConfig::new(&directory)).unwrap();

        let unauthorized_providers = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/providers")
                    .header(HOST, "127.0.0.1:8787")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized_providers.status(), StatusCode::UNAUTHORIZED);
        let providers = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/providers")
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(providers.status(), StatusCode::OK);
        let providers = json(providers).await;
        assert_eq!(
            providers["providers"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["outlook", "gmail", "qq", "yahoo", "hotmail", "icloud", "custom"]
        );
        assert_eq!(providers["oauth"][0]["id"], "google");
        assert!(providers.to_string().find("clientSecret").is_none());

        let wrong_password = router
            .clone()
            .oneshot(Request::builder().method(Method::POST).uri("/api/security/mail-authorization-exports").header(HOST, "127.0.0.1:8787").header(CONTENT_TYPE, "application/json").header("cookie", format!("imail_session={owner_session}")).body(Body::from(r#"{"currentPassword":"wrong-password","exportPassword":"portable-password-123"}"#)).unwrap())
            .await
            .unwrap();
        assert_eq!(wrong_password.status(), StatusCode::FORBIDDEN);

        let prepared = router
            .clone()
            .oneshot(Request::builder().method(Method::POST).uri("/api/security/mail-authorization-exports").header(HOST, "127.0.0.1:8787").header(CONTENT_TYPE, "application/json").header("cookie", format!("imail_session={owner_session}")).body(Body::from(r#"{"currentPassword":"owner-password-123","exportPassword":"portable-password-123"}"#)).unwrap())
            .await
            .unwrap();
        assert_eq!(prepared.status(), StatusCode::OK);
        assert_eq!(
            prepared.headers()[CACHE_CONTROL],
            "private, no-store, max-age=0"
        );
        let prepared = json(prepared).await;
        assert_eq!(prepared["accountCount"], 1);
        let download_path = prepared["downloadPath"].as_str().unwrap().to_string();

        let isolated = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&download_path)
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={other_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(isolated.status(), StatusCode::NOT_FOUND);
        let download = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&download_path)
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(download.status(), StatusCode::OK);
        assert_eq!(
            download.headers()[CONTENT_TYPE],
            "application/vnd.imail.mail-authorization-export+json; charset=utf-8"
        );
        assert!(download.headers()[CONTENT_DISPOSITION]
            .to_str()
            .unwrap()
            .contains(".imailauth"));
        let envelope: imail_protocol::MailAuthorizationExportEnvelope =
            serde_json::from_slice(&to_bytes(download.into_body(), 1024 * 1024).await.unwrap())
                .unwrap();
        let plaintext = decrypt_portable_export(
            &PortableEncryptedPayload {
                salt: envelope.kdf.salt,
                iv: envelope.cipher.iv,
                auth_tag: envelope.cipher.auth_tag,
                ciphertext: envelope.ciphertext,
            },
            "portable-password-123",
            b"imail-mail-authorizations:v1",
        )
        .unwrap();
        let payload: imail_protocol::MailAuthorizationExportPayload =
            serde_json::from_slice(&plaintext).unwrap();
        assert_eq!(payload.accounts.len(), 1);
        assert_eq!(payload.accounts[0].email, "owner@example.com");
        assert_eq!(
            payload.accounts[0].authorization.password.as_deref(),
            Some("owner-mail-secret")
        );
        assert!(!String::from_utf8_lossy(&plaintext).contains("other-mail-secret"));
        let consumed = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&download_path)
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(consumed.status(), StatusCode::NOT_FOUND);

        let pending = router
            .clone()
            .oneshot(Request::builder().method(Method::POST).uri("/api/security/mail-authorization-exports").header(HOST, "127.0.0.1:8787").header(CONTENT_TYPE, "application/json").header("cookie", format!("imail_session={owner_session}")).body(Body::from(r#"{"currentPassword":"owner-password-123","exportPassword":"portable-password-456"}"#)).unwrap())
            .await
            .unwrap();
        let pending_path = json(pending).await["downloadPath"]
            .as_str()
            .unwrap()
            .to_string();
        let invalid_confirmation = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/security/clear-user-data")
                    .header(HOST, "127.0.0.1:8787")
                    .header(CONTENT_TYPE, "application/json")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::from(
                        r#"{"currentPassword":"owner-password-123","confirmation":"clear"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invalid_confirmation.status(), StatusCode::BAD_REQUEST);
        let cleared = router
            .clone()
            .oneshot(Request::builder().method(Method::POST).uri("/api/security/clear-user-data").header(HOST, "127.0.0.1:8787").header(CONTENT_TYPE, "application/json").header("cookie", format!("imail_session={owner_session}")).body(Body::from(r#"{"currentPassword":"owner-password-123","confirmation":"清除我的邮箱数据"}"#)).unwrap())
            .await
            .unwrap();
        assert_eq!(cleared.status(), StatusCode::NO_CONTENT);
        let invalidated = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&pending_path)
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invalidated.status(), StatusCode::NOT_FOUND);

        for (session, expected_count) in [(&owner_session, 0), (&other_session, 1)] {
            let accounts = router
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/api/accounts")
                        .header(HOST, "127.0.0.1:8787")
                        .header("cookie", format!("imail_session={session}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                json(accounts).await["accounts"].as_array().unwrap().len(),
                expected_count
            );
        }
        let session_still_valid = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/auth/session")
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(session_still_valid.status(), StatusCode::OK);
        let audit = router
            .oneshot(
                Request::builder()
                    .uri("/api/security/audit-events?limit=20")
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let audit = json(audit).await;
        let event_types = audit["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|event| event["eventType"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(event_types.contains(&"sensitive-action.reauthentication-failed"));
        assert!(event_types.contains(&"privacy.mail-authorization-export.prepared"));
        assert!(event_types.contains(&"privacy.mail-authorization-export.downloaded"));
        assert!(event_types.contains(&"privacy.user-data-cleared"));
        assert!(!audit.to_string().contains("owner-password-123"));
        assert!(!audit.to_string().contains("owner-mail-secret"));

        let mut store = SqliteAuthStore::open_database(directory.join("imail.sqlite")).unwrap();
        let rate_key = format!("sensitive-action:clear-user-data:user:{}", other.id);
        for _ in 0..5 {
            assert!(
                store
                    .consume_attempt(&rate_key, 5, 15 * 60_000)
                    .unwrap()
                    .allowed
            );
        }
        drop(store);
        let limited = build_router(HttpAdapterConfig::new(&directory))
            .unwrap()
            .oneshot(Request::builder().method(Method::POST).uri("/api/security/clear-user-data").header(HOST, "127.0.0.1:8787").header(CONTENT_TYPE, "application/json").header("cookie", format!("imail_session={other_session}")).body(Body::from(r#"{"currentPassword":"other-password-123","confirmation":"清除我的邮箱数据"}"#)).unwrap())
            .await
            .unwrap();
        assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(limited.headers().contains_key("retry-after"));
        assert_eq!(
            SqliteAuthStore::open_database(directory.join("imail.sqlite"))
                .unwrap()
                .list_accounts(&other.id)
                .unwrap()
                .len(),
            1
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn creates_updates_lists_and_deletes_only_owned_drafts() {
        let directory = authentication_directory("drafts");
        let mut store = SqliteAuthStore::open_database(directory.join("imail.sqlite")).unwrap();
        let owner = store
            .create_user("draft-http-owner", "Owner", "owner-password-123")
            .unwrap();
        let other = store
            .create_user("draft-http-other", "Other", "other-password-123")
            .unwrap();
        let owner_session = store.create_session(&owner.id).unwrap();
        let other_session = store.create_session(&other.id).unwrap();
        let account_id = Uuid::new_v4().to_string();
        store
            .upsert_account(&AccountRecord {
                id: account_id.clone(),
                owner_id: owner.id.clone(),
                provider: "custom".into(),
                email: "draft-owner@example.com".into(),
                display_name: "Draft Owner".into(),
                group: "个人".into(),
                group_icon: "folder".into(),
                color: "#168f78".into(),
                settings: json!({}),
                proxy: None,
                encrypted_secret: "encrypted".into(),
                auth_method: Some("app-password".into()),
                created_at: "2026-08-10T00:00:00.000Z".into(),
                last_sync_at: None,
                status: "connected".into(),
                last_error: None,
                mailboxes: json!([]),
            })
            .unwrap();
        drop(store);
        let router = build_router(HttpAdapterConfig::new(&directory)).unwrap();
        let draft_id = Uuid::new_v4().to_string();

        let created = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/drafts")
                    .header(HOST, "127.0.0.1:8787")
                    .header(CONTENT_TYPE, "application/json")
                    .header("x-draft-id", &draft_id)
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::from(format!(
                        r#"{{"accountId":"{account_id}","to":[" recipient@example.com "],"subject":"First"}}"#
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::CREATED);
        let created = json(created).await;
        assert_eq!(created["draft"]["id"], draft_id);
        assert_eq!(created["draft"]["to"], json!(["recipient@example.com"]));
        assert_eq!(created["draft"]["cc"], json!([]));
        assert_eq!(created["draft"]["html"], "");
        let created_at = created["draft"]["createdAt"].clone();

        let replaced = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/drafts")
                    .header(HOST, "127.0.0.1:8787")
                    .header(CONTENT_TYPE, "application/json")
                    .header("x-draft-id", &draft_id)
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::from(format!(
                        r#"{{"accountId":"{account_id}","subject":"Replaced"}}"#
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        let replaced = json(replaced).await;
        assert_eq!(replaced["draft"]["createdAt"], created_at);
        assert_eq!(replaced["draft"]["subject"], "Replaced");

        let isolated = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/drafts")
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={other_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(json(isolated).await["drafts"], json!([]));

        let forbidden_update = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/api/drafts/{draft_id}"))
                    .header(HOST, "127.0.0.1:8787")
                    .header(CONTENT_TYPE, "application/json")
                    .header("cookie", format!("imail_session={other_session}"))
                    .body(Body::from(format!(
                        r#"{{"accountId":"{account_id}","subject":"Stolen"}}"#
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(forbidden_update.status(), StatusCode::NOT_FOUND);

        let oversized = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/drafts")
                    .header(HOST, "127.0.0.1:8787")
                    .header(CONTENT_TYPE, "application/json")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::from(format!(
                        r#"{{"accountId":"{account_id}","attachments":[{{"id":"a","filename":"a.bin","contentType":"application/octet-stream","size":5242881,"data":""}}]}}"#
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(oversized.status(), StatusCode::BAD_REQUEST);

        let foreign_delete = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/api/drafts/{draft_id}"))
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={other_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(foreign_delete.status(), StatusCode::NO_CONTENT);

        let removed = router
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/api/drafts/{draft_id}"))
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(removed.status(), StatusCode::NO_CONTENT);
        assert!(
            SqliteAuthStore::open_database(directory.join("imail.sqlite"))
                .unwrap()
                .list_drafts(&owner.id)
                .unwrap()
                .is_empty()
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn isolates_sync_policies_status_and_jobs_by_session_owner() {
        let directory = authentication_directory("sync-control");
        let database = directory.join("imail.sqlite");
        let mut store = SqliteAuthStore::open_database(&database).unwrap();
        let owner = store
            .create_user("sync-owner", "Owner", "owner-password-123")
            .unwrap();
        let other = store
            .create_user("sync-other", "Other", "other-password-123")
            .unwrap();
        let owner_session = store.create_session(&owner.id).unwrap();
        let other_session = store.create_session(&other.id).unwrap();
        let owner_account_id = Uuid::new_v4().to_string();
        let other_account_id = Uuid::new_v4().to_string();
        let account = |id: String, owner_id: String, email: &str, mailboxes: Value| AccountRecord {
            id,
            owner_id,
            provider: "custom".into(),
            email: email.into(),
            display_name: email.into(),
            group: "个人".into(),
            group_icon: "folder".into(),
            color: "#168f78".into(),
            settings: json!({}),
            proxy: None,
            encrypted_secret: "encrypted".into(),
            auth_method: Some("app-password".into()),
            created_at: "2026-08-10T00:00:00.000Z".into(),
            last_sync_at: None,
            status: "connected".into(),
            last_error: None,
            mailboxes,
        };
        store
            .upsert_account(&account(
                owner_account_id.clone(),
                owner.id.clone(),
                "owner@example.com",
                json!([
                    {"path":"INBOX","specialUse":"\\Inbox","selectable":true},
                    {"path":"Archive","specialUse":"\\Archive","selectable":true},
                    {"path":"Hidden","selectable":false}
                ]),
            ))
            .unwrap();
        store
            .upsert_account(&account(
                other_account_id.clone(),
                other.id.clone(),
                "other@example.com",
                json!([]),
            ))
            .unwrap();
        drop(store);
        let mut sync = SyncRuntimeStore::open_database(&database).unwrap();
        let owner_job = sync
            .enqueue(
                &SyncEnqueue {
                    account_id: owner_account_id.clone(),
                    mailbox: None,
                    mailbox_role: "inbox".into(),
                    reason: "manual".into(),
                    priority: 100,
                    not_before: None,
                },
                chrono::Utc::now(),
            )
            .unwrap();
        let other_job = sync
            .enqueue(
                &SyncEnqueue {
                    account_id: other_account_id.clone(),
                    mailbox: None,
                    mailbox_role: "inbox".into(),
                    reason: "manual".into(),
                    priority: 100,
                    not_before: None,
                },
                chrono::Utc::now(),
            )
            .unwrap();
        let owner_event_job = sync
            .enqueue(
                &SyncEnqueue {
                    account_id: owner_account_id.clone(),
                    mailbox: None,
                    mailbox_role: "sent".into(),
                    reason: "manual".into(),
                    priority: 200,
                    not_before: None,
                },
                chrono::Utc::now(),
            )
            .unwrap();
        let other_event_job = sync
            .enqueue(
                &SyncEnqueue {
                    account_id: other_account_id.clone(),
                    mailbox: None,
                    mailbox_role: "sent".into(),
                    reason: "manual".into(),
                    priority: 199,
                    not_before: None,
                },
                chrono::Utc::now(),
            )
            .unwrap();
        let claimed_owner = sync
            .claim_next(
                "sse-worker",
                std::time::Duration::from_secs(30),
                chrono::Utc::now(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(claimed_owner.id, owner_event_job.id);
        sync.mark_started(&claimed_owner, "Sent", chrono::Utc::now())
            .unwrap();
        let claimed_other = sync
            .claim_next(
                "sse-worker",
                std::time::Duration::from_secs(30),
                chrono::Utc::now(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(claimed_other.id, other_event_job.id);
        sync.mark_started(&claimed_other, "Sent", chrono::Utc::now())
            .unwrap();
        drop(sync);
        let router = build_router(HttpAdapterConfig::new(&directory)).unwrap();

        let defaults = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/sync-policy")
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let defaults = json(defaults).await;
        assert_eq!(defaults["policy"]["folderMode"], "inbox");
        assert_eq!(defaults["policy"]["notifyOnError"], true);

        let updated_default = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri("/api/sync-policy")
                    .header(HOST, "127.0.0.1:8787")
                    .header(CONTENT_TYPE, "application/json")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::from(
                        r#"{"folderMode":"standard","notifyOnError":false}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let updated_default = json(updated_default).await;
        assert_eq!(updated_default["policy"]["folderMode"], "standard");
        assert_eq!(updated_default["policy"]["notifyOnError"], false);

        let isolated_default = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/sync-policy")
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={other_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            json(isolated_default).await["policy"]["folderMode"],
            "inbox"
        );

        let forbidden_policy = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/accounts/{owner_account_id}/sync-policy"))
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={other_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(forbidden_policy.status(), StatusCode::NOT_FOUND);

        let account_policy = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri(format!("/api/accounts/{owner_account_id}/sync-policy"))
                    .header(HOST, "127.0.0.1:8787")
                    .header(CONTENT_TYPE, "application/json")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::from(
                        r#"{"folderMode":"selected","selectedMailboxes":[" Archive "]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(account_policy.status(), StatusCode::OK);
        let account_policy = json(account_policy).await;
        assert_eq!(account_policy["policy"]["folderMode"], "selected");
        assert_eq!(
            account_policy["policy"]["selectedMailboxes"],
            json!(["Archive"])
        );

        let invalid_selection = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri(format!("/api/accounts/{owner_account_id}/sync-policy"))
                    .header(HOST, "127.0.0.1:8787")
                    .header(CONTENT_TYPE, "application/json")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::from(r#"{"selectedMailboxes":["Hidden"]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invalid_selection.status(), StatusCode::BAD_REQUEST);

        let status = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/sync-status")
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = json(status).await;
        assert_eq!(status["accounts"].as_array().unwrap().len(), 1);
        assert_eq!(status["accounts"][0]["accountId"], owner_account_id);
        assert!(status["accounts"][0]["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|job| job["id"] == owner_job.id));
        assert!(status.to_string().find(&other_account_id).is_none());
        assert!(status.to_string().find(&other_job.id).is_none());

        let event_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/events?after=0")
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            event_response.headers()[CONTENT_TYPE],
            "text/event-stream; charset=utf-8"
        );
        assert_eq!(
            event_response.headers()["cache-control"],
            "no-cache, no-transform"
        );
        let mut event_stream = event_response.into_body().into_data_stream();
        let initial = tokio::time::timeout(std::time::Duration::from_secs(2), event_stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let initial = String::from_utf8(initial.to_vec()).unwrap();
        assert!(initial.contains("event: connected"));
        assert!(initial.contains("event: sync.started"));
        assert!(initial.contains("event: sync.status"));
        assert!(initial.contains(&owner_event_job.id));
        assert!(!initial.contains(&other_event_job.id));
        assert!(!initial.contains(&other_account_id));
        let mut live_sync = SyncRuntimeStore::open_database(&database).unwrap();
        let live_job = live_sync
            .enqueue(
                &SyncEnqueue {
                    account_id: owner_account_id.clone(),
                    mailbox: None,
                    mailbox_role: "archive".into(),
                    reason: "manual".into(),
                    priority: 300,
                    not_before: None,
                },
                chrono::Utc::now(),
            )
            .unwrap();
        let claimed_live = live_sync
            .claim_next(
                "sse-worker",
                std::time::Duration::from_secs(30),
                chrono::Utc::now(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(claimed_live.id, live_job.id);
        live_sync
            .mark_started(&claimed_live, "Archive", chrono::Utc::now())
            .unwrap();
        drop(live_sync);
        let live = tokio::time::timeout(std::time::Duration::from_secs(3), event_stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let live = String::from_utf8(live.to_vec()).unwrap();
        assert!(live.contains("event: sync.started"));
        assert!(live.contains(&live_job.id));
        assert!(live.contains("event: sync.status"));
        drop(event_stream);

        let resumed = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/events?after=0")
                    .header(HOST, "127.0.0.1:8787")
                    .header("last-event-id", "9223372036854775807")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let mut resumed_stream = resumed.into_body().into_data_stream();
        let resumed = resumed_stream.next().await.unwrap().unwrap();
        let resumed = String::from_utf8(resumed.to_vec()).unwrap();
        assert!(resumed.contains("event: connected"));
        assert!(!resumed.contains("event: sync.started"));
        drop(resumed_stream);

        let owned_job = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/sync-jobs/{}", owner_job.id))
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(owned_job.status(), StatusCode::OK);

        let foreign_job = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/sync-jobs/{}", other_job.id))
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(foreign_job.status(), StatusCode::NOT_FOUND);

        let disabled = router
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri(format!("/api/accounts/{owner_account_id}/sync-policy"))
                    .header(HOST, "127.0.0.1:8787")
                    .header(CONTENT_TYPE, "application/json")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::from(r#"{"enabled":false}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(json(disabled).await["policy"]["enabled"], false);
        assert_eq!(
            SyncRuntimeStore::open_database(&database)
                .unwrap()
                .job(&owner_job.id)
                .unwrap()
                .unwrap()
                .status,
            "cancelled"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn lists_and_updates_only_owned_accounts_without_exposing_internal_fields() {
        let directory = authentication_directory("accounts");
        let mut store = SqliteAuthStore::open_database(directory.join("imail.sqlite")).unwrap();
        let owner = store
            .create_user("account-owner", "Owner", "owner-password-123")
            .unwrap();
        let other = store
            .create_user("account-other", "Other", "other-password-123")
            .unwrap();
        let owner_session = store.create_session(&owner.id).unwrap();
        let other_session = store.create_session(&other.id).unwrap();
        let account_id = Uuid::new_v4().to_string();
        store
            .upsert_account(&AccountRecord {
                id: account_id.clone(),
                owner_id: owner.id.clone(),
                provider: "gmail".into(),
                email: "owner@example.com".into(),
                display_name: "Owner Mail".into(),
                group: "个人".into(),
                group_icon: "folder".into(),
                color: "#168f78".into(),
                settings: json!({
                    "imapHost": "imap.gmail.com", "imapPort": 993, "imapSecure": true,
                    "smtpHost": "smtp.gmail.com", "smtpPort": 465, "smtpSecure": true,
                    "password": "settings-plaintext-must-never-leak"
                }),
                proxy: Some(json!({
                    "protocol": "socks5", "host": "127.0.0.1", "port": 1080,
                    "username": "proxy-user", "password": "proxy-plaintext-must-never-leak",
                    "proxyPassword": "proxy-secret-must-never-leak"
                })),
                encrypted_secret: "encrypted-must-never-leak".into(),
                auth_method: Some("oauth2".into()),
                created_at: "2026-08-10T00:00:00.000Z".into(),
                last_sync_at: None,
                status: "connected".into(),
                last_error: None,
                mailboxes: json!([]),
            })
            .unwrap();
        drop(store);
        let router = build_router(HttpAdapterConfig::new(&directory)).unwrap();

        let owner_list = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/accounts")
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(owner_list.status(), StatusCode::OK);
        let owner_list = json(owner_list).await;
        assert_eq!(owner_list["accounts"].as_array().unwrap().len(), 1);
        let account = &owner_list["accounts"][0];
        assert_eq!(account["id"], account_id);
        assert_eq!(account["authMethod"], "oauth2");
        assert_eq!(account["groupIcon"], "folder");
        assert!(account.get("ownerId").is_none());
        assert!(account.get("encryptedSecret").is_none());
        assert!(account.get("lastError").is_none());
        assert!(!owner_list.to_string().contains("encrypted-must-never-leak"));
        assert!(!owner_list.to_string().contains("proxyPassword"));
        assert!(!owner_list
            .to_string()
            .contains("settings-plaintext-must-never-leak"));
        assert!(!owner_list
            .to_string()
            .contains("proxy-plaintext-must-never-leak"));
        assert!(!owner_list
            .to_string()
            .contains("proxy-secret-must-never-leak"));

        let isolated = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/accounts")
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={other_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(json(isolated).await["accounts"], json!([]));

        let forbidden = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri(format!("/api/accounts/{account_id}"))
                    .header(HOST, "127.0.0.1:8787")
                    .header(CONTENT_TYPE, "application/json")
                    .header("cookie", format!("imail_session={other_session}"))
                    .body(Body::from(r#"{"displayName":"Stolen"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(forbidden.status(), StatusCode::NOT_FOUND);
        assert_eq!(json(forbidden).await["error"], "邮箱账户不存在");

        let updated = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri(format!("/api/accounts/{account_id}"))
                    .header(HOST, "127.0.0.1:8787")
                    .header(CONTENT_TYPE, "application/json")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::from(format!(
                        r##"{{"ownerId":"{}","displayName":" Updated Mail ","group":" Work ","groupIcon":"briefcase","color":"#123aBc"}}"##,
                        other.id
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(updated.status(), StatusCode::OK);
        let updated = json(updated).await;
        assert_eq!(updated["account"]["displayName"], "Updated Mail");
        assert_eq!(updated["account"]["group"], "Work");
        assert_eq!(updated["account"]["groupIcon"], "briefcase");
        assert_eq!(updated["account"]["color"], "#123aBc");
        assert!(updated["account"].get("ownerId").is_none());

        let invalid = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri(format!("/api/accounts/{account_id}"))
                    .header(HOST, "127.0.0.1:8787")
                    .header(CONTENT_TYPE, "application/json")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json(invalid).await["error"], "至少提供一个要更新的字段");

        let forbidden_sync = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/api/accounts/{account_id}/sync"))
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={other_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(forbidden_sync.status(), StatusCode::NOT_FOUND);

        let queued = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/api/accounts/{account_id}/sync"))
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(queued.status(), StatusCode::OK);
        let queued = json(queued).await;
        assert_eq!(queued["synced"], 0);
        assert_eq!(queued["queued"], true);
        let inbox_job_id = queued["jobId"].as_str().unwrap().to_string();

        let duplicate = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/api/accounts/{account_id}/mailboxes/sync"))
                    .header(HOST, "127.0.0.1:8787")
                    .header(CONTENT_TYPE, "application/json")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::from(r#"{"mailbox":"INBOX"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(json(duplicate).await["jobId"], inbox_job_id);

        let all = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/sync")
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let all = json(all).await;
        assert_eq!(all["results"].as_array().unwrap().len(), 1);
        assert_eq!(all["results"][0]["accountId"], account_id);
        assert_eq!(all["results"][0]["status"], "fulfilled");
        assert_eq!(all["results"][0]["jobId"], inbox_job_id);

        let forbidden_delete = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/api/accounts/{account_id}"))
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={other_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(forbidden_delete.status(), StatusCode::NOT_FOUND);

        let removed = router
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/api/accounts/{account_id}"))
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(removed.status(), StatusCode::NO_CONTENT);
        assert!(
            SyncRuntimeStore::open_database(directory.join("imail.sqlite"))
                .unwrap()
                .job(&inbox_job_id)
                .unwrap()
                .is_none()
        );
        assert!(
            SqliteAuthStore::open_database(directory.join("imail.sqlite"))
                .unwrap()
                .list_accounts(&owner.id)
                .unwrap()
                .is_empty()
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn validates_sensitive_account_changes_before_persisting_and_redacts_failures() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let directory = authentication_directory("account-sensitive");
        let database = directory.join("imail.sqlite");
        let key_hex = "42".repeat(32);
        fs::write(directory.join("master.key"), &key_hex).unwrap();
        let key = MasterKey::from_hex(&key_hex).unwrap();
        let mut store = SqliteAuthStore::open_database(&database).unwrap();
        let owner = store
            .create_user("sensitive-owner", "Owner", "owner-password-123")
            .unwrap();
        let other = store
            .create_user("sensitive-other", "Other", "other-password-123")
            .unwrap();
        let owner_session = store.create_session(&owner.id).unwrap();
        let other_session = store.create_session(&other.id).unwrap();
        let target_id = Uuid::new_v4().to_string();
        let source_id = Uuid::new_v4().to_string();
        let oauth_id = Uuid::new_v4().to_string();
        let settings = json!({
            "imapHost":"imap.example.com","imapPort":993,"imapSecure":true,
            "smtpHost":"smtp.example.com","smtpPort":465,"smtpSecure":true
        });
        let account = |id: String,
                       email: &str,
                       encrypted_secret: String,
                       auth_method: &str,
                       proxy: Option<Value>| AccountRecord {
            id,
            owner_id: owner.id.clone(),
            provider: "custom".into(),
            email: email.into(),
            display_name: email.into(),
            group: "个人".into(),
            group_icon: "folder".into(),
            color: "#168f78".into(),
            settings: settings.clone(),
            proxy,
            encrypted_secret,
            auth_method: Some(auth_method.into()),
            created_at: "2026-08-10T00:00:00.000Z".into(),
            last_sync_at: None,
            status: "connected".into(),
            last_error: None,
            mailboxes: json!([]),
        };
        store
            .upsert_account(&account(
                target_id.clone(),
                "target@example.com",
                key.encrypt_json(&json!({
                    "authType":"app-password","password":"old-password",
                    "proxyPassword":"old-proxy-secret"
                }))
                .unwrap(),
                "app-password",
                None,
            ))
            .unwrap();
        store
            .upsert_account(&account(
                source_id.clone(),
                "source@example.com",
                key.encrypt_json(&json!({
                    "authType":"app-password","password":"source-password",
                    "proxyPassword":"source-proxy-secret"
                }))
                .unwrap(),
                "app-password",
                Some(json!({
                    "protocol":"socks5","host":"proxy.example.com","port":1080,
                    "username":"proxy-user"
                })),
            ))
            .unwrap();
        store
            .upsert_account(&account(
                oauth_id.clone(),
                "oauth@example.com",
                key.encrypt_json(&json!({
                    "authType":"oauth2","accessToken":"oauth-access-secret"
                }))
                .unwrap(),
                "oauth2",
                None,
            ))
            .unwrap();
        drop(store);

        let force_failure = Arc::new(AtomicBool::new(false));
        let probe_failure = Arc::clone(&force_failure);
        let probe = move |config: &MailConnectionConfig| {
            if probe_failure.load(Ordering::SeqCst) {
                return Err("IMAP authentication failed password=connection-secret authorization=token-secret".into());
            }
            if matches!(&config.authentication, MailAuthentication::Password(password) if password == "reject-password")
            {
                return Err("IMAP authentication failed password=reject-password".into());
            }
            if config
                .proxy
                .as_ref()
                .is_some_and(|proxy| proxy.host == "reject.example.com")
            {
                return Err("proxy password=proxy-leak".into());
            }
            Ok(())
        };
        let router =
            build_router(HttpAdapterConfig::new(&directory).with_connection_probe(Arc::new(probe)))
                .unwrap();

        let invalid_custom = router.clone().oneshot(Request::builder().method(Method::POST).uri("/api/accounts").header(HOST, "127.0.0.1:8787").header(CONTENT_TYPE, "application/json").header("cookie", format!("imail_session={owner_session}")).body(Body::from(r#"{"provider":"custom","email":"missing-settings@example.com","displayName":"Missing","password":"password"}"#)).unwrap()).await.unwrap();
        assert_eq!(invalid_custom.status(), StatusCode::BAD_REQUEST);

        let rejected_create = router.clone().oneshot(Request::builder().method(Method::POST).uri("/api/accounts").header(HOST, "127.0.0.1:8787").header(CONTENT_TYPE, "application/json").header("cookie", format!("imail_session={owner_session}")).body(Body::from(r#"{"provider":"gmail","email":"rejected@example.com","displayName":"Rejected","password":"reject-password"}"#)).unwrap()).await.unwrap();
        assert_eq!(rejected_create.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let rejected_create = json(rejected_create).await;
        assert!(rejected_create["error"]
            .as_str()
            .unwrap()
            .contains("IMAP 认证失败"));
        assert!(!rejected_create.to_string().contains("reject-password"));
        assert!(SqliteAuthStore::open_database(&database)
            .unwrap()
            .list_accounts(&owner.id)
            .unwrap()
            .iter()
            .all(|account| account.email != "rejected@example.com"));

        let created = router.clone().oneshot(Request::builder().method(Method::POST).uri("/api/accounts").header(HOST, "127.0.0.1:8787").header(CONTENT_TYPE, "application/json").header("cookie", format!("imail_session={owner_session}")).body(Body::from(r#"{"provider":"custom","email":"Created@Example.com","displayName":" Created Account ","password":"created-password","settings":{"imapHost":"imap.created.example","imapPort":993,"imapSecure":true,"smtpHost":"smtp.created.example","smtpPort":465,"smtpSecure":true},"proxy":{"protocol":"socks5","host":"proxy.created.example","port":1080,"password":"created-proxy-secret"}}"#)).unwrap()).await.unwrap();
        assert_eq!(created.status(), StatusCode::CREATED);
        let created = json(created).await;
        let created_id = created["account"]["id"].as_str().unwrap().to_string();
        assert_eq!(created["account"]["email"], "created@example.com");
        assert_eq!(created["account"]["displayName"], "Created Account");
        assert!(!created.to_string().contains("created-password"));
        assert!(!created.to_string().contains("created-proxy-secret"));
        let created_record = SqliteAuthStore::open_database(&database)
            .unwrap()
            .account(&owner.id, &created_id)
            .unwrap()
            .unwrap();
        let created_secret: Value = key.decrypt_json(&created_record.encrypted_secret).unwrap();
        assert_eq!(created_secret["password"], "created-password");
        assert_eq!(created_secret["proxyPassword"], "created-proxy-secret");
        assert!(SyncRuntimeStore::open_database(&database)
            .unwrap()
            .policy(&created_id)
            .unwrap()
            .is_some());

        let token_account = router.clone().oneshot(Request::builder().method(Method::POST).uri("/api/accounts").header(HOST, "127.0.0.1:8787").header(CONTENT_TYPE, "application/json").header("cookie", format!("imail_session={owner_session}")).body(Body::from(r#"{"provider":"gmail","email":"direct-token@example.com","displayName":"Direct Token","accessToken":"direct-access-secret"}"#)).unwrap()).await.unwrap();
        assert_eq!(token_account.status(), StatusCode::CREATED);
        let token_account = json(token_account).await;
        assert_eq!(token_account["account"]["authMethod"], "oauth2");
        let token_account_id = token_account["account"]["id"].as_str().unwrap();
        let token_test = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/api/accounts/{token_account_id}/connection-test"))
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(json(token_test).await["account"]["status"], "connected");

        let concurrent_body = r#"{"provider":"gmail","email":"race-create@example.com","displayName":"Race","password":"race-password"}"#;
        let create_once = || {
            router.clone().oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/accounts")
                    .header(HOST, "127.0.0.1:8787")
                    .header(CONTENT_TYPE, "application/json")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::from(concurrent_body))
                    .unwrap(),
            )
        };
        let (first, second) = tokio::join!(create_once(), create_once());
        let statuses = [first.unwrap().status(), second.unwrap().status()];
        assert_eq!(
            statuses
                .iter()
                .filter(|status| **status == StatusCode::CREATED)
                .count(),
            1
        );
        assert_eq!(
            statuses
                .iter()
                .filter(|status| **status == StatusCode::CONFLICT)
                .count(),
            1
        );

        let before = SqliteAuthStore::open_database(&database)
            .unwrap()
            .account(&owner.id, &target_id)
            .unwrap()
            .unwrap();
        let forbidden = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/api/accounts/{target_id}/credential"))
                    .header(HOST, "127.0.0.1:8787")
                    .header(CONTENT_TYPE, "application/json")
                    .header("cookie", format!("imail_session={other_session}"))
                    .body(Body::from(r#"{"password":"stolen"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(forbidden.status(), StatusCode::NOT_FOUND);

        let rejected = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/api/accounts/{target_id}/credential"))
                    .header(HOST, "127.0.0.1:8787")
                    .header(CONTENT_TYPE, "application/json")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::from(r#"{"password":"reject-password"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(rejected.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let after_rejected = SqliteAuthStore::open_database(&database)
            .unwrap()
            .account(&owner.id, &target_id)
            .unwrap()
            .unwrap();
        assert!(after_rejected == before);

        let oauth_rejected = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/api/accounts/{oauth_id}/credential"))
                    .header(HOST, "127.0.0.1:8787")
                    .header(CONTENT_TYPE, "application/json")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::from(r#"{"password":"must-not-replace"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(oauth_rejected.status(), StatusCode::CONFLICT);

        let credential = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/api/accounts/{target_id}/credential"))
                    .header(HOST, "127.0.0.1:8787")
                    .header(CONTENT_TYPE, "application/json")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::from(r#"{"password":"new-password"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(credential.status(), StatusCode::OK);
        let credential = json(credential).await;
        assert_eq!(credential["account"]["status"], "connected");
        assert!(!credential.to_string().contains("new-password"));
        assert!(!credential.to_string().contains("old-proxy-secret"));
        let stored = SqliteAuthStore::open_database(&database)
            .unwrap()
            .account(&owner.id, &target_id)
            .unwrap()
            .unwrap();
        let secret: Value = key.decrypt_json(&stored.encrypted_secret).unwrap();
        assert_eq!(secret["password"], "new-password");
        assert_eq!(secret["proxyPassword"], "old-proxy-secret");

        let before_proxy = stored.clone();
        let rejected_proxy = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/api/accounts/{target_id}/proxy"))
                    .header(HOST, "127.0.0.1:8787")
                    .header(CONTENT_TYPE, "application/json")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::from(r#"{"enabled":true,"protocol":"http","host":"reject.example.com","port":8080,"password":"proxy-new-secret"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(rejected_proxy.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert!(
            SqliteAuthStore::open_database(&database)
                .unwrap()
                .account(&owner.id, &target_id)
                .unwrap()
                .unwrap()
                == before_proxy
        );

        let copied_proxy = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/api/accounts/{target_id}/proxy"))
                    .header(HOST, "127.0.0.1:8787")
                    .header(CONTENT_TYPE, "application/json")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::from(format!(
                        r#"{{"enabled":true,"sourceAccountId":"{source_id}"}}"#
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(copied_proxy.status(), StatusCode::OK);
        let copied_proxy = json(copied_proxy).await;
        assert_eq!(
            copied_proxy["account"]["proxy"]["host"],
            "proxy.example.com"
        );
        assert!(!copied_proxy.to_string().contains("source-proxy-secret"));
        let stored = SqliteAuthStore::open_database(&database)
            .unwrap()
            .account(&owner.id, &target_id)
            .unwrap()
            .unwrap();
        let secret: Value = key.decrypt_json(&stored.encrypted_secret).unwrap();
        assert_eq!(secret["proxyPassword"], "source-proxy-secret");

        force_failure.store(true, Ordering::SeqCst);
        let failed_test = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/api/accounts/{target_id}/connection-test"))
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(failed_test.status(), StatusCode::OK);
        let failed_test = json(failed_test).await;
        assert_eq!(failed_test["account"]["status"], "error");
        assert!(failed_test["account"]["lastError"]
            .as_str()
            .unwrap()
            .contains("[redacted]"));
        assert!(!failed_test.to_string().contains("connection-secret"));
        assert!(!failed_test.to_string().contains("token-secret"));

        force_failure.store(false, Ordering::SeqCst);
        let recovered = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/api/accounts/{target_id}/connection-test"))
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let recovered = json(recovered).await;
        assert_eq!(recovered["account"]["status"], "connected");
        assert!(recovered["account"].get("lastError").is_none());

        let foreign_test = router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/api/accounts/{target_id}/connection-test"))
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={other_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(foreign_test.status(), StatusCode::NOT_FOUND);
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn embedded_oauth_uses_transient_loopback_callback_without_business_listener() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct EmbeddedOAuthProvider {
            exchanges: Arc<AtomicUsize>,
        }

        impl OAuthProviderPort for EmbeddedOAuthProvider {
            fn token_request(
                &mut self,
                _config: &OAuthConfig,
                grant: OAuthGrant<'_>,
            ) -> Result<OAuthTokenResponse, OAuthError> {
                assert!(matches!(grant, OAuthGrant::AuthorizationCode { .. }));
                self.exchanges.fetch_add(1, Ordering::SeqCst);
                Ok(OAuthTokenResponse {
                    access_token: "embedded-access-token-must-never-leak".into(),
                    refresh_token: Some("embedded-refresh-token-must-never-leak".into()),
                    expires_in: Some(3600),
                    token_type: Some("Bearer".into()),
                    scope: Some("openid email".into()),
                    id_token: None,
                })
            }

            fn fetch_identity(
                &mut self,
                _config: &OAuthConfig,
                _token: &OAuthTokenResponse,
                _nonce: &str,
            ) -> Result<OAuthIdentity, OAuthError> {
                Ok(OAuthIdentity {
                    email: "embedded-oauth@example.com".into(),
                    name: Some("Embedded OAuth".into()),
                })
            }
        }

        let directory = authentication_directory("embedded-oauth-loopback");
        fs::write(directory.join("master.key"), "74".repeat(32)).unwrap();
        let database = directory.join("imail.sqlite");
        let mut store = SqliteAuthStore::open_database(&database).unwrap();
        let owner = store
            .create_user("embedded-oauth-owner", "Owner", "owner-password-123")
            .unwrap();
        drop(store);

        let exchanges = Arc::new(AtomicUsize::new(0));
        let factory_exchanges = Arc::clone(&exchanges);
        let factory: Arc<dyn OAuthProviderPortFactory> = Arc::new(move || {
            Box::new(EmbeddedOAuthProvider {
                exchanges: Arc::clone(&factory_exchanges),
            }) as Box<dyn OAuthProviderPort>
        });
        let host = EmbeddedServiceHost::start(
            HttpAdapterConfig::new(&directory)
                .with_oauth_environment(OAuthEnvironment {
                    callback_base_url: "http://127.0.0.1:0/api/oauth".into(),
                    google_client_id: Some("embedded-client-id".into()),
                    google_client_secret: Some("embedded-client-secret".into()),
                    ..OAuthEnvironment::default()
                })
                .with_oauth_provider_factory(factory)
                .with_connection_probe(Arc::new(|_: &MailConnectionConfig| Ok(()))),
        )
        .unwrap();

        let started = host
            .start_oauth(
                owner.id.clone(),
                "embedded-test".into(),
                serde_json::json!({
                    "provider":"gmail",
                    "displayName":"Embedded OAuth",
                    "group":"个人",
                    "color":"#168f78"
                }),
            )
            .await
            .unwrap();
        let state = started["state"].as_str().unwrap().to_string();
        let authorization_url = url::Url::parse(started["authorizationUrl"].as_str().unwrap())
            .expect("authorization URL is valid");
        let redirect_uri = authorization_url
            .query_pairs()
            .find(|(key, _)| key == "redirect_uri")
            .map(|(_, value)| value.into_owned())
            .expect("authorization URL contains redirect URI");
        assert!(redirect_uri.starts_with("http://127.0.0.1:"));
        assert!(!redirect_uri.contains(":0/"));
        assert!(authorization_url.as_str().contains("code_challenge="));

        let callback_url = format!(
            "{redirect_uri}?{}",
            url::form_urlencoded::Serializer::new(String::new())
                .append_pair("state", &state)
                .append_pair("code", "embedded-one-time-code")
                .finish()
        );
        let callback_status = tokio::task::spawn_blocking(move || {
            ureq::get(&callback_url)
                .call()
                .map(|response| response.status())
                .map_err(|error| error.to_string())
        })
        .await
        .unwrap()
        .unwrap();
        assert!((200..300).contains(&callback_status));

        let mut completed = None;
        for _ in 0..50 {
            let status = host
                .oauth_status(owner.id.clone(), serde_json::json!({"state":state}))
                .await
                .unwrap();
            if status["completed"] == true {
                completed = Some(status);
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let completed = completed.expect("loopback callback completes");
        assert_eq!(completed["account"]["email"], "embedded-oauth@example.com");
        assert_eq!(exchanges.load(Ordering::SeqCst), 1);
        let response_text = completed.to_string();
        assert!(!response_text.contains("embedded-access-token-must-never-leak"));
        assert!(!response_text.contains("embedded-refresh-token-must-never-leak"));

        let store = SqliteAuthStore::open_database(&database).unwrap();
        let account = store
            .list_accounts(&owner.id)
            .unwrap()
            .into_iter()
            .find(|account| account.email == "embedded-oauth@example.com")
            .unwrap();
        assert!(!account
            .encrypted_secret
            .contains("embedded-access-token-must-never-leak"));
        assert!(!account
            .encrypted_secret
            .contains("embedded-refresh-token-must-never-leak"));
        drop(store);
        drop(host);
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn completes_oauth_once_and_isolates_status_and_reconnection_by_owner() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct FakeOAuthProvider {
            exchanges: Arc<AtomicUsize>,
        }

        impl OAuthProviderPort for FakeOAuthProvider {
            fn token_request(
                &mut self,
                _config: &OAuthConfig,
                grant: OAuthGrant<'_>,
            ) -> Result<OAuthTokenResponse, OAuthError> {
                assert!(matches!(grant, OAuthGrant::AuthorizationCode { .. }));
                self.exchanges.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(100));
                Ok(OAuthTokenResponse {
                    access_token: "access-token-must-never-leak".into(),
                    refresh_token: Some("refresh-token-must-never-leak".into()),
                    expires_in: Some(3600),
                    token_type: Some("Bearer".into()),
                    scope: Some("openid email".into()),
                    id_token: None,
                })
            }

            fn fetch_identity(
                &mut self,
                _config: &OAuthConfig,
                _token: &OAuthTokenResponse,
                _nonce: &str,
            ) -> Result<OAuthIdentity, OAuthError> {
                Ok(OAuthIdentity {
                    email: "oauth-owner@example.com".into(),
                    name: Some("OAuth Owner".into()),
                })
            }
        }

        let directory = authentication_directory("oauth-loop");
        let database = directory.join("imail.sqlite");
        let key_hex = "73".repeat(32);
        fs::write(directory.join("master.key"), &key_hex).unwrap();
        let mut store = SqliteAuthStore::open_database(&database).unwrap();
        let owner = store
            .create_user("oauth-owner", "Owner", "owner-password-123")
            .unwrap();
        let other = store
            .create_user("oauth-other", "Other", "other-password-123")
            .unwrap();
        let owner_session = store.create_session(&owner.id).unwrap();
        let other_session = store.create_session(&other.id).unwrap();
        drop(store);

        let exchanges = Arc::new(AtomicUsize::new(0));
        let factory_exchanges = Arc::clone(&exchanges);
        let factory: Arc<dyn OAuthProviderPortFactory> = Arc::new(move || {
            Box::new(FakeOAuthProvider {
                exchanges: Arc::clone(&factory_exchanges),
            }) as Box<dyn OAuthProviderPort>
        });
        let environment = OAuthEnvironment {
            callback_base_url: "http://127.0.0.1:8787/api/oauth".into(),
            google_client_id: Some("test-client-id".into()),
            google_client_secret: Some("test-client-secret".into()),
            ..OAuthEnvironment::default()
        };
        let router = build_router(
            HttpAdapterConfig::new(&directory)
                .with_oauth_environment(environment)
                .with_oauth_provider_factory(factory)
                .with_connection_probe(Arc::new(|_: &MailConnectionConfig| Ok(())))
                .with_oauth_frontend_origin("http://localhost:5173"),
        )
        .unwrap();

        let start = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/oauth/start")
                    .header(HOST, "127.0.0.1:8787")
                    .header(CONTENT_TYPE, "application/json")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::from(r##"{"provider":"gmail","displayName":"OAuth Owner","group":"个人","color":"#168f78"}"##))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start.status(), StatusCode::OK);
        let start = json(start).await;
        let oauth_state = start["state"].as_str().unwrap().to_string();
        assert_eq!(start["provider"], "google");
        assert!(start["authorizationUrl"]
            .as_str()
            .unwrap()
            .contains("code_challenge="));

        let callback_uri = format!(
            "/api/oauth/google/callback?{}",
            url::form_urlencoded::Serializer::new(String::new())
                .append_pair("state", &oauth_state)
                .append_pair("code", "one-time-code")
                .finish()
        );
        let callback_request = || {
            router.clone().oneshot(
                Request::builder()
                    .uri(&callback_uri)
                    .header(HOST, "127.0.0.1:8787")
                    .body(Body::empty())
                    .unwrap(),
            )
        };
        let (callback, concurrent_duplicate) = tokio::join!(callback_request(), callback_request());
        let callback = callback.unwrap();
        let concurrent_duplicate = concurrent_duplicate.unwrap();
        assert_eq!(callback.status(), StatusCode::OK);
        assert_eq!(concurrent_duplicate.status(), StatusCode::ACCEPTED);
        let callback_html = text_body(callback).await;
        assert!(callback_html.contains("邮箱授权成功"));
        assert!(callback_html.contains("http://localhost:5173"));
        assert!(!callback_html.contains("access-token-must-never-leak"));
        assert!(!callback_html.contains("refresh-token-must-never-leak"));
        assert_eq!(exchanges.load(Ordering::SeqCst), 1);

        let duplicate = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&callback_uri)
                    .header(HOST, "127.0.0.1:8787")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(duplicate.status(), StatusCode::OK);
        assert_eq!(exchanges.load(Ordering::SeqCst), 1);

        let status_request = |session: &str| {
            Request::builder()
                .method(Method::POST)
                .uri("/api/oauth/status")
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={session}"))
                .body(Body::from(json!({ "state": oauth_state }).to_string()))
                .unwrap()
        };
        let owner_status = json(
            router
                .clone()
                .oneshot(status_request(&owner_session))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(owner_status["completed"], true);
        assert_eq!(owner_status["account"]["email"], "oauth-owner@example.com");
        assert!(!owner_status.to_string().contains("token-must-never-leak"));
        let account_id = owner_status["account"]["id"].as_str().unwrap().to_string();
        let other_status = json(
            router
                .clone()
                .oneshot(status_request(&other_session))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(other_status, json!({ "completed": false }));

        let reconnect = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/api/accounts/{account_id}/oauth/reconnect"))
                    .header(HOST, "127.0.0.1:8787")
                    .header("cookie", format!("imail_session={owner_session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(reconnect.status(), StatusCode::OK);
        let reconnect_state = json(reconnect).await["state"].as_str().unwrap().to_string();
        let reconnect_uri = format!(
            "/api/oauth/google/callback?{}",
            url::form_urlencoded::Serializer::new(String::new())
                .append_pair("state", &reconnect_state)
                .append_pair("code", "reconnect-code")
                .finish()
        );
        assert_eq!(
            router
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(reconnect_uri)
                        .header(HOST, "127.0.0.1:8787")
                        .body(Body::empty())
                        .unwrap()
                )
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        assert_eq!(exchanges.load(Ordering::SeqCst), 2);
        let accounts = SqliteAuthStore::open_database(&database)
            .unwrap()
            .list_accounts(&owner.id)
            .unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].id, account_id);
        assert!(SyncRuntimeStore::open_database(&database)
            .unwrap()
            .policy(&account_id)
            .unwrap()
            .is_some());
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn serves_owned_mail_queries_and_commits_remote_mail_operations() {
        let directory = authentication_directory("mail-http");
        let database = directory.join("imail.sqlite");
        let key_hex = "91".repeat(32);
        fs::write(directory.join("master.key"), &key_hex).unwrap();
        let key = MasterKey::from_hex(&key_hex).unwrap();
        let mut store = SqliteAuthStore::open_database(&database).unwrap();
        let owner = store
            .create_user("mail-owner", "Owner", "owner-password-123")
            .unwrap();
        let other = store
            .create_user("mail-other", "Other", "other-password-123")
            .unwrap();
        let owner_session = store.create_session(&owner.id).unwrap();
        let other_session = store.create_session(&other.id).unwrap();
        let settings = json!({
            "imapHost":"imap.example.com","imapPort":993,"imapSecure":true,
            "smtpHost":"smtp.example.com","smtpPort":465,"smtpSecure":true
        });
        let account = |id: String, owner_id: String, email: &str| AccountRecord {
            id,
            owner_id,
            provider: "custom".into(),
            email: email.into(),
            display_name: "Mail Owner".into(),
            group: "工作".into(),
            group_icon: "folder".into(),
            color: "#168f78".into(),
            settings: settings.clone(),
            proxy: None,
            encrypted_secret: key
                .encrypt_json(&json!({"authType":"app-password","password":"mail-secret"}))
                .unwrap(),
            auth_method: Some("app-password".into()),
            created_at: "2026-08-10T00:00:00.000Z".into(),
            last_sync_at: None,
            status: "connected".into(),
            last_error: None,
            mailboxes: json!([{"name":"Projects","path":"Projects/2026","selectable":true}]),
        };
        let account_id = Uuid::new_v4().to_string();
        let other_account_id = Uuid::new_v4().to_string();
        store
            .upsert_account(&account(
                account_id.clone(),
                owner.id.clone(),
                "owner@example.com",
            ))
            .unwrap();
        store
            .upsert_account(&account(
                other_account_id.clone(),
                other.id.clone(),
                "other@example.com",
            ))
            .unwrap();
        let message_id = "owned-message".to_string();
        let message = |id: String, account_id: String, sender: &str| {
            imail_protocol::MessageReadModel {
                headers: Default::default(),
                id,
                account_id,
                mailbox: "INBOX".into(),
                mailbox_role: "inbox".into(),
                uid: 12,
                message_id: Some("<owned@example.com>".into()),
                from: json!({"name":"Sender","address":sender}),
                to: json!([{"name":"Owner","address":"owner@example.com"}]),
                subject: "Quarterly invoice".into(),
                preview: "Invoice preview".into(),
                text: "full confidential body".into(),
                html: Some("<p>full confidential body</p>".into()),
                date: "2026-08-10T08:00:00.000Z".into(),
                unread: true,
                flagged: false,
                has_attachments: true,
                attachments: json!([{"filename":"report.txt","contentType":"text/plain","size":5,"index":0}]),
                labels: json!(["客户"]),
                snoozed_until: None,
            }
        };
        store
            .upsert_message(
                &owner.id,
                &message(message_id.clone(), account_id.clone(), "sender@example.com"),
            )
            .unwrap();
        let mut project_message = message(
            "project-message".into(),
            account_id.clone(),
            "sender@example.com",
        );
        project_message.mailbox = "Projects/2026".into();
        project_message.mailbox_role = "custom".into();
        project_message.subject = "Project note".into();
        project_message.message_id = Some("<project@example.com>".into());
        project_message.headers.reply.in_reply_to = vec!["<owned@example.com>".into()];
        project_message.date = "2026-08-09T08:00:00.000Z".into();
        store.upsert_message(&owner.id, &project_message).unwrap();
        store
            .upsert_message(
                &other.id,
                &message(
                    "foreign-message".into(),
                    other_account_id,
                    "foreign@example.com",
                ),
            )
            .unwrap();
        store
            .upsert_contact(&imail_protocol::ContactReadModel {
                owner_id: owner.id.clone(),
                address: "sender@example.com".into(),
                name: "Sender".into(),
                message_count: 1,
                last_contact_at: "2026-08-10T08:00:00.000Z".into(),
                logo_key: Some("domain:example.com".into()),
                logo_content_type: Some("image/png".into()),
                logo_source_url: Some("https://example.com/logo.png".into()),
                logo_fetched_at: Some("2026-08-10T08:01:00.000Z".into()),
            })
            .unwrap();
        let draft_id = Uuid::new_v4().to_string();
        store
            .upsert_draft(
                &owner.id,
                &imail_protocol::DraftReadModel {
                    envelope: Default::default(),
                    id: draft_id.clone(),
                    account_id: account_id.clone(),
                    to: json!([]),
                    cc: json!([]),
                    subject: String::new(),
                    text: String::new(),
                    html: String::new(),
                    attachments: json!([]),
                    created_at: "2026-08-10T08:00:00.000Z".into(),
                    updated_at: "2026-08-10T08:00:00.000Z".into(),
                },
            )
            .unwrap();
        store
            .set_user_metadata(
                &owner.id,
                "external_access_v1",
                r#"{"gatewayEnabled":true,"mcpEnabled":false}"#,
            )
            .unwrap();
        let gateway_token = store
            .issue_developer_token(
                &owner.id,
                "Mail gateway",
                &[
                    "accounts:read".into(),
                    "messages:read".into(),
                    "messages:send".into(),
                ],
                std::slice::from_ref(&account_id),
                3600,
            )
            .unwrap()
            .raw;
        drop(store);

        use sha2::Digest as _;
        let logo_directory = directory.join("sender-logos");
        fs::create_dir(&logo_directory).unwrap();
        let logo_digest = format!("{:x}", sha2::Sha256::digest(b"domain:example.com"));
        let logo_bytes = [137, 80, 78, 71, 13, 10, 26, 10, 1, 2, 3, 4];
        fs::write(
            logo_directory.join(format!("{logo_digest}.json")),
            r#"{"contentType":"image/png","sourceUrl":"https://example.com/logo.png","fetchedAt":"2026-08-10T08:01:00.000Z"}"#,
        )
        .unwrap();
        fs::write(
            logo_directory.join(format!("{logo_digest}.bin")),
            logo_bytes,
        )
        .unwrap();

        let mail_state = Arc::new(Mutex::new(MailTestState::default()));
        let source = b"From: Sender <sender@example.com>\r\nTo: Owner <owner@example.com>\r\nSubject: Attachment\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=x\r\n\r\n--x\r\nContent-Type: text/plain\r\n\r\nbody\r\n--x\r\nContent-Type: text/plain; name=report.txt\r\nContent-Disposition: attachment; filename=report.txt\r\nContent-Transfer-Encoding: base64\r\n\r\naGVsbG8=\r\n--x--\r\n".to_vec();
        let mut config = HttpAdapterConfig::new(&directory).with_mail_transport_factory(Arc::new(
            TestMailTransportFactory {
                state: Arc::clone(&mail_state),
                source,
            },
        ));
        config.gateway = true;
        let router = build_router(config).unwrap();
        let request = |method: Method, uri: String, session: &str, body: Body| {
            Request::builder()
                .method(method)
                .uri(uri)
                .header(HOST, "127.0.0.1:8787")
                .header(CONTENT_TYPE, "application/json")
                .header("cookie", format!("imail_session={session}"))
                .body(body)
                .unwrap()
        };

        let conversation_uri = format!("/api/messages/{message_id}/conversation");
        let conversation = router
            .clone()
            .oneshot(request(
                Method::GET,
                conversation_uri.clone(),
                &owner_session,
                Body::empty(),
            ))
            .await
            .unwrap();
        assert_eq!(conversation.status(), StatusCode::OK);
        let conversation = json(conversation).await;
        assert_eq!(conversation["messages"].as_array().unwrap().len(), 2);
        assert_eq!(conversation["messages"][0]["id"], "project-message");
        assert_eq!(conversation["messages"][0]["mailbox"], "Projects/2026");
        assert!(conversation["messages"][0].get("text").is_none());
        assert!(conversation["messages"]
            .as_array()
            .unwrap()
            .iter()
            .all(|message| message["unread"] == true));
        let foreign = router
            .clone()
            .oneshot(request(
                Method::GET,
                conversation_uri,
                &other_session,
                Body::empty(),
            ))
            .await
            .unwrap();
        assert_eq!(foreign.status(), StatusCode::NOT_FOUND);
        let anonymous = router
            .clone()
            .oneshot(request(
                Method::GET,
                format!("/api/messages/{message_id}/conversation"),
                "",
                Body::empty(),
            ))
            .await
            .unwrap();
        assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

        let listed = router
            .clone()
            .oneshot(request(
                Method::GET,
                "/api/messages?q=invoice&unread=true&limit=10&offset=0".into(),
                &owner_session,
                Body::empty(),
            ))
            .await
            .unwrap();
        assert_eq!(listed.status(), StatusCode::OK);
        let listed = json(listed).await;
        assert_eq!(listed["total"], 1);
        assert_eq!(listed["messages"][0]["id"], message_id);
        assert!(listed["messages"][0].get("text").is_none());
        assert!(listed["messages"][0].get("html").is_none());
        assert_eq!(
            listed["messages"][0]["from"]["logo"]["key"],
            "domain:example.com"
        );
        assert!(listed["messages"][0]["from"]["logo"]["url"]
            .as_str()
            .unwrap()
            .contains("sender%40example.com"));
        let by_mailbox_name = json(
            router
                .clone()
                .oneshot(request(
                    Method::GET,
                    "/api/messages?mailboxName=projects".into(),
                    &owner_session,
                    Body::empty(),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(by_mailbox_name["total"], 1);
        assert_eq!(by_mailbox_name["messages"][0]["id"], "project-message");

        let foreign_list = json(
            router
                .clone()
                .oneshot(request(
                    Method::GET,
                    "/api/messages".into(),
                    &other_session,
                    Body::empty(),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(foreign_list["total"], 1);
        assert_eq!(foreign_list["messages"][0]["id"], "foreign-message");

        let detail = json(
            router
                .clone()
                .oneshot(request(
                    Method::GET,
                    format!("/api/messages/{message_id}"),
                    &owner_session,
                    Body::empty(),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(detail["message"]["text"], "full confidential body");
        let stats = json(
            router
                .clone()
                .oneshot(request(
                    Method::GET,
                    "/api/message-stats".into(),
                    &owner_session,
                    Body::empty(),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(stats["total"], 1);
        assert_eq!(stats["unread"], 1);
        assert_eq!(stats["byAccount"][0]["accountId"], account_id);
        let contacts = json(
            router
                .clone()
                .oneshot(request(
                    Method::GET,
                    "/api/contacts".into(),
                    &owner_session,
                    Body::empty(),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert!(contacts.to_string().contains("domain:example.com"));
        assert!(!contacts.to_string().contains("ownerId"));
        let logo = router
            .clone()
            .oneshot(request(
                Method::GET,
                "/api/contacts/logo?address=sender%40example.com".into(),
                &owner_session,
                Body::empty(),
            ))
            .await
            .unwrap();
        assert_eq!(logo.status(), StatusCode::OK);
        assert_eq!(logo.headers()[CONTENT_TYPE], "image/png");
        assert_eq!(
            to_bytes(logo.into_body(), 1024).await.unwrap().as_ref(),
            logo_bytes
        );
        let message_logo = router
            .clone()
            .oneshot(request(
                Method::GET,
                format!("/api/messages/{message_id}/sender-logo"),
                &owner_session,
                Body::empty(),
            ))
            .await
            .unwrap();
        assert_eq!(message_logo.status(), StatusCode::OK);
        let foreign_logo = router
            .clone()
            .oneshot(request(
                Method::GET,
                "/api/contacts/logo?address=sender%40example.com".into(),
                &other_session,
                Body::empty(),
            ))
            .await
            .unwrap();
        assert_eq!(foreign_logo.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            json(
                router
                    .clone()
                    .oneshot(request(
                        Method::GET,
                        "/api/labels".into(),
                        &owner_session,
                        Body::empty(),
                    ))
                    .await
                    .unwrap()
            )
            .await["labels"],
            json!(["客户"])
        );

        let forbidden = router
            .clone()
            .oneshot(request(
                Method::GET,
                format!("/api/messages/{message_id}"),
                &other_session,
                Body::empty(),
            ))
            .await
            .unwrap();
        assert_eq!(forbidden.status(), StatusCode::NOT_FOUND);

        let patched = router
            .clone()
            .oneshot(request(
                Method::PATCH,
                format!("/api/messages/{message_id}"),
                &owner_session,
                Body::from(
                    r#"{"unread":false,"flagged":true,"labels":["重点"],"snoozedUntil":null}"#,
                ),
            ))
            .await
            .unwrap();
        assert_eq!(patched.status(), StatusCode::OK);
        let patched = json(patched).await;
        assert_eq!(patched["message"]["unread"], false);
        assert_eq!(patched["message"]["flagged"], true);
        assert_eq!(patched["message"]["labels"], json!(["重点"]));

        mail_state.lock().unwrap().reject_flags = true;
        let rejected_patch = router
            .clone()
            .oneshot(request(
                Method::PATCH,
                format!("/api/messages/{message_id}"),
                &owner_session,
                Body::from(r#"{"unread":true}"#),
            ))
            .await
            .unwrap();
        assert_eq!(rejected_patch.status(), StatusCode::BAD_GATEWAY);
        let rejected_patch = json(rejected_patch).await;
        assert!(!rejected_patch.to_string().contains("remote-secret"));
        assert!(
            !SqliteAuthStore::open_database(&database)
                .unwrap()
                .list_messages(&owner.id)
                .unwrap()[0]
                .unread
        );
        mail_state.lock().unwrap().reject_flags = false;

        let foreign_attachment = router
            .clone()
            .oneshot(request(
                Method::GET,
                format!("/api/messages/{message_id}/attachments/0"),
                &other_session,
                Body::empty(),
            ))
            .await
            .unwrap();
        assert_eq!(foreign_attachment.status(), StatusCode::NOT_FOUND);
        assert_eq!(mail_state.lock().unwrap().fetches, 0);

        let attachment = router
            .clone()
            .oneshot(request(
                Method::GET,
                format!("/api/messages/{message_id}/attachments/0"),
                &owner_session,
                Body::empty(),
            ))
            .await
            .unwrap();
        assert_eq!(attachment.status(), StatusCode::OK);
        assert_eq!(attachment.headers()[CONTENT_TYPE], "text/plain");
        assert!(attachment.headers()["content-disposition"]
            .to_str()
            .unwrap()
            .contains("report.txt"));
        assert_eq!(
            to_bytes(attachment.into_body(), 1024).await.unwrap(),
            "hello"
        );
        let fetches_after_download = mail_state.lock().unwrap().fetches;

        let preview = router
            .clone()
            .oneshot(request(
                Method::POST,
                format!("/api/messages/{message_id}/attachments/0/preview"),
                &owner_session,
                Body::empty(),
            ))
            .await
            .unwrap();
        assert_eq!(preview.status(), StatusCode::CREATED);
        let preview = json(preview).await;
        assert_eq!(preview["descriptor"]["kind"], "text");
        assert_eq!(
            preview["descriptor"]["contentType"],
            "text/plain; charset=utf-8"
        );
        assert_eq!(
            mail_state.lock().unwrap().fetches,
            fetches_after_download,
            "preview should reuse the attachment downloaded into the local cache"
        );
        let preview_id = preview["previewId"].as_str().unwrap();

        let foreign_preview = router
            .clone()
            .oneshot(request(
                Method::GET,
                format!("/api/attachment-previews/{preview_id}/content"),
                &other_session,
                Body::empty(),
            ))
            .await
            .unwrap();
        assert_eq!(foreign_preview.status(), StatusCode::NOT_FOUND);

        let mut range_request = request(
            Method::GET,
            format!("/api/attachment-previews/{preview_id}/content"),
            &owner_session,
            Body::empty(),
        );
        range_request
            .headers_mut()
            .insert("range", HeaderValue::from_static("bytes=1-3"));
        let ranged = router.clone().oneshot(range_request).await.unwrap();
        assert_eq!(ranged.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(ranged.headers()["content-range"], "bytes 1-3/5");
        assert_eq!(to_bytes(ranged.into_body(), 1024).await.unwrap(), "ell");

        let removed = router
            .clone()
            .oneshot(request(
                Method::DELETE,
                format!("/api/attachment-previews/{preview_id}"),
                &owner_session,
                Body::empty(),
            ))
            .await
            .unwrap();
        assert_eq!(removed.status(), StatusCode::NO_CONTENT);

        let moved = router
            .clone()
            .oneshot(request(
                Method::POST,
                format!("/api/messages/{message_id}/move"),
                &owner_session,
                Body::from(r#"{"destination":"archive"}"#),
            ))
            .await
            .unwrap();
        assert_eq!(moved.status(), StatusCode::OK);
        let moved = json(moved).await;
        assert_eq!(moved["message"]["mailbox"], "Archive");
        assert_eq!(moved["message"]["mailboxRole"], "archive");
        assert_eq!(moved["message"]["uid"], 44);

        let invalid_send = router
            .clone()
            .oneshot(request(
                Method::POST,
                "/api/send".into(),
                &owner_session,
                Body::from(format!(
                    r#"{{"accountId":"{account_id}","to":["recipient@example.com"],"subject":"Hello","text":"Body","attachments":[{{"id":"a1","filename":"hello.txt","contentType":"text/plain","size":5,"data":"not-base64"}}],"draftId":"{draft_id}"}}"#
                )),
            ))
            .await
            .unwrap();
        assert_eq!(invalid_send.status(), StatusCode::BAD_REQUEST);
        assert_eq!(mail_state.lock().unwrap().sends, 0);
        assert_eq!(
            SqliteAuthStore::open_database(&database)
                .unwrap()
                .list_drafts(&owner.id)
                .unwrap()
                .len(),
            1
        );

        let gateway_send = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/gateway/v1/send")
                    .header(HOST, "127.0.0.1:8787")
                    .header("authorization", format!("Bearer {gateway_token}"))
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"mailbox":"owner@example.com","to":["gateway@example.com"],"subject":"Gateway","text":"Gateway body"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(gateway_send.status(), StatusCode::CREATED);
        assert_eq!(
            json(gateway_send).await["delivery"]["messageId"],
            "<sent@example.com>"
        );
        let gateway_attachment = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/gateway/v1/messages/{message_id}/attachments/0"))
                    .header(HOST, "127.0.0.1:8787")
                    .header("authorization", format!("Bearer {gateway_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(gateway_attachment.status(), StatusCode::OK);
        assert_eq!(gateway_attachment.headers()[CONTENT_TYPE], "text/plain");

        let sent = router
            .clone()
            .oneshot(request(
                Method::POST,
                "/api/send".into(),
                &owner_session,
                Body::from(format!(
                    r#"{{"accountId":"{account_id}","to":["recipient@example.com"],"subject":"Hello","text":"Body","attachments":[{{"id":"a1","filename":"hello.txt","contentType":"text/plain","size":5,"data":"aGVsbG8="}}],"draftId":"{draft_id}"}}"#
                )),
            ))
            .await
            .unwrap();
        assert_eq!(sent.status(), StatusCode::CREATED);
        assert_eq!(json(sent).await["messageId"], "<sent@example.com>");
        assert!(SqliteAuthStore::open_database(&database)
            .unwrap()
            .list_drafts(&owner.id)
            .unwrap()
            .is_empty());
        let state = mail_state.lock().unwrap();
        assert_eq!(state.flag_updates, 2);
        assert_eq!(state.fetches, 2);
        assert_eq!(state.moves, 1);
        assert_eq!(state.sends, 2);
        assert_eq!(
            state.last_sent.as_ref().unwrap().attachments[0].content,
            b"hello"
        );
        drop(state);
        fs::remove_dir_all(directory).unwrap();
    }
}
