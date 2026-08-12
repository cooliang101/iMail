use std::{path::Path, sync::Arc};

use chrono::Utc;
use imail_core::{
    oauth_refresh::RefreshingConnectionService,
    sync_execution::MailboxSyncApplicationService,
    sync_runtime::{classify_sync_failure, retry_minutes},
    AccountRepository,
};
use imail_mail::{ImapWakePort, SyncCursor, SyncTarget};
use imail_mail_network::{NetworkCancellation, NetworkMailAdapter};
use imail_oauth::{
    OAuthConfigResolver, OAuthEnvironment, OAuthProviderPort, OAuthProviderPortFactory,
    RefreshCoordinator, StandardOAuthConfigResolver,
};
use imail_oauth_http::OAuthHttpAdapter;
use imail_protocol::SyncJobReadModel;
use imail_security::{MasterKey, SecurityError};
use imail_storage_sqlite::{
    AuthStoreError, MasterKeyCredentialCodec, SyncCompletion, SyncFailure, SyncRuntimeError,
    SyncRuntimeStore,
};

use crate::{AccountWakeContext, AccountWakeExecutor, JobExecutionContext};

pub trait SyncImapPort: imail_mail::ImapSyncPort + ImapWakePort + Send {}

impl<T> SyncImapPort for T where T: imail_mail::ImapSyncPort + ImapWakePort + Send {}

pub trait SyncMailTransportFactory: Send + Sync + 'static {
    fn create(
        &self,
        cancellation: Arc<dyn NetworkCancellation>,
    ) -> Result<Box<dyn SyncImapPort>, String>;
}

#[derive(Default)]
pub struct NetworkSyncMailTransportFactory;

impl SyncMailTransportFactory for NetworkSyncMailTransportFactory {
    fn create(
        &self,
        cancellation: Arc<dyn NetworkCancellation>,
    ) -> Result<Box<dyn SyncImapPort>, String> {
        NetworkMailAdapter::with_cancellation(cancellation)
            .map(|adapter| Box::new(adapter) as Box<dyn SyncImapPort>)
            .map_err(|error| error.to_string())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EmbeddedSyncExecutorError {
    #[error("无法读取 iMail 主密钥")]
    MasterKey(#[from] SecurityError),
}

pub struct EmbeddedSyncExecutor {
    database_path: std::path::PathBuf,
    master_key: MasterKey,
    oauth_environment: OAuthEnvironment,
    refresh_coordinator: Arc<RefreshCoordinator>,
    oauth_provider_factory: Arc<dyn OAuthProviderPortFactory>,
    oauth_config_resolver: Arc<dyn OAuthConfigResolver>,
    mail_transport_factory: Arc<dyn SyncMailTransportFactory>,
}

struct HttpOAuthProviderFactory;

impl OAuthProviderPortFactory for HttpOAuthProviderFactory {
    fn create(&self) -> Box<dyn OAuthProviderPort> {
        Box::new(OAuthHttpAdapter::new())
    }
}

impl EmbeddedSyncExecutor {
    pub fn open_data_dir(
        data_dir: impl AsRef<Path>,
        oauth_environment: OAuthEnvironment,
    ) -> Result<Self, EmbeddedSyncExecutorError> {
        Self::open_data_dir_with_coordinator(
            data_dir,
            oauth_environment,
            Arc::new(RefreshCoordinator::default()),
        )
    }

    pub fn open_data_dir_with_coordinator(
        data_dir: impl AsRef<Path>,
        oauth_environment: OAuthEnvironment,
        refresh_coordinator: Arc<RefreshCoordinator>,
    ) -> Result<Self, EmbeddedSyncExecutorError> {
        Self::open_data_dir_with_dependencies(
            data_dir,
            oauth_environment,
            refresh_coordinator,
            Arc::new(NetworkSyncMailTransportFactory),
            Arc::new(HttpOAuthProviderFactory),
            Arc::new(StandardOAuthConfigResolver),
        )
    }

    pub fn open_data_dir_with_dependencies(
        data_dir: impl AsRef<Path>,
        oauth_environment: OAuthEnvironment,
        refresh_coordinator: Arc<RefreshCoordinator>,
        mail_transport_factory: Arc<dyn SyncMailTransportFactory>,
        oauth_provider_factory: Arc<dyn OAuthProviderPortFactory>,
        oauth_config_resolver: Arc<dyn OAuthConfigResolver>,
    ) -> Result<Self, EmbeddedSyncExecutorError> {
        let data_dir = data_dir.as_ref();
        Ok(Self {
            database_path: data_dir.join("imail.sqlite"),
            master_key: MasterKey::from_file(data_dir.join("master.key"))?,
            oauth_environment,
            refresh_coordinator,
            oauth_provider_factory,
            oauth_config_resolver,
            mail_transport_factory,
        })
    }

    fn execute_inner(
        &self,
        job: &SyncJobReadModel,
        context: &JobExecutionContext,
    ) -> Result<SyncCompletion, String> {
        if context.is_cancelled() {
            return Err("同步已取消".into());
        }
        let mut repository =
            imail_storage_sqlite::SqliteAuthStore::open_database(&self.database_path)
                .map_err(display_auth_error)?;
        let mut sync_store =
            SyncRuntimeStore::open_database(&self.database_path).map_err(display_sync_error)?;
        let owner_id = sync_store
            .account_owner_id(&job.account_id)
            .map_err(display_sync_error)?
            .ok_or_else(|| "邮箱账户不存在".to_string())?;
        let state = sync_store
            .mailbox_state_for_target(&job.account_id, job.mailbox.as_deref(), &job.mailbox_role)
            .map_err(display_sync_error)?;
        let cursor = state
            .map(|state| SyncCursor {
                uid_validity: state.uid_validity,
                last_seen_uid: state.last_seen_uid,
                highest_modseq: state.highest_modseq,
            })
            .unwrap_or_default();
        let target = SyncTarget {
            mailbox_role: job.mailbox_role.clone(),
            requested_mailbox: job.mailbox.clone(),
        };
        let codec = MasterKeyCredentialCodec::new(&self.master_key);
        let mut oauth = self.oauth_provider_factory.create();
        let config = RefreshingConnectionService::new_with_config_resolver(
            &mut repository,
            &codec,
            oauth.as_mut(),
            &self.refresh_coordinator,
            &self.oauth_environment,
            self.oauth_config_resolver.as_ref(),
        )
        .resolve(&owner_id, &job.account_id, Utc::now().timestamp_millis())
        .map_err(|error| error.to_string())?;
        if context.is_cancelled() {
            return Err("同步已取消".into());
        }
        let mut imap = self
            .mail_transport_factory
            .create(Arc::new(JobNetworkCancellation(context.clone())))?;
        let execution =
            MailboxSyncApplicationService::new(&repository, &codec, &mut imap, &mut sync_store)
                .execute_with_config(
                    &owner_id,
                    &job.account_id,
                    &config,
                    target,
                    cursor,
                    Utc::now(),
                )
                .map_err(|error| error.to_string())?;
        let account_email = repository
            .account(&owner_id, &job.account_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "邮箱账户不存在".to_string())?
            .email;
        sync_store
            .record_message_created(
                &job.account_id,
                &account_email,
                &execution.notification_messages,
                Utc::now(),
            )
            .map_err(|error| error.to_string())?;
        SyncCompletion::from_mailbox_commit(&execution.plan, &execution.commit)
            .map_err(|error| error.to_string())
    }
}

impl crate::SyncJobExecutor for EmbeddedSyncExecutor {
    fn execute(
        &self,
        job: &SyncJobReadModel,
        context: &JobExecutionContext,
    ) -> Result<SyncCompletion, SyncFailure> {
        self.execute_inner(job, context).map_err(|detail| {
            let classified = classify_sync_failure(&detail);
            SyncFailure {
                mailbox: job
                    .mailbox
                    .clone()
                    .unwrap_or_else(|| format!("@role:{}", job.mailbox_role)),
                code: classified.code.into(),
                message: classified.message,
                auth_required: classified.auth_required,
                retry_minutes: (!classified.auth_required
                    && classified.code != "ACCOUNT_NOT_FOUND")
                    .then(|| retry_minutes(job.attempts)),
            }
        })
    }
}

impl AccountWakeExecutor for EmbeddedSyncExecutor {
    fn wait_for_wake(
        &self,
        account_id: &str,
        context: &AccountWakeContext,
        maximum_wait: std::time::Duration,
    ) -> Result<imail_mail::MailboxWakeReason, String> {
        if context.is_cancelled() {
            return Err("同步已取消".into());
        }
        let mut repository =
            imail_storage_sqlite::SqliteAuthStore::open_database(&self.database_path)
                .map_err(display_auth_error)?;
        let sync_store =
            SyncRuntimeStore::open_database(&self.database_path).map_err(display_sync_error)?;
        let owner_id = sync_store
            .account_owner_id(account_id)
            .map_err(display_sync_error)?
            .ok_or_else(|| "邮箱账户不存在".to_string())?;
        let codec = MasterKeyCredentialCodec::new(&self.master_key);
        let mut oauth = self.oauth_provider_factory.create();
        let config = RefreshingConnectionService::new_with_config_resolver(
            &mut repository,
            &codec,
            oauth.as_mut(),
            &self.refresh_coordinator,
            &self.oauth_environment,
            self.oauth_config_resolver.as_ref(),
        )
        .resolve(&owner_id, account_id, Utc::now().timestamp_millis())
        .map_err(|error| error.to_string())?;
        if context.is_cancelled() {
            return Err("同步已取消".into());
        }
        let mut imap = self
            .mail_transport_factory
            .create(Arc::new(WakeNetworkCancellation(context.clone())))?;
        imap.wait_for_inbox_change(&config, maximum_wait)
            .map_err(|error| error.to_string())
    }
}

struct JobNetworkCancellation(JobExecutionContext);

impl NetworkCancellation for JobNetworkCancellation {
    fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
}

struct WakeNetworkCancellation(AccountWakeContext);

impl NetworkCancellation for WakeNetworkCancellation {
    fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
}

fn display_auth_error(error: AuthStoreError) -> String {
    error.to_string()
}

fn display_sync_error(error: SyncRuntimeError) -> String {
    error.to_string()
}
