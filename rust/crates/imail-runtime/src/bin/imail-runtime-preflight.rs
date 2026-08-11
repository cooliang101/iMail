use std::{
    collections::BTreeMap,
    env,
    error::Error,
    path::PathBuf,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use chrono::Utc;
use imail_core::ReadOnlyRepository;
use imail_protocol::SyncJobReadModel;
use imail_runtime::{
    JobExecutionContext, PersistentSyncRuntime, SyncJobExecutor, SyncWorkerConfig,
};
use imail_storage_sqlite::{
    migrate_database, SqliteReadOnlyStore, SyncCompletion, SyncFailure, SyncRuntimeStore,
};
use serde_json::json;

type CursorKey = (String, String);
type CursorValue = (Option<String>, i64, Option<String>);

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments.len() != 2 || arguments[1] != "--write-copy" {
        return Err("用法：imail-runtime-preflight <备份副本目录> --write-copy".into());
    }
    let data_dir = PathBuf::from(&arguments[0]).canonicalize()?;
    if data_dir.file_name().and_then(|name| name.to_str()) == Some(".data")
        || !data_dir.join("backup-manifest.json").is_file()
    {
        return Err("预检只允许写入带 backup-manifest.json 的明确备份副本".into());
    }
    let database_path = data_dir.join("imail.sqlite");
    let before_store = SqliteReadOnlyStore::open_data_dir(&data_dir)?;
    let before_inventory = before_store.inventory()?;
    let before_models = before_store.read_snapshot()?;
    let before_cursors = cursor_map(&before_models.mailbox_sync_states);
    drop(before_store);

    let migration = migrate_database(&database_path)?;
    let mut runtime_store = SyncRuntimeStore::open_database(&database_path)?;
    let queue_before = runtime_store.worker_health(Utc::now())?.queued_jobs;
    let due = runtime_store.enqueue_due_syncs("startup", Utc::now())?;
    let queue_after = runtime_store.worker_health(Utc::now())?.queued_jobs;
    drop(runtime_store);

    let executor = Arc::new(CursorPreservingExecutor {
        database_path: database_path.clone(),
    });
    let mut config = SyncWorkerConfig::new(&database_path);
    config.scheduler_enabled = false;
    config.worker_count = 2;
    config.poll_interval = Duration::from_millis(25);
    config.lease_duration = Duration::from_secs(5);
    let mut runtime = PersistentSyncRuntime::start(config, executor)?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let store = SyncRuntimeStore::open_database(&database_path)?;
        let terminal = due.iter().all(|job| {
            store
                .job(&job.id)
                .ok()
                .flatten()
                .is_some_and(|job| matches!(job.status.as_str(), "succeeded" | "failed"))
        });
        if terminal {
            break;
        }
        if Instant::now() >= deadline {
            return Err("runtime 未在 30 秒内完成副本任务".into());
        }
        thread::sleep(Duration::from_millis(25));
    }
    let runtime_health = runtime.health();
    let shutdown = runtime.shutdown(Duration::from_secs(2));
    if !shutdown.graceful() {
        return Err("runtime 副本预检未能优雅关闭".into());
    }
    let final_store = SyncRuntimeStore::open_database(&database_path)?;
    let queue_final = final_store.worker_health(Utc::now())?.queued_jobs;
    let due_final_statuses = due
        .iter()
        .map(|job| {
            final_store
                .job(&job.id)
                .map(|current| current.map(|current| current.status))
        })
        .collect::<Result<Vec<_>, _>>()?;
    drop(final_store);

    let after_store = SqliteReadOnlyStore::open_data_dir(&data_dir)?;
    let after_inventory = after_store.inventory()?;
    let after_models = after_store.read_snapshot()?;
    let after_cursors = cursor_map(&after_models.mailbox_sync_states);
    let preserved = json!({
        "accounts": before_models.accounts.len() == after_models.accounts.len(),
        "messages": before_models.messages.len() == after_models.messages.len(),
        "drafts": before_models.drafts.len() == after_models.drafts.len(),
        "contacts": before_models.contacts.len() == after_models.contacts.len(),
        "developerTokens": before_models.developer_tokens.len() == after_models.developer_tokens.len(),
        "mailboxSyncStates": before_models.mailbox_sync_states.len() == after_models.mailbox_sync_states.len(),
        "existingCursors": before_cursors.iter().all(|(key, value)| after_cursors.get(key) == Some(value)),
    });
    if preserved
        .as_object()
        .is_some_and(|checks| checks.values().any(|value| value != &json!(true)))
    {
        return Err("runtime 预检改变了必须保留的领域数据计数".into());
    }
    println!(
        "{}",
        serde_json::to_string(&json!({
            "ok": true,
            "dataDirectory": data_dir,
            "schemaVersion": after_inventory.schema_version,
            "migration": {
                "fromVersion": migration.from_version,
                "toVersion": migration.to_version,
                "appliedVersions": migration.applied_versions,
            },
            "integrity": {
                "beforeQuickCheck": before_inventory.quick_check,
                "afterQuickCheck": after_inventory.quick_check,
                "beforeForeignKeys": before_inventory.foreign_keys_verified,
                "afterForeignKeys": after_inventory.foreign_keys_verified,
            },
            "preserved": preserved,
            "queue": {
                "before": queue_before,
                "after": queue_after,
                "final": queue_final,
                "dueFinalStatuses": due_final_statuses,
                "dueTargets": due.iter().map(|job| json!({
                    "accountId": job.account_id,
                    "mailbox": job.mailbox,
                    "mailboxRole": job.mailbox_role,
                    "reason": job.reason,
                    "status": job.status,
                })).collect::<Vec<_>>(),
            },
            "runtime": {
                "health": {
                    "jobsStarted": runtime_health.jobs_started,
                    "jobsSucceeded": runtime_health.jobs_succeeded,
                    "jobsFailed": runtime_health.jobs_failed,
                    "jobsCancelled": runtime_health.jobs_cancelled,
                },
                "shutdownGraceful": shutdown.graceful(),
            },
        }))?
    );
    Ok(())
}

struct CursorPreservingExecutor {
    database_path: PathBuf,
}

impl SyncJobExecutor for CursorPreservingExecutor {
    fn execute(
        &self,
        job: &SyncJobReadModel,
        _context: &JobExecutionContext,
    ) -> Result<SyncCompletion, SyncFailure> {
        let store =
            SyncRuntimeStore::open_database(&self.database_path).map_err(|error| SyncFailure {
                mailbox: job
                    .mailbox
                    .clone()
                    .unwrap_or_else(|| format!("@role:{}", job.mailbox_role)),
                code: "PREFLIGHT_STORAGE".into(),
                message: error.to_string(),
                auth_required: false,
                retry_minutes: None,
            })?;
        let state = store
            .mailbox_state_for_target(&job.account_id, job.mailbox.as_deref(), &job.mailbox_role)
            .map_err(|error| SyncFailure {
                mailbox: job
                    .mailbox
                    .clone()
                    .unwrap_or_else(|| format!("@role:{}", job.mailbox_role)),
                code: "PREFLIGHT_STORAGE".into(),
                message: error.to_string(),
                auth_required: false,
                retry_minutes: None,
            })?;
        Ok(SyncCompletion {
            mailbox: state
                .as_ref()
                .map(|state| state.mailbox.clone())
                .or_else(|| job.mailbox.clone())
                .unwrap_or_else(|| format!("@role:{}", job.mailbox_role)),
            uid_validity: state.as_ref().and_then(|state| state.uid_validity.clone()),
            highest_modseq: state
                .as_ref()
                .and_then(|state| state.highest_modseq.clone()),
            last_seen_uid: state.as_ref().map_or(0, |state| state.last_seen_uid),
            synced: 0,
            created: 0,
            updated: 0,
            deleted: 0,
            message_changes: vec![],
        })
    }
}

fn cursor_map(
    states: &[imail_protocol::MailboxSyncStateReadModel],
) -> BTreeMap<CursorKey, CursorValue> {
    states
        .iter()
        .map(|state| {
            (
                (state.account_id.clone(), state.mailbox.clone()),
                (
                    state.uid_validity.clone(),
                    state.last_seen_uid,
                    state.highest_modseq.clone(),
                ),
            )
        })
        .collect()
}
