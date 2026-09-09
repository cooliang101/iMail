//! In-process service facade and owned runtime lifecycle.
use crate::{
    accounts, apple_hme, attachment_previews, host::start_sync_runtime, messages, oauth, outbox,
    router::build_router_state, security, sync_control, system, work_queue, AppState,
    EmbeddedOperationError, HttpAdapterConfig, HttpAdapterError,
};
use axum::Router;
use imail_runtime::{PersistentSyncRuntime, SyncEventSignal};
use std::{sync::Arc, time::Duration};

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

    pub async fn work_queue_operation(
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
                    message: "邮件处理队列暂时不可用".into(),
                })?;
            work_queue::execute(&mut store, &owner_id, operation, id.as_deref(), input)
        })
        .await
        .map_err(|_| EmbeddedOperationError {
            status: 500,
            message: "邮件处理任务中断".into(),
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

    pub async fn download_message_source(
        &self,
        owner_id: String,
        message_id: String,
    ) -> Result<Vec<u8>, EmbeddedOperationError> {
        messages::embedded_download_message_source(Arc::clone(&self.state), owner_id, message_id)
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
