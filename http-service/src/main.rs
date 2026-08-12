use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    time::Duration,
};

use imail_http::{serve_with_shutdown, BridgeMode, HttpAdapterConfig, SyncRuntimeOptions};
use imail_oauth::OAuthEnvironment;
use imail_security::MasterKey;
use imail_storage_sqlite::{migrate_database, SqliteAuthStore};

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("iMail Rust service failed: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let ServerArguments {
        data_dir,
        daemon_control_file,
        bridge: bridge_arguments,
    } = arguments()?;
    let bridge_arguments = bridge_environment_defaults(bridge_arguments);
    let bridge = BridgeMode::parse(bridge_arguments.iter().map(String::as_str))?;
    let BridgeMode::Http { host, port } = bridge else {
        println!("iMail Rust core selected embedded mode; no HTTP listener was started");
        return Ok(());
    };

    prepare_data_directory(&data_dir)?;
    let mut config = HttpAdapterConfig::production(data_dir);
    if let Some(path) = daemon_control_file
        .or_else(|| optional_environment("IMAIL_DAEMON_CONTROL_FILE").map(PathBuf::from))
    {
        config = config.with_daemon_control_file(path);
    }
    if let Some(hosts) = csv_environment("IMAIL_ALLOWED_HOSTS")? {
        config.allowed_hosts = hosts;
    }
    if let Some(origins) = csv_environment_with_fallback("IMAIL_CORS_ORIGINS", "CORS_ORIGIN")? {
        config.cors_origins = origins;
    }
    config.gateway = boolean_environment("IMAIL_GATEWAY", false)?;
    config.mcp = boolean_environment("IMAIL_MCP", false)?;
    let sync_worker = boolean_environment("IMAIL_SYNC_WORKER", true)?;
    let worker_count = integer_environment("IMAIL_SYNC_CONCURRENCY", 3, 1, 10)?;
    let worker_poll_ms = integer_environment("IMAIL_SYNC_WORKER_POLL_MS", 1_000, 250, 60_000)?;
    let lease_ms = integer_environment("IMAIL_SYNC_JOB_LEASE_MS", 120_000, 10_000, 3_600_000)?;
    let reconciliation_minutes = integer_environment("IMAIL_SYNC_RECONCILE_MINUTES", 30, 5, 1_440)?;
    let scheduler_interval_ms =
        integer_environment("IMAIL_SYNC_SCHEDULER_INTERVAL_MS", 5_000, 5_000, 3_600_000)?;
    let scheduler_startup_delay_ms =
        integer_environment("IMAIL_SYNC_STARTUP_DELAY_MS", 1_000, 0, 3_600_000)?;
    let idle_enabled = boolean_environment("IMAIL_SYNC_IDLE_ENABLED", true)?;
    let idle_reconcile_ms =
        integer_environment("IMAIL_SYNC_IDLE_RECONCILE_MS", 5_000, 5_000, 60_000)?;
    let idle_refresh_ms =
        integer_environment("IMAIL_SYNC_IDLE_REFRESH_MS", 60_000, 15_000, 60_000)?;
    let worker_host = optional_environment("IMAIL_SYNC_WORKER_HOST")
        .or_else(|| optional_environment("HOSTNAME"))
        .or_else(|| optional_environment("COMPUTERNAME"))
        .unwrap_or_else(|| "local".into());
    let oauth_environment = oauth_environment(port)?;
    config = config
        .with_oauth_environment(oauth_environment)
        .with_sync_worker(sync_worker)
        .with_sync_worker_options(SyncRuntimeOptions {
            worker_count,
            poll_interval: Duration::from_millis(worker_poll_ms as u64),
            lease_duration: Duration::from_millis(lease_ms as u64),
            reconciliation_minutes: reconciliation_minutes as i64,
            host_name: worker_host,
            idle_enabled,
            watcher_reconcile_interval: Duration::from_millis(idle_reconcile_ms as u64),
            watcher_maximum_wait: Duration::from_millis(idle_refresh_ms as u64),
            scheduler_interval: Duration::from_millis(scheduler_interval_ms as u64),
            scheduler_startup_delay: Duration::from_millis(scheduler_startup_delay_ms as u64),
        })
        .with_registration_open(registration_open_environment()?)
        .with_trusted_proxy_one_hop(boolean_environment_with_fallback(
            "IMAIL_TRUST_PROXY_ONE_HOP",
            "IMAIL_TRUST_PROXY",
            false,
        )?);
    if let Some(root) = optional_environment("IMAIL_WEB_DIST") {
        config = config.with_web_client_root(root);
    }

    println!("iMail Rust HTTP bridge starting on {host}:{port}");
    serve_with_shutdown(config, host, port, shutdown_signal()).await?;
    println!("iMail Rust HTTP bridge stopped gracefully");
    Ok(())
}

struct ServerArguments {
    data_dir: PathBuf,
    daemon_control_file: Option<PathBuf>,
    bridge: Vec<String>,
}

fn arguments() -> Result<ServerArguments, Box<dyn std::error::Error>> {
    let mut data_dir = optional_environment("IMAIL_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".data"));
    let mut bridge = vec!["--http".to_string()];
    let mut daemon_control_file = None;
    let mut values = std::env::args().skip(1);
    while let Some(value) = values.next() {
        if value == "--data-dir" {
            data_dir = values
                .next()
                .map(PathBuf::from)
                .ok_or("--data-dir requires a value")?;
        } else if value == "--daemon-control-file" {
            daemon_control_file = Some(
                values
                    .next()
                    .map(PathBuf::from)
                    .ok_or("--daemon-control-file requires a value")?,
            );
        } else {
            bridge.push(value);
        }
    }
    Ok(ServerArguments {
        data_dir,
        daemon_control_file,
        bridge,
    })
}

fn bridge_environment_defaults(mut arguments: Vec<String>) -> Vec<String> {
    if !arguments.iter().any(|value| value == "--http") {
        return arguments;
    }
    if !arguments.iter().any(|value| value == "--host") {
        if let Some(host) = optional_environment("HOST") {
            arguments.extend(["--host".into(), host]);
        }
    }
    if !arguments.iter().any(|value| value == "--port") {
        if let Some(port) = optional_environment("PORT") {
            arguments.extend(["--port".into(), port]);
        }
    }
    arguments
}

fn optional_environment(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn csv_environment(name: &str) -> Result<Option<BTreeSet<String>>, Box<dyn std::error::Error>> {
    let Some(value) = optional_environment(name) else {
        return Ok(None);
    };
    let values = value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    if values.is_empty() {
        return Err(format!("{name} must contain at least one value").into());
    }
    Ok(Some(values))
}

fn csv_environment_with_fallback(
    primary: &str,
    fallback: &str,
) -> Result<Option<BTreeSet<String>>, Box<dyn std::error::Error>> {
    if optional_environment(primary).is_some() {
        csv_environment(primary)
    } else {
        csv_environment(fallback)
    }
}

fn boolean_environment(name: &str, default: bool) -> Result<bool, Box<dyn std::error::Error>> {
    let value = optional_environment(name).map(|value| value.to_ascii_lowercase());
    match value.as_deref() {
        None => Ok(default),
        Some("1" | "true" | "yes" | "on") => Ok(true),
        Some("0" | "false" | "no" | "off") => Ok(false),
        Some(_) => Err(format!("{name} must be a boolean").into()),
    }
}

fn boolean_environment_with_fallback(
    primary: &str,
    fallback: &str,
    default: bool,
) -> Result<bool, Box<dyn std::error::Error>> {
    if optional_environment(primary).is_some() {
        boolean_environment(primary, default)
    } else {
        boolean_environment(fallback, default)
    }
}

fn registration_open_environment() -> Result<bool, Box<dyn std::error::Error>> {
    if optional_environment("IMAIL_REGISTRATION_OPEN").is_some() {
        return boolean_environment("IMAIL_REGISTRATION_OPEN", false);
    }
    match optional_environment("IMAIL_REGISTRATION_MODE")
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        None | Some("initial-only" | "closed") => Ok(false),
        Some("open") => Ok(true),
        Some(_) => Err("IMAIL_REGISTRATION_MODE must be open, initial-only, or closed".into()),
    }
}

fn integer_environment(
    name: &str,
    default: usize,
    minimum: usize,
    maximum: usize,
) -> Result<usize, Box<dyn std::error::Error>> {
    let value = match optional_environment(name) {
        Some(value) => value
            .parse::<usize>()
            .map_err(|_| format!("{name} must be an integer"))?,
        None => default,
    };
    if !(minimum..=maximum).contains(&value) {
        return Err(format!("{name} must be between {minimum} and {maximum}").into());
    }
    Ok(value)
}

fn oauth_environment(port: u16) -> Result<OAuthEnvironment, Box<dyn std::error::Error>> {
    Ok(OAuthEnvironment {
        callback_base_url: optional_environment("OAUTH_CALLBACK_BASE_URL")
            .unwrap_or_else(|| format!("http://localhost:{port}/api/oauth")),
        google_client_id: optional_environment("GOOGLE_OAUTH_CLIENT_ID"),
        google_client_secret: optional_environment("GOOGLE_OAUTH_CLIENT_SECRET"),
        google_redirect_uri: optional_environment("GOOGLE_OAUTH_REDIRECT_URI"),
        microsoft_client_id: optional_environment("MICROSOFT_OAUTH_CLIENT_ID"),
        microsoft_client_secret: optional_environment("MICROSOFT_OAUTH_CLIENT_SECRET"),
        microsoft_redirect_uri: optional_environment("MICROSOFT_OAUTH_REDIRECT_URI"),
        yahoo_client_id: optional_environment("YAHOO_OAUTH_CLIENT_ID"),
        yahoo_client_secret: optional_environment("YAHOO_OAUTH_CLIENT_SECRET"),
        yahoo_redirect_uri: optional_environment("YAHOO_OAUTH_REDIRECT_URI"),
        yahoo_mail_oauth_approved: boolean_environment("YAHOO_MAIL_OAUTH_APPROVED", false)?,
    })
}

fn prepare_data_directory(data_dir: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(data_dir)?;
    let database = data_dir.join("imail.sqlite");
    let master_key = data_dir.join("master.key");
    if database.is_file() {
        MasterKey::from_file(&master_key)?;
        SqliteAuthStore::open_database(&database)?;
        return Ok(());
    }
    if fs::read_dir(data_dir)?.next().is_some() {
        return Err(
            "refusing to initialize a non-empty data directory without imail.sqlite".into(),
        );
    }

    let key = MasterKey::generate_hex();
    let mut key_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&master_key)?;
    key_file.write_all(key.as_bytes())?;
    key_file.sync_all()?;
    let _database_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&database)?;
    migrate_database(&database)?;
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("SIGTERM handler must be installable");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
