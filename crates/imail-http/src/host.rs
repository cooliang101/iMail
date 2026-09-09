//! TCP listener lifecycle and shared persistent sync runtime startup.
use crate::{apple_hme, outbox, router::build_router_state, HttpAdapterConfig, HttpAdapterError};
use axum::Router;
use imail_runtime::{
    EmbeddedSyncExecutor, PersistentSyncRuntime, SyncEventSignal, SyncWorkerConfig,
};
use std::{
    future::Future,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};

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

pub(super) fn start_sync_runtime(
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
