//! Service configuration and validation shared by embedded and TCP hosts.
use crate::{
    logo,
    ports::{HttpOAuthProviderFactory, NetworkConnectionProbe, NetworkMailTransportFactory},
    MailConnectionProbe, MailTransportFactory,
};
use imail_oauth::{
    OAuthConfigResolver, OAuthEnvironment, OAuthProviderPortFactory, RefreshCoordinator,
    StandardOAuthConfigResolver,
};
use imail_runtime::{NetworkSyncMailTransportFactory, SyncMailTransportFactory};
use std::{
    collections::BTreeSet, net::IpAddr, path::PathBuf, str::FromStr, sync::Arc, time::Duration,
};
use url::Url;

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

#[derive(Clone)]
pub struct HttpAdapterConfig {
    pub data_dir: PathBuf,
    pub(super) production: bool,
    pub allowed_hosts: BTreeSet<String>,
    pub cors_origins: BTreeSet<String>,
    pub(super) web_client_root: Option<PathBuf>,
    pub gateway: bool,
    pub mcp: bool,
    pub(super) sync_worker: bool,
    pub(super) apple_hme_keepalive: bool,
    pub(super) sync_runtime: SyncRuntimeOptions,
    pub(super) registration_open: bool,
    pub(super) secure_cookies: bool,
    pub(super) trust_proxy_one_hop: bool,
    pub(super) oauth_environment: OAuthEnvironment,
    pub(super) oauth_config_resolver: Arc<dyn OAuthConfigResolver>,
    pub(super) refresh_coordinator: Arc<RefreshCoordinator>,
    pub(super) connection_probe: Arc<dyn MailConnectionProbe>,
    pub(super) oauth_provider_factory: Arc<dyn OAuthProviderPortFactory>,
    pub(super) mail_transport_factory: Arc<dyn MailTransportFactory>,
    pub(super) sync_mail_transport_factory: Arc<dyn SyncMailTransportFactory>,
    pub(super) logo_discovery: Arc<dyn logo::LogoDiscoveryPort>,
    pub(super) oauth_frontend_origin: String,
    pub(super) daemon_control_file: Option<PathBuf>,
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

    pub(super) fn validate(mut self) -> Result<Self, ConfigError> {
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

pub(super) fn normalize_origin(value: &str) -> Result<String, ConfigError> {
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

pub(super) fn is_loopback_host(value: &str) -> bool {
    value.eq_ignore_ascii_case("localhost")
        || value
            .parse::<IpAddr>()
            .map(|address| address.is_loopback())
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_incomplete_invalid_and_implicit_http_arguments() {
        for (arguments, expected) in [
            (vec!["--host"], ConfigError::MissingValue("--host")),
            (vec!["--port"], ConfigError::MissingValue("--port")),
            (
                vec!["--http", "--host", "localhost"],
                ConfigError::InvalidHost,
            ),
            (vec!["--http", "--port", "0"], ConfigError::InvalidPort),
            (vec!["--http", "--port", "65536"], ConfigError::InvalidPort),
            (vec!["--http", "--port", "-1"], ConfigError::InvalidPort),
            (vec!["--port", "8787"], ConfigError::HttpOptionWithoutBridge),
            (
                vec!["--unknown"],
                ConfigError::UnknownArgument("--unknown".into()),
            ),
        ] {
            assert_eq!(
                BridgeMode::parse(arguments.iter().copied()),
                Err(expected),
                "{arguments:?}"
            );
        }
        assert_eq!(
            BridgeMode::parse(["--http", "--host", "::1", "--port", "65535"]),
            Ok(BridgeMode::Http {
                host: "::1".parse().unwrap(),
                port: 65535
            })
        );
    }

    #[test]
    fn normalizes_origins_but_rejects_credentials_paths_and_remote_plaintext() {
        for (input, expected) in [
            ("https://CLIENT.example:443/", "https://client.example"),
            ("https://client.example:8443", "https://client.example:8443"),
            ("http://localhost:5173/", "http://localhost:5173"),
            ("http://127.0.0.1:5173", "http://127.0.0.1:5173"),
        ] {
            assert_eq!(normalize_origin(input).unwrap(), expected);
        }
        for input in [
            "null",
            "https://user:secret@client.example",
            "https://client.example/path",
            "https://client.example/?token=secret",
            "https://client.example/#fragment",
            "http://client.example",
            "ftp://client.example",
            "http://localhost.attacker.example",
        ] {
            assert!(
                matches!(
                    normalize_origin(input),
                    Err(ConfigError::InvalidCorsOrigin(_))
                ),
                "{input}"
            );
        }
    }

    #[test]
    fn validates_hosts_and_oauth_origin_before_allocating_service_state() {
        let mut config = HttpAdapterConfig::new("unused-config-test-directory");
        config.allowed_hosts = [
            " MAIL.Example ".into(),
            "mail.example".into(),
            "[::1]".into(),
        ]
        .into_iter()
        .collect();
        let config = config.validate().unwrap();
        assert_eq!(
            config.allowed_hosts,
            ["mail.example".into(), "::1".into()].into_iter().collect()
        );
        for host in [
            "",
            "mail.example:443",
            "https://mail.example",
            "user@mail.example",
            "mail example",
        ] {
            let mut config = config.clone();
            config.allowed_hosts = [host.into()].into_iter().collect();
            assert!(
                matches!(config.validate(), Err(ConfigError::InvalidAllowedHost(_))),
                "{host}"
            );
        }
        let mut empty = config.clone();
        empty.allowed_hosts.clear();
        assert!(matches!(
            empty.validate(),
            Err(ConfigError::MissingAllowedHost)
        ));
        assert!(matches!(
            config
                .with_oauth_frontend_origin("https://client.example/callback")
                .validate(),
            Err(ConfigError::InvalidCorsOrigin(_))
        ));
    }
}
