//! Compose domain routes and allocate their shared application state.
use crate::{
    accounts, apple_hme, attachment_previews, auth, boundary::security_boundary, developer_tokens,
    drafts, external_access, gateway, instance::load_or_create_instance_id, mcp, messages, oauth,
    outbox, preferences, rules, search, security, sync_control, system, translation_settings,
    translations, web_client, work_queue, AppState, HttpAdapterConfig, HttpAdapterError,
    ServiceCapabilities, ServiceInfo, PROTOCOL_VERSION, SERVICE_VERSION,
};
use axum::{
    extract::{DefaultBodyLimit, State},
    http::{header::CACHE_CONTROL, StatusCode},
    middleware,
    response::IntoResponse,
    routing::{any, get},
    Json, Router,
};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

#[derive(Serialize)]
struct Health {
    ok: bool,
    service: &'static str,
}

pub fn build_router(config: HttpAdapterConfig) -> Result<Router, HttpAdapterError> {
    build_router_state(config, None).map(|(router, _, _)| router)
}

pub(super) fn build_router_state(
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
        .merge(work_queue::routes())
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
