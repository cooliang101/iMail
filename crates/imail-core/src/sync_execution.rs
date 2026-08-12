use std::error::Error;

use chrono::{DateTime, SecondsFormat, Utc};
use imail_mail::{
    ImapSyncPort, MailConnectionConfig, MailboxSyncCommitResult, MailboxSyncError, MailboxSyncPlan,
    MailboxSyncService, SyncCursor, SyncTarget,
};

use crate::{
    accounts::AccountSecretCodec,
    mail_operations::{connection_config, MailApplicationError},
    AccountRepository, LocalRepository,
};

pub trait MailboxSyncCommitRepository {
    type Error: Error + Send + Sync + 'static;

    fn commit_mailbox_sync(
        &mut self,
        plan: &MailboxSyncPlan,
        completed_at: DateTime<Utc>,
    ) -> Result<MailboxSyncCommitResult, Self::Error>;
}

#[derive(Debug)]
pub struct MailboxSyncExecution {
    pub plan: MailboxSyncPlan,
    pub commit: MailboxSyncCommitResult,
    pub notification_messages: Vec<imail_protocol::MessageReadModel>,
}

#[derive(Debug, thiserror::Error)]
pub enum MailboxSyncApplicationError<RE, CE>
where
    RE: Error + Send + Sync + 'static,
    CE: Error + Send + Sync + 'static,
{
    #[error("邮箱账户不存在")]
    AccountNotFound,
    #[error("邮箱连接配置与目标账户不一致")]
    ConnectionAccountMismatch,
    #[error(transparent)]
    Repository(RE),
    #[error(transparent)]
    Configuration(MailApplicationError<RE>),
    #[error(transparent)]
    Sync(#[from] MailboxSyncError),
    #[error(transparent)]
    Commit(CE),
}

pub struct MailboxSyncApplicationService<'a, R, C, P, M>
where
    R: LocalRepository,
    C: AccountSecretCodec,
    P: ImapSyncPort,
    M: MailboxSyncCommitRepository,
{
    repository: &'a R,
    codec: &'a C,
    imap: &'a mut P,
    commit_repository: &'a mut M,
}

struct LoadedExecution<'a> {
    user_id: &'a str,
    account_id: &'a str,
    previously_synced: bool,
    config: &'a MailConnectionConfig,
    target: SyncTarget,
    cursor: SyncCursor,
    completed_at: DateTime<Utc>,
}

impl<'a, R, C, P, M> MailboxSyncApplicationService<'a, R, C, P, M>
where
    R: LocalRepository,
    C: AccountSecretCodec,
    P: ImapSyncPort,
    M: MailboxSyncCommitRepository,
{
    pub fn new(
        repository: &'a R,
        codec: &'a C,
        imap: &'a mut P,
        commit_repository: &'a mut M,
    ) -> Self {
        Self {
            repository,
            codec,
            imap,
            commit_repository,
        }
    }

    pub fn execute(
        &mut self,
        user_id: &str,
        account_id: &str,
        target: SyncTarget,
        cursor: SyncCursor,
        completed_at: DateTime<Utc>,
    ) -> Result<
        MailboxSyncExecution,
        MailboxSyncApplicationError<<R as AccountRepository>::Error, M::Error>,
    > {
        let account = self
            .repository
            .account(user_id, account_id)
            .map_err(MailboxSyncApplicationError::Repository)?
            .ok_or(MailboxSyncApplicationError::AccountNotFound)?;
        let previously_synced = account.last_sync_at.is_some();
        let config = connection_config::<<R as AccountRepository>::Error, C>(&account, self.codec)
            .map_err(MailboxSyncApplicationError::Configuration)?;
        self.execute_loaded(LoadedExecution {
            user_id,
            account_id,
            previously_synced,
            config: &config,
            target,
            cursor,
            completed_at,
        })
    }

    pub fn execute_with_config(
        &mut self,
        user_id: &str,
        account_id: &str,
        config: &MailConnectionConfig,
        target: SyncTarget,
        cursor: SyncCursor,
        completed_at: DateTime<Utc>,
    ) -> Result<
        MailboxSyncExecution,
        MailboxSyncApplicationError<<R as AccountRepository>::Error, M::Error>,
    > {
        let account = self
            .repository
            .account(user_id, account_id)
            .map_err(MailboxSyncApplicationError::Repository)?
            .ok_or(MailboxSyncApplicationError::AccountNotFound)?;
        if !account.email.eq_ignore_ascii_case(&config.email) {
            return Err(MailboxSyncApplicationError::ConnectionAccountMismatch);
        }
        self.execute_loaded(LoadedExecution {
            user_id,
            account_id,
            previously_synced: account.last_sync_at.is_some(),
            config,
            target,
            cursor,
            completed_at,
        })
    }

    fn execute_loaded(
        &mut self,
        execution: LoadedExecution<'_>,
    ) -> Result<
        MailboxSyncExecution,
        MailboxSyncApplicationError<<R as AccountRepository>::Error, M::Error>,
    > {
        let cached = self
            .repository
            .list_messages(execution.user_id)
            .map_err(MailboxSyncApplicationError::Repository)?;
        let fallback_date = execution
            .completed_at
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        let plan = MailboxSyncService::new(self.imap).plan(
            execution.config,
            execution.account_id,
            execution.target,
            execution.cursor,
            &cached,
            &fallback_date,
        )?;
        let commit = self
            .commit_repository
            .commit_mailbox_sync(&plan, execution.completed_at)
            .map_err(MailboxSyncApplicationError::Commit)?;
        let notification_messages = if execution.previously_synced {
            commit.created_messages.clone()
        } else {
            vec![]
        };
        Ok(MailboxSyncExecution {
            plan,
            commit,
            notification_messages,
        })
    }
}
