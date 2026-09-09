//! Shared HTTP and embedded adapter. Domain handlers use the same application state.
use imail_oauth::RefreshCoordinator;
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

mod boundary;
mod config;
mod embedded;
mod error;
mod host;
mod instance;
mod ports;
mod router;

pub use config::{BridgeMode, ConfigError, HttpAdapterConfig, SyncRuntimeOptions};
pub use embedded::EmbeddedServiceHost;
pub use error::{EmbeddedOperationError, HttpAdapterError};
pub use host::{serve, serve_with_shutdown};
pub use ports::{MailConnectionProbe, MailTransportFactory};
pub use router::build_router;

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
pub mod work_queue;

const SERVICE_VERSION: &str = env!("CARGO_PKG_VERSION");
const PROTOCOL_VERSION: u16 = 1;

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

#[cfg(test)]
mod tests;
