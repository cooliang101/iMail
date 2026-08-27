use std::{path::Path, time::Duration as StdDuration};

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use imail_core::{
    contacts::{reconcile_contacts_for_addresses, PublicSuffixDomainResolver},
    sync_execution::MailboxSyncCommitRepository,
    sync_runtime::targets_for_policy,
    AccountRecord,
};
use imail_mail::{MailboxMessageChange, MailboxSyncCommitResult, MailboxSyncPlan};
use imail_protocol::{
    MailboxSyncStateReadModel, MessageReadModel, SyncEventReadModel, SyncJobReadModel,
    SyncPolicyReadModel, SyncWorkerHealthReadModel, SyncWorkerReadModel, CURRENT_SCHEMA_VERSION,
};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, Transaction};
use serde_json::{json, Value};
use thiserror::Error;
use uuid::Uuid;

const WORKER_STALE_SECONDS: i64 = 60;

#[derive(Debug, Error)]
pub enum SyncRuntimeError {
    #[error("找不到 iMail 数据库")]
    MissingDatabase,
    #[error("数据库 schema v{actual} 不允许 Rust 同步运行时写入，当前要求 v{required}")]
    UnsupportedSchema { actual: u32, required: u32 },
    #[error("iMail 数据库 schema 版本无效")]
    InvalidSchema,
    #[error("同步任务租约已失效")]
    LeaseLost,
    #[error("同步参数无效：{0}")]
    InvalidInput(&'static str),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone)]
pub struct SyncEnqueue {
    pub account_id: String,
    pub mailbox: Option<String>,
    pub mailbox_role: String,
    pub reason: String,
    pub priority: i64,
    pub not_before: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct SyncCompletion {
    pub mailbox: String,
    pub uid_validity: Option<String>,
    pub highest_modseq: Option<String>,
    pub last_seen_uid: i64,
    pub synced: i64,
    pub created: i64,
    pub updated: i64,
    pub deleted: i64,
    pub message_changes: Vec<Value>,
}

impl SyncCompletion {
    pub fn from_mailbox_commit(
        plan: &MailboxSyncPlan,
        commit: &MailboxSyncCommitResult,
    ) -> Result<Self, SyncRuntimeError> {
        Ok(Self {
            mailbox: plan.mailbox.clone(),
            uid_validity: plan.uid_validity.clone(),
            highest_modseq: plan.highest_modseq.clone(),
            last_seen_uid: plan.last_seen_uid,
            synced: plan.synced,
            created: commit.created_messages.len() as i64,
            updated: plan.updated,
            deleted: plan.deleted,
            message_changes: commit
                .message_changes
                .iter()
                .map(serde_json::to_value)
                .collect::<Result<Vec<_>, _>>()?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct SyncFailure {
    pub mailbox: String,
    pub code: String,
    pub message: String,
    pub auth_required: bool,
    pub retry_minutes: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncPolicySettings {
    pub enabled: bool,
    pub folder_mode: String,
    pub selected_mailboxes: Vec<String>,
    pub notify_on_error: bool,
}

impl Default for SyncPolicySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            folder_mode: "inbox".into(),
            selected_mailboxes: vec![],
            notify_on_error: true,
        }
    }
}

pub struct SyncRuntimeStore {
    connection: Connection,
}

impl SyncRuntimeStore {
    pub fn open_database(database_path: impl AsRef<Path>) -> Result<Self, SyncRuntimeError> {
        let database_path = database_path.as_ref();
        if !database_path.is_file() {
            return Err(SyncRuntimeError::MissingDatabase);
        }
        let connection = Connection::open_with_flags(
            database_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.busy_timeout(StdDuration::from_secs(5))?;
        connection.pragma_update(None, "foreign_keys", true)?;
        let raw: String = connection
            .query_row(
                "SELECT value FROM metadata WHERE key='schema_version'",
                [],
                |row| row.get(0),
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => SyncRuntimeError::InvalidSchema,
                other => SyncRuntimeError::Sqlite(other),
            })?;
        let actual = raw
            .parse::<u32>()
            .map_err(|_| SyncRuntimeError::InvalidSchema)?;
        if actual != CURRENT_SCHEMA_VERSION {
            return Err(SyncRuntimeError::UnsupportedSchema {
                actual,
                required: CURRENT_SCHEMA_VERSION,
            });
        }
        Ok(Self { connection })
    }

    pub fn default_policy(&self, user_id: &str) -> Result<SyncPolicySettings, SyncRuntimeError> {
        let raw: Option<String> = self
            .connection
            .query_row(
                "SELECT value FROM metadata WHERE key=?1",
                [format!("sync_default_policy:{user_id}")],
                |row| row.get(0),
            )
            .optional()?;
        let Some(raw) = raw else {
            return Ok(SyncPolicySettings::default());
        };
        let value: Value = match serde_json::from_str(&raw) {
            Ok(value) => value,
            Err(_) => return Ok(SyncPolicySettings::default()),
        };
        Ok(policy_settings_from_value(&value))
    }

    pub fn update_default_policy(
        &self,
        user_id: &str,
        settings: &SyncPolicySettings,
    ) -> Result<(), SyncRuntimeError> {
        validate_policy(settings)?;
        let value = serde_json::to_string(&json!({
            "enabled": settings.enabled,
            "folderMode": settings.folder_mode,
            "selectedMailboxes": settings.selected_mailboxes,
            "notifyOnError": settings.notify_on_error,
        }))?;
        self.connection.execute(
            "INSERT INTO metadata(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![format!("sync_default_policy:{user_id}"), value],
        )?;
        Ok(())
    }

    pub fn ensure_policy(
        &self,
        account_id: &str,
        user_id: &str,
        now: DateTime<Utc>,
    ) -> Result<SyncPolicyReadModel, SyncRuntimeError> {
        let defaults = self.default_policy(user_id)?;
        self.connection.execute(
            "INSERT OR IGNORE INTO sync_policies(account_id,enabled,folder_mode,selected_mailboxes_json,notify_on_error,updated_at) VALUES (?1,?2,?3,?4,?5,?6)",
            params![account_id,i64::from(defaults.enabled),defaults.folder_mode,serde_json::to_string(&defaults.selected_mailboxes)?,i64::from(defaults.notify_on_error),timestamp(now)],
        )?;
        self.policy(account_id)?
            .ok_or(SyncRuntimeError::InvalidInput("account_id"))
    }

    pub fn policy(
        &self,
        account_id: &str,
    ) -> Result<Option<SyncPolicyReadModel>, SyncRuntimeError> {
        self.connection.query_row("SELECT account_id,enabled,folder_mode,selected_mailboxes_json,notify_on_error,updated_at FROM sync_policies WHERE account_id=?1", [account_id], |row| {
            let selected: String = row.get(3)?;
            Ok(SyncPolicyReadModel { account_id:row.get(0)?,enabled:row.get::<_,i64>(1)? != 0,folder_mode:row.get(2)?,selected_mailboxes:serde_json::from_str(&selected).map_err(|error| rusqlite::Error::FromSqlConversionFailure(3,rusqlite::types::Type::Text,Box::new(error)))?,notify_on_error:row.get::<_,i64>(4)? != 0,updated_at:row.get(5)? })
        }).optional().map_err(Into::into)
    }

    pub fn update_policy(
        &mut self,
        account_id: &str,
        settings: &SyncPolicySettings,
        now: DateTime<Utc>,
    ) -> Result<SyncPolicyReadModel, SyncRuntimeError> {
        validate_policy(settings)?;
        let now = timestamp(now);
        let transaction = self.connection.transaction()?;
        if transaction.execute("UPDATE sync_policies SET enabled=?1,folder_mode=?2,selected_mailboxes_json=?3,notify_on_error=?4,updated_at=?5 WHERE account_id=?6", params![i64::from(settings.enabled),settings.folder_mode,serde_json::to_string(&settings.selected_mailboxes)?,i64::from(settings.notify_on_error),now,account_id])? != 1 {
            return Err(SyncRuntimeError::InvalidInput("account policy missing"));
        }
        if settings.enabled {
            transaction.execute("UPDATE mailbox_sync_states SET sync_state=CASE WHEN sync_state='paused' THEN 'idle' ELSE sync_state END WHERE account_id=?1", [account_id])?;
        } else {
            transaction.execute("UPDATE sync_jobs SET status='cancelled',finished_at=?1 WHERE account_id=?2 AND status='queued'", params![now,account_id])?;
            transaction.execute("UPDATE mailbox_sync_states SET sync_state='paused',next_sync_at=NULL WHERE account_id=?1", [account_id])?;
        }
        transaction.commit()?;
        self.policy(account_id)?
            .ok_or(SyncRuntimeError::InvalidInput("account policy missing"))
    }

    pub fn resume_after_authorization(
        &self,
        account_id: &str,
        now: DateTime<Utc>,
    ) -> Result<usize, SyncRuntimeError> {
        Ok(self.connection.execute("UPDATE mailbox_sync_states SET connection_status='connected',sync_state='idle',next_sync_at=?1,consecutive_failures=0,last_error_code=NULL,last_error_message=NULL WHERE account_id=?2 AND connection_status='authRequired'", params![timestamp(now),account_id])?)
    }

    pub fn enqueue(
        &mut self,
        input: &SyncEnqueue,
        now: DateTime<Utc>,
    ) -> Result<SyncJobReadModel, SyncRuntimeError> {
        validate_enqueue(input)?;
        let not_before = timestamp(input.not_before.unwrap_or(now));
        let now = timestamp(now);
        let transaction = self.connection.transaction()?;
        let existing_id: Option<String> = transaction
            .query_row(
                "SELECT id FROM sync_jobs WHERE account_id=?1 AND coalesce(mailbox,'')=coalesce(?2,'') AND mailbox_role=?3 AND status IN ('queued','running') ORDER BY created_at LIMIT 1",
                params![input.account_id, input.mailbox, input.mailbox_role],
                |row| row.get(0),
            )
            .optional()?;
        let id = if let Some(id) = existing_id {
            let status: String = transaction.query_row(
                "SELECT status FROM sync_jobs WHERE id=?1",
                [&id],
                |row| row.get(0),
            )?;
            if status == "running" {
                transaction.execute(
                    "UPDATE sync_jobs SET rerun_requested=1, priority=max(priority,?1) WHERE id=?2",
                    params![input.priority, id],
                )?;
            } else {
                transaction.execute(
                    "UPDATE sync_jobs SET priority=max(priority,?1), not_before=min(not_before,?2) WHERE id=?3",
                    params![input.priority, not_before, id],
                )?;
            }
            id
        } else {
            let id = Uuid::new_v4().to_string();
            transaction.execute(
                "INSERT INTO sync_jobs(id,account_id,mailbox,mailbox_role,reason,status,priority,not_before,attempts,created_at) VALUES (?1,?2,?3,?4,?5,'queued',?6,?7,0,?8)",
                params![id, input.account_id, input.mailbox, input.mailbox_role, input.reason, input.priority, not_before, now],
            )?;
            id
        };
        let job = job_by_id(&transaction, &id)?.expect("inserted sync job must exist");
        transaction.commit()?;
        Ok(job)
    }

    pub fn commit_mailbox_sync(
        &mut self,
        plan: &MailboxSyncPlan,
        completed_at: DateTime<Utc>,
    ) -> Result<MailboxSyncCommitResult, SyncRuntimeError> {
        if plan.account_id.is_empty() || plan.mailbox.is_empty() {
            return Err(SyncRuntimeError::InvalidInput("mailbox sync plan"));
        }
        let completed_at = timestamp(completed_at);
        let transaction = self.connection.transaction()?;
        let owner_id: Option<String> = transaction
            .query_row(
                "SELECT user_id FROM accounts WHERE id=?1",
                [&plan.account_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(owner_id) = owner_id else {
            return Err(SyncRuntimeError::InvalidInput("account_id"));
        };
        let before = sync_message_summaries(&transaction, &plan.account_id)?;
        let mut created_messages = Vec::new();
        let mut incoming = plan.incoming.clone();
        for message in &mut incoming {
            if message.account_id != plan.account_id
                || message.mailbox != plan.mailbox
                || message.mailbox_role != plan.mailbox_role
            {
                return Err(SyncRuntimeError::InvalidInput("incoming message target"));
            }
            let previous: Option<(String, Option<String>)> = transaction
                .query_row(
                    "SELECT labels_json,snoozed_until FROM messages WHERE id=?1",
                    [&message.id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            if let Some((labels, snoozed_until)) = previous {
                message.labels = serde_json::from_str(&labels)?;
                message.snoozed_until = snoozed_until;
            } else {
                created_messages.push(message.clone());
            }
        }
        if plan.uid_validity_changed {
            transaction.execute(
                "DELETE FROM messages WHERE account_id=?1 AND mailbox=?2",
                params![plan.account_id, plan.mailbox],
            )?;
        } else {
            for uid in &plan.removed_uids {
                transaction.execute(
                    "DELETE FROM messages WHERE account_id=?1 AND mailbox=?2 AND uid=?3",
                    params![plan.account_id, plan.mailbox, i64::from(*uid)],
                )?;
            }
        }
        for message in &incoming {
            if let Some(message_id) = message.message_id.as_deref() {
                transaction.execute(
                    "DELETE FROM messages WHERE account_id=?1 AND id<>?2 AND message_id=?3 AND (CASE WHEN ?4='custom' THEN mailbox=?5 ELSE mailbox_role=?4 END)",
                    params![plan.account_id,message.id,message_id,plan.mailbox_role,plan.mailbox],
                )?;
            }
            upsert_sync_message(&transaction, message)?;
        }
        for flags in &plan.flag_updates {
            transaction.execute(
                "UPDATE messages SET unread=?1,flagged=?2 WHERE account_id=?3 AND mailbox=?4 AND uid=?5",
                params![i64::from(flags.unread),i64::from(flags.flagged),plan.account_id,plan.mailbox,i64::from(flags.uid)],
            )?;
        }
        transaction.execute(
            "DELETE FROM messages WHERE account_id=?1 AND id NOT IN (SELECT id FROM messages WHERE account_id=?1 ORDER BY received_at DESC,id DESC LIMIT 5000)",
            [&plan.account_id],
        )?;
        transaction.execute(
            "UPDATE accounts SET status='connected',last_sync_at=?1,last_error=NULL,mailboxes_json=?2 WHERE id=?3",
            params![completed_at,serde_json::to_string(&plan.folders)?,plan.account_id],
        )?;
        reconcile_sync_contacts(&transaction, &owner_id)?;
        let after = sync_message_summaries(&transaction, &plan.account_id)?;
        let before_by_id = before
            .iter()
            .map(|message| (message.id.clone(), message))
            .collect::<std::collections::BTreeMap<_, _>>();
        let after_by_id = after
            .iter()
            .map(|message| (message.id.clone(), message))
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut message_changes = before
            .iter()
            .filter(|message| !after_by_id.contains_key(&message.id))
            .map(|message| MailboxMessageChange {
                before: Some(message.clone()),
                after: None,
            })
            .collect::<Vec<_>>();
        message_changes.extend(after.iter().filter_map(|message| {
            let previous = before_by_id.get(&message.id).copied();
            (previous != Some(message)).then(|| MailboxMessageChange {
                before: previous.cloned(),
                after: Some(message.clone()),
            })
        }));
        transaction.commit()?;
        Ok(MailboxSyncCommitResult {
            created_messages,
            message_changes,
        })
    }

    pub fn claim_next(
        &mut self,
        worker_id: &str,
        lease: StdDuration,
        now: DateTime<Utc>,
    ) -> Result<Option<SyncJobReadModel>, SyncRuntimeError> {
        if worker_id.trim().is_empty() || lease.is_zero() {
            return Err(SyncRuntimeError::InvalidInput("worker_id/lease"));
        }
        let now_iso = timestamp(now);
        let locked_until = timestamp(now + chrono_duration(lease)?);
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE sync_jobs SET status='queued',locked_by=NULL,locked_until=NULL WHERE status='running' AND locked_until IS NOT NULL AND locked_until<=?1",
            [&now_iso],
        )?;
        let id: Option<String> = transaction
            .query_row(
                "SELECT id FROM sync_jobs WHERE status='queued' AND not_before<=?1 ORDER BY priority DESC,created_at ASC LIMIT 1",
                [&now_iso],
                |row| row.get(0),
            )
            .optional()?;
        let Some(id) = id else {
            transaction.commit()?;
            return Ok(None);
        };
        transaction.execute(
            "UPDATE sync_jobs SET status='running',locked_by=?1,locked_until=?2,attempts=attempts+1,started_at=coalesce(started_at,?3) WHERE id=?4",
            params![worker_id, locked_until, now_iso, id],
        )?;
        let job = job_by_id(&transaction, &id)?;
        transaction.commit()?;
        Ok(job)
    }

    pub fn renew_lease(
        &self,
        job_id: &str,
        worker_id: &str,
        lease: StdDuration,
        now: DateTime<Utc>,
    ) -> Result<bool, SyncRuntimeError> {
        let until = timestamp(now + chrono_duration(lease)?);
        Ok(self.connection.execute(
            "UPDATE sync_jobs SET locked_until=?1 WHERE id=?2 AND status='running' AND locked_by=?3",
            params![until, job_id, worker_id],
        )? == 1)
    }

    pub fn mark_started(
        &mut self,
        job: &SyncJobReadModel,
        mailbox: &str,
        now: DateTime<Utc>,
    ) -> Result<(), SyncRuntimeError> {
        let now = timestamp(now);
        let transaction = self.connection.transaction()?;
        require_lease(&transaction, job)?;
        transaction.execute(
            "INSERT INTO mailbox_sync_states(account_id,mailbox,mailbox_role,last_seen_uid,last_attempt_at,consecutive_failures,connection_status,sync_state) VALUES (?1,?2,?3,0,?4,0,'connected','running') ON CONFLICT(account_id,mailbox) DO UPDATE SET mailbox_role=excluded.mailbox_role,last_attempt_at=excluded.last_attempt_at,sync_state='running',last_error_code=NULL,last_error_message=NULL",
            params![job.account_id, mailbox, job.mailbox_role, now],
        )?;
        insert_event(
            &transaction,
            "sync.started",
            job,
            json!({"mailbox": mailbox, "mailboxRole": job.mailbox_role, "reason": job.reason}),
            &now,
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn complete(
        &mut self,
        job: &SyncJobReadModel,
        result: &SyncCompletion,
        reconciliation_minutes: i64,
        now: DateTime<Utc>,
    ) -> Result<(), SyncRuntimeError> {
        if reconciliation_minutes <= 0 || result.last_seen_uid < 0 {
            return Err(SyncRuntimeError::InvalidInput("completion"));
        }
        let now_iso = timestamp(now);
        let next = timestamp(now + Duration::minutes(reconciliation_minutes));
        let transaction = self.connection.transaction()?;
        let rerun = require_lease(&transaction, job)?;
        let changed = transaction.execute(
            "UPDATE sync_jobs SET status='succeeded',finished_at=?1,locked_by=NULL,locked_until=NULL,synced_count=?2,new_count=?3,updated_count=?4,deleted_count=?5,error_code=NULL,error_message=NULL,rerun_requested=0 WHERE id=?6 AND status='running' AND locked_by=?7",
            params![now_iso,result.synced,result.created,result.updated,result.deleted,job.id,job.locked_by],
        )?;
        if changed != 1 {
            return Err(SyncRuntimeError::LeaseLost);
        }
        transaction.execute(
            "INSERT INTO mailbox_sync_states(account_id,mailbox,mailbox_role,uid_validity,last_seen_uid,highest_modseq,last_attempt_at,last_success_at,next_sync_at,consecutive_failures,connection_status,sync_state) VALUES (?1,?2,?3,?4,?5,?6,?7,?7,?8,0,'connected','idle') ON CONFLICT(account_id,mailbox) DO UPDATE SET mailbox_role=excluded.mailbox_role,uid_validity=excluded.uid_validity,last_seen_uid=excluded.last_seen_uid,highest_modseq=excluded.highest_modseq,last_attempt_at=excluded.last_attempt_at,last_success_at=excluded.last_success_at,next_sync_at=excluded.next_sync_at,consecutive_failures=0,connection_status='connected',sync_state='idle',last_error_code=NULL,last_error_message=NULL",
            params![job.account_id,result.mailbox,job.mailbox_role,result.uid_validity,result.last_seen_uid,result.highest_modseq,now_iso,next],
        )?;
        transaction.execute("DELETE FROM mailbox_sync_states WHERE account_id=?1 AND mailbox LIKE '@role:%' AND mailbox<>?2", params![job.account_id,result.mailbox])?;
        insert_event(
            &transaction,
            "sync.completed",
            job,
            json!({"mailbox":result.mailbox,"mailboxRole":job.mailbox_role,"synced":result.synced,"created":result.created,"updated":result.updated,"deleted":result.deleted,"messageChanges":result.message_changes}),
            &now_iso,
        )?;
        if rerun {
            transaction.execute(
                "INSERT INTO sync_jobs(id,account_id,mailbox,mailbox_role,reason,status,priority,not_before,attempts,created_at) VALUES (?1,?2,?3,?4,'recovery','queued',?5,?6,0,?6)",
                params![Uuid::new_v4().to_string(),job.account_id,job.mailbox,job.mailbox_role,job.priority.max(50),now_iso],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn fail(
        &mut self,
        job: &SyncJobReadModel,
        failure: &SyncFailure,
        now: DateTime<Utc>,
    ) -> Result<(), SyncRuntimeError> {
        let now_iso = timestamp(now);
        let next = failure
            .retry_minutes
            .map(|minutes| timestamp(now + Duration::minutes(minutes)));
        let (connection_status, sync_state) = if failure.auth_required {
            ("authRequired", "paused")
        } else {
            ("unreachable", "backoff")
        };
        let transaction = self.connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE sync_jobs SET status='failed',finished_at=?1,locked_by=NULL,locked_until=NULL,error_code=?2,error_message=?3 WHERE id=?4 AND status='running' AND locked_by=?5",
            params![now_iso,failure.code,failure.message,job.id,job.locked_by],
        )?;
        if changed != 1 {
            return Err(SyncRuntimeError::LeaseLost);
        }
        transaction.execute(
            "INSERT INTO mailbox_sync_states(account_id,mailbox,mailbox_role,last_seen_uid,last_attempt_at,next_sync_at,consecutive_failures,connection_status,sync_state,last_error_code,last_error_message) VALUES (?1,?2,?3,0,?4,?5,1,?6,?7,?8,?9) ON CONFLICT(account_id,mailbox) DO UPDATE SET last_attempt_at=excluded.last_attempt_at,next_sync_at=excluded.next_sync_at,consecutive_failures=mailbox_sync_states.consecutive_failures+1,connection_status=excluded.connection_status,sync_state=excluded.sync_state,last_error_code=excluded.last_error_code,last_error_message=excluded.last_error_message",
            params![job.account_id,failure.mailbox,job.mailbox_role,now_iso,next,connection_status,sync_state,failure.code,failure.message],
        )?;
        insert_event(
            &transaction,
            "sync.failed",
            job,
            json!({"mailbox":failure.mailbox,"mailboxRole":job.mailbox_role,"code":failure.code,"message":failure.message,"nextSyncAt":next}),
            &now_iso,
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn cancel(
        &mut self,
        job: &SyncJobReadModel,
        mailbox: &str,
        now: DateTime<Utc>,
    ) -> Result<(), SyncRuntimeError> {
        let transaction = self.connection.transaction()?;
        if transaction.execute("UPDATE sync_jobs SET status='cancelled',finished_at=?1,locked_by=NULL,locked_until=NULL,error_code=NULL,error_message=NULL WHERE id=?2 AND status='running' AND locked_by=?3", params![timestamp(now),job.id,job.locked_by])? != 1 {
            return Err(SyncRuntimeError::LeaseLost);
        }
        transaction.execute("INSERT INTO mailbox_sync_states(account_id,mailbox,mailbox_role,last_seen_uid,consecutive_failures,connection_status,sync_state) VALUES (?1,?2,?3,0,0,'connected','paused') ON CONFLICT(account_id,mailbox) DO UPDATE SET sync_state='paused',next_sync_at=NULL", params![job.account_id,mailbox,job.mailbox_role])?;
        transaction.commit()?;
        Ok(())
    }

    pub fn job(&self, id: &str) -> Result<Option<SyncJobReadModel>, SyncRuntimeError> {
        job_by_id(&self.connection, id)
    }

    pub fn account_jobs(
        &self,
        account_id: &str,
        limit: usize,
    ) -> Result<Vec<SyncJobReadModel>, SyncRuntimeError> {
        if account_id.is_empty() || limit == 0 || limit > 500 {
            return Err(SyncRuntimeError::InvalidInput("account jobs"));
        }
        let mut statement = self.connection.prepare(
            "SELECT id,account_id,mailbox,mailbox_role,reason,status,priority,not_before,
                    locked_by,locked_until,attempts,created_at,started_at,finished_at,
                    synced_count,new_count,updated_count,deleted_count,error_code,error_message,
                    rerun_requested
             FROM sync_jobs WHERE account_id=?1
             ORDER BY created_at DESC,rowid DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![account_id, limit as i64], map_job)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn account_mailbox_states(
        &self,
        account_id: &str,
    ) -> Result<Vec<MailboxSyncStateReadModel>, SyncRuntimeError> {
        if account_id.is_empty() {
            return Err(SyncRuntimeError::InvalidInput("account states"));
        }
        let mut statement = self.connection.prepare(
            "SELECT account_id,mailbox,mailbox_role,uid_validity,last_seen_uid,highest_modseq,
                    last_attempt_at,last_success_at,next_sync_at,consecutive_failures,
                    connection_status,sync_state,last_error_code,last_error_message
             FROM mailbox_sync_states WHERE account_id=?1 ORDER BY mailbox",
        )?;
        let rows = statement.query_map([account_id], map_state)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn mailbox_state(
        &self,
        account_id: &str,
        mailbox: &str,
    ) -> Result<Option<MailboxSyncStateReadModel>, SyncRuntimeError> {
        self.connection.query_row("SELECT account_id,mailbox,mailbox_role,uid_validity,last_seen_uid,highest_modseq,last_attempt_at,last_success_at,next_sync_at,consecutive_failures,connection_status,sync_state,last_error_code,last_error_message FROM mailbox_sync_states WHERE account_id=?1 AND mailbox=?2", params![account_id,mailbox], map_state).optional().map_err(Into::into)
    }

    pub fn mailbox_state_for_target(
        &self,
        account_id: &str,
        mailbox: Option<&str>,
        mailbox_role: &str,
    ) -> Result<Option<MailboxSyncStateReadModel>, SyncRuntimeError> {
        self.connection
            .query_row(
                "SELECT account_id,mailbox,mailbox_role,uid_validity,last_seen_uid,highest_modseq,last_attempt_at,last_success_at,next_sync_at,consecutive_failures,connection_status,sync_state,last_error_code,last_error_message
                 FROM mailbox_sync_states
                 WHERE account_id=?1 AND ((?2 IS NOT NULL AND mailbox=?2) OR (?2 IS NULL AND mailbox_role=?3))
                 ORDER BY CASE WHEN mailbox LIKE '@role:%' THEN 1 ELSE 0 END, last_success_at DESC
                 LIMIT 1",
                params![account_id, mailbox, mailbox_role],
                map_state,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn account_owner_id(&self, account_id: &str) -> Result<Option<String>, SyncRuntimeError> {
        self.connection
            .query_row(
                "SELECT user_id FROM accounts WHERE id=?1",
                [account_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn enabled_sync_account_ids(&self) -> Result<Vec<String>, SyncRuntimeError> {
        let mut statement = self.connection.prepare(
            "SELECT p.account_id
             FROM sync_policies p
             JOIN accounts a ON a.id=p.account_id
             WHERE p.enabled=1
             ORDER BY p.account_id",
        )?;
        let rows = statement.query_map([], |row| row.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn enqueue_due_syncs(
        &mut self,
        reason: &str,
        now: DateTime<Utc>,
    ) -> Result<Vec<SyncJobReadModel>, SyncRuntimeError> {
        if !matches!(reason, "startup" | "scheduled") {
            return Err(SyncRuntimeError::InvalidInput("scheduler reason"));
        }
        let accounts = sync_accounts(&self.connection)?;
        let now_iso = timestamp(now);
        let mut jobs = Vec::new();
        for account in accounts {
            let policy = self.ensure_policy(&account.id, &account.owner_id, now)?;
            if !policy.enabled {
                continue;
            }
            if account.status == "connected" {
                self.resume_after_authorization(&account.id, now)?;
            }
            for target in targets_for_policy(&account, &policy) {
                let state = self.mailbox_state_for_target(
                    &account.id,
                    target.requested_mailbox.as_deref(),
                    &target.mailbox_role,
                )?;
                if state.as_ref().is_some_and(|state| {
                    state.connection_status == "authRequired"
                        || state.sync_state == "paused"
                        || state
                            .next_sync_at
                            .as_deref()
                            .is_some_and(|next| next > now_iso.as_str())
                }) {
                    continue;
                }
                let job_reason = if state
                    .as_ref()
                    .is_some_and(|state| state.consecutive_failures > 0)
                {
                    "recovery"
                } else {
                    reason
                };
                jobs.push(self.enqueue(
                    &SyncEnqueue {
                        account_id: account.id.clone(),
                        mailbox: target.requested_mailbox,
                        mailbox_role: target.mailbox_role,
                        reason: job_reason.into(),
                        priority: if reason == "startup" { 5 } else { 0 },
                        not_before: Some(now),
                    },
                    now,
                )?);
            }
        }
        self.prune_events(now - Duration::days(7))?;
        Ok(jobs)
    }

    pub fn prune_events(&self, before: DateTime<Utc>) -> Result<usize, SyncRuntimeError> {
        Ok(self.connection.execute(
            "DELETE FROM sync_events WHERE created_at<?1",
            [timestamp(before)],
        )?)
    }

    pub fn events(
        &self,
        after_id: i64,
        limit: usize,
    ) -> Result<Vec<SyncEventReadModel>, SyncRuntimeError> {
        let mut statement = self.connection.prepare("SELECT id,event_type,account_id,job_id,payload_json,created_at FROM sync_events WHERE id>?1 ORDER BY id LIMIT ?2")?;
        let rows = statement.query_map(params![after_id, limit.clamp(1, 500)], |row| {
            let payload: String = row.get(4)?;
            Ok(SyncEventReadModel {
                id: row.get(0)?,
                event_type: row.get(1)?,
                account_id: row.get(2)?,
                job_id: row.get(3)?,
                payload: serde_json::from_str(&payload).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        4,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?,
                created_at: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn latest_event_id(&self) -> Result<i64, SyncRuntimeError> {
        self.connection
            .query_row("SELECT coalesce(max(id),0) FROM sync_events", [], |row| {
                row.get(0)
            })
            .map_err(Into::into)
    }

    pub fn record_message_created(
        &mut self,
        account_id: &str,
        account_email: &str,
        messages: &[MessageReadModel],
        now: DateTime<Utc>,
    ) -> Result<(), SyncRuntimeError> {
        if messages.is_empty() {
            return Ok(());
        }
        let created_at = timestamp(now);
        let transaction = self.connection.transaction()?;
        for message in messages {
            if message.account_id != account_id {
                return Err(SyncRuntimeError::InvalidInput("message owner"));
            }
            let attachments = message
                .attachments
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .enumerate()
                        .map(|(index, item)| {
                            let mut item = item.clone();
                            if let Some(object) = item.as_object_mut() {
                                object
                                    .entry("index")
                                    .or_insert_with(|| Value::from(index as u64));
                            }
                            item
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let summary = json!({
                "id": message.id,
                "accountEmail": account_email,
                "folder": message.mailbox,
                "mailboxRole": message.mailbox_role,
                "from": message.from,
                "to": message.to,
                "subject": message.subject,
                "preview": message.preview,
                "date": message.date,
                "unread": message.unread,
                "flagged": message.flagged,
                "hasAttachments": message.has_attachments,
                "attachments": attachments,
                "labels": message.labels,
            });
            transaction.execute(
                "INSERT INTO sync_events(event_type,account_id,job_id,payload_json,created_at)
                 VALUES ('message.created',?1,NULL,?2,?3)",
                params![
                    account_id,
                    serde_json::to_string(&json!({ "message": summary }))?,
                    created_at,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn heartbeat(
        &self,
        worker_id: &str,
        process_id: i64,
        host_name: &str,
        started_at: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<(), SyncRuntimeError> {
        let now_iso = timestamp(now);
        self.connection.execute("INSERT INTO sync_worker_heartbeats(worker_id,process_id,host_name,started_at,heartbeat_at) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(worker_id) DO UPDATE SET heartbeat_at=excluded.heartbeat_at", params![worker_id,process_id,host_name,timestamp(started_at),now_iso])?;
        self.connection.execute(
            "DELETE FROM sync_worker_heartbeats WHERE heartbeat_at<?1",
            [timestamp(now - Duration::seconds(WORKER_STALE_SECONDS))],
        )?;
        Ok(())
    }

    pub fn remove_heartbeat(&self, worker_id: &str) -> Result<(), SyncRuntimeError> {
        self.connection.execute(
            "DELETE FROM sync_worker_heartbeats WHERE worker_id=?1",
            [worker_id],
        )?;
        Ok(())
    }

    pub fn worker_health(
        &self,
        now: DateTime<Utc>,
    ) -> Result<SyncWorkerHealthReadModel, SyncRuntimeError> {
        let cutoff = timestamp(now - Duration::seconds(WORKER_STALE_SECONDS));
        let mut statement = self.connection.prepare("SELECT worker_id,process_id,host_name,started_at,heartbeat_at FROM sync_worker_heartbeats WHERE heartbeat_at>=?1 ORDER BY heartbeat_at DESC")?;
        let workers = statement
            .query_map([cutoff], |row| {
                Ok(SyncWorkerReadModel {
                    worker_id: row.get(0)?,
                    process_id: row.get(1)?,
                    host_name: row.get(2)?,
                    started_at: row.get(3)?,
                    heartbeat_at: row.get(4)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        let (queued_jobs, oldest_queued_at) = self.connection.query_row(
            "SELECT count(*),min(created_at) FROM sync_jobs WHERE status='queued'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok(SyncWorkerHealthReadModel {
            workers,
            queued_jobs,
            oldest_queued_at,
        })
    }
}

impl MailboxSyncCommitRepository for SyncRuntimeStore {
    type Error = SyncRuntimeError;

    fn commit_mailbox_sync(
        &mut self,
        plan: &MailboxSyncPlan,
        completed_at: DateTime<Utc>,
    ) -> Result<MailboxSyncCommitResult, Self::Error> {
        SyncRuntimeStore::commit_mailbox_sync(self, plan, completed_at)
    }
}

#[allow(clippy::type_complexity)]
fn sync_accounts(connection: &Connection) -> Result<Vec<AccountRecord>, SyncRuntimeError> {
    let mut statement = connection.prepare(
        "SELECT id,user_id,provider,email,display_name,group_name,group_icon,color,
                settings_json,proxy_json,encrypted_secret,auth_method,created_at,last_sync_at,
                status,last_error,mailboxes_json
         FROM accounts ORDER BY created_at,id",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, Option<String>>(11)?,
                row.get::<_, String>(12)?,
                row.get::<_, Option<String>>(13)?,
                row.get::<_, String>(14)?,
                row.get::<_, Option<String>>(15)?,
                row.get::<_, String>(16)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|row| {
            Ok(AccountRecord {
                id: row.0,
                owner_id: row.1,
                provider: row.2,
                email: row.3,
                display_name: row.4,
                group: row.5,
                group_icon: row.6,
                color: row.7,
                settings: serde_json::from_str(&row.8)?,
                proxy: row
                    .9
                    .map(|value| serde_json::from_str(&value))
                    .transpose()?,
                encrypted_secret: row.10,
                auth_method: row.11,
                created_at: row.12,
                last_sync_at: row.13,
                status: row.14,
                last_error: row.15,
                mailboxes: serde_json::from_str(&row.16)?,
            })
        })
        .collect()
}

fn validate_enqueue(input: &SyncEnqueue) -> Result<(), SyncRuntimeError> {
    if input.account_id.is_empty()
        || !matches!(
            input.reason.as_str(),
            "scheduled" | "startup" | "manual" | "recovery"
        )
        || input.mailbox_role.is_empty()
    {
        return Err(SyncRuntimeError::InvalidInput("enqueue"));
    }
    Ok(())
}

fn validate_policy(settings: &SyncPolicySettings) -> Result<(), SyncRuntimeError> {
    if !matches!(
        settings.folder_mode.as_str(),
        "inbox" | "standard" | "selected"
    ) || settings
        .selected_mailboxes
        .iter()
        .any(|mailbox| mailbox.trim().is_empty())
    {
        return Err(SyncRuntimeError::InvalidInput("policy"));
    }
    Ok(())
}

fn policy_settings_from_value(value: &Value) -> SyncPolicySettings {
    let defaults = SyncPolicySettings::default();
    let folder_mode = value
        .get("folderMode")
        .and_then(Value::as_str)
        .filter(|mode| matches!(*mode, "inbox" | "standard" | "selected"))
        .unwrap_or(&defaults.folder_mode)
        .to_string();
    let selected_mailboxes = value
        .get("selectedMailboxes")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    SyncPolicySettings {
        enabled: value
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(defaults.enabled),
        folder_mode,
        selected_mailboxes,
        notify_on_error: value
            .get("notifyOnError")
            .and_then(Value::as_bool)
            .unwrap_or(defaults.notify_on_error),
    }
}

fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn chrono_duration(value: StdDuration) -> Result<Duration, SyncRuntimeError> {
    Duration::from_std(value).map_err(|_| SyncRuntimeError::InvalidInput("lease"))
}

fn require_lease(
    transaction: &Transaction<'_>,
    job: &SyncJobReadModel,
) -> Result<bool, SyncRuntimeError> {
    transaction.query_row("SELECT rerun_requested FROM sync_jobs WHERE id=?1 AND status='running' AND locked_by=?2", params![job.id,job.locked_by], |row| row.get::<_,i64>(0).map(|value| value != 0)).optional()?.ok_or(SyncRuntimeError::LeaseLost)
}

fn insert_event(
    transaction: &Transaction<'_>,
    event_type: &str,
    job: &SyncJobReadModel,
    payload: Value,
    created_at: &str,
) -> Result<(), SyncRuntimeError> {
    transaction.execute("INSERT INTO sync_events(event_type,account_id,job_id,payload_json,created_at) VALUES (?1,?2,?3,?4,?5)", params![event_type,job.account_id,job.id,serde_json::to_string(&payload)?,created_at])?;
    Ok(())
}

fn map_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<SyncJobReadModel> {
    Ok(SyncJobReadModel {
        id: row.get(0)?,
        account_id: row.get(1)?,
        mailbox: row.get(2)?,
        mailbox_role: row.get(3)?,
        reason: row.get(4)?,
        status: row.get(5)?,
        priority: row.get(6)?,
        not_before: row.get(7)?,
        locked_by: row.get(8)?,
        locked_until: row.get(9)?,
        attempts: row.get(10)?,
        created_at: row.get(11)?,
        started_at: row.get(12)?,
        finished_at: row.get(13)?,
        synced_count: row.get(14)?,
        new_count: row.get(15)?,
        updated_count: row.get(16)?,
        deleted_count: row.get(17)?,
        error_code: row.get(18)?,
        error_message: row.get(19)?,
        rerun_requested: row.get::<_, i64>(20)? != 0,
    })
}

fn job_by_id(
    connection: &Connection,
    id: &str,
) -> Result<Option<SyncJobReadModel>, SyncRuntimeError> {
    connection.query_row("SELECT id,account_id,mailbox,mailbox_role,reason,status,priority,not_before,locked_by,locked_until,attempts,created_at,started_at,finished_at,synced_count,new_count,updated_count,deleted_count,error_code,error_message,rerun_requested FROM sync_jobs WHERE id=?1", [id], map_job).optional().map_err(Into::into)
}

fn map_state(row: &rusqlite::Row<'_>) -> rusqlite::Result<MailboxSyncStateReadModel> {
    Ok(MailboxSyncStateReadModel {
        account_id: row.get(0)?,
        mailbox: row.get(1)?,
        mailbox_role: row.get(2)?,
        uid_validity: row.get(3)?,
        last_seen_uid: row.get(4)?,
        highest_modseq: row.get(5)?,
        last_attempt_at: row.get(6)?,
        last_success_at: row.get(7)?,
        next_sync_at: row.get(8)?,
        consecutive_failures: row.get(9)?,
        connection_status: row.get(10)?,
        sync_state: row.get(11)?,
        last_error_code: row.get(12)?,
        last_error_message: row.get(13)?,
    })
}

fn upsert_sync_message(
    transaction: &Transaction<'_>,
    message: &MessageReadModel,
) -> Result<(), SyncRuntimeError> {
    transaction.execute(
        "INSERT INTO messages(id,account_id,mailbox,mailbox_role,uid,message_id,from_json,to_json,subject,preview,text_body,html_body,received_at,unread,flagged,has_attachments,attachments_json,labels_json,snoozed_until) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19) ON CONFLICT(id) DO UPDATE SET mailbox=excluded.mailbox,mailbox_role=excluded.mailbox_role,uid=excluded.uid,message_id=excluded.message_id,from_json=excluded.from_json,to_json=excluded.to_json,subject=excluded.subject,preview=excluded.preview,text_body=excluded.text_body,html_body=excluded.html_body,received_at=excluded.received_at,unread=excluded.unread,flagged=excluded.flagged,has_attachments=excluded.has_attachments,attachments_json=excluded.attachments_json,labels_json=excluded.labels_json,snoozed_until=excluded.snoozed_until",
        params![message.id,message.account_id,message.mailbox,message.mailbox_role,message.uid,message.message_id,serde_json::to_string(&message.from)?,serde_json::to_string(&message.to)?,message.subject,message.preview,message.text,message.html,message.date,i64::from(message.unread),i64::from(message.flagged),i64::from(message.has_attachments),serde_json::to_string(&message.attachments)?,serde_json::to_string(&message.labels)?,message.snoozed_until],
    )?;
    Ok(())
}

fn sync_message_summaries(
    connection: &Connection,
    account_id: &str,
) -> Result<Vec<MessageReadModel>, SyncRuntimeError> {
    let mut statement = connection.prepare(
        "SELECT id,account_id,mailbox,mailbox_role,uid,message_id,from_json,to_json,subject,preview,received_at,unread,flagged,has_attachments,attachments_json,labels_json,snoozed_until FROM messages WHERE account_id=?1 ORDER BY received_at DESC,id",
    )?;
    let rows = statement.query_map([account_id], sync_summary_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn sync_summary_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MessageReadModel> {
    let from: String = row.get(6)?;
    let to: String = row.get(7)?;
    let attachments: String = row.get(14)?;
    let labels: String = row.get(15)?;
    let parse = |index, raw: &str| {
        serde_json::from_str(raw).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                index,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })
    };
    Ok(MessageReadModel {
        id: row.get(0)?,
        account_id: row.get(1)?,
        mailbox: row.get(2)?,
        mailbox_role: row.get(3)?,
        uid: row.get(4)?,
        message_id: row.get(5)?,
        from: parse(6, &from)?,
        to: parse(7, &to)?,
        subject: row.get(8)?,
        preview: row.get(9)?,
        text: String::new(),
        html: None,
        date: row.get(10)?,
        unread: row.get::<_, i64>(11)? != 0,
        flagged: row.get::<_, i64>(12)? != 0,
        has_attachments: row.get::<_, i64>(13)? != 0,
        attachments: parse(14, &attachments)?,
        labels: parse(15, &labels)?,
        snoozed_until: row.get(16)?,
    })
}

fn reconcile_sync_contacts(
    transaction: &Transaction<'_>,
    owner_id: &str,
) -> Result<(), SyncRuntimeError> {
    let own_addresses = transaction
        .prepare("SELECT email FROM accounts WHERE user_id=?1 ORDER BY id")?
        .query_map([owner_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let messages = transaction
        .prepare(
            "SELECT m.id,m.account_id,m.mailbox,m.mailbox_role,m.uid,m.message_id,m.from_json,m.to_json,m.subject,m.preview,m.received_at,m.unread,m.flagged,m.has_attachments,m.attachments_json,m.labels_json,m.snoozed_until FROM messages m JOIN accounts a ON a.id=m.account_id WHERE a.user_id=?1 ORDER BY m.received_at DESC,m.id",
        )?
        .query_map([owner_id], sync_summary_row)?
        .collect::<Result<Vec<_>, _>>()?;
    let previous = transaction
        .prepare("SELECT user_id,address,name,message_count,last_contact_at,logo_key,logo_content_type,logo_source_url,logo_fetched_at FROM contacts WHERE user_id=?1 ORDER BY address")?
        .query_map([owner_id], |row| {
            Ok(imail_protocol::ContactReadModel {
                owner_id: row.get(0)?,
                address: row.get(1)?,
                name: row.get(2)?,
                message_count: row.get(3)?,
                last_contact_at: row.get(4)?,
                logo_key: row.get(5)?,
                logo_content_type: row.get(6)?,
                logo_source_url: row.get(7)?,
                logo_fetched_at: row.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let contacts = reconcile_contacts_for_addresses(
        owner_id,
        &own_addresses,
        &messages,
        &previous,
        &PublicSuffixDomainResolver,
    );
    transaction.execute("DELETE FROM contacts WHERE user_id=?1", [owner_id])?;
    for contact in contacts {
        transaction.execute("INSERT INTO contacts(user_id,address,name,message_count,last_contact_at,logo_key,logo_content_type,logo_source_url,logo_fetched_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![contact.owner_id,contact.address,contact.name,contact.message_count,contact.last_contact_at,contact.logo_key,contact.logo_content_type,contact.logo_source_url,contact.logo_fetched_at])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use chrono::TimeZone;
    use imail_core::sync_execution::MailboxSyncApplicationService;
    use imail_mail::{
        ImapSyncPort, MailAuthentication, MailConnectionConfig, ProtocolFailure,
        RemoteFetchedSyncMessage, RemoteSyncBatch, RemoteSyncRequest, SyncCursor, SyncTarget,
    };
    use imail_oauth::{
        OAuthEnvironment, OAuthGrant, OAuthIdentity, OAuthProviderPort, OAuthTokenResponse,
        RefreshCoordinator,
    };
    use imail_security::MasterKey;
    use rusqlite::Connection;

    use super::*;
    use crate::{MasterKeyCredentialCodec, SqliteAuthStore};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "imail-r5-sync-{}-{}.sqlite",
                std::process::id(),
                Uuid::new_v4()
            ));
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(include_str!("../sql/schema-v10.sql"))
                .unwrap();
            connection
                .execute(
                    "INSERT INTO metadata(key,value) VALUES ('schema_version',?1)",
                    [CURRENT_SCHEMA_VERSION.to_string()],
                )
                .unwrap();
            connection.execute("INSERT INTO accounts(id,provider,email,display_name,group_name,color,settings_json,encrypted_secret,created_at,status,user_id) VALUES ('account-1','custom','owner@example.com','Owner','personal','#000','{}','cipher','2026-08-10T00:00:00.000Z','connected','user-1')", []).unwrap();
            drop(connection);
            Self(path)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
            let _ = fs::remove_file(self.0.with_extension("sqlite-wal"));
            let _ = fs::remove_file(self.0.with_extension("sqlite-shm"));
        }
    }

    fn at(second: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 10, 0, 0, 0).unwrap() + Duration::seconds(i64::from(second))
    }

    fn input(reason: &str, priority: i64) -> SyncEnqueue {
        SyncEnqueue {
            account_id: "account-1".into(),
            mailbox: None,
            mailbox_role: "inbox".into(),
            reason: reason.into(),
            priority,
            not_before: None,
        }
    }

    fn message(id: &str, uid: i64, subject: &str) -> MessageReadModel {
        MessageReadModel {
            id: id.into(),
            account_id: "account-1".into(),
            mailbox: "INBOX".into(),
            mailbox_role: "inbox".into(),
            uid,
            message_id: Some(format!("<{id}@example.com>")),
            from: json!({"name":"Sender","address":"sender@example.com"}),
            to: json!([]),
            subject: subject.into(),
            preview: subject.into(),
            text: format!("private body {subject}"),
            html: None,
            date: format!("2026-08-10T00:00:{uid:02}.000Z"),
            unread: true,
            flagged: false,
            has_attachments: false,
            attachments: json!([]),
            labels: json!([]),
            snoozed_until: None,
        }
    }

    #[test]
    fn commits_mailbox_differences_atomically_and_preserves_local_fields() {
        let fixture = Fixture::new();
        let mut store = SyncRuntimeStore::open_database(&fixture.0).unwrap();
        let mut keep = message("keep", 8, "old");
        keep.labels = json!(["important"]);
        keep.snoozed_until = Some("2026-08-11T00:00:00.000Z".into());
        let remove = message("remove", 7, "remove");
        let transaction = store.connection.transaction().unwrap();
        upsert_sync_message(&transaction, &keep).unwrap();
        upsert_sync_message(&transaction, &remove).unwrap();
        transaction.execute("INSERT INTO contacts(user_id,address,name,message_count,last_contact_at,logo_key,logo_content_type,logo_source_url,logo_fetched_at) VALUES ('user-1','sender@example.com','Sender',1,'2026-08-01T00:00:00.000Z','domain:example.com','image/png','https://logo.example.com/logo.png','2026-08-01T00:00:00.000Z')", []).unwrap();
        transaction.commit().unwrap();

        let mut refreshed = message("keep", 8, "refreshed");
        refreshed.unread = false;
        refreshed.flagged = true;
        let new_message = message("new", 9, "new");
        let result = store
            .commit_mailbox_sync(
                &MailboxSyncPlan {
                    account_id: "account-1".into(),
                    mailbox: "INBOX".into(),
                    mailbox_role: "inbox".into(),
                    uid_validity: Some("44".into()),
                    highest_modseq: Some("90".into()),
                    last_seen_uid: 9,
                    incoming: vec![refreshed, new_message.clone()],
                    removed_uids: vec![7],
                    uid_validity_changed: false,
                    flag_updates: vec![],
                    folders: vec![imail_mail::SyncMailboxFolder {
                        path: "INBOX".into(),
                        name: "INBOX".into(),
                        delimiter: "/".into(),
                        special_use: Some("\\Inbox".into()),
                        selectable: true,
                        subscribed: true,
                        total: Some(2),
                        unread: Some(1),
                    }],
                    synced: 2,
                    updated: 1,
                    deleted: 1,
                },
                at(20),
            )
            .unwrap();
        assert_eq!(result.created_messages, [new_message]);
        assert!(result.message_changes.iter().all(|change| change
            .before
            .as_ref()
            .map_or(true, |item| item.text.is_empty())
            && change
                .after
                .as_ref()
                .map_or(true, |item| item.text.is_empty())));
        let messages = sync_message_summaries(&store.connection, "account-1").unwrap();
        assert_eq!(messages.len(), 2);
        let kept = messages.iter().find(|item| item.id == "keep").unwrap();
        assert_eq!(kept.labels, json!(["important"]));
        assert_eq!(
            kept.snoozed_until.as_deref(),
            Some("2026-08-11T00:00:00.000Z")
        );
        assert_eq!(kept.subject, "refreshed");
        let contact: (i64, Option<String>) = store
            .connection
            .query_row(
                "SELECT message_count,logo_key FROM contacts WHERE user_id='user-1' AND address='sender@example.com'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(contact, (2, Some("domain:example.com".into())));
    }

    #[test]
    fn contact_failure_rolls_back_the_mailbox_cache_transaction() {
        let fixture = Fixture::new();
        let mut store = SyncRuntimeStore::open_database(&fixture.0).unwrap();
        let original = message("keep", 8, "before");
        let transaction = store.connection.transaction().unwrap();
        upsert_sync_message(&transaction, &original).unwrap();
        transaction.commit().unwrap();
        store.connection.execute_batch("CREATE TRIGGER reject_sync_contact BEFORE INSERT ON contacts BEGIN SELECT RAISE(ABORT,'contact failure'); END;").unwrap();
        let updated = message("keep", 8, "after");
        let result = store.commit_mailbox_sync(
            &MailboxSyncPlan {
                account_id: "account-1".into(),
                mailbox: "INBOX".into(),
                mailbox_role: "inbox".into(),
                uid_validity: Some("44".into()),
                highest_modseq: None,
                last_seen_uid: 8,
                incoming: vec![updated],
                removed_uids: vec![],
                uid_validity_changed: false,
                flag_updates: vec![],
                folders: vec![],
                synced: 1,
                updated: 1,
                deleted: 0,
            },
            at(20),
        );
        assert!(matches!(result, Err(SyncRuntimeError::Sqlite(_))));
        let subject: String = store
            .connection
            .query_row("SELECT subject FROM messages WHERE id='keep'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(subject, "before");
        let last_sync: Option<String> = store
            .connection
            .query_row(
                "SELECT last_sync_at FROM accounts WHERE id='account-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(last_sync, None);
    }

    struct DirectSyncPort;

    impl ImapSyncPort for DirectSyncPort {
        fn fetch_incremental(
            &mut self,
            config: &MailConnectionConfig,
            request: &RemoteSyncRequest,
        ) -> Result<RemoteSyncBatch, ProtocolFailure> {
            assert_eq!(config.email, "owner@example.com");
            assert_eq!(request.target.mailbox_role, "inbox");
            Ok(RemoteSyncBatch {
                mailbox: "INBOX".into(),
                mailbox_role: "inbox".into(),
                uid_validity: Some("50".into()),
                highest_modseq: Some("100".into()),
                uid_validity_changed: false,
                last_seen_uid: 10,
                incoming: vec![RemoteFetchedSyncMessage {
                    uid: 10,
                    source: b"Message-ID: <direct@example.com>\r\nFrom: Sender <sender@example.com>\r\nTo: Owner <owner@example.com>\r\nSubject: Direct service\r\nDate: Sun, 10 Aug 2026 08:00:00 +0800\r\n\r\nBody".to_vec(),
                    internal_date: None,
                    unread: true,
                    flagged: false,
                }],
                removed_uids: vec![],
                flag_updates: vec![],
                folders: vec![],
            })
        }
    }

    #[test]
    fn direct_rust_application_service_plans_and_commits_without_http() {
        let fixture = Fixture::new();
        let key = MasterKey::from_hex(&"42".repeat(32)).unwrap();
        let encrypted = key
            .encrypt_json(&json!({"password":"mail-secret"}))
            .unwrap();
        let connection = Connection::open(&fixture.0).unwrap();
        connection.execute("UPDATE accounts SET encrypted_secret=?1,settings_json=?2,last_sync_at='2026-08-09T00:00:00.000Z' WHERE id='account-1'", params![encrypted,json!({"imapHost":"imap.example.com","imapPort":993,"imapSecure":true,"smtpHost":"smtp.example.com","smtpPort":465,"smtpSecure":true}).to_string()]).unwrap();
        drop(connection);

        let repository = SqliteAuthStore::open_database(&fixture.0).unwrap();
        let codec = MasterKeyCredentialCodec::new(&key);
        let mut port = DirectSyncPort;
        let mut commits = SyncRuntimeStore::open_database(&fixture.0).unwrap();
        let execution =
            MailboxSyncApplicationService::new(&repository, &codec, &mut port, &mut commits)
                .execute(
                    "user-1",
                    "account-1",
                    SyncTarget {
                        mailbox_role: "inbox".into(),
                        requested_mailbox: None,
                    },
                    SyncCursor::default(),
                    at(30),
                )
                .unwrap();
        assert_eq!(execution.plan.synced, 1);
        assert_eq!(execution.commit.created_messages.len(), 1);
        assert_eq!(execution.notification_messages.len(), 1);
        let summaries = sync_message_summaries(&commits.connection, "account-1").unwrap();
        assert_eq!(summaries[0].subject, "Direct service");
    }

    #[derive(Default)]
    struct RefreshPort {
        calls: usize,
    }

    impl OAuthProviderPort for RefreshPort {
        fn token_request(
            &mut self,
            _config: &imail_oauth::OAuthConfig,
            grant: OAuthGrant<'_>,
        ) -> Result<OAuthTokenResponse, imail_oauth::OAuthError> {
            assert!(matches!(grant, OAuthGrant::RefreshToken { .. }));
            self.calls += 1;
            Ok(OAuthTokenResponse {
                access_token: "rotated-access".into(),
                refresh_token: None,
                expires_in: Some(3_600),
                token_type: Some("Bearer".into()),
                scope: Some("openid email https://mail.google.com/".into()),
                id_token: None,
            })
        }

        fn fetch_identity(
            &mut self,
            _config: &imail_oauth::OAuthConfig,
            _token: &OAuthTokenResponse,
            _nonce: &str,
        ) -> Result<OAuthIdentity, imail_oauth::OAuthError> {
            unreachable!("refresh does not fetch identity")
        }
    }

    #[test]
    fn refreshes_and_persists_oauth_before_building_sync_connection() {
        use imail_core::oauth_refresh::RefreshingConnectionService;

        let fixture = Fixture::new();
        let key = MasterKey::from_hex(&"42".repeat(32)).unwrap();
        let encrypted = key
            .encrypt_json(&json!({
                "authType":"oauth2",
                "oauthProvider":"google",
                "accessToken":"expired-access",
                "refreshToken":"refresh-old",
                "expiresAt":"2020-01-01T00:00:00.000Z",
                "scopes":["openid","email"],
                "tokenType":"Bearer",
                "proxyPassword":"proxy-secret"
            }))
            .unwrap();
        let connection = Connection::open(&fixture.0).unwrap();
        connection.execute("UPDATE accounts SET provider='gmail',auth_method='oauth2',encrypted_secret=?1,settings_json=?2 WHERE id='account-1'", params![encrypted,json!({"imapHost":"imap.gmail.com","imapPort":993,"imapSecure":true,"smtpHost":"smtp.gmail.com","smtpPort":465,"smtpSecure":true}).to_string()]).unwrap();
        drop(connection);
        let mut repository = SqliteAuthStore::open_database(&fixture.0).unwrap();
        repository.connection.execute("INSERT OR IGNORE INTO app_users(id,login,display_name,password_hash,created_at) VALUES ('user-1','owner','Owner','test-hash','2026-08-10T00:00:00.000Z')", []).unwrap();
        let codec = MasterKeyCredentialCodec::new(&key);
        let mut provider = RefreshPort::default();
        let coordinator = RefreshCoordinator::default();
        let environment = OAuthEnvironment {
            callback_base_url: "http://127.0.0.1".into(),
            google_client_id: Some("client-id".into()),
            google_client_secret: Some("client-secret".into()),
            ..OAuthEnvironment::default()
        };
        let now_ms = 1_786_336_496_000;
        let mut service = RefreshingConnectionService::new(
            &mut repository,
            &codec,
            &mut provider,
            &coordinator,
            &environment,
        );
        let first = service.resolve("user-1", "account-1", now_ms).unwrap();
        let second = service.resolve("user-1", "account-1", now_ms).unwrap();
        assert_eq!(provider.calls, 1);
        assert!(matches!(
            first.authentication,
            MailAuthentication::OAuth { access_token, .. } if access_token == "rotated-access"
        ));
        assert!(matches!(
            second.authentication,
            MailAuthentication::OAuth { access_token, .. } if access_token == "rotated-access"
        ));
        let persisted: String = repository
            .connection
            .query_row(
                "SELECT encrypted_secret FROM accounts WHERE id='account-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let secret: serde_json::Value = key.decrypt_json(&persisted).unwrap();
        assert_eq!(secret["accessToken"], "rotated-access");
        assert_eq!(secret["refreshToken"], "refresh-old");
        assert_eq!(secret["proxyPassword"], "proxy-secret");
    }

    #[test]
    fn deduplicates_reclaims_and_rejects_a_stale_lease() {
        let fixture = Fixture::new();
        let mut store = SyncRuntimeStore::open_database(&fixture.0).unwrap();
        let first = store.enqueue(&input("scheduled", 0), at(0)).unwrap();
        let same = store.enqueue(&input("manual", 100), at(1)).unwrap();
        assert_eq!(same.id, first.id);
        assert_eq!(same.priority, 100);

        let stale = store
            .claim_next("worker-a", StdDuration::from_secs(10), at(2))
            .unwrap()
            .unwrap();
        assert!(store
            .claim_next("worker-b", StdDuration::from_secs(10), at(11))
            .unwrap()
            .is_none());
        let active = store
            .claim_next("worker-b", StdDuration::from_secs(10), at(13))
            .unwrap()
            .unwrap();
        assert_eq!(active.attempts, 2);
        assert!(matches!(
            store.complete(
                &stale,
                &SyncCompletion {
                    mailbox: "INBOX".into(),
                    uid_validity: None,
                    highest_modseq: None,
                    last_seen_uid: 1,
                    synced: 0,
                    created: 0,
                    updated: 0,
                    deleted: 0,
                    message_changes: vec![],
                },
                5,
                at(14),
            ),
            Err(SyncRuntimeError::LeaseLost)
        ));
    }

    #[test]
    fn completes_cursor_and_queues_requested_rerun_with_events() {
        let fixture = Fixture::new();
        let mut store = SyncRuntimeStore::open_database(&fixture.0).unwrap();
        store.enqueue(&input("scheduled", 1), at(0)).unwrap();
        let job = store
            .claim_next("worker-a", StdDuration::from_secs(60), at(1))
            .unwrap()
            .unwrap();
        store.mark_started(&job, "INBOX", at(2)).unwrap();
        store.enqueue(&input("recovery", 50), at(3)).unwrap();
        store
            .complete(
                &job,
                &SyncCompletion {
                    mailbox: "INBOX".into(),
                    uid_validity: Some("44".into()),
                    highest_modseq: Some("90".into()),
                    last_seen_uid: 102,
                    synced: 2,
                    created: 1,
                    updated: 1,
                    deleted: 0,
                    message_changes: vec![json!({"after":{"id":"message-1"}})],
                },
                5,
                at(4),
            )
            .unwrap();

        let completed = store.job(&job.id).unwrap().unwrap();
        assert_eq!(completed.status, "succeeded");
        assert_eq!(completed.synced_count, Some(2));
        let state = store.mailbox_state("account-1", "INBOX").unwrap().unwrap();
        assert_eq!(state.uid_validity.as_deref(), Some("44"));
        assert_eq!(state.last_seen_uid, 102);
        assert_eq!(state.sync_state, "idle");
        assert_eq!(
            store.account_owner_id("account-1").unwrap().as_deref(),
            Some("user-1")
        );
        assert_eq!(
            store
                .mailbox_state_for_target("account-1", None, "inbox")
                .unwrap()
                .unwrap()
                .mailbox,
            "INBOX"
        );
        let next = store
            .claim_next("worker-b", StdDuration::from_secs(60), at(5))
            .unwrap()
            .unwrap();
        assert_eq!(next.reason, "recovery");
        assert_eq!(next.priority, 50);
        assert_eq!(
            store
                .events(0, 100)
                .unwrap()
                .into_iter()
                .map(|event| event.event_type)
                .collect::<Vec<_>>(),
            ["sync.started", "sync.completed"]
        );
    }

    #[test]
    fn persists_backoff_auth_pause_and_worker_health() {
        let fixture = Fixture::new();
        let mut store = SyncRuntimeStore::open_database(&fixture.0).unwrap();
        let defaults = SyncPolicySettings {
            folder_mode: "standard".into(),
            notify_on_error: false,
            ..SyncPolicySettings::default()
        };
        store.update_default_policy("user-1", &defaults).unwrap();
        assert_eq!(
            store
                .ensure_policy("account-1", "user-1", at(0))
                .unwrap()
                .folder_mode,
            "standard"
        );
        store.enqueue(&input("manual", 0), at(0)).unwrap();
        let job = store
            .claim_next("worker-a", StdDuration::from_secs(60), at(1))
            .unwrap()
            .unwrap();
        store
            .fail(
                &job,
                &SyncFailure {
                    mailbox: "INBOX".into(),
                    code: "AUTH_REQUIRED".into(),
                    message: "authorization failed".into(),
                    auth_required: true,
                    retry_minutes: None,
                },
                at(2),
            )
            .unwrap();
        let state = store.mailbox_state("account-1", "INBOX").unwrap().unwrap();
        assert_eq!(state.connection_status, "authRequired");
        assert_eq!(state.sync_state, "paused");
        assert_eq!(state.next_sync_at, None);
        store
            .resume_after_authorization("account-1", at(3))
            .unwrap();
        let resumed = store.mailbox_state("account-1", "INBOX").unwrap().unwrap();
        assert_eq!(resumed.connection_status, "connected");
        assert_eq!(resumed.consecutive_failures, 0);

        store.enqueue(&input("scheduled", 0), at(4)).unwrap();
        store
            .update_policy(
                "account-1",
                &SyncPolicySettings {
                    enabled: false,
                    ..defaults
                },
                at(5),
            )
            .unwrap();
        assert_eq!(store.worker_health(at(5)).unwrap().queued_jobs, 0);

        store.heartbeat("old", 1, "host", at(0), at(0)).unwrap();
        store
            .heartbeat("active", 2, "host", at(30), at(30))
            .unwrap();
        let health = store.worker_health(at(61)).unwrap();
        assert_eq!(health.workers.len(), 1);
        assert_eq!(health.workers[0].worker_id, "active");
    }

    #[test]
    fn scheduler_expands_policy_and_respects_due_and_paused_states() {
        let fixture = Fixture::new();
        let mut store = SyncRuntimeStore::open_database(&fixture.0).unwrap();
        store.ensure_policy("account-1", "user-1", at(0)).unwrap();
        store
            .update_policy(
                "account-1",
                &SyncPolicySettings {
                    folder_mode: "standard".into(),
                    ..SyncPolicySettings::default()
                },
                at(0),
            )
            .unwrap();
        let jobs = store.enqueue_due_syncs("startup", at(1)).unwrap();
        assert_eq!(
            jobs.iter()
                .map(|job| (job.mailbox_role.as_str(), job.reason.as_str(), job.priority))
                .collect::<Vec<_>>(),
            [
                ("inbox", "startup", 5),
                ("sent", "startup", 5),
                ("archive", "startup", 5)
            ]
        );

        store
            .update_policy(
                "account-1",
                &SyncPolicySettings {
                    enabled: false,
                    folder_mode: "standard".into(),
                    ..SyncPolicySettings::default()
                },
                at(2),
            )
            .unwrap();
        assert!(store
            .enqueue_due_syncs("scheduled", at(3))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn scheduler_waits_for_backoff_and_then_enqueues_recovery() {
        let fixture = Fixture::new();
        let mut store = SyncRuntimeStore::open_database(&fixture.0).unwrap();
        let first = store.enqueue_due_syncs("scheduled", at(0)).unwrap();
        assert_eq!(first.len(), 1);
        let job = store
            .claim_next("worker-a", StdDuration::from_secs(60), at(1))
            .unwrap()
            .unwrap();
        store
            .fail(
                &job,
                &SyncFailure {
                    mailbox: "INBOX".into(),
                    code: "TIMEOUT".into(),
                    message: "network timeout".into(),
                    auth_required: false,
                    retry_minutes: Some(5),
                },
                at(2),
            )
            .unwrap();
        assert!(store
            .enqueue_due_syncs("scheduled", at(2) + Duration::minutes(4))
            .unwrap()
            .is_empty());
        let recovery = store
            .enqueue_due_syncs("scheduled", at(2) + Duration::minutes(6))
            .unwrap();
        assert_eq!(recovery.len(), 1);
        assert_eq!(recovery[0].reason, "recovery");
    }
}
